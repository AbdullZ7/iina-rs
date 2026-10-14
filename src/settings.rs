//! User preferences (the Preferences window), stored as JSON in the config dir.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Resume from the last position (mpv watch-later).
    pub resume_playback: bool,
    /// When opening one file, add the other media files of its folder to the playlist.
    pub auto_add_folder: bool,
    /// Show thumbnail previews when hovering the seek bar.
    pub thumbnails: bool,
    pub hardware_decoding: bool,
    /// Seconds of inactivity before the on-screen controller hides.
    pub osc_hide_secs: f32,
    /// Show remaining time instead of total duration.
    pub show_remaining: bool,
    /// Single click on the video toggles pause (IINA: off by default).
    pub click_to_pause: bool,
    /// Resize the window to the video size when a file opens.
    pub resize_to_video: bool,
    pub screenshot_dir: String,
    pub volume: f64,
    /// Open files in the running instance instead of a new window.
    pub single_instance: bool,
    /// OpenSubtitles.com API key (https://www.opensubtitles.com/consumers).
    pub opensubtitles_key: String,
    /// Preferred subtitle languages for online search, e.g. "ar,en".
    pub sub_languages: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            resume_playback: true,
            auto_add_folder: true,
            thumbnails: true,
            hardware_decoding: true,
            osc_hide_secs: 2.5,
            show_remaining: false,
            click_to_pause: false,
            resize_to_video: true,
            screenshot_dir: dirs::picture_dir().unwrap_or_default().to_string_lossy().into_owned(),
            volume: 100.0,
            single_instance: true,
            opensubtitles_key: String::new(),
            sub_languages: "ar,en".into(),
        }
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(file(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(file(dir), s);
        }
    }

    /// mpv options derived from these settings.
    pub fn mpv_options(&self) -> Vec<(&'static str, String)> {
        let yn = |b: bool| if b { "yes" } else { "no" }.to_string();
        vec![
            ("resume-playback", yn(self.resume_playback)),
            ("save-position-on-quit", yn(self.resume_playback)),
            ("hwdec", if self.hardware_decoding { "auto-safe".into() } else { "no".into() }),
            ("screenshot-dir", self.screenshot_dir.clone()),
        ]
    }
}
