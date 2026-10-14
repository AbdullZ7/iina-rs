//! Single-instance support: opening files while IINA-RS is already running
//! hands them to the existing window (like IINA) instead of starting another
//! player. The running instance listens on a loopback TCP port; the port and a
//! random token are kept in `instance.lock` in the config dir, and requests
//! without the token are ignored.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

fn lock_file(dir: &Path) -> std::path::PathBuf {
    dir.join("instance.lock")
}

/// Tries to pass `args` to a running instance. Returns true if it accepted them.
pub fn forward(dir: &Path, args: &[String]) -> bool {
    let Ok(text) = std::fs::read_to_string(lock_file(dir)) else { return false };
    let mut parts = text.split_whitespace();
    let (Some(port), Some(token)) = (parts.next().and_then(|p| p.parse::<u16>().ok()), parts.next()) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, port).into(), Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let msg = serde_json::json!({ "token": token, "args": args });
    if writeln!(stream, "{msg}").is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}

/// Becomes the primary instance: listens for forwarded arguments and calls
/// `on_open` with them (on a background thread).
pub fn listen(dir: &Path, on_open: impl Fn(Vec<String>) + Send + 'static) {
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else { return };
    let Ok(addr) = listener.local_addr() else { return };
    let token: String = (0..32).map(|_| format!("{:x}", rand::random::<u8>() % 16)).collect();
    if std::fs::write(lock_file(dir), format!("{} {token}", addr.port())).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                let mut reader = BufReader::new(&stream);
                if reader.read_line(&mut line).is_err() {
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
                let _ = (&stream).write_all(b"ok\n");
                on_open(args);
            }
        })
        .ok();
}

/// Removes the lock file on exit (only if it is still ours).
pub fn release(dir: &Path) {
    let _ = std::fs::remove_file(lock_file(dir));
}
