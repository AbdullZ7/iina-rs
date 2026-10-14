//! Player state mirrored from observed mpv properties.

use crate::mpv::{Mpv, PropValue, FORMAT_DOUBLE, FORMAT_FLAG, FORMAT_INT64, FORMAT_STRING};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct Track {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    #[serde(default)]
    pub external: bool,
    #[serde(rename = "demux-channel-count")]
    pub channels: Option<i64>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PlaylistItem {
    pub filename: String,
    pub title: Option<String>,
    #[serde(default)]
    pub current: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Chapter {
    #[serde(default)]
    pub title: String,
    pub time: f64,
}

pub struct Player {
    pub idle: bool,
    pub pause: bool,
    pub time_pos: f64,
    pub duration: f64,
    pub volume: f64,
    pub mute: bool,
    pub title: String,
    pub path: String,
    pub speed: f64,
    pub fullscreen: bool,
    pub ontop: bool,
    pub window_scale: f64,
    pub playlist_count: i64,
    pub tracks: Vec<Track>,
    pub playlist: Vec<PlaylistItem>,
    pub chapters: Vec<Chapter>,
    pub chapter: i64,
    pub sub_delay: f64,
    pub audio_delay: f64,
    pub sub_scale: f64,
    pub sub_pos: f64,
    pub sub_visible: bool,
    pub aspect: String,
    pub rotate: i64,
    pub deinterlace: bool,
    pub hwdec_enabled: bool,
    pub hwdec_current: String,
    pub loop_playlist: bool,
    pub vid: String,
    pub aid: String,
    pub sid: String,
    pub secondary_sid: String,
    pub vf: String,
    pub cache_time: f64,
    pub ab_loop_a: Option<f64>,
    pub ab_loop_b: Option<f64>,
    pub brightness: i64,
    pub contrast: i64,
    pub saturation: i64,
    pub gamma: i64,
    pub hue: i64,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            idle: true,
            pause: false,
            time_pos: 0.0,
            duration: 0.0,
            volume: 100.0,
            mute: false,
            title: String::new(),
            path: String::new(),
            speed: 1.0,
            fullscreen: false,
            ontop: false,
            window_scale: 1.0,
            playlist_count: 0,
            tracks: vec![],
            playlist: vec![],
            chapters: vec![],
            chapter: -1,
            sub_delay: 0.0,
            audio_delay: 0.0,
            sub_scale: 1.0,
            sub_pos: 100.0,
            sub_visible: true,
            aspect: String::new(),
            rotate: 0,
            deinterlace: false,
            hwdec_enabled: true,
            hwdec_current: String::new(),
            loop_playlist: false,
            vid: String::new(),
            aid: String::new(),
            sid: String::new(),
            secondary_sid: String::new(),
            vf: String::new(),
            cache_time: 0.0,
            ab_loop_a: None,
            ab_loop_b: None,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            gamma: 0,
            hue: 0,
        }
    }
}

pub const OBSERVED: &[(&str, i32)] = &[
    ("idle-active", FORMAT_FLAG),
    ("pause", FORMAT_FLAG),
    ("time-pos", FORMAT_DOUBLE),
    ("duration", FORMAT_DOUBLE),
    ("volume", FORMAT_DOUBLE),
    ("mute", FORMAT_FLAG),
    ("media-title", FORMAT_STRING),
    ("path", FORMAT_STRING),
    ("speed", FORMAT_DOUBLE),
    ("fullscreen", FORMAT_FLAG),
    ("ontop", FORMAT_FLAG),
    ("window-scale", FORMAT_DOUBLE),
    ("playlist-count", FORMAT_INT64),
    ("track-list", FORMAT_STRING),
    ("playlist", FORMAT_STRING),
    ("chapter-list", FORMAT_STRING),
    ("chapter", FORMAT_INT64),
    ("sub-delay", FORMAT_DOUBLE),
    ("audio-delay", FORMAT_DOUBLE),
    ("sub-scale", FORMAT_DOUBLE),
    ("sub-pos", FORMAT_DOUBLE),
    ("sub-visibility", FORMAT_FLAG),
    ("video-aspect-override", FORMAT_STRING),
    ("video-rotate", FORMAT_INT64),
    ("deinterlace", FORMAT_STRING),
    ("hwdec", FORMAT_STRING),
    ("hwdec-current", FORMAT_STRING),
    ("loop-playlist", FORMAT_STRING),
    ("vid", FORMAT_STRING),
    ("aid", FORMAT_STRING),
    ("sid", FORMAT_STRING),
    ("secondary-sid", FORMAT_STRING),
    ("vf", FORMAT_STRING),
    ("demuxer-cache-time", FORMAT_DOUBLE),
    ("ab-loop-a", FORMAT_STRING),
    ("ab-loop-b", FORMAT_STRING),
    ("brightness", FORMAT_INT64),
    ("contrast", FORMAT_INT64),
    ("saturation", FORMAT_INT64),
    ("gamma", FORMAT_INT64),
    ("hue", FORMAT_INT64),
];

