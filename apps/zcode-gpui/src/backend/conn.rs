//! Connection lifecycle and IO pumping for the backend child process.
//! Backlog accounting lives in backend/backlog.rs.

use crate::backend::backlog::{
    BoundedLine, EventBacklog, MAX_LOG_BACKLOG_BYTES, MAX_RAW_LINE_BYTES, push_event,
    read_bounded_line,
};
use crate::backend::launcher::BackendLaunch;
use crate::backend::lifecycle_log;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

pub enum ConnEvent {
    /// One protocol line from child stdout.
    Line(String),
    /// Diagnostics from child stderr (never parsed as protocol).
    Log(String),
    /// stdout EOF → child is gone.
    Exited,
}

pub struct Connection {
    pub(crate) pid: u32,
    pub events: futures::channel::mpsc::UnboundedReceiver<ConnEvent>,
    /// Protocol lines to write to child stdin (LF-terminated).
    pub inbound: Sender<String>,
    /// Force-kills the child (and its tree on Windows). Called on idle unload;
    /// on Windows the job object also guarantees death on frontend crash.
    pub kill: Box<dyn FnOnce() + Send>,
    /// Protocol (stdout) backlog: flow watermarks + hard bound; crossing the
    /// bound latches overflow and the pump restarts the connection.
    pub backlog: EventBacklog,
    /// Independent lossy stderr backlog — a log storm can neither latch the
    /// protocol overflow flag nor consume protocol capacity.
    pub log_backlog: EventBacklog,
}

/// Owned job object handle. Dropping it closes the handle, which (with
/// `KILL_ON_JOB_CLOSE`) also kills any process still in the job; the OS does
/// the same on frontend crash, so agents cannot outlive the app.
#[cfg(windows)]
struct JobHandle(windows::Win32::Foundation::HANDLE);

// SAFETY: a job object handle is a process-wide kernel handle with no thread
// affinity; it is only used for TerminateJobObject/CloseHandle.
#[cfg(windows)]
unsafe impl Send for JobHandle {}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Put `child` in a kill-on-close job. `None` on restricted/virtualized
/// Windows setups (nested jobs denied, sandboxed tokens, ...): the caller
/// falls back to tree-killing by pid. Never panics.
#[cfg(windows)]
fn attach_job(child: &Child) -> Option<JobHandle> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    // The handle is owned from here on, so every failure path closes it.
    let job = JobHandle(unsafe { CreateJobObjectW(None, None) }.ok()?);
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle() as _)).ok()?;
    }
    Some(job)
}

#[cfg(windows)]
pub(crate) fn make_kill(child: &Child) -> Box<dyn FnOnce() + Send> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let pid = child.id();
    let job = attach_job(child);
    if job.is_none() {
        eprintln!(
            "[zcode-gpui] warning: no job object for child pid {pid}; falling back to taskkill"
        );
    }
    Box::new(move || match job {
        Some(job) => unsafe {
            let _ = windows::Win32::System::JobObjects::TerminateJobObject(job.0, 0);
            // `job` drops here and closes the handle.
        },
        None => {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
    })
}

#[cfg(not(windows))]
fn make_kill(child: &std::sync::Arc<std::sync::Mutex<Option<Child>>>) -> Box<dyn FnOnce() + Send> {
    let child = child.clone();
    Box::new(move || {
        if let Ok(mut guard) = child.lock()
            && let Some(c) = guard.as_mut()
        {
            let _ = c.kill();
        }
    })
}

/// Bounded stdout pump: reads LF-delimited protocol lines with a hard cap on
/// how far a line may grow BEFORE any large allocation happens (follow-up
/// review finding 6 — `BufRead::lines()` would allocate a hostile
/// unterminated line in full before any admission check could run).
fn pump_stdout(stdout: impl Read, backlog: EventBacklog, tx: Sender2) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_bounded_line(&mut reader, MAX_RAW_LINE_BYTES) {
            BoundedLine::Eof => break,
            BoundedLine::Overlong => continue,
            BoundedLine::Line(bytes) if bytes.is_empty() => continue,
            BoundedLine::Line(bytes) => {
                let Ok(line) = String::from_utf8(bytes) else {
                    continue;
                };
                let len = line.len();
                if !push_event(&backlog, &tx, ConnEvent::Line(line), len) {
                    break;
                }
            }
        }
    }
    let _ = tx.unbounded_send(ConnEvent::Exited);
}

type Sender2 = futures::channel::mpsc::UnboundedSender<ConnEvent>;

