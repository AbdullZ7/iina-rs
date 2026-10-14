//! Seek-bar preview thumbnails, like IINA's. A second, headless mpv instance
//! seeks to evenly spaced keyframes and renders each one into memory through
//! the software render API, on a background thread.

use crate::mpv::{Event, Mpv};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Number of thumbnails per file (IINA uses ~100).
const COUNT: usize = 100;
/// Thumbnail width in pixels; height follows the video's aspect ratio.
const WIDTH: i32 = 320;

#[derive(Default)]
pub struct Thumbnails {
    pub path: String,
    /// `(time, image)` in increasing time order.
    pub images: Vec<(f64, egui::ColorImage)>,
    pub done: bool,
}

impl Thumbnails {
    /// Index of the thumbnail closest to `t`, if any are ready.
    pub fn nearest(&self, t: f64) -> Option<usize> {
        if self.images.is_empty() {
            return None;
        }
        let i = self.images.partition_point(|(ts, _)| *ts < t);
        let best = match (i.checked_sub(1), self.images.get(i)) {
            (Some(a), Some(_)) if (t - self.images[a].0).abs() <= (self.images[i].0 - t).abs() => a,
            (Some(a), None) => a,
            _ => i,
        };
        Some(best)
    }
}

pub struct Generator {
    pub shared: Arc<Mutex<Thumbnails>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Generator {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Starts generating thumbnails for `path`; `wake` is called whenever new
/// images are available. Dropping the returned generator cancels the work.
pub fn start(path: String, wake: impl Fn() + Send + 'static) -> Generator {
    let shared = Arc::new(Mutex::new(Thumbnails { path: path.clone(), ..Default::default() }));
    let cancel = Arc::new(AtomicBool::new(false));
    let (s, c) = (shared.clone(), cancel.clone());
    std::thread::Builder::new()
        .name("thumbnails".into())
        .spawn(move || {
            if let Err(e) = run(&path, &s, &c, &wake) {
                eprintln!("thumbnails: {e}");
            }
            s.lock().unwrap().done = true;
            wake();
        })
        .expect("spawn thumbnail thread");
    Generator { shared, cancel }
}

fn wait_for(mpv: &Mpv, cancel: &AtomicBool, timeout: Duration, want: impl Fn(&Event) -> bool) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("timeout".into());
        }
        match mpv.wait_event(left.as_secs_f64().min(0.25)) {
            Some(Event::Shutdown) => return Err("mpv shut down".into()),
            Some(Event::EndFile) => return Err("file ended".into()),
            Some(ev) if want(&ev) => return Ok(()),
            _ => {}
        }
    }
}

fn run(path: &str, shared: &Mutex<Thumbnails>, cancel: &AtomicBool, wake: &dyn Fn()) -> Result<(), String> {
    let mut mpv = Mpv::new()?;
    for (k, v) in [
        ("vo", "libmpv"),
        ("ao", "null"),
        ("aid", "no"),
        ("sid", "no"),
        ("pause", "yes"),
        ("hr-seek", "no"),
        ("hwdec", "auto-copy-safe"),
        ("vd-lavc-skiploopfilter", "all"),
        ("vd-lavc-fast", "yes"),
        ("cache", "no"),
        ("ytdl", "no"),
        ("config", "no"),
        ("load-scripts", "no"),
        ("resume-playback", "no"),
        ("sub-auto", "no"),
        ("audio-file-auto", "no"),
        ("keep-open", "always"),
        ("terminal", "no"),
        ("msg-level", "all=no"),
    ] {
        mpv.set_option(k, v);
    }
    mpv.init()?;
    let (tx, rx) = mpsc::channel::<()>();
    let tx = Mutex::new(tx);
    mpv.create_sw_render_context(move || {
        let _ = tx.lock().unwrap().send(());
    })?;

    mpv.command(&["loadfile", path]);
    wait_for(&mpv, cancel, Duration::from_secs(20), |e| matches!(e, Event::FileLoaded))?;
    let duration = mpv.get_string("duration").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let (vw, vh) = (
        mpv.get_string("width").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
        mpv.get_string("height").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
    );
    if duration <= 1.0 || vw <= 0.0 || vh <= 0.0 {
        return Err("no seekable video".into());
    }
    // Respect the display aspect ratio (anamorphic video) when available.
    let aspect = mpv.get_string("video-params/aspect").and_then(|s| s.parse::<f64>().ok()).unwrap_or(vw / vh);
    let (w, h) = (WIDTH, ((WIDTH as f64 / aspect).round() as i32).max(2));
    let stride = ((w as usize * 4) + 63) & !63;
    let mut buf = vec![0u8; stride * h as usize];

    let started = Instant::now();
    for (n, i) in coarse_to_fine(COUNT).into_iter().enumerate() {
        let t = duration * (i as f64 + 0.5) / COUNT as f64;
        while rx.try_recv().is_ok() {}
        mpv.command(&["seek", &format!("{t:.3}"), "absolute+keyframes"]);
        if wait_for(&mpv, cancel, Duration::from_secs(10), |e| matches!(e, Event::PlaybackRestart)).is_err() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            continue;
        }
        // Wait until mpv has a decoded frame ready for this position.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = false;
        while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
            if mpv.render_update() {
                got = mpv.render_sw(w, h, stride, &mut buf);
                break;
            }
            let _ = rx.recv_timeout(Duration::from_millis(100));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !got {
            continue;
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in buf.chunks(stride).take(h as usize) {
            for px in row[..w as usize * 4].chunks(4) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        let actual = mpv.get_string("time-pos").and_then(|s| s.parse::<f64>().ok()).unwrap_or(t);
        let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        {
            let mut s = shared.lock().unwrap();
            let pos = s.images.partition_point(|(ts, _)| *ts < actual);
            // Keyframe seeks can land on the same frame twice; skip duplicates.
            if !s.images.get(pos).is_some_and(|(ts, _)| (*ts - actual).abs() < 1e-3) {
                s.images.insert(pos, (actual, img));
            }
        }
        if n % 5 == 4 {
            wake();
        }
        if n % 10 == 9 && std::env::var_os("IINA_RS_DEBUG").is_some() {
            eprintln!("[thumbs] {} done after {:.1}s", n + 1, started.elapsed().as_secs_f64());
        }
    }
    if std::env::var_os("IINA_RS_DEBUG").is_some() {
        eprintln!("[thumbs] {} images in {:.1}s", shared.lock().unwrap().images.len(), started.elapsed().as_secs_f64());
    }
    Ok(())
}

/// Visit order that covers the whole timeline coarsely first, then refines
/// (0, 16, 32, …, 8, 24, …, 4, 12, …), so previews are useful immediately.
fn coarse_to_fine(n: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut stride = n.next_power_of_two();
    while stride >= 1 {
        for i in (0..n).step_by(stride) {
            if !seen[i] {
                seen[i] = true;
                order.push(i);
            }
        }
        stride /= 2;
    }
    order
}

#[cfg(test)]
mod tests {
    #[test]
    fn coarse_to_fine_visits_all_once() {
        let o = super::coarse_to_fine(100);
        assert_eq!(o.len(), 100);
        let mut s = o.clone();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), 100);
        assert_eq!(&o[..2], &[0, 64]);
    }
}
