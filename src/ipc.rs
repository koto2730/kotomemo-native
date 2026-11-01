use crate::io::config;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

const LOCK_FILE: &str = "instance.lock";
const PORT_FILE: &str = "instance.port";

#[derive(Debug, Serialize, Deserialize)]
struct IpcMessage {
    paths: Vec<PathBuf>,
}

pub struct PrimaryInstance {
    pub rx: Receiver<Vec<PathBuf>>,
    pub ctx_slot: Arc<OnceLock<egui::Context>>,
    _lock_file: Option<File>,
}

impl PrimaryInstance {
    pub fn set_egui_ctx(&self, ctx: egui::Context) {
        let _ = self.ctx_slot.set(ctx);
    }
}

/// Attempts to become the primary running instance of `kotomemo`.
///
/// - If no other instance is running, acquires the OS-level exclusive lock on
///   `~/.kotomemo/instance.lock`, starts a local TCP listener on `127.0.0.1:0`,
///   writes the port to `~/.kotomemo/instance.port`, and returns `Some(PrimaryInstance)`.
/// - If another instance is already running (including when Windows Explorer
///   simultaneously spawns one process per selected file via the context menu),
///   connects to the primary instance's local TCP port, forwards `initial_paths`,
///   and returns `None` so the caller can exit immediately.
pub fn try_acquire_primary_or_forward(initial_paths: &[PathBuf]) -> Option<PrimaryInstance> {
    let dir = config::config_dir();
    let _ = fs::create_dir_all(&dir);
    let lock_path = dir.join(LOCK_FILE);
    let port_path = dir.join(PORT_FILE);

    match try_lock_exclusive(&lock_path, &port_path) {
        Some(lock_file) => {
            if let Ok(listener) = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)) {
                if let Ok(addr) = listener.local_addr() {
                    let _ = fs::write(&port_path, addr.port().to_string());
                }
                let (tx, rx) = mpsc::channel();
                let ctx_slot: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
                let ctx_for_thread = Arc::clone(&ctx_slot);

                std::thread::Builder::new()
                    .name("kotomemo-ipc-listener".to_string())
                    .spawn(move || {
                        for stream in listener.incoming().flatten() {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let mut reader = BufReader::new(stream);
                            let mut line = String::new();
                            if reader.read_line(&mut line).is_ok() && !line.trim().is_empty() {
                                if let Ok(msg) = serde_json::from_str::<IpcMessage>(&line) {
                                    let _ = tx.send(msg.paths);
                                    if let Some(ctx) = ctx_for_thread.get() {
                                        ctx.request_repaint();
                                    }
                                }
                            }
                        }
                    })
                    .ok();

                return Some(PrimaryInstance {
                    rx,
                    ctx_slot,
                    _lock_file: lock_file,
                });
            }
            Some(PrimaryInstance {
                rx: mpsc::channel().1,
                ctx_slot: Arc::new(OnceLock::new()),
                _lock_file: lock_file,
            })
        }
        None => {
            // Another instance holds the lock. Retry briefly in case the primary
            // instance won the lock a few milliseconds ago and is still writing
            // `instance.port`.
            let payload = serde_json::to_string(&IpcMessage {
                paths: initial_paths.to_vec(),
            })
            .unwrap_or_default()
                + "\n";

            for _ in 0..60 {
                if let Ok(port_str) = fs::read_to_string(&port_path) {
                    if let Ok(port) = port_str.trim().parse::<u16>() {
                        let addr = SocketAddr::from(([127, 0, 0, 1], port));
                        if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                            if stream.write_all(payload.as_bytes()).is_ok() {
                                let _ = stream.flush();
                                return None;
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            None
        }
    }
}

#[cfg(windows)]
fn try_lock_exclusive(lock_path: & std::path::Path, _port_path: &std::path::Path) -> Option<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    match fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(lock_path)
    {
        Ok(file) => Some(Some(file)),
        Err(_) => None,
    }
}

#[cfg(not(windows))]
fn try_lock_exclusive(lock_path: &std::path::Path, port_path: &std::path::Path) -> Option<Option<File>> {
    if let Ok(port_str) = fs::read_to_string(port_path) {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
                return None;
            }
        }
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)
        .ok();
    Some(file)
}
