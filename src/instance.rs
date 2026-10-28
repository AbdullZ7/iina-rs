//! Single-instance support: opening files while IINA-RS is already running
//! hands them to the existing window (like IINA) instead of starting another
//! player. The running instance listens on a local channel that never shows
//! up as a network connection: a named pipe on Windows (local clients only),
//! a Unix socket elsewhere. Its name and a random token are kept in
//! `instance.lock` in the config dir, and requests without the token are
//! ignored.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::time::Duration;

fn lock_file(dir: &Path) -> std::path::PathBuf {
    dir.join("instance.lock")
}

/// Tries to pass `args` to a running instance. Returns true if it accepted them.
pub fn forward(dir: &Path, args: &[String]) -> bool {
    let Ok(text) = std::fs::read_to_string(lock_file(dir)) else { return false };
    let mut parts = text.split_whitespace();
    let (Some(name), Some(token)) = (parts.next(), parts.next()) else { return false };
    let msg = serde_json::json!({ "token": token, "args": args }).to_string();
    let name = name.to_string();
    // A hung instance must not hang this one: talk on a thread, wait 2 s.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let ok = (|| -> std::io::Result<bool> {
            let mut conn = connect(&name)?;
            writeln!(conn, "{msg}")?;
            conn.flush()?;
            let mut reply = String::new();
            BufReader::new(conn).read_line(&mut reply)?;
            Ok(reply.trim() == "ok")
        })()
        .unwrap_or(false);
        let _ = tx.send(ok);
    });
    rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
}

/// Becomes the primary instance: listens for forwarded arguments and calls
/// `on_open` with them (on a background thread).
pub fn listen(dir: &Path, on_open: impl Fn(Vec<String>) + Send + 'static) {
    let token: String = (0..32).map(|_| format!("{:x}", rand::random::<u8>() % 16)).collect();
    let Some((name, server)) = Server::create(dir) else { return };
    if std::fs::write(lock_file(dir), format!("{name} {token}")).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            let mut server = server;
            while let Some(conn) = server.accept() {
                let mut line = String::new();
                let mut reader = BufReader::new(conn);
                if reader.by_ref().take(64 * 1024).read_line(&mut line).is_err() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                if v["token"].as_str() != Some(token.as_str()) {
                    continue;
                }
                let args: Vec<String> = v["args"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let mut conn = reader.into_inner();
                let _ = conn.write_all(b"ok\n");
                let _ = conn.flush();
                on_open(args);
            }
        })
        .ok();
}

/// Removes the lock file on exit (only if it is still ours).
pub fn release(dir: &Path) {
    let _ = std::fs::remove_file(lock_file(dir));
}

// ------------------------------------------------------------------ Windows

#[cfg(windows)]
fn connect(name: &str) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).open(name)
}

#[cfg(windows)]
struct Server {
    name: String,
    /// The pipe instance clients connect to next: one is always waiting.
    next: Option<std::fs::File>,
}

#[cfg(windows)]
impl Server {
    /// `\\.\pipe\iina-rs-<pid>`; the first instance must succeed.
    fn create(_dir: &Path) -> Option<(String, Server)> {
        let name = format!(r"\\.\pipe\iina-rs-{}", std::process::id());
        let mut s = Server { name: name.clone(), next: None };
        // Created now (and refused if the name is taken), so a forward right
        // after start finds it.
        s.next = Some(s.instance(true)?);
        Some((name, s))
    }

    fn instance(&self, first: bool) -> Option<std::fs::File> {
        use std::os::windows::io::FromRawHandle;
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
        use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT};
        let open_mode = if first { PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_DUPLEX };
        let h = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(self.name.as_str()),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                4,
                4096,
                4096,
                0,
                None,
            )
        };
        if h.is_invalid() {
            return None;
        }
        Some(unsafe { std::fs::File::from_raw_handle(h.0) })
    }

    /// Waits for the next client.
    fn accept(&mut self) -> Option<std::fs::File> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, HANDLE};
        use windows::Win32::System::Pipes::ConnectNamedPipe;
        let pipe = match self.next.take() {
            Some(p) => p,
            None => self.instance(false)?,
        };
        let connected = match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        // The next client already finds a waiting instance.
        self.next = self.instance(false);
        connected.then_some(pipe)
    }
}

// --------------------------------------------------------------- Unix-likes

#[cfg(not(windows))]
fn connect(name: &str) -> std::io::Result<std::os::unix::net::UnixStream> {
    let s = std::os::unix::net::UnixStream::connect(name)?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    Ok(s)
}

#[cfg(not(windows))]
struct Server {
    listener: std::os::unix::net::UnixListener,
}

#[cfg(not(windows))]
impl Server {
    /// `<config>/instance-<pid>.sock`, readable by this user only.
    fn create(dir: &Path) -> Option<(String, Server)> {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(format!("instance-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).ok()?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        Some((path.to_string_lossy().into_owned(), Server { listener }))
    }

    fn accept(&mut self) -> Option<std::os::unix::net::UnixStream> {
        loop {
            match self.listener.accept() {
                Ok((s, _)) => {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                    return Some(s);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_to_listener() {
        let dir = std::env::temp_dir().join(format!("iina-rs-test-inst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        listen(&dir, move |args| {
            let _ = tx.send(args);
        });
        for round in 0..3 {
            let args = vec![format!("file {round}.mkv"), "--start=1".to_string()];
            assert!(forward(&dir, &args), "round {round}");
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), args);
        }
        // A wrong token is ignored.
        let text = std::fs::read_to_string(lock_file(&dir)).unwrap();
        let name = text.split_whitespace().next().unwrap().to_string();
        std::fs::write(lock_file(&dir), format!("{name} badtoken")).unwrap();
        assert!(!forward(&dir, &["x".to_string()]));
        // No instance: nothing to forward to.
        release(&dir);
        assert!(!forward(&dir, &["x".to_string()]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
