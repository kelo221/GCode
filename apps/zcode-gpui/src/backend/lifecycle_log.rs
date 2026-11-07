//! On-disk record of each backend child: spawn, stderr, exit status
//! (BACKEND_LOG.md).
//!
//! 修复依据：2026-11-07 后端在 12:25:24 无 shutdown 日志、无 WER/低内存事件地退出，
//! stderr 只进了 GPUI 内存日志、退出码也没记录，事后无法定位原因。这里把它们落盘。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Mutex, Once};
use std::time::Duration;

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const KEEP_DAYS: u64 = 14;
const PREFIX: &str = "gpui-backend-";

struct Sink {
    day: String,
    file: File,
    written: u64,
    capped: bool,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static PRUNE: Once = Once::new();

pub(crate) fn spawned(pid: u32, workspace: &Path, via: &str) {
    write_line(
        pid,
        &format!("spawned workspace={} via={via}", workspace.display()),
        true,
    );
}

pub(crate) fn stderr_line(pid: u32, line: &str) {
    write_line(pid, line, false);
}

pub(crate) fn exited(
    pid: u32,
    status: Option<&ExitStatus>,
    killed_by_gpui: bool,
    uptime: Duration,
) {
    write_line(pid, &exit_text(status, killed_by_gpui, uptime), true);
}

fn exit_text(status: Option<&ExitStatus>, killed_by_gpui: bool, uptime: Duration) -> String {
    let who = if killed_by_gpui {
        "killed by GPUI"
    } else {
        "not requested by GPUI"
    };
    format!(
        "exited {} uptime={} {who}",
        status_text(status),
        uptime_text(uptime)
    )
}

fn status_text(status: Option<&ExitStatus>) -> String {
    let Some(status) = status else {
        return "code=unknown (wait failed)".into();
    };
    if let Some(code) = status.code() {
        return format!("code={code} (0x{:08X})", code as u32);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal={signal}");
        }
    }
    "code=unknown".into()
}

fn uptime_text(uptime: Duration) -> String {
    let s = uptime.as_secs();
    match (s / 3600, (s % 3600) / 60, s % 60) {
        (0, 0, sec) => format!("{sec}s"),
        (0, min, sec) => format!("{min}m{sec:02}s"),
        (h, min, sec) => format!("{h}h{min:02}m{sec:02}s"),
    }
}

fn log_dir() -> PathBuf {
    crate::shared::settings::resolve_user_home_dir()
        .join(".zcode")
        .join("v2")
        .join("gpui-logs")
}

/// `(YYYY-MM-DD, YYYY-MM-DD HH:MM:SS.mmm)` in local time.
fn stamp() -> (String, String) {
    use crate::shared::local_time::{civil_time, local_offset_secs, now_ms};
    let ms = now_ms();
    let (y, m, d, h, mi, s) = civil_time(ms, local_offset_secs());
    let day = format!("{y:04}-{m:02}-{d:02}");
    let ts = format!("{day} {h:02}:{mi:02}:{s:02}.{:03}", ms % 1000);
    (day, ts)
}

fn open(dir: &Path, day: String) -> Option<Sink> {
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("{PREFIX}{day}.log"));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    let written = file.metadata().map_or(0, |m| m.len());
    Some(Sink {
        day,
        file,
        written,
        capped: written >= MAX_FILE_BYTES,
    })
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let max_age = Duration::from_secs(KEEP_DAYS * 86_400);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(PREFIX) && name.ends_with(".log")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// `force` lines (spawn/exit) bypass the size cap.
fn write_line(pid: u32, text: &str, force: bool) {
    if cfg!(test) {
        return;
    }
    let (day, ts) = stamp();
    let Ok(mut guard) = SINK.lock() else {
        return;
    };
    if guard.as_ref().is_none_or(|s| s.day != day) {
        let dir = log_dir();
        PRUNE.call_once(|| prune(&dir));
        *guard = open(&dir, day);
    }
    let Some(sink) = guard.as_mut() else {
        return;
    };
    if sink.capped && !force {
        return;
    }
    let line = format!("{ts} [pid {pid}] {text}\n");
    if !force && sink.written + line.len() as u64 > MAX_FILE_BYTES {
        sink.capped = true;
        let note =
            format!("{ts} [gpui] size cap reached; stderr dropped for the rest of the day\n");
        let _ = sink.file.write_all(note.as_bytes());
        return;
    }
    if sink.file.write_all(line.as_bytes()).is_ok() {
        sink.written += line.len() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_formats() {
        assert_eq!(uptime_text(Duration::from_secs(7)), "7s");
        assert_eq!(uptime_text(Duration::from_secs(125)), "2m05s");
        assert_eq!(uptime_text(Duration::from_secs(5_712)), "1h35m12s");
    }

    #[test]
    fn exit_text_names_the_killer() {
        let text = exit_text(None, false, Duration::from_secs(3));
        assert_eq!(
            text,
            "exited code=unknown (wait failed) uptime=3s not requested by GPUI"
        );
        assert!(exit_text(None, true, Duration::ZERO).ends_with("killed by GPUI"));
    }

    #[cfg(windows)]
    #[test]
    fn exit_code_shows_hex() {
        use std::os::windows::process::ExitStatusExt;
        let status = ExitStatus::from_raw(0xC000_0005);
        assert_eq!(status_text(Some(&status)), "code=-1073741819 (0xC0000005)");
    }
}
