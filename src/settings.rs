//! User preferences (the Preferences window), stored as JSON in the config dir.
//! Defaults follow IINA's (Preference.swift) unless noted.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    // ---------------------------------------------------------------- general
    /// Resume from the last position (mpv watch-later).
    pub resume_playback: bool,
    /// When opening one file, add the other media files of its folder to the playlist.
    pub auto_add_folder: bool,
    /// Resize the window to the video size when a file opens.
    pub resize_to_video: bool,
    /// Scale used when resizing to the video (IINA: 1.0×).
    pub resize_scale: f64,
    /// Open files in the running instance instead of a new window.
    pub single_instance: bool,
    pub pause_on_open: bool,
    pub fullscreen_on_open: bool,
    /// Keep the display awake while video plays (IINA: on).
    pub prevent_sleep: bool,
    /// Pause when the window is minimized, resume when it's restored.
    pub pause_minimized: bool,
    /// Black out the other monitors while in fullscreen (IINA: off).
    pub blackout_monitors: bool,
    /// Switch to music mode for audio files (IINA: on).
    pub auto_music_mode: bool,
    /// Check GitHub for a newer release once a day.
    pub check_updates: bool,
    /// Unix time of the last update check.
    pub last_update_check: u64,
    /// Interface language: "auto" (system), "en" or "ar".
    pub language: String,

    // --------------------------------------------------------------------- UI
    /// Show thumbnail previews when hovering the seek bar.
    pub thumbnails: bool,
    /// Thumbnail preview width in points (0 = automatic).
    pub thumbnail_width: f32,
    /// Auto-hide the on-screen controller (IINA: on).
    pub osc_autohide: bool,
    /// Controller layout: "floating", "top" or "bottom" (IINA's OSC position).
    pub osc_position: String,
    /// Seconds of inactivity before the on-screen controller hides.
    pub osc_hide_secs: f32,
    /// Show remaining time instead of total duration.
    pub show_remaining: bool,
    pub osd_enabled: bool,
    /// Seconds an OSD message stays before fading (IINA: 1).
    pub osd_timeout: f32,
    pub osd_text_size: f32,

    // ---------------------------------------------------------- video & audio
    pub hardware_decoding: bool,
    /// mpv `hwdec` when hardware decoding is on: "auto-safe", "auto", "auto-copy".
    pub hwdec_mode: String,
    /// Present through Vulkan in HDR10 when the display is in HDR mode.
    pub hdr_output: bool,
    /// Display peak luminance for HDR output (0 = as reported by the OS).
    pub hdr_peak_nits: f32,
    /// Volume used at startup; updated with the last volume when `remember_volume`.
    pub volume: f64,
    pub remember_volume: bool,
    /// mpv `volume-max` (IINA: 100).
    pub volume_max: f64,
    /// Preferred audio languages (mpv `alang`).
    pub audio_languages: String,
    /// mpv `audio-device` ("auto" or a device name from `audio-device-list`).
    pub audio_device: String,
    /// Codecs passed through to an external decoder (mpv `audio-spdif`).
    pub spdif: Vec<String>,
    /// mpv `replaygain`: "no", "track" or "album".
    pub replaygain: String,
    /// mpv `gapless-audio`: "no", "yes" or "weak".
    pub gapless: String,

    // ------------------------------------------------------------- screenshot
    pub screenshot_dir: String,
    /// "png", "jpg", "webp", "jxl", "avif".
    pub screenshot_format: String,
    pub screenshot_template: String,
    pub screenshot_with_subs: bool,
    /// Also copy screenshots to the clipboard.
    pub screenshot_clipboard: bool,
    /// Show a small preview of the screenshot just taken.
    pub screenshot_preview: bool,
    /// User filters from the Filters window (IINA's saved filters).
    pub saved_filters: Vec<SavedFilter>,

    // ---------------------------------------------------------------- control
    /// What the controller's ⏪ ⏩ buttons do: "speed" (IINA), "playlist" or "seek".
    pub arrow_action: String,
    /// Seek step (seconds) for the arrow buttons in "seek" mode.
    pub arrow_seek_step: f64,
    /// Mouse actions: "none", "pause", "fullscreen", "hide-osc", "pip", "menu".
    pub single_click: String,
    pub double_click: String,
    pub right_click: String,
    pub middle_click: String,
    /// Scroll actions: "volume", "seek", "speed", "none".
    pub scroll_vertical: String,
    pub scroll_horizontal: String,
    /// Kept for settings written by older versions (now `single_click`).
    #[serde(skip_serializing)]
    pub click_to_pause: bool,

    // ---------------------------------------------------------------- network
    pub cache: bool,
    /// mpv `demuxer-max-bytes` in MiB (IINA: 150).
    pub cache_mib: f64,
    /// mpv `cache-secs` (IINA: 36000).
    pub cache_secs: f64,
    pub user_agent: String,
    pub http_proxy: String,
    pub ytdl: bool,
    pub ytdl_raw_options: String,
    /// Path to yt-dlp ("" = search PATH).
    pub ytdl_path: String,

    // ------------------------------------------------------- online subtitles
    /// Online subtitle provider: "SubDL" or "OpenSubtitles".
    pub sub_provider: String,
    /// SubDL API key (https://subdl.com/panel/api).
    pub subdl_key: String,
    /// OpenSubtitles.com API key (https://www.opensubtitles.com/consumers).
    pub opensubtitles_key: String,
    /// Preferred subtitle languages for online search, e.g. "ar,en".
    pub sub_languages: String,

    pub subs: SubStyle,

    // --------------------------------------------------------------- advanced
    /// Extra mpv options, one `name=value` per line (applied last).
    pub mpv_options: String,

    /// Last Picture-in-Picture window (x, y, width, height in physical pixels).
    pub pip_rect: Option<[i32; 4]>,
}

