//! Checks GitHub for a newer release (IINA uses Sparkle for this). Only the
//! public "latest release" endpoint is queried; nothing is downloaded or
//! installed automatically — the user gets a link to the release page.

use std::sync::{Arc, Mutex};

const API: &str = "https://api.github.com/repos/AbdullZ7/iina-rs/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, url: String },
    Failed(String),
}

pub type Shared = Arc<Mutex<Status>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(Status::Idle))
}

/// "v0.4.1" / "0.4.1" → (0, 4, 1).
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let mut it = v.split(['.', '-']).map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    matches!((parse(remote), parse(current)), (Some(r), Some(c)) if r > c)
}

/// Starts a background check; `done` runs on the worker thread afterwards.
pub fn check(state: Shared, done: impl FnOnce() + Send + 'static) {
    *state.lock().unwrap() = Status::Checking;
    std::thread::spawn(move || {
        let result = (|| -> Result<Status, String> {
            let resp: serde_json::Value = ureq::get(API)
                .set("User-Agent", concat!("IINA-RS/", env!("CARGO_PKG_VERSION")))
                .set("Accept", "application/vnd.github+json")
                .timeout(std::time::Duration::from_secs(15))
                .call()
                .map_err(|e| e.to_string())?
                .into_json()
                .map_err(|e| e.to_string())?;
            let tag = resp["tag_name"].as_str().ok_or("no tag_name")?.to_string();
            let url = resp["html_url"].as_str().unwrap_or("https://github.com/AbdullZ7/iina-rs/releases").to_string();
            Ok(if is_newer(&tag, env!("CARGO_PKG_VERSION")) {
                Status::Available { version: tag.trim_start_matches('v').to_string(), url }
            } else {
                Status::UpToDate
            })
        })();
        *state.lock().unwrap() = result.unwrap_or_else(Status::Failed);
        done();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.4.1", "0.4.0"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(!is_newer("v0.4.0", "0.4.0"));
        assert!(!is_newer("v0.3.9", "0.4.0"));
        assert!(!is_newer("nightly", "0.4.0"));
    }
}
