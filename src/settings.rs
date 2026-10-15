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
    /// Online subtitle provider: "SubDL" or "OpenSubtitles".
    pub sub_provider: String,
    /// SubDL API key (https://subdl.com/panel/api).
    pub subdl_key: String,
    /// OpenSubtitles.com API key (https://www.opensubtitles.com/consumers).
    pub opensubtitles_key: String,
    /// Preferred subtitle languages for online search, e.g. "ar,en".
    pub sub_languages: String,
    /// Interface language: "auto" (system), "en" or "ar".
    pub language: String,
    /// Present through Vulkan in HDR10 when the display is in HDR mode.
    pub hdr_output: bool,
    pub subs: SubStyle,
}

/// Subtitle preferences (PotPlayer-style), applied as mpv options.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct SubStyle {
    /// Preferred subtitle languages, comma separated (mpv `slang`).
    pub languages: String,
    /// Font family ("" = mpv default).
    pub font: String,
    pub size: f64,
    pub bold: bool,
    pub color: [u8; 4],
    pub outline_color: [u8; 4],
    pub outline_size: f64,
    pub shadow_color: [u8; 4],
    pub shadow_offset: f64,
    pub letter_spacing: f64,
    /// "outline-and-shadow", "opaque-box" or "background-box".
    pub border_style: String,
    pub box_color: [u8; 4],
    /// Vertical position, 100 = bottom (mpv `sub-pos`).
    pub position: f64,
    /// Bottom margin in scaled pixels (mpv `sub-margin-y`).
    pub margin: f64,
    pub secondary_position: f64,
    /// Text encoding of external subtitles ("auto", "cp1256", "utf-8", …).
    pub encoding: String,
    /// mpv `sub-ass-override`: "scale" (as authored), "force" (use my style), "strip".
    pub ass_override: String,
    /// mpv `sub-auto`: "exact", "fuzzy" or "all".
    pub autoload: String,
    /// Draw text subtitles in the black bars when possible.
    pub use_margins: bool,
    /// Remove hearing-impaired annotations ([MUSIC], (laughs)…).
    pub remove_sdh: bool,
    /// Subtitle delay steps in seconds (small / medium / large).
    pub sync_steps: [f64; 3],
}

impl Default for SubStyle {
    fn default() -> Self {
        Self {
            languages: "ar,ara,arabic,en,eng".into(),
            font: String::new(),
            size: 38.0,
            bold: false,
            color: [255, 255, 255, 255],
            outline_color: [0, 0, 0, 255],
            outline_size: 3.0,
            shadow_color: [0, 0, 0, 255],
            shadow_offset: 2.0,
            letter_spacing: 0.0,
            border_style: "outline-and-shadow".into(),
            box_color: [0, 0, 0, 160],
            position: 100.0,
            margin: 22.0,
            secondary_position: 5.0,
            encoding: "auto".into(),
            ass_override: "scale".into(),
            autoload: "fuzzy".into(),
            use_margins: false,
            remove_sdh: false,
            sync_steps: [0.5, 5.0, 50.0],
        }
    }
}

/// mpv color string: #AARRGGBB.
fn mpv_color(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c[3], c[0], c[1], c[2])
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
            sub_provider: "SubDL".into(),
            subdl_key: String::new(),
            opensubtitles_key: String::new(),
            sub_languages: "ar,en".into(),
            language: "auto".into(),
            hdr_output: true,
            subs: SubStyle::default(),
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

    pub fn subtitle_key(&self, p: crate::subtitles::Provider) -> &str {
        match p {
            crate::subtitles::Provider::SubDL => self.subdl_key.trim(),
            crate::subtitles::Provider::OpenSubtitles => self.opensubtitles_key.trim(),
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
            ("slang", self.subs.languages.replace(' ', "")),
            ("sub-font", if self.subs.font.is_empty() { "sans-serif".into() } else { self.subs.font.clone() }),
            ("sub-font-size", format!("{:.0}", self.subs.size)),
            ("sub-bold", yn(self.subs.bold)),
            ("sub-color", mpv_color(self.subs.color)),
            ("sub-outline-color", mpv_color(self.subs.outline_color)),
            ("sub-outline-size", format!("{:.1}", self.subs.outline_size)),
            ("sub-shadow-color", mpv_color(self.subs.shadow_color)),
            ("sub-shadow-offset", format!("{:.1}", self.subs.shadow_offset)),
            ("sub-spacing", format!("{:.1}", self.subs.letter_spacing)),
            ("sub-border-style", self.subs.border_style.clone()),
            ("sub-back-color", mpv_color(self.subs.box_color)),
            ("sub-pos", format!("{:.0}", self.subs.position)),
            ("secondary-sub-pos", format!("{:.0}", self.subs.secondary_position)),
            ("sub-codepage", self.subs.encoding.clone()),
            ("sub-ass-override", self.subs.ass_override.clone()),
            ("sub-auto", self.subs.autoload.clone()),
            ("sub-use-margins", yn(self.subs.use_margins)),
            ("sub-ass-force-margins", yn(self.subs.use_margins)),
            ("sub-filter-sdh", yn(self.subs.remove_sdh)),
        ]
    }
}