/// A saved video/audio filter (mpv `vf`/`af` syntax), toggled from the
/// Filters window.
#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct SavedFilter {
    pub name: String,
    /// "vf" or "af".
    pub kind: String,
    pub value: String,
}

impl SavedFilter {
    /// mpv label used while the filter is active.
    pub fn label(&self) -> String {
        let mut h: u32 = 0x811c9dc5;
        for b in self.kind.bytes().chain(self.value.bytes()) {
            h = (h ^ b as u32).wrapping_mul(0x01000193);
        }
        format!("iina-user-{h:08x}")
    }
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
    pub italic: bool,
    pub color: [u8; 4],
    pub outline_color: [u8; 4],
    pub outline_size: f64,
    pub shadow_color: [u8; 4],
    pub shadow_offset: f64,
    pub blur: f64,
    pub letter_spacing: f64,
    /// "outline-and-shadow", "opaque-box" or "background-box".
    pub border_style: String,
    pub box_color: [u8; 4],
    /// Vertical position, 100 = bottom (mpv `sub-pos`).
    pub position: f64,
    /// Bottom margin in scaled pixels (mpv `sub-margin-y`).
    pub margin: f64,
    /// Side margin (mpv `sub-margin-x`, IINA: 25).
    pub margin_x: f64,
    pub secondary_position: f64,
    /// Scale subtitles with the window size (IINA: on).
    pub scale_by_window: bool,
    /// Text encoding of external subtitles ("auto", "cp1256", "utf-8", …).
    pub encoding: String,
    /// mpv `sub-ass-override`: "scale" (as authored), "force" (use my style), "strip".
    pub ass_override: String,
    /// mpv `sub-auto`: "exact", "fuzzy" or "all".
    pub autoload: String,
    /// Also look for matching subtitles in subfolders (Subs/, Subtitles/…), like IINA.
    pub search_subfolders: bool,
    /// Draw text subtitles in the black bars when possible (IINA: on).
    pub use_margins: bool,
    /// Remove hearing-impaired annotations ([MUSIC], (laughs)…).
    pub remove_sdh: bool,
    /// Subtitle delay steps in seconds (small / medium / large).
    pub sync_steps: [f64; 3],
}

