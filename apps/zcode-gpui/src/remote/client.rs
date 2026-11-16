//! Phone side: the stored pairing, pairing and listing over a short socket,
//! and `connect`, which produces the same `Connection` a local CLI spawn
//! does, so everything above it is unchanged.

use crate::backend::backlog::{
    EventBacklog, MAX_LOG_BACKLOG_BYTES, MAX_PROTOCOL_BACKLOG_BYTES, MAX_RAW_LINE_BYTES, push_event,
};
use crate::backend::conn::{ConnEvent, Connection};
use crate::remote::handshake::{DesktopReply, HANDSHAKE_TIMEOUT, PhoneHello, RemoteWorkspace};
use crate::remote::line_socket::{LineSocket, POLL_INTERVAL};
use crate::remote::link::PairLink;
use serde::{Deserialize, Serialize};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// `remote-pairing.json` in the phone's data dir (app-private storage).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteTarget {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub device_token: String,
}

impl RemoteTarget {
    fn path() -> PathBuf {
        crate::shared::data_paths::paths()
            .data_root()
            .join("remote-pairing.json")
    }

    pub(crate) fn load() -> Option<Self> {
        let bytes = std::fs::read(Self::path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn forget() {
        let _ = std::fs::remove_file(Self::path());
    }
}

type ClientSocket = LineSocket<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>;

fn open_socket(host: &str, port: u16, fingerprint: &str) -> Result<ClientSocket, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("bad desktop address: {e}"))?
        .next()
        .ok_or("bad desktop address")?;
    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("desktop unreachable: {e}"))?;
    let _ = tcp.set_nodelay(true);
    tcp.set_read_timeout(Some(POLL_INTERVAL))
        .map_err(|e| e.to_string())?;
    let config = crate::remote::tls::client_config(fingerprint)?;
    let conn = rustls::ClientConnection::new(config, crate::remote::tls::server_name())
        .map_err(|e| e.to_string())?;
    Ok(LineSocket::new(
        rustls::StreamOwned::new(conn, tcp),
        MAX_RAW_LINE_BYTES,
    ))
}

fn send(sock: &mut ClientSocket, hello: &PhoneHello) -> Result<(), String> {
    let line = serde_json::to_string(hello).map_err(|e| e.to_string())?;
    sock.write_line(&line)
        .map_err(|e| format!("desktop link failed: {e}"))
}

fn expect(sock: &mut ClientSocket) -> Result<DesktopReply, String> {
    let line = sock
        .read_line_within(HANDSHAKE_TIMEOUT)
        .map_err(|e| format!("desktop link failed: {e}"))?;
    match serde_json::from_str(&line) {
        Ok(DesktopReply::Error { code }) => Err(format!("desktop refused: {code}")),
        Ok(reply) => Ok(reply),
        Err(_) => Err("unexpected reply from the desktop".into()),
    }
}

fn expect_workspaces(sock: &mut ClientSocket) -> Result<Vec<RemoteWorkspace>, String> {
    match expect(sock)? {
        DesktopReply::Workspaces { workspaces } => Ok(workspaces),
        _ => Err("unexpected reply from the desktop".into()),
    }
}

fn auth(target: &RemoteTarget) -> Result<(ClientSocket, Vec<RemoteWorkspace>), String> {
    let mut sock = open_socket(&target.host, target.port, &target.fingerprint)?;
    send(
        &mut sock,
        &PhoneHello::Auth {
            device_token: target.device_token.clone(),
        },
    )?;
    let workspaces = expect_workspaces(&mut sock)?;
    Ok((sock, workspaces))
}

/// Pair with the desktop from a link. Blocking; run off the UI thread.
pub(crate) fn pair(
    link: &PairLink,
    device_name: &str,
) -> Result<(RemoteTarget, Vec<RemoteWorkspace>), String> {
    let mut sock = open_socket(&link.host, link.port, &link.fingerprint)?;
    send(
        &mut sock,
        &PhoneHello::Pair {
            pairing_token: link.token.clone(),
            device_name: device_name.into(),
        },
    )?;
    let DesktopReply::Paired { device_token } = expect(&mut sock)? else {
        return Err("unexpected reply from the desktop".into());
    };
    let workspaces = expect_workspaces(&mut sock)?;
    Ok((
        RemoteTarget {
            host: link.host.clone(),
            port: link.port,
            fingerprint: link.fingerprint.clone(),
            device_token,
        },
        workspaces,
    ))
}

/// The desktop's workspace list. Blocking; run off the UI thread.
pub(crate) fn list_workspaces(target: &RemoteTarget) -> Result<Vec<RemoteWorkspace>, String> {
    auth(target).map(|(_, workspaces)| workspaces)
}

/// Open `ws_key` on the desktop. Returns at once; the TLS handshake and
/// `auth`/`open` run on the IO thread, and failures arrive as a log line
/// followed by `Exited`, like a CLI that died during startup.
pub(crate) fn connect(target: &RemoteTarget, ws_key: &str) -> Connection {
    let (in_tx, in_rx) = std::sync::mpsc::channel::<String>();
    let (ev_tx, ev_rx) = futures::channel::mpsc::unbounded::<ConnEvent>();
    let backlog = EventBacklog::new(MAX_PROTOCOL_BACKLOG_BYTES);
    let log_backlog = EventBacklog::new(MAX_LOG_BACKLOG_BYTES);
    let stop = Arc::new(AtomicBool::new(false));
    let (target, ws_key) = (target.clone(), ws_key.to_string());
    let (thread_backlog, thread_log_backlog, thread_stop) =
        (backlog.clone(), log_backlog.clone(), stop.clone());
    std::thread::spawn(move || {
        let reason = run(
            &target,
            &ws_key,
            &in_rx,
            &ev_tx,
            &thread_backlog,
            &thread_stop,
        );
        let line = format!("desktop link closed: {reason}");
        let len = line.len();
        push_event(&thread_log_backlog, &ev_tx, ConnEvent::Log(line), len);
        let _ = ev_tx.unbounded_send(ConnEvent::Exited(None));
    });
    Connection {
        pid: 0,
        events: ev_rx,
        inbound: in_tx,
        kill: Box::new(move || stop.store(true, Ordering::Relaxed)),
        backlog,
        log_backlog,
    }
}

fn run(
    target: &RemoteTarget,
    ws_key: &str,
    inbound: &std::sync::mpsc::Receiver<String>,
    events: &futures::channel::mpsc::UnboundedSender<ConnEvent>,
    backlog: &EventBacklog,
    stop: &AtomicBool,
) -> String {
    let (mut sock, _) = match auth(target) {
        Ok(ok) => ok,
        Err(error) => return error,
    };
    if let Err(error) = send(
        &mut sock,
        &PhoneHello::Open {
            workspace_key: ws_key.into(),
        },
    ) {
        return error;
    }
    match expect(&mut sock) {
        Ok(DesktopReply::Opened) => {}
        Ok(_) => return "unexpected reply from the desktop".into(),
        Err(error) => return error,
    }
    while !stop.load(Ordering::Relaxed) {
        loop {
            match inbound.try_recv() {
                Ok(line) => {
                    if let Err(error) = sock.write_line(&line) {
                        return error.to_string();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return "closed".into(),
            }
        }
        match sock.read_lines() {
            Ok(lines) => {
                for line in lines {
                    let len = line.len();
                    if !push_event(backlog, events, ConnEvent::Line(line), len) {
                        return "event queue closed".into();
                    }
                }
            }
            Err(error) => return error.to_string(),
        }
    }
    "closed".into()
}