pub fn spawn_connection(launch: &BackendLaunch, workspace: &Path) -> std::io::Result<Connection> {
    // Sanitized environment: the launcher builds an explicit allowlist, and
    // env_clear() guarantees nothing else leaks through (audit P0.4 — the
    // GPUI process env can carry provider secrets or override variables from
    // whatever shell launched the app).
    //
    // cwd fallback: a deleted/unmounted workspace must not prevent the agent
    // from starting (review finding 8); workspace identity still travels on
    // the protocol.
    let cwd = crate::backend::workspace::nearest_existing_dir(workspace);
    let mut child = Command::new(&launch.program)
        .args(&launch.args)
        .env_clear()
        .envs(launch.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = child.id();
    let missing = |what: &str| std::io::Error::other(format!("child {what} was not piped"));
    let (Some(mut stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = child.kill();
        return Err(missing("stdio"));
    };

    let (in_tx, in_rx) = std::sync::mpsc::channel::<String>();
    let (ev_tx, ev_rx) = futures::channel::mpsc::unbounded::<ConnEvent>();
    let backlog = EventBacklog::new(crate::backend::backlog::MAX_PROTOCOL_BACKLOG_BYTES);
    let log_backlog = EventBacklog::new(MAX_LOG_BACKLOG_BYTES);

    std::thread::spawn(move || {
        for line in in_rx {
            let mut bytes = line.into_bytes();
            bytes.push(b'\n');
            if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
                break;
            }
        }
    });

    let out_tx = ev_tx.clone();
    let out_backlog = backlog.clone();
    std::thread::spawn(move || pump_stdout(stdout, out_backlog, out_tx));

    let err_tx = ev_tx.clone();
    let err_backlog = log_backlog.clone();
    let (log_workspace, log_via) = (workspace.to_path_buf(), launch.describe.clone());
    std::thread::spawn(move || {
        lifecycle_log::spawned(pid, &log_workspace, &log_via);
        let reader = BufReader::new(stderr);
        // Keep draining into the lifecycle log after GPUI stops listening:
        // the child's dying words are what BACKEND_LOG.md is for.
        let mut forward = true;
        for line in reader.lines() {
            let Ok(l) = line else { break };
            lifecycle_log::stderr_line(pid, &l);
            let len = l.len();
            if forward && !push_event(&err_backlog, &err_tx, ConnEvent::Log(l), len) {
                forward = false;
            }
        }
        // stderr EOF is not death; stdout's Exited event owns that signal.
    });

    let started = std::time::Instant::now();
    let killed_by_gpui = Arc::new(AtomicBool::new(false));
    #[cfg(windows)]
    let kill = make_kill(&child);
    #[cfg(windows)]
    {
        let killed_by_gpui = killed_by_gpui.clone();
        std::thread::spawn(move || {
            let mut child = child;
            let status = child.wait().ok();
            let killed = killed_by_gpui.load(Ordering::SeqCst);
            lifecycle_log::exited(pid, status.as_ref(), killed, started.elapsed());
        });
    }
    #[cfg(not(windows))]
    let child = Arc::new(std::sync::Mutex::new(Some(child)));
    #[cfg(not(windows))]
    let kill = make_kill(&child);
    #[cfg(not(windows))]
    {
        let watcher_child = child.clone();
        let killed_by_gpui = killed_by_gpui.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let Ok(mut guard) = watcher_child.lock() else {
                    break;
                };
                match guard.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(None)) => {}
                    Some(Ok(Some(status))) => {
                        let killed = killed_by_gpui.load(Ordering::SeqCst);
                        lifecycle_log::exited(pid, Some(&status), killed, started.elapsed());
                        break;
                    }
                    Some(Err(_)) => {
                        let killed = killed_by_gpui.load(Ordering::SeqCst);
                        lifecycle_log::exited(pid, None, killed, started.elapsed());
                        break;
                    }
                    None => break,
                }
            }
        });
    }
    // Mark GPUI-initiated kills so the exit line can tell them from crashes.
    let kill: Box<dyn FnOnce() + Send> = Box::new(move || {
        killed_by_gpui.store(true, Ordering::SeqCst);
        kill();
    });

    Ok(Connection {
        pid,
        events: ev_rx,
        inbound: in_tx,
        kill,
        backlog,
        log_backlog,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// The kill closure must end the child (job object or taskkill fallback)
    /// and never panic, including when no job could be attached.
    #[test]
    fn kill_terminates_child_tree() {
        let mut child = Command::new("cmd")
            .args(["/c", "ping -n 30 127.0.0.1 >nul"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let kill = make_kill(&child);
        kill();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "child survived kill");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn dropping_unused_kill_closure_does_not_panic() {
        let mut child = Command::new("cmd").args(["/c", "exit 0"]).spawn().unwrap();
        let _ = child.wait();
        drop(make_kill(&child));
    }
}