impl Default for SubStyle {
    fn default() -> Self {
        Self {
            // The user's choices (Arabic first, PotPlayer-like outline and shadow).
            languages: "ar,ara,arabic,en,eng".into(),
            font: String::new(),
            size: 55.0,
            bold: false,
            italic: false,
            color: [255, 255, 255, 255],
            outline_color: [0, 0, 0, 255],
            outline_size: 3.0,
            shadow_color: [0, 0, 0, 255],
            shadow_offset: 2.0,
            blur: 0.0,
            letter_spacing: 0.0,
            border_style: "outline-and-shadow".into(),
            box_color: [0, 0, 0, 160],
            position: 100.0,
            margin: 22.0,
            margin_x: 25.0,
            secondary_position: 5.0,
            scale_by_window: true,
            encoding: "auto".into(),
            ass_override: "scale".into(),
            autoload: "fuzzy".into(),
            search_subfolders: true,
            use_margins: true,
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
            resize_to_video: true,
            resize_scale: 1.0,
            single_instance: true,
            pause_on_open: false,
            fullscreen_on_open: false,
            prevent_sleep: true,
            pause_minimized: false,
            blackout_monitors: false,
            auto_music_mode: true,
            check_updates: true,
            last_update_check: 0,
            language: "auto".into(),

            thumbnails: true,
            thumbnail_width: 0.0,
            osc_autohide: true,
            osc_position: "floating".into(),
            osc_hide_secs: 2.5,
            show_remaining: false,
            osd_enabled: true,
            osd_timeout: 1.0,
            osd_text_size: 20.0,

            hardware_decoding: true,
            hwdec_mode: "auto-safe".into(),
            hdr_output: true,
            hdr_peak_nits: 0.0,
            volume: 100.0,
            remember_volume: true,
            volume_max: 100.0,
            audio_languages: String::new(),
            audio_device: "auto".into(),
            spdif: vec![],
            replaygain: "no".into(),
            gapless: "weak".into(),

            screenshot_dir: dirs::picture_dir()
                .map(|p| p.join("Screenshots"))
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            screenshot_format: "png".into(),
            screenshot_template: "%F-%n".into(),
            screenshot_with_subs: true,
            screenshot_clipboard: false,
            screenshot_preview: true,
            saved_filters: Vec::new(),

            arrow_action: "speed".into(),
            arrow_seek_step: 10.0,
            single_click: "hide-osc".into(),
            double_click: "fullscreen".into(),
            // IINA pauses on right click; without a menu bar, the context menu
            // is the only way to reach many commands on Windows/Linux.
            right_click: "menu".into(),
            middle_click: "none".into(),
            scroll_vertical: "volume".into(),
            scroll_horizontal: "seek".into(),
            click_to_pause: false,

            cache: true,
            cache_mib: 150.0,
            cache_secs: 36000.0,
            user_agent: String::new(),
            http_proxy: String::new(),
            ytdl: true,
            ytdl_raw_options: String::new(),
            ytdl_path: String::new(),

            sub_provider: "SubDL".into(),
            subdl_key: String::new(),
            opensubtitles_key: String::new(),
            sub_languages: "ar,en".into(),
            subs: SubStyle::default(),

            mpv_options: String::new(),
            pip_rect: None,
        }
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        let mut s: Settings = std::fs::read_to_string(file(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // Migrate the old "single click toggles pause" switch.
        if s.click_to_pause {
            s.single_click = "pause".into();
            s.click_to_pause = false;
        }
        s
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

    /// The user's extra mpv options (`name=value` per line, `#` comments).
    pub fn extra_mpv_options(&self) -> Vec<(String, String)> {
        self.mpv_options
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let l = l.trim_start_matches("--");
                match l.split_once('=') {
                    Some((k, v)) => Some((k.trim().to_string(), v.trim().to_string())),
                    None => Some((l.to_string(), "yes".into())),
                }
            })
            .collect()
    }

    /// mpv options derived from these settings.
    pub fn mpv_options(&self) -> Vec<(&'static str, String)> {
        let yn = |b: bool| if b { "yes" } else { "no" }.to_string();
        let mut v = vec![
            ("resume-playback", yn(self.resume_playback)),
            ("save-position-on-quit", yn(self.resume_playback)),
            ("hwdec", if self.hardware_decoding { self.hwdec_mode.clone() } else { "no".into() }),
            ("volume-max", format!("{:.0}", self.volume_max.max(100.0))),
            ("alang", self.audio_languages.replace(' ', "")),
            ("audio-device", if self.audio_device.is_empty() { "auto".into() } else { self.audio_device.clone() }),
            ("audio-spdif", self.spdif.join(",")),
            ("replaygain", self.replaygain.clone()),
            ("gapless-audio", self.gapless.clone()),
            ("screenshot-dir", self.screenshot_dir.clone()),
            ("screenshot-format", self.screenshot_format.clone()),
            ("screenshot-template", self.screenshot_template.clone()),
            ("cache", yn(self.cache)),
            ("demuxer-max-bytes", format!("{:.0}MiB", self.cache_mib.max(1.0))),
            ("cache-secs", format!("{:.0}", self.cache_secs.max(1.0))),
            ("cache-pause-initial", "no".into()),
            ("cache-pause-wait", "1".into()),
            ("rtsp-transport", "tcp".into()),
            ("http-proxy", self.http_proxy.clone()),
            ("ytdl", yn(self.ytdl)),
            ("ytdl-raw-options", self.ytdl_raw_options.clone()),
            ("slang", self.subs.languages.replace(' ', "")),
            ("sub-font", if self.subs.font.is_empty() { "sans-serif".into() } else { self.subs.font.clone() }),
            ("sub-font-size", format!("{:.0}", self.subs.size)),
            ("sub-bold", yn(self.subs.bold)),
            ("sub-italic", yn(self.subs.italic)),
            ("sub-color", mpv_color(self.subs.color)),
            ("sub-outline-color", mpv_color(self.subs.outline_color)),
            ("sub-outline-size", format!("{:.1}", self.subs.outline_size)),
            ("sub-shadow-color", mpv_color(self.subs.shadow_color)),
            ("sub-shadow-offset", format!("{:.1}", self.subs.shadow_offset)),
            ("sub-blur", format!("{:.1}", self.subs.blur)),
            ("sub-spacing", format!("{:.1}", self.subs.letter_spacing)),
            ("sub-border-style", self.subs.border_style.clone()),
            ("sub-back-color", mpv_color(self.subs.box_color)),
            ("sub-pos", format!("{:.0}", self.subs.position)),
            ("sub-margin-x", format!("{:.0}", self.subs.margin_x)),
            ("secondary-sub-pos", format!("{:.0}", self.subs.secondary_position)),
            ("sub-scale-by-window", yn(self.subs.scale_by_window)),
            ("sub-codepage", self.subs.encoding.clone()),
            ("sub-ass-override", self.subs.ass_override.clone()),
            ("secondary-sub-ass-override", self.subs.ass_override.clone()),
            ("sub-auto", self.subs.autoload.clone()),
            ("sub-use-margins", yn(self.subs.use_margins)),
            ("sub-ass-force-margins", yn(self.subs.use_margins)),
            ("sub-filter-sdh", yn(self.subs.remove_sdh)),
        ];
        if !self.user_agent.trim().is_empty() {
            v.push(("user-agent", self.user_agent.trim().to_string()));
        }
        if !self.ytdl_path.trim().is_empty() {
            v.push(("script-opts-append", format!("ytdl_hook-ytdl_path={}", self.ytdl_path.trim())));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extra_options() {
        let s = Settings { mpv_options: "# comment\n--deband=yes\nscale = ewa_lanczossharp\n\nloop-file".into(), ..Default::default() };
        assert_eq!(
            s.extra_mpv_options(),
            vec![
                ("deband".to_string(), "yes".to_string()),
                ("scale".to_string(), "ewa_lanczossharp".to_string()),
                ("loop-file".to_string(), "yes".to_string())
            ]
        );
    }

    #[test]
    fn migrates_click_to_pause() {
        let dir = std::env::temp_dir().join("iina-rs-settings-test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("settings.json"), r#"{"click_to_pause": true}"#).unwrap();
        assert_eq!(Settings::load(&dir).single_click, "pause");
    }
}
