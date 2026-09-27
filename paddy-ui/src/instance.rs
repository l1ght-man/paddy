//! Single instance. The first paddy listens on a Unix socket; a later launch
//! connects, asks it to show its main window, and exits instead of starting a
//! second copy with a second tray icon.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::desktop::DesktopEvent;

const SHOW: &[u8] = b"show\n";

/// Outcome of trying to become the running paddy.
pub enum Claim {
    /// We are the only paddy: keep this and call `listen` once the UI is up.
    First(Primary),
    /// Another paddy is running and was asked to show its window.
    Forwarded,
}

/// The listening socket of the running instance; the file is removed on drop.
pub struct Primary {
    listener: Option<UnixListener>,
    path: PathBuf,
}

/// Become the running instance at `path`, or hand over to the one already there.
/// A socket file left behind by a crash (nobody answers) is replaced.
pub fn claim(path: &Path) -> io::Result<Claim> {
    if let Some(dir) = path.parent() {
        paddy_core::ensure_private_dir(dir)?;
    }
    match UnixListener::bind(path) {
        Ok(l) => return Ok(Claim::First(Primary { listener: Some(l), path: path.to_path_buf() })),
        Err(e) if e.kind() != io::ErrorKind::AddrInUse => return Err(e),
        Err(_) => {}
    }
    if ask_to_show(path).is_ok() {
        return Ok(Claim::Forwarded);
    }
    // Stale: the file exists but no one is listening. Only ever remove a socket.
    if !is_socket(path) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} is not a socket", path.display())));
    }
    std::fs::remove_file(path)?;
    let l = UnixListener::bind(path)?;
    Ok(Claim::First(Primary { listener: Some(l), path: path.to_path_buf() }))
}

fn ask_to_show(path: &Path) -> io::Result<()> {
    let mut s = UnixStream::connect(path)?;
    s.set_write_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(SHOW)
}

fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket())
}

impl Primary {
    /// Answer later launches from a background thread: each "show" becomes a
    /// `DesktopEvent::ShowMain` on the same channel the tray uses.
    pub fn listen(&mut self, tx: Sender<DesktopEvent>) {
        let Some(listener) = self.listener.take() else { return };
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 16];
                let n = conn.read(&mut buf).unwrap_or(0);
                if buf[..n].starts_with(b"show") && tx.send(DesktopEvent::ShowMain).is_err() {
                    break;
                }
            }
        });
    }
}

impl Drop for Primary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn first(c: Claim) -> Primary {
        match c {
            Claim::First(p) => p,
            Claim::Forwarded => panic!("expected to be the first instance"),
        }
    }

    #[test]
    fn second_launch_reaches_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("run/paddy.sock");
        let mut p = first(claim(&sock).unwrap());
        let (tx, rx) = channel();
        p.listen(tx);
        assert!(matches!(claim(&sock).unwrap(), Claim::Forwarded));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(DesktopEvent::ShowMain));
        assert!(matches!(claim(&sock).unwrap(), Claim::Forwarded), "and again");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(DesktopEvent::ShowMain));
        drop(p);
        assert!(!sock.exists(), "a clean exit removes the socket");
    }

    #[test]
    fn stale_socket_from_a_crash_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("paddy.sock");
        drop(UnixListener::bind(&sock).unwrap()); // leaves the file, nobody listening
        assert!(sock.exists());
        let mut p = first(claim(&sock).unwrap());
        let (tx, rx) = channel();
        p.listen(tx);
        assert!(matches!(claim(&sock).unwrap(), Claim::Forwarded));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(DesktopEvent::ShowMain));
    }

    #[test]
    fn a_regular_file_in_the_way_is_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("paddy.sock");
        std::fs::write(&sock, "keep me").unwrap();
        assert!(claim(&sock).is_err());
        assert_eq!(std::fs::read_to_string(&sock).unwrap(), "keep me");
    }

    #[test]
    fn junk_on_the_socket_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("paddy.sock");
        let mut p = first(claim(&sock).unwrap());
        let (tx, rx) = channel();
        p.listen(tx);
        UnixStream::connect(&sock).unwrap().write_all(b"rm -rf\n").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        assert!(matches!(claim(&sock).unwrap(), Claim::Forwarded));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(DesktopEvent::ShowMain));
    }
}
