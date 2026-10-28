//! Remembers, per GPU / driver / display state, whether the enhanced output
//! (HDR through Direct3D 11 or Vulkan, RTX Video) worked, so a window that
//! stays black isn't repeated on every start.
//!
//! A new combination starts "on trial": each start counts as unconfirmed
//! until the program has run 60 s; a run closed after 10-60 s counts as
//! neither. Two unconfirmed starts in a row (closed or crashed within 10 s)
//! fall back to plain OpenGL. Ctrl+F12 ("safe-output") does that at once.
//! A driver update or another GPU gives a new combination, tried again.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    /// Ran 60 s with the enhanced output.
    pub ok: bool,
    /// Starts not confirmed yet.
    pub pending: u32,
    /// The user chose the safe output (Ctrl+F12).
    pub safe: bool,
}

/// Why the enhanced output is skipped for this combination.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skip {
    /// Ctrl+F12.
    Chosen,
    /// Two quick exits or crashes in a row.
    Failed,
}

pub struct RenderMemory {
    path: PathBuf,
    key: String,
    map: BTreeMap<String, Entry>,
}

impl RenderMemory {
    pub fn load(dir: &Path, key: &str) -> Self {
        let path = dir.join("render-ok.json");
        let map = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { path, key: key.to_string(), map }
    }

    pub fn entry(&self) -> Entry {
        self.map.get(&self.key).copied().unwrap_or_default()
    }

    pub fn skip(&self) -> Option<Skip> {
        let e = self.entry();
        if e.safe {
            Some(Skip::Chosen)
        } else if !e.ok && e.pending >= 2 {
            Some(Skip::Failed)
        } else {
            None
        }
    }

    fn update(&mut self, f: impl FnOnce(&mut Entry)) {
        f(self.map.entry(self.key.clone()).or_default());
        if let Ok(text) = serde_json::to_string_pretty(&self.map) {
            let _ = std::fs::write(&self.path, text);
        }
    }

    /// The enhanced output is starting; true while it is on trial.
    pub fn starting(&mut self) -> bool {
        if self.entry().ok {
            return false;
        }
        self.update(|e| e.pending += 1);
        true
    }

    /// Ran long enough: the picture was presumably fine.
    pub fn confirm(&mut self) {
        self.update(|e| {
            e.ok = true;
            e.pending = 0;
        });
    }

    /// Closed normally after 10-60 s: neither a success nor a failure.
    pub fn neutral_exit(&mut self) {
        self.update(|e| e.pending = e.pending.saturating_sub(1));
    }

    pub fn choose_safe(&mut self) {
        self.update(|e| {
            e.safe = true;
            e.ok = false;
        });
    }

    /// Forget this combination: the enhanced output is tried again.
    pub fn reset(&mut self) {
        self.map.remove(&self.key);
        if let Ok(text) = serde_json::to_string_pretty(&self.map) {
            let _ = std::fs::write(&self.path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("iina-rs-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn two_quick_exits_fall_back() {
        let dir = temp_dir("rm1");
        let mut m = RenderMemory::load(&dir, "gpu|hdr=true");
        assert!(m.starting());
        assert_eq!(m.skip(), None);
        // crashed / closed within 10 s: nothing more recorded
        let mut m = RenderMemory::load(&dir, "gpu|hdr=true");
        assert!(m.starting());
        let m = RenderMemory::load(&dir, "gpu|hdr=true");
        assert_eq!(m.skip(), Some(Skip::Failed));
        // another driver: tried again
        assert_eq!(RenderMemory::load(&dir, "gpu2|hdr=true").skip(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confirmed_and_neutral_runs() {
        let dir = temp_dir("rm2");
        let mut m = RenderMemory::load(&dir, "k");
        m.starting();
        m.neutral_exit(); // closed after 30 s
        m.starting();
        m.neutral_exit();
        assert_eq!(RenderMemory::load(&dir, "k").skip(), None);
        let mut m = RenderMemory::load(&dir, "k");
        m.starting();
        m.confirm();
        // once confirmed, starts are no longer on trial
        let mut m = RenderMemory::load(&dir, "k");
        assert!(!m.starting());
        assert_eq!(m.skip(), None);
        m.choose_safe();
        assert_eq!(RenderMemory::load(&dir, "k").skip(), Some(Skip::Chosen));
        let mut m = RenderMemory::load(&dir, "k");
        m.reset();
        assert_eq!(RenderMemory::load(&dir, "k").skip(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