pub fn observe_all(mpv: &Mpv) {
    for (name, fmt) in OBSERVED {
        mpv.observe(name, *fmt);
    }
}

impl Player {
    pub fn apply(&mut self, name: &str, v: &PropValue) {
        use PropValue::*;
        match (name, v) {
            ("idle-active", Flag(b)) => self.idle = *b,
            ("pause", Flag(b)) => self.pause = *b,
            ("time-pos", Double(d)) => self.time_pos = *d,
            ("time-pos", None) => self.time_pos = 0.0,
            ("duration", Double(d)) => self.duration = *d,
            ("duration", None) => self.duration = 0.0,
            ("volume", Double(d)) => self.volume = *d,
            ("mute", Flag(b)) => self.mute = *b,
            ("media-title", Str(s)) => self.title = s.clone(),
            ("path", Str(s)) => self.path = s.clone(),
            ("path", None) => self.path.clear(),
            ("speed", Double(d)) => self.speed = *d,
            ("fullscreen", Flag(b)) => self.fullscreen = *b,
            ("ontop", Flag(b)) => self.ontop = *b,
            ("window-scale", Double(d)) => self.window_scale = *d,
            ("playlist-count", Int(i)) => self.playlist_count = *i,
            ("track-list", Str(s)) => self.tracks = serde_json::from_str(s).unwrap_or_default(),
            ("playlist", Str(s)) => self.playlist = serde_json::from_str(s).unwrap_or_default(),
            ("chapter-list", Str(s)) => self.chapters = serde_json::from_str(s).unwrap_or_default(),
            ("chapter", Int(i)) => self.chapter = *i,
            ("sub-delay", Double(d)) => self.sub_delay = *d,
            ("audio-delay", Double(d)) => self.audio_delay = *d,
            ("sub-scale", Double(d)) => self.sub_scale = *d,
            ("sub-pos", Double(d)) => self.sub_pos = *d,
            ("sub-visibility", Flag(b)) => self.sub_visible = *b,
            ("video-aspect-override", Str(s)) => self.aspect = s.clone(),
            ("video-rotate", Int(i)) => self.rotate = *i,
            ("deinterlace", Str(s)) => self.deinterlace = s == "yes",
            ("hwdec", Str(s)) => self.hwdec_enabled = s != "no",
            ("hwdec-current", Str(s)) => self.hwdec_current = if s == "no" { String::new() } else { s.clone() },
            ("hwdec-current", None) => self.hwdec_current.clear(),
            ("loop-playlist", Str(s)) => self.loop_playlist = s != "no",
            ("vid", Str(s)) => self.vid = s.clone(),
            ("aid", Str(s)) => self.aid = s.clone(),
            ("sid", Str(s)) => self.sid = s.clone(),
            ("secondary-sid", Str(s)) => self.secondary_sid = s.clone(),
            ("vf", Str(s)) => self.vf = s.clone(),
            ("vf", None) => self.vf.clear(),
            ("demuxer-cache-time", Double(d)) => self.cache_time = *d,
            ("demuxer-cache-time", None) => self.cache_time = 0.0,
            ("ab-loop-a", Str(s)) => self.ab_loop_a = s.parse().ok(),
            ("ab-loop-b", Str(s)) => self.ab_loop_b = s.parse().ok(),
            ("brightness", Int(i)) => self.brightness = *i,
            ("contrast", Int(i)) => self.contrast = *i,
            ("saturation", Int(i)) => self.saturation = *i,
            ("gamma", Int(i)) => self.gamma = *i,
            ("hue", Int(i)) => self.hue = *i,
            _ => {}
        }
    }
}
