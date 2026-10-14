//! Playback history (IINA's "Playback History" window), stored as JSON.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub title: String,
    /// Unix seconds of the last time the file was opened.
    pub last_played: u64,
    pub position: f64,
    pub duration: f64,
}

impl Entry {
    pub fn progress(&self) -> f32 {
        if self.duration > 0.0 { (self.position / self.duration).clamp(0.0, 1.0) as f32 } else { 0.0 }
    }
}

pub struct History {
    file: PathBuf,
    pub entries: Vec<Entry>,
}

const MAX_ENTRIES: usize = 500;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl History {
    pub fn load(dir: &Path) -> Self {
        let file = dir.join("history.json");
        let entries = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { file, entries }
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string(&self.entries) {
            let _ = std::fs::write(&self.file, s);
        }
    }

    /// Records that `path` started playing (moves it to the top).
    pub fn opened(&mut self, path: &str, title: &str, duration: f64) {
        let prev = self.entries.iter().position(|e| e.path == path).map(|i| self.entries.remove(i));
        let position = prev.as_ref().map_or(0.0, |e| e.position);
        self.entries.insert(0, Entry { path: path.into(), title: title.into(), last_played: now(), position, duration });
        self.entries.truncate(MAX_ENTRIES);
        self.save();
    }

    /// Updates the last known position of `path`.
    pub fn update_position(&mut self, path: &str, position: f64, duration: f64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
            e.position = position;
            if duration > 0.0 {
                e.duration = duration;
            }
        }
    }

    pub fn remove(&mut self, path: &str) {
        self.entries.retain(|e| e.path != path);
        self.save();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "12 days ago", …
pub fn ago(ts: u64) -> String {
    use crate::i18n::{tl, tlf};
    let d = now().saturating_sub(ts);
    match d {
        0..=59 => tl("just now"),
        60..=3599 => tlf("{} min ago", &[&(d / 60)]),
        3600..=86399 => tlf("{} h ago", &[&(d / 3600)]),
        86400..=172799 => tl("yesterday"),
        _ if d < 86400 * 60 => tlf("{} days ago", &[&(d / 86400)]),
        _ => tlf("{} months ago", &[&(d / (86400 * 30))]),
    }
}
