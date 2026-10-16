//! External subtitles in subfolders, like IINA's matcher (which searches the
//! video's folder and its immediate subfolders). Same-folder files are left to
//! mpv's own `sub-auto`; this adds the common release layouts:
//!
//!   Movie.2010.mkv + Subs/Movie.2010.ar.srt          (name match)
//!   Show.S01E02.mkv + Subs/Show.S01E02/2_Arabic.srt  (folder named after the video)
//!   Movie.mkv + Subs/English.srt                      (only video in the folder)
//!   Show.S01E02.mkv + Subtitles/S01E02.ass            (same episode)

use std::path::{Path, PathBuf};

const SUB_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "sup", "idx", "smi", "lrc"];
const VIDEO_EXTS: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "webm", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "ogv", "3gp", "rmvb"];
const SUB_DIR_NAMES: &[&str] = &["subs", "sub", "subtitles", "subtitle", "srt", "ترجمة", "ترجمات"];

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn lower_stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// "S01E02" style tag (season, episode) anywhere in a name.
fn episode(name: &str) -> Option<(u32, u32)> {
    let b = name.to_lowercase();
    let bytes = b.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b's' {
            continue;
        }
        let rest = &b[i + 1..];
        let s_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if s_digits.is_empty() || s_digits.len() > 2 {
            continue;
        }
        let after = &rest[s_digits.len()..];
        let Some(after) = after.strip_prefix('e') else { continue };
        let e_digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if e_digits.is_empty() || e_digits.len() > 3 {
            continue;
        }
        return Some((s_digits.parse().ok()?, e_digits.parse().ok()?));
    }
    None
}

fn list(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect())
        .unwrap_or_default()
}

/// Subtitle files for `video` found in its subfolders (not its own folder).
pub fn find(video: &Path) -> Vec<PathBuf> {
    let Some(dir) = video.parent() else { return vec![] };
    let stem = lower_stem(video);
    if stem.is_empty() {
        return vec![];
    }
    let ep = episode(&stem);
    let siblings = list(dir);
    let videos_here = siblings.iter().filter(|p| p.is_file() && VIDEO_EXTS.contains(&ext_of(p).as_str())).count();

    let mut out = Vec::new();
    for sub_dir in siblings.iter().filter(|p| p.is_dir()) {
        let dir_name = sub_dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let is_subs_dir = SUB_DIR_NAMES.contains(&dir_name.as_str());
        let named_after_video = dir_name == stem;
        let mut consider = |p: &Path, whole_dir: bool| {
            if !p.is_file() || !SUB_EXTS.contains(&ext_of(p).as_str()) {
                return;
            }
            let name = lower_stem(p);
            let matches = whole_dir
                || name.contains(&stem)
                || (ep.is_some() && episode(&name) == ep)
                || (is_subs_dir && videos_here == 1);
            if matches && !out.iter().any(|q: &PathBuf| q == p) {
                out.push(p.to_path_buf());
            }
        };
        for p in list(sub_dir) {
            if p.is_dir() {
                // Subs/<video name>/… : everything in it belongs to this video.
                let inner = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                if is_subs_dir && (inner == stem || (ep.is_some() && episode(&inner) == ep)) {
                    for q in list(&p) {
                        consider(&q, true);
                    }
                }
            } else {
                consider(&p, named_after_video);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    fn names(v: &[PathBuf], base: &Path) -> Vec<String> {
        v.iter().map(|p| p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/")).collect()
    }

    #[test]
    fn finds_release_layouts() {
        let base = std::env::temp_dir().join(format!("iina-rs-subfind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // Series folder: several episodes, per-episode subtitle folders.
        let show = base.join("show");
        touch(&show.join("Show.S01E02.mkv"));
        touch(&show.join("Show.S01E03.mkv"));
        touch(&show.join("Subs/Show.S01E02/2_Arabic.srt"));
        touch(&show.join("Subs/Show.S01E03/2_Arabic.srt"));
        touch(&show.join("Subtitles/S01E02.ass"));
        touch(&show.join("Subtitles/S01E03.ass"));
        let found = find(&show.join("Show.S01E02.mkv"));
        assert_eq!(names(&found, &show), ["Subs/Show.S01E02/2_Arabic.srt", "Subtitles/S01E02.ass"]);

        // Movie folder: the only video, generic Subs/ folder.
        let movie = base.join("movie");
        touch(&movie.join("Movie.2010.1080p.mkv"));
        touch(&movie.join("Subs/English.srt"));
        touch(&movie.join("Subs/Arabic.srt"));
        touch(&movie.join("Extras/notes.txt"));
        let found = find(&movie.join("Movie.2010.1080p.mkv"));
        assert_eq!(names(&found, &movie), ["Subs/Arabic.srt", "Subs/English.srt"]);

        // Unrelated subtitles in a folder with several videos are ignored.
        touch(&show.join("Subs/Other.Movie.srt"));
        assert!(!names(&find(&show.join("Show.S01E02.mkv")), &show).iter().any(|n| n.contains("Other")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parses_episode_tags() {
        assert_eq!(episode("show.s01e02.1080p"), Some((1, 2)));
        assert_eq!(episode("S3E104"), Some((3, 104)));
        assert_eq!(episode("season one"), None);
    }
}
