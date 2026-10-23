//! Live subtitle sample for the font dialog, drawn by libass itself (in a
//! hidden mpv instance) so it looks exactly like the subtitle on the video:
//! OpenType Arabic shaping, outline, shadow, blur and box included.

use crate::mpv::{Event, Mpv};
use crate::settings::{Settings, SubStyle};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Frame mpv renders (subtitle sizes are relative to a 720-line video).
const W: i32 = 1280;
const H: i32 = 720;
/// The part of the frame shown: the band around the two lines, at about
/// the size they have on a full-HD screen.
const CROP_TOP: i32 = 500;
const CROP_X: i32 = 240;

const SAMPLE: &str = "1\n00:00:00,000 --> 99:00:00,000\nAaBbYyZz 0123\nأبجد هوز حطي كلمن\n";

#[derive(Default)]
pub struct Output {
    pub image: Option<egui::ColorImage>,
    /// Bumped on every new image.
    pub generation: u64,
    pub error: Option<String>,
}

pub struct Preview {
    tx: Sender<Option<SubStyle>>,
    pub out: Arc<Mutex<Output>>,
}

impl Preview {
    pub fn start(fonts_dir: PathBuf, wake: impl Fn() + Send + 'static) -> Preview {
        let (tx, rx) = mpsc::channel();
        let out = Arc::new(Mutex::new(Output::default()));
        let o = out.clone();
        std::thread::Builder::new()
            .name("sub-preview".into())
            .spawn(move || {
                if let Err(e) = run(rx, &o, &fonts_dir, &wake) {
                    o.lock().unwrap().error = Some(e);
                    wake();
                }
            })
            .expect("spawn sub-preview thread");
        Preview { tx, out }
    }

    /// Renders the sample with this style (the latest request wins).
    pub fn show(&self, style: &SubStyle) {
        let _ = self.tx.send(Some(style.clone()));
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.tx.send(None);
    }
}

fn run(rx: Receiver<Option<SubStyle>>, out: &Mutex<Output>, fonts_dir: &std::path::Path, wake: &dyn Fn()) -> Result<(), String> {
    let srt = std::env::temp_dir().join(format!("iina-rs-sample-{}.srt", std::process::id()));
    std::fs::write(&srt, SAMPLE).map_err(|e| e.to_string())?;
    let mut mpv = Mpv::new()?;
    for (k, v) in [
        ("vo", "libmpv"),
        ("ao", "null"),
        ("pause", "yes"),
        ("keep-open", "always"),
        ("config", "no"),
        ("load-scripts", "no"),
        ("resume-playback", "no"),
        ("save-position-on-quit", "no"),
        ("sub-auto", "no"),
        ("audio-file-auto", "no"),
        ("ytdl", "no"),
        ("terminal", "no"),
        ("msg-level", "all=no"),
        ("sub-codepage", "utf-8"),
        ("sub-scale-by-window", "no"),
        ("sub-use-margins", "no"),
        ("sub-pos", "100"),
        ("sub-margin-y", "40"),
    ] {
        mpv.set_option(k, v);
    }
    mpv.set_option("sub-fonts-dir", &fonts_dir.to_string_lossy());
    mpv.init()?;
    let (utx, urx) = mpsc::channel::<()>();
    let utx = Mutex::new(utx);
    mpv.create_sw_render_context(move || {
        let _ = utx.lock().unwrap().send(());
    })?;
    // A two-tone backdrop, so outline and shadow read as on a picture.
    mpv.command(&["loadfile", &format!("av://lavfi:color=c=0x465260:s={W}x{H}:r=1,drawbox=y=ih*0.84:w=iw:h=ih*0.16:c=0x282c32:t=fill")]);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            return Err("preview video did not load".into());
        }
        if let Some(Event::FileLoaded) = mpv.wait_event(0.2) {
            break;
        }
    }
    mpv.command(&["sub-add", &srt.to_string_lossy(), "select"]);

    let stride = (W as usize * 4 + 63) & !63;
    let mut buf = vec![0u8; stride * H as usize];
    let mut pending: Option<SubStyle> = None;
    loop {
        // Take the newest request; stop when the dialog is gone.
        let next = if pending.is_some() { rx.try_recv().ok() } else { rx.recv().ok() };
        match next {
            Some(None) => break,
            None if pending.is_none() => break,
            Some(Some(s)) => {
                pending = Some(s);
                continue;
            }
            None => {}
        }
        let Some(style) = pending.take() else { continue };
        let settings = Settings { subs: style, ..Default::default() };
        for (k, v) in settings.mpv_options() {
            if k.starts_with("sub-") && !matches!(k, "sub-auto" | "sub-codepage" | "sub-pos" | "sub-scale-by-window" | "sub-use-margins" | "sub-ass-force-margins" | "sub-filter-sdh") {
                mpv.set_string(k, &v);
            }
        }
        while urx.try_recv().is_ok() {}
        // Wait for mpv to redraw with the new style, then grab the frame.
        let until = Instant::now() + Duration::from_secs(2);
        let mut got = false;
        while Instant::now() < until {
            while mpv.wait_event(0.0).is_some() {}
            if mpv.render_update() && mpv.render_sw(W, H, stride, &mut buf) {
                got = true;
                // A style change can take a second pass to reach the frame.
                if urx.recv_timeout(Duration::from_millis(60)).is_ok() {
                    continue;
                }
                break;
            }
            let _ = urx.recv_timeout(Duration::from_millis(50));
        }
        if !got {
            continue;
        }
        let (h, w) = ((H - CROP_TOP) as usize, (W - 2 * CROP_X) as usize);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in buf.chunks(stride).skip(CROP_TOP as usize).take(h) {
            for px in row[CROP_X as usize * 4..(CROP_X as usize + w) * 4].chunks(4) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        let mut o = out.lock().unwrap();
        o.image = Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba));
        o.generation += 1;
        drop(o);
        wake();
    }
    let _ = std::fs::remove_file(&srt);
    Ok(())
}
