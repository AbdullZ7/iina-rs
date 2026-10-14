#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! IINA-RS — an IINA-inspired media player for Windows and Linux.
//! Video is rendered by libmpv's OpenGL render API into the window's default
//! framebuffer; the egui overlay (controller, sidebar, menus) is drawn on top.

mod assoc;
mod history;
mod instance;
mod keys;
mod mpv;
mod settings;
mod state;
mod subtitles;
mod thumbs;
mod ui;

use glutin::context::{NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use keys::{Binding, Bindings};
use mpv::{Event, Mpv};
use settings::Settings;
use state::Player;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui::{Tab, UiState};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Fullscreen, Icon, Window, WindowAttributes, WindowLevel};

const SUB_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "idx", "sup", "smi", "lrc"];

#[derive(Debug, Clone)]
enum UserEvent {
    MpvEvents,
    MpvRender,
    ThumbsReady,
    /// Files handed over by a second instance.
    OpenPaths(Vec<String>),
    Repaint(Duration),
}

struct Gfx {
    window: Arc<Window>,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    egui: egui_glow::EguiGlow,
}

/// Saved geometry while in PiP / music mode.
#[derive(Clone, Copy)]
struct Geometry {
    pos: Option<PhysicalPosition<i32>>,
    size: PhysicalSize<u32>,
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    mpv: Mpv,
    gfx: Option<Gfx>,
    player: Player,
    ui: UiState,
    bindings: Bindings,
    modifiers: ModifiersState,
    config_dir: PathBuf,
    pending_open: Vec<String>,
    resize_on_video: bool,
    saved_geometry: Option<Geometry>,
    press: Option<(PhysicalPosition<f64>, Instant)>,
    last_click: Option<Instant>,
    last_drop: Option<Instant>,
    cursor: PhysicalPosition<f64>,
    cursor_hidden: bool,
    subs_lifted: bool,
    repaint_at: Option<Instant>,
    /// Debug aid: `IINA_RS_SNAPSHOT=out.png` saves the composited frame after
    /// a few seconds and quits (desktop capture tools often miss GL windows).
    snapshot: Option<String>,
    started: Instant,
    last_stats: Instant,
    fs_toggles: u32,
    /// Window geometry (and maximized flag) to restore when leaving fullscreen.
    fs_saved: Option<(Geometry, bool)>,
    frames: u32,
    thumbs: Option<thumbs::Generator>,
    /// File whose folder siblings should be added once it has loaded.
    siblings_for: Option<String>,
    /// Pending single-click pause (delayed so a double click can cancel it).
    click_pause_at: Option<Instant>,
    history: history::History,
    /// Last known (path, position, duration) of the current file, for history.
    last_pos: Option<(String, f64, f64)>,
    debug_cmds_done: bool,
}

fn config_dir() -> PathBuf {
    let dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("iina-rs");
    let _ = std::fs::create_dir_all(dir.join("watch_later"));
    dir
}

fn load_recent(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("recent.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>, args: Vec<String>, mpv_opts: Vec<(String, String)>) -> Result<Self, String> {
        let config_dir = config_dir();
        let settings = Settings::load(&config_dir);
        let mut mpv = Mpv::new()?;
        let cfg = config_dir.to_string_lossy().into_owned();
        for (k, v) in [
            ("vo", "libmpv"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("force-window", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("osc", "no"),
            ("ytdl", "yes"),
            ("watch-later-dir", &format!("{cfg}/watch_later")),
            ("screenshot-template", "IINA-RS %F %wH.%wM.%wS"),
            ("osd-font-size", "40"),
            ("osd-bar", "yes"),
            ("sub-auto", "fuzzy"),
            ("audio-file-auto", "fuzzy"),
            ("config-dir", &cfg),
            ("config", "yes"),
            ("terminal", if cfg!(debug_assertions) { "yes" } else { "no" }),
            ("msg-level", "all=warn"),
        ] {
            mpv.set_option(k, v);
        }
        for (k, v) in settings.mpv_options() {
            mpv.set_option(k, &v);
        }
        mpv.set_option("volume", &format!("{}", settings.volume.round() as i64));
        // `--name=value` / `--flag` / `--no-flag` arguments are passed to mpv, like iina-cli.
        for (k, v) in &mpv_opts {
            mpv.set_option(k, v);
        }
        mpv.init()?;
        state::observe_all(&mpv);
        let p = proxy.clone();
        mpv.set_wakeup_callback(move || {
            let _ = p.send_event(UserEvent::MpvEvents);
        });

        let user_conf = std::fs::read_to_string(config_dir.join("input.conf")).ok();
        let config_dir_for_history = config_dir.clone();
        if settings.single_instance {
            let p = proxy.clone();
            instance::listen(&config_dir, move |args| {
                let _ = p.send_event(UserEvent::OpenPaths(args));
            });
        }
        Ok(Self {
            proxy,
            mpv,
            gfx: None,
            player: Player::default(),
            ui: UiState::new(load_recent(&config_dir), settings),
            bindings: Bindings::load(user_conf),
            modifiers: ModifiersState::empty(),
            config_dir,
            pending_open: args,
            resize_on_video: true,
            saved_geometry: None,
            press: None,
            last_click: None,
            last_drop: None,
            cursor: PhysicalPosition::new(0.0, 0.0),
            cursor_hidden: false,
            subs_lifted: false,
            repaint_at: None,
            snapshot: std::env::var("IINA_RS_SNAPSHOT").ok(),
            started: Instant::now(),
            last_stats: Instant::now(),
            fs_toggles: 0,
            fs_saved: None,
            frames: 0,
            thumbs: None,
            siblings_for: None,
            click_pause_at: None,
            history: history::History::load(&config_dir_for_history),
            last_pos: None,
            debug_cmds_done: false,
        })
    }

    fn window(&self) -> Option<&Arc<Window>> {
        self.gfx.as_ref().map(|g| &g.window)
    }

    fn redraw(&self) {
        if let Some(w) = self.window() {
            w.request_redraw();
        }
    }

    fn create_window(&mut self, el: &ActiveEventLoop) {
        let icon = Icon::from_rgba(include_bytes!("../assets/icon_64.rgba").to_vec(), 64, 64).ok();
        let attrs = WindowAttributes::default()
            .with_title("IINA-RS")
            .with_inner_size(LogicalSize::new(960.0, 540.0))
            .with_min_inner_size(LogicalSize::new(320.0, 140.0))
            .with_window_icon(icon)
            .with_visible(false);
        let template = glutin::config::ConfigTemplateBuilder::new().with_depth_size(0).with_stencil_size(0);
        let (window, config) = glutin_winit::DisplayBuilder::new()
            .with_preference(glutin_winit::ApiPreference::FallbackEgl)
            .with_window_attributes(Some(attrs.clone()))
            .build(el, template, |mut it| it.next().expect("no OpenGL config"))
            .expect("failed to create OpenGL display");
        let window = window.unwrap_or_else(|| glutin_winit::finalize_window(el, attrs, &config).expect("window"));
        let raw = window.window_handle().expect("window handle").as_raw();
        let display = config.display();
        let ctx_attrs = glutin::context::ContextAttributesBuilder::new().build(Some(raw));
        let gles_attrs = glutin::context::ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::Gles(None))
            .build(Some(raw));
        let not_current = unsafe {
            display
                .create_context(&config, &ctx_attrs)
                .or_else(|_| display.create_context(&config, &gles_attrs))
                .expect("failed to create OpenGL context")
        };
        let size = window.inner_size();
        let surf_attrs = glutin::surface::SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw,
            NonZeroU32::new(size.width).unwrap_or(NonZeroU32::MIN),
            NonZeroU32::new(size.height).unwrap_or(NonZeroU32::MIN),
        );
        let surface = unsafe { display.create_window_surface(&config, &surf_attrs).expect("surface") };
        let context = not_current.make_current(&surface).expect("make_current");
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));

        let gl = Arc::new(unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) });
        let egui = egui_glow::EguiGlow::new(el, gl, None, None, true);
        egui_extras::install_image_loaders(&egui.egui_ctx);
        ui::setup_style(&egui.egui_ctx);
        let p = self.proxy.clone();
        egui.egui_ctx.set_request_repaint_callback(move |info| {
            let _ = p.send_event(UserEvent::Repaint(info.delay));
        });

        let gl_display = display.clone();
        let p = self.proxy.clone();
        if let Err(e) = self.mpv.create_render_context(
            move |name| gl_display.get_proc_address(name),
            move || {
                let _ = p.send_event(UserEvent::MpvRender);
            },
        ) {
            eprintln!("mpv render context: {e}");
        }

        let window = Arc::new(window);
        window.set_visible(true);
        if std::env::var_os("IINA_RS_DEBUG").is_some() {
            use glow::HasContext;
            let gl = egui.painter.gl();
            eprintln!("[gl] {} / {}", unsafe { gl.get_parameter_string(glow::VENDOR) }, unsafe { gl.get_parameter_string(glow::RENDERER) });
        }
        self.gfx = Some(Gfx { window, surface, context, egui });
        if self.player.fullscreen {
            self.apply_fullscreen();
        }

        if let Ok(panel) = std::env::var("IINA_RS_PANEL") {
            self.run_app_command(&format!("{panel}-panel"));
        }
        // Debug aid: IINA_RS_CMD="inspector;preferences" runs app commands once
        // the first file has loaded (or right away when nothing is opened).
        if self.pending_open.is_empty() {
            self.run_debug_cmds();
        }
        let args = std::mem::take(&mut self.pending_open);
        self.open_many(args);
    }

    // ---------------------------------------------------------- opening ---

    fn open(&mut self, path: &str, replace_first: bool) {
        self.open_ex(path, replace_first, replace_first);
    }

    /// `siblings`: once loaded, add the other media files of the folder (IINA's
    /// "add files in the same folder" behaviour) if it is the only playlist entry.
    fn open_ex(&mut self, path: &str, replace_first: bool, siblings: bool) {
        let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if SUB_EXTS.contains(&ext.as_str()) && !self.player.idle {
            self.mpv.command(&["sub-add", path, "select"]);
            return;
        }
        let abs = if path.contains("://") {
            path.to_string()
        } else {
            std::fs::canonicalize(path)
                .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
                .unwrap_or_else(|_| path.to_string())
        };
        if replace_first {
            if !self.player.idle {
                self.mpv.command(&["write-watch-later-config"]);
            }
            self.mpv.command(&["loadfile", &abs, "replace"]);
            self.resize_on_video = true;
            self.siblings_for = (siblings && !abs.contains("://") && self.ui.settings.auto_add_folder).then(|| abs.clone());
        } else {
            self.mpv.command(&["loadfile", &abs, "append-play"]);
        }
        self.add_recent(abs);
    }

    fn open_many(&mut self, paths: Vec<String>) {
        let single = paths.len() == 1;
        for (i, p) in paths.iter().enumerate() {
            self.open_ex(p, i == 0, single);
        }
    }

    fn add_recent(&mut self, path: String) {
        let r = &mut self.ui.recent;
        r.retain(|p| p != &path);
        r.insert(0, path);
        r.truncate(15);
        self.save_recent();
    }

    fn save_recent(&self) {
        let _ = std::fs::write(self.config_dir.join("recent.json"), serde_json::to_string_pretty(&self.ui.recent).unwrap());
    }

    fn pick_files(&self, title: &str, filters: &[(&str, &[&str])]) -> Vec<String> {
        let mut dlg = rfd::FileDialog::new().set_title(title);
        for (name, exts) in filters {
            dlg = dlg.add_filter(*name, exts);
        }
        dlg.pick_files().unwrap_or_default().into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }

    // ------------------------------------------------------ app commands ---

    fn run_app_command(&mut self, cmd: &str) {
        let (name, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
        match name {
            "mpv" => self.mpv.command_string(arg),
            "open-path" => self.open(arg, true),
            "open-file" => {
                let files = self.pick_files("Open", &[("Media", MEDIA_EXTS), ("All files", &["*"])]);
                self.open_many(files);
            }
            "add-files" => {
                for f in self.pick_files("Add to playlist", &[("Media", MEDIA_EXTS), ("All files", &["*"])]) {
                    let idle = self.player.idle;
                    self.open_ex(&f, idle, false);
                }
            }
            "add-sub" => {
                for f in self.pick_files("Load subtitle", &[("Subtitles", SUB_EXTS), ("All files", &["*"])]) {
                    self.mpv.command(&["sub-add", &f, "select"]);
                }
            }
            "add-audio" => {
                for f in self.pick_files("Load audio", &[("Audio", &["mka", "mp3", "aac", "ac3", "eac3", "dts", "flac", "opus", "ogg", "wav", "m4a"]), ("All files", &["*"])]) {
                    self.mpv.command(&["audio-add", &f, "select"]);
                }
            }
            "open-url" => self.ui.url_input = Some(String::new()),
            "close-file" => self.mpv.command(&["stop"]),
            "clear-recent" => {
                self.ui.recent.clear();
                self.save_recent();
            }
            "playlist-panel" => self.ui.toggle_tab(Tab::Playlist),
            "chapter-panel" => self.ui.toggle_tab(Tab::Chapters),
            "video-panel" => self.ui.toggle_tab(Tab::Video),
            "audio-panel" => self.ui.toggle_tab(Tab::Audio),
            "sub-panel" => self.ui.toggle_tab(Tab::Subtitles),
            "toggle-fullscreen" => self.mpv.command(&["cycle", "fullscreen"]),
            "toggle-pip" => self.toggle_pip(),
            "toggle-music-mode" => self.toggle_music_mode(),
            "fit-to-screen" => {
                if let Some(w) = self.window() {
                    w.set_maximized(!w.is_maximized());
                }
            }
            "bigger-window" => self.scale_window(1.1),
            "smaller-window" => self.scale_window(1.0 / 1.1),
            "show-current-file-in-finder" => {
                let p = self.player.path.clone();
                reveal(&p);
            }
            "reveal" => reveal(arg),
            "about" => self.ui.show_about = true,
            "inspector" => self.ui.show_inspector = !self.ui.show_inspector,
            "history" => self.ui.show_history = !self.ui.show_history,
            "history-remove" => self.history.remove(arg),
            "history-clear" => self.history.clear(),
            "find-online-subs" => self.open_subtitle_search(),
            "subs-search" => {
                let p = self.proxy.clone();
                subtitles::start_search(self.ui.subs.clone(), self.ui.settings.opensubtitles_key.trim().to_string(), move || {
                    let _ = p.send_event(UserEvent::MpvRender);
                });
            }
            "subs-download" => {
                if let Ok(id) = arg.parse::<u64>() {
                    let p = self.proxy.clone();
                    let dest = self.config_dir.join("subtitles");
                    subtitles::start_download(self.ui.subs.clone(), self.ui.settings.opensubtitles_key.trim().to_string(), id, dest, move || {
                        let _ = p.send_event(UserEvent::MpvRender);
                    });
                }
            }
            "crop" => self.apply_crop(arg),
            "assoc-register" => {
                self.ui.assoc_status = match assoc::register(VIDEO_AUDIO_EXTS) {
                    Ok(()) => "Registered. Pick IINA-RS under “Open with”, or set it as default in Default apps.".into(),
                    Err(e) => format!("Failed: {e}"),
                };
            }
            "assoc-unregister" => {
                self.ui.assoc_status = match assoc::unregister(VIDEO_AUDIO_EXTS) {
                    Ok(()) => "Removed IINA-RS from “Open with”.".into(),
                    Err(e) => format!("Failed: {e}"),
                };
            }
            "open-default-apps" => assoc::open_default_apps_settings(),
            "open-config-dir" => open_folder(&self.config_dir),
            "preferences" => self.ui.show_prefs = true,
            "jump-to" => {
                if !self.player.idle {
                    self.ui.jump_input = Some(String::new());
                }
            }
            "live-text" => {
                self.mpv.command(&["show-text", "Not available on this platform yet"]);
            }
            other => eprintln!("unknown app command: {other}"),
        }
        self.redraw();
    }

    // ------------------------------------------------- window geometry ---

    /// Resizes the window to the video size (like IINA). Returns false if the
    /// video dimensions aren't known yet.
    fn fit_to_video(&mut self, scale: f64) -> bool {
        let (Some(w), Some(h)) = (
            self.mpv.get_string("dwidth").and_then(|s| s.parse::<f64>().ok()),
            self.mpv.get_string("dheight").and_then(|s| s.parse::<f64>().ok()),
        ) else {
            return false;
        };
        let Some(window) = self.window().cloned() else { return false };
        if self.player.fullscreen || window.is_maximized() || self.ui.pip || self.ui.music_mode {
            return true;
        }
        // mpv sizes windows in physical pixels scaled by the display's DPI
        // (hidpi-window-scale); do the same.
        let dpi = window.scale_factor();
        let (mut tw, mut th) = (w * scale * dpi, h * scale * dpi);
        if let Some(mon) = window.current_monitor() {
            let ms = mon.size();
            let (mw, mh) = (ms.width as f64 * 0.85, ms.height as f64 * 0.85);
            let k = (mw / tw).min(mh / th).min(1.0);
            tw *= k;
            th *= k;
            let _ = window.request_inner_size(PhysicalSize::new(tw as u32, th as u32));
            // Re-center like IINA does when a new video opens.
            let mp = mon.position();
            let outer = window.outer_size();
            let dx = outer.width.saturating_sub(window.inner_size().width) as f64;
            let dy = outer.height.saturating_sub(window.inner_size().height) as f64;
            let x = mp.x as f64 + (ms.width as f64 - tw - dx) / 2.0;
            let y = mp.y as f64 + (ms.height as f64 - th - dy) / 2.0;
            window.set_outer_position(PhysicalPosition::new(x.max(mp.x as f64) as i32, y.max(mp.y as f64) as i32));
        } else {
            let _ = window.request_inner_size(PhysicalSize::new(tw as u32, th as u32));
        }
        true
    }

    fn scale_window(&mut self, k: f64) {
        if let Some(w) = self.window() {
            let s = w.inner_size();
            let _ = w.request_inner_size(PhysicalSize::new((s.width as f64 * k) as u32, (s.height as f64 * k) as u32));
        }
    }

    fn save_geometry(&mut self) {
        if let Some(w) = self.window() {
            self.saved_geometry = Some(Geometry { pos: w.outer_position().ok(), size: w.inner_size() });
        }
    }

    fn restore_geometry(&mut self) {
        let saved = self.saved_geometry.take();
        if let (Some(w), Some(g)) = (self.window(), saved) {
            w.set_decorations(true);
            w.set_window_level(if self.player.ontop { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal });
            let _ = w.request_inner_size(g.size);
            if let Some(p) = g.pos {
                w.set_outer_position(p);
            }
        }
    }

    fn toggle_pip(&mut self) {
        if self.ui.pip {
            self.ui.pip = false;
            self.restore_geometry();
            return;
        }
        if self.player.idle {
            return;
        }
        if self.player.fullscreen {
            self.mpv.set_flag("fullscreen", false);
        }
        if self.ui.music_mode {
            self.ui.music_mode = false;
        } else {
            self.save_geometry();
        }
        self.ui.pip = true;
        self.ui.sidebar = None;
        let Some(w) = self.window().cloned() else { return };
        let (vw, vh) = (
            self.mpv.get_string("dwidth").and_then(|s| s.parse::<f64>().ok()).unwrap_or(16.0),
            self.mpv.get_string("dheight").and_then(|s| s.parse::<f64>().ok()).unwrap_or(9.0),
        );
        if let Some(mon) = w.current_monitor() {
            let ms = mon.size();
            let width = ms.width as f64 * 0.25;
            let height = width * vh.max(1.0) / vw.max(1.0);
            w.set_decorations(false);
            w.set_window_level(WindowLevel::AlwaysOnTop);
            let _ = w.request_inner_size(PhysicalSize::new(width as u32, height as u32));
            let margin = 32.0 * w.scale_factor();
            let mp = mon.position();
            w.set_outer_position(PhysicalPosition::new(
                (mp.x as f64 + ms.width as f64 - width - margin) as i32,
                (mp.y as f64 + ms.height as f64 - height - margin * 2.0) as i32,
            ));
        }
    }

    fn toggle_music_mode(&mut self) {
        if self.ui.music_mode {
            self.ui.music_mode = false;
            self.restore_geometry();
            return;
        }
        if self.player.fullscreen {
            self.mpv.set_flag("fullscreen", false);
        }
        if self.ui.pip {
            self.ui.pip = false;
            if let Some(w) = self.window() {
                w.set_decorations(true);
            }
        } else {
            self.save_geometry();
        }
        self.ui.music_mode = true;
        if let Some(w) = self.window() {
            let _ = w.request_inner_size(LogicalSize::new(480.0, 270.0));
        }
    }

    // ------------------------------------------------------- mpv events ---

    fn handle_mpv_events(&mut self, el: &ActiveEventLoop) {
        for ev in self.mpv.poll_events() {
            if std::env::var_os("IINA_RS_DEBUG").is_some() { eprintln!("mpv event: {ev:?}"); }
            match ev {
                Event::Shutdown => {
                    el.exit();
                    return;
                }
                Event::StartFile => self.resize_on_video = self.ui.settings.resize_to_video,
                Event::FileLoaded => {
                    self.add_siblings();
                    self.start_thumbnails();
                    let path = self.mpv.get_string("path").unwrap_or_default();
                    let title = self.mpv.get_string("media-title").unwrap_or_default();
                    let dur = self.mpv.get_string("duration").and_then(|d| d.parse().ok()).unwrap_or(0.0);
                    if !path.is_empty() {
                        self.history.opened(&path, &title, dur);
                    }
                    self.player.crop_label = "None".into();
                }
                Event::EndFile => self.flush_history(),
                Event::PlaybackRestart if !self.debug_cmds_done => self.run_debug_cmds(),
                Event::VideoReconfig => {
                    if self.resize_on_video && self.fit_to_video(1.0) {
                        self.resize_on_video = false;
                    }
                }
                Event::Property(name, value) => {
                    if name == "time-pos" {
                        if let mpv::PropValue::Double(t) = value {
                            if !self.player.path.is_empty() {
                                self.last_pos = Some((self.player.path.clone(), t, self.player.duration));
                            }
                        }
                    }
                    let before = (self.player.fullscreen, self.player.ontop, self.player.window_scale);
                    self.player.apply(&name, &value);
                    match name.as_str() {
                        "fullscreen" if before.0 != self.player.fullscreen => self.apply_fullscreen(),
                        "ontop" if before.1 != self.player.ontop => {
                            if let Some(w) = self.window() {
                                w.set_window_level(if self.player.ontop { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal });
                            }
                        }
                        "window-scale" if (before.2 - self.player.window_scale).abs() > 1e-6 => {
                            let s = self.player.window_scale;
                            let _ = self.fit_to_video(s);
                        }
                        "media-title" => self.update_title(),
                        "idle-active" => {
                            self.update_title();
                            if self.player.idle {
                                self.thumbs = None;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        self.redraw();
    }

    fn apply_fullscreen(&mut self) {
        dbg_log(format!("apply_fullscreen -> {}", self.player.fullscreen));
        self.ui.fullscreen = self.player.fullscreen;
        if self.player.fullscreen && (self.ui.pip || self.ui.music_mode) {
            self.ui.pip = false;
            self.ui.music_mode = false;
            self.restore_geometry();
        }
        let Some(w) = self.window().cloned() else { return };
        let t = Instant::now();
        if cfg!(windows) {
            self.set_fullscreen_windows(&w, self.player.fullscreen);
        } else {
            w.set_fullscreen(self.player.fullscreen.then_some(Fullscreen::Borderless(None)));
        }
        dbg_log(format!("fullscreen switch took {:?}", t.elapsed()));
    }

    /// Borderless fullscreen done by hand on Windows. A GL window that exactly
    /// covers the monitor gets promoted by Windows/the driver to a pseudo-
    /// exclusive "fullscreen optimizations" mode; entering and leaving that mode
    /// blanks the screen and stalls rendering for ~2 s. Making the window one
    /// pixel taller than the monitor keeps it a normal composited window.
    fn set_fullscreen_windows(&mut self, w: &Window, on: bool) {
        if on {
            if self.fs_saved.is_some() {
                return;
            }
            let Some(mon) = w.current_monitor() else { return };
            self.fs_saved = Some((Geometry { pos: w.outer_position().ok(), size: w.inner_size() }, w.is_maximized()));
            if w.is_maximized() {
                w.set_maximized(false);
            }
            w.set_decorations(false);
            w.set_outer_position(mon.position());
            let size = mon.size();
            let _ = w.request_inner_size(PhysicalSize::new(size.width, size.height + 1));
        } else if let Some((g, maximized)) = self.fs_saved.take() {
            w.set_decorations(true);
            let _ = w.request_inner_size(g.size);
            if let Some(p) = g.pos {
                w.set_outer_position(p);
            }
            if maximized {
                w.set_maximized(true);
            }
        }
    }

    fn update_title(&self) {
        if let Some(w) = self.window() {
            if self.player.idle || self.player.title.is_empty() {
                w.set_title("IINA-RS");
            } else {
                w.set_title(&format!("{} — IINA-RS", self.player.title));
            }
        }
    }

    // ----------------------------------------------------------- render ---

    fn render(&mut self) {
        self.frames += 1;
        let Some(gfx) = self.gfx.as_mut() else { return };
        let size = gfx.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let t0 = Instant::now();
        self.mpv.render_update();
        self.mpv.render(size.width as i32, size.height as i32);
        let t_mpv = t0.elapsed();

        let window = gfx.window.clone();
        let (player, ui_state, mpv, history) = (&self.player, &mut self.ui, &self.mpv, &self.history);
        // Debug aid: IINA_RS_HOVER=0.4 hovers the seek bar at 40% (for snapshots).
        if let (Some(f), Some(r)) = (std::env::var("IINA_RS_HOVER").ok().and_then(|v| v.parse::<f32>().ok()), ui_state.seek_bar_rect) {
            ui_state.last_activity = Instant::now();
            let pos = egui::pos2(r.left() + r.width() * f, r.center().y);
            gfx.egui.egui_winit.egui_input_mut().events.push(egui::Event::PointerMoved(pos));
        }
        let thumbs = self.thumbs.as_ref().map(|t| t.shared.clone());
        let guard = thumbs.as_ref().map(|t| t.lock().unwrap());
        let mut cmds = Vec::new();
        gfx.egui.run(&window, |ctx| {
            cmds = ui::draw(ctx, ui_state, player, mpv, guard.as_deref(), history);
        });
        drop(guard);
        gfx.egui.paint(&window);
        if std::env::var_os("IINA_RS_DEBUG").is_some() && self.last_stats.elapsed() > Duration::from_secs(2) {
            self.last_stats = Instant::now();
            let g = |p: &str| self.mpv.get_string(p).unwrap_or_else(|| "-".into());
            eprintln!(
                "[stats] frames={} t={} hwdec={} vo-drop={} dec-drop={} delayed={} vf-fps={} display-fps={} size={}x{}",
                std::mem::take(&mut self.frames), g("time-pos"), g("hwdec-current"), g("frame-drop-count"), g("decoder-frame-drop-count"),
                g("vo-delayed-frame-count"), g("estimated-vf-fps"), g("display-fps"), size.width, size.height
            );
        }
        let snap_after = std::env::var("IINA_RS_SNAPSHOT_AFTER").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
        if let Some(path) = self.snapshot.as_ref().filter(|_| self.started.elapsed() > Duration::from_secs(snap_after)) {
            save_snapshot(gfx.egui.painter.gl(), size, path);
            self.snapshot = None;
            self.mpv.command(&["quit"]);
        }
        let t1 = Instant::now();
        let _ = gfx.surface.swap_buffers(&gfx.context);
        self.mpv.report_swap();
        let t_swap = t1.elapsed();
        if t0.elapsed() > Duration::from_millis(60) {
            dbg_log(format!("slow frame: mpv={:?} ui={:?} swap={:?} size={}x{}", t_mpv, t1 - t0 - t_mpv, t_swap, size.width, size.height));
        }

        // Like IINA, lift subtitles above the controller while it is shown.
        // sub-margin-y is in units of a 720px-tall frame; the bar is ~130pt high.
        let lift = self.ui.osc_visible && !self.player.idle && !self.ui.pip;
        if lift != self.subs_lifted {
            self.subs_lifted = lift;
            let margin = if lift {
                let h_pt = size.height as f64 / window.scale_factor();
                (130.0 / h_pt * 720.0).clamp(22.0, 300.0)
            } else {
                22.0
            };
            self.mpv.set_string("sub-margin-y", &format!("{}", margin.round() as i64));
        }

        // Hide the cursor over the video while the controller is hidden.
        let hide = !self.ui.osc_visible && !self.player.idle && !self.player.pause && !self.ui.pointer_over_ui;
        if hide != self.cursor_hidden {
            self.cursor_hidden = hide;
            window.set_cursor_visible(!hide);
        }
        for c in cmds {
            self.run_app_command(&c);
        }
        let ready: Vec<PathBuf> = std::mem::take(&mut self.ui.subs.lock().unwrap().ready);
        for sub in ready {
            self.mpv.command(&["sub-add", &sub.to_string_lossy(), "select"]);
        }
        if self.ui.settings_dirty {
            self.ui.settings_dirty = false;
            self.apply_settings();
        }
        if self.snapshot.is_some() {
            window.request_redraw();
        }
    }

    fn run_debug_cmds(&mut self) {
        if std::mem::replace(&mut self.debug_cmds_done, true) {
            return;
        }
        if let Ok(cmds) = std::env::var("IINA_RS_CMD") {
            for c in cmds.split(';').filter(|c| !c.is_empty()) {
                self.run_app_command(c);
            }
        }
    }

    fn flush_history(&mut self) {
        if let Some((path, pos, dur)) = self.last_pos.take() {
            self.history.update_position(&path, pos, dur);
            self.history.save();
        }
    }

    fn open_subtitle_search(&mut self) {
        if self.player.idle {
            return;
        }
        let path = self.player.path.clone();
        // Search text: file name without extension, separators as spaces.
        let stem = Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let query: String = stem.chars().map(|c| if "._[]()".contains(c) { ' ' } else { c }).collect();
        {
            let mut s = self.ui.subs.lock().unwrap();
            if s.media_path != path {
                s.results.clear();
                s.status.clear();
            }
            s.media_path = path;
            s.query = query.split_whitespace().collect::<Vec<_>>().join(" ");
            s.languages = self.ui.settings.sub_languages.clone();
        }
        self.ui.show_subs = true;
    }

    /// Crops the video to the aspect ratio in `arg` ("<ratio> <label>" or "none None").
    fn apply_crop(&mut self, arg: &str) {
        let (ratio, label) = arg.split_once(' ').unwrap_or((arg, arg));
        if ratio == "none" {
            self.mpv.set_string("video-crop", "");
            self.player.crop_label = "None".into();
            return;
        }
        let (Some(w), Some(h), Some(r)) = (
            self.mpv.get_string("video-params/w").and_then(|s| s.parse::<f64>().ok()),
            self.mpv.get_string("video-params/h").and_then(|s| s.parse::<f64>().ok()),
            ratio.parse::<f64>().ok(),
        ) else {
            return;
        };
        let (cw, ch) = if w / h > r { ((h * r).round(), h) } else { (w, (w / r).round()) };
        let (x, y) = (((w - cw) / 2.0).round(), ((h - ch) / 2.0).round());
        self.mpv.set_string("video-crop", &format!("{}x{}+{}+{}", cw as i64, ch as i64, x as i64, y as i64));
        self.player.crop_label = label.to_string();
    }

    fn apply_settings(&mut self) {
        let s = self.ui.settings.clone();
        for (k, v) in s.mpv_options() {
            self.mpv.set_string(k, &v);
        }
        if s.thumbnails && self.thumbs.is_none() {
            self.start_thumbnails();
        } else if !s.thumbnails {
            self.thumbs = None;
        }
        s.save(&self.config_dir);
    }

    fn start_thumbnails(&mut self) {
        let path = self.mpv.get_string("path").unwrap_or_default();
        let local = !path.is_empty() && !path.contains("://") && Path::new(&path).is_file();
        if !self.ui.settings.thumbnails || !local {
            self.thumbs = None;
            return;
        }
        if self.thumbs.as_ref().is_some_and(|t| t.shared.lock().unwrap().path == path) {
            return;
        }
        let p = self.proxy.clone();
        self.thumbs = Some(thumbs::start(path, move || {
            let _ = p.send_event(UserEvent::ThumbsReady);
        }));
    }

    fn add_siblings(&mut self) {
        let Some(path) = self.siblings_for.take() else { return };
        if self.mpv.get_string("playlist-count").as_deref() != Some("1") {
            return;
        }
        let p = Path::new(&path);
        let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.is_file())
            .filter(|f| {
                f.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .is_some_and(|e| VIDEO_AUDIO_EXTS.contains(&e.as_str()))
            })
            .collect();
        files.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        let Some(k) = files.iter().position(|f| f.file_name() == Some(name)) else { return };
        if files.len() < 2 || files.len() > 1000 {
            return;
        }
        // Playlist is [current]; append the files after it, then those before
        // it, and move the latter to the front so folder order is preserved.
        let after = files.len() - 1 - k;
        for f in files[k + 1..].iter().chain(files[..k].iter()) {
            self.mpv.command(&["loadfile", &f.to_string_lossy(), "append"]);
        }
        for j in 0..k {
            self.mpv.command(&["playlist-move", &(1 + after + j).to_string(), &j.to_string()]);
        }
    }

    fn on_activity(&mut self) {
        self.ui.last_activity = Instant::now();
        if self.cursor_hidden {
            self.cursor_hidden = false;
            if let Some(w) = self.window() {
                w.set_cursor_visible(true);
            }
        }
        self.redraw();
    }
}

/// Extensions considered when auto-adding a folder (no playlists / images).
const VIDEO_AUDIO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "vob", "ogv", "3gp", "rmvb", "mp3",
    "flac", "m4a", "aac", "ogg", "opus", "wav", "wma", "ape",
];

/// Natural ordering ("ep2" < "ep10"), case-insensitive.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    b.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                a.next();
                b.next();
            }
        }
    }
}

const MEDIA_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "vob", "ogv", "3gp", "rmvb", "mp3",
    "flac", "m4a", "aac", "ogg", "opus", "wav", "wma", "alac", "ape", "m3u", "m3u8", "pls", "iso",
];

fn dbg_log(msg: String) {
    if std::env::var_os("IINA_RS_DEBUG").is_some() {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
        eprintln!("[{t:7.3}] {msg}");
    }
}

fn save_snapshot(gl: &glow::Context, size: PhysicalSize<u32>, path: &str) {
    use glow::HasContext;
    let (w, h) = (size.width as i32, size.height as i32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    unsafe {
        gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut px)));
    }
    let row = (w * 4) as usize;
    let flipped: Vec<u8> = px.chunks(row).rev().flatten().copied().collect();
    if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, flipped) {
        let _ = img.save(path);
    }
}

fn open_folder(dir: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

fn reveal(path: &str) {
    if path.is_empty() || path.contains("://") {
        return;
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    }
    #[cfg(not(windows))]
    {
        let dir = Path::new(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            self.create_window(el);
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::MpvEvents => self.handle_mpv_events(el),
            UserEvent::MpvRender | UserEvent::ThumbsReady => self.redraw(),
            UserEvent::OpenPaths(paths) => {
                if !paths.is_empty() {
                    self.open_many(paths);
                }
                if let Some(w) = self.window() {
                    w.set_minimized(false);
                    w.focus_window();
                }
            }
            UserEvent::Repaint(delay) => {
                if delay.is_zero() {
                    self.redraw();
                } else if let Some(at) = Instant::now().checked_add(delay) {
                    if self.repaint_at.is_none_or(|t| at < t) {
                        self.repaint_at = Some(at);
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // Debug aid: IINA_RS_FS_TOGGLES=n cycles fullscreen every 2 s, n times.
        if let Some(n) = std::env::var("IINA_RS_FS_TOGGLES").ok().and_then(|v| v.parse::<u32>().ok()) {
            if self.fs_toggles < n && self.started.elapsed() > Duration::from_secs(2 * (self.fs_toggles as u64 + 1)) {
                self.fs_toggles += 1;
                dbg_log(format!("--- toggle #{} ---", self.fs_toggles));
                self.mpv.command(&["cycle", "fullscreen"]);
            }
            let next = self.started + Duration::from_secs(2 * (self.fs_toggles as u64 + 1));
            if self.repaint_at.is_none_or(|r| next < r) {
                self.repaint_at = Some(next);
            }
        }
        if let Some(t) = self.click_pause_at {
            if t <= Instant::now() {
                self.click_pause_at = None;
                self.mpv.command(&["cycle", "pause"]);
            } else if self.repaint_at.is_none_or(|r| t < r) {
                self.repaint_at = Some(t);
            }
        }
        match self.repaint_at {
            Some(t) if t <= Instant::now() => {
                self.repaint_at = None;
                self.redraw();
                el.set_control_flow(ControlFlow::Wait);
            }
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: winit::window::WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let resp = gfx.egui.on_window_event(&gfx.window, &event);
        if resp.repaint {
            gfx.window.request_redraw();
        }
        let egui_ctx = gfx.egui.egui_ctx.clone();

        match event {
            WindowEvent::CloseRequested => {
                // `quit` saves the resume position; mpv then sends SHUTDOWN.
                self.mpv.command(&["quit"]);
            }
            WindowEvent::Destroyed => el.exit(),
            WindowEvent::Resized(size) => {
                dbg_log(format!("Resized {}x{} win_fs={} mpv_fs={}", size.width, size.height, gfx.window.fullscreen().is_some(), self.player.fullscreen));
                if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                    gfx.surface.resize(&gfx.context, w, h);
                }
                // Keep mpv's fullscreen flag in sync if the OS changed it
                // (only meaningful where winit's own fullscreen is used).
                if !cfg!(windows) {
                    let fs = gfx.window.fullscreen().is_some();
                    if fs != self.player.fullscreen {
                        self.mpv.set_flag("fullscreen", fs);
                    }
                }
                self.redraw();
            }
            WindowEvent::RedrawRequested => self.render(),
            WindowEvent::DroppedFile(path) => {
                // winit delivers one event per file; files dropped together
                // arrive back-to-back: the first replaces playback, the rest append.
                let p = path.to_string_lossy().into_owned();
                let batch = self.last_drop.is_some_and(|t| t.elapsed() < Duration::from_millis(300));
                self.open(&p, !batch);
                self.last_drop = Some(Instant::now());
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || egui_ctx.wants_keyboard_input() {
                    return;
                }
                if let Some(spec) = keys::event_to_spec(&event, self.modifiers) {
                    match self.bindings.lookup(&spec).cloned() {
                        Some(Binding::Mpv(cmd)) => self.mpv.command_string(&cmd),
                        Some(Binding::App(cmd)) => self.run_app_command(&cmd),
                        None => {}
                    }
                    self.on_activity();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let moved = (position.x - self.cursor.x).abs() + (position.y - self.cursor.y).abs() > 0.5;
                self.cursor = position;
                if let Some((start, _)) = self.press {
                    let d = (position.x - start.x).abs() + (position.y - start.y).abs();
                    if d > 6.0 && !self.player.fullscreen {
                        self.press = None;
                        self.click_pause_at = None;
                        let _ = gfx.window.drag_window();
                    }
                }
                if moved {
                    self.on_activity();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let over_ui = egui_ctx.is_pointer_over_area() || egui_ctx.wants_pointer_input();
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) if !over_ui => {
                        self.ui.context_menu = None;
                        let now = Instant::now();
                        if self.last_click.is_some_and(|t| now.duration_since(t) < Duration::from_millis(400)) {
                            self.last_click = None;
                            self.press = None;
                            self.click_pause_at = None;
                            if !self.player.idle && !self.ui.pip {
                                self.mpv.command(&["cycle", "fullscreen"]);
                            } else if self.ui.pip {
                                self.toggle_pip();
                            }
                        } else {
                            self.last_click = Some(now);
                            self.press = Some((self.cursor, now));
                            if self.ui.settings.click_to_pause && !self.player.idle {
                                self.click_pause_at = Some(now + Duration::from_millis(350));
                            }
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => self.press = None,
                    (MouseButton::Right, ElementState::Pressed) if !over_ui => {
                        let ppp = egui_ctx.pixels_per_point() as f64;
                        self.ui.context_menu = Some(egui::pos2((self.cursor.x / ppp) as f32, (self.cursor.y / ppp) as f32));
                    }
                    (MouseButton::Middle, ElementState::Pressed) if !over_ui => self.mpv.command(&["cycle", "pause"]),
                    (MouseButton::Back, ElementState::Pressed) => self.mpv.command(&["seek", "-10"]),
                    (MouseButton::Forward, ElementState::Pressed) => self.mpv.command(&["seek", "10"]),
                    _ => {}
                }
                self.on_activity();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if egui_ctx.is_pointer_over_area() || self.player.idle {
                    return;
                }
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(p) => (p.x / 40.0, p.y / 40.0),
                };
                // Like IINA: vertical scroll = volume, horizontal = seek.
                if dy.abs() >= dx.abs() {
                    self.mpv.command(&["add", "volume", &format!("{:.1}", dy * 2.0)]);
                } else {
                    self.mpv.command(&["seek", &format!("{:.1}", dx * 2.0)]);
                }
                self.on_activity();
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        self.flush_history();
        if self.ui.settings.single_instance {
            instance::release(&self.config_dir);
        }
        // The render context must be freed while the GL context is current.
        self.mpv.free_render_context();
        if let Some(mut g) = self.gfx.take() {
            g.egui.destroy();
        }
    }
}

fn main() {
    let mut args = Vec::new();
    let mut mpv_opts = Vec::new();
    for a in std::env::args().skip(1) {
        if let Some(opt) = a.strip_prefix("--") {
            match opt.split_once('=') {
                Some((k, v)) => mpv_opts.push((k.to_string(), v.to_string())),
                None => match opt.strip_prefix("no-") {
                    Some(k) => mpv_opts.push((k.to_string(), "no".into())),
                    None => mpv_opts.push((opt.to_string(), "yes".into())),
                },
            }
        } else {
            args.push(a);
        }
    }
    // Hand files to an already running IINA-RS (same window), like IINA does.
    let cfg = config_dir();
    if Settings::load(&cfg).single_instance && std::env::var_os("IINA_RS_NEW_INSTANCE").is_none() && mpv_opts.is_empty() {
        let abs: Vec<String> = args
            .iter()
            .map(|a| {
                if a.contains("://") {
                    a.clone()
                } else {
                    std::fs::canonicalize(a)
                        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
                        .unwrap_or_else(|_| a.clone())
                }
            })
            .collect();
        if instance::forward(&cfg, &abs) {
            return;
        }
    }
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let proxy = event_loop.create_proxy();
    let mut app = match App::new(proxy, args, mpv_opts) {
        Ok(a) => a,
        Err(e) => {
            let msg = format!(
                "IINA-RS could not start mpv:\n\n{e}\n\nInstall libmpv (Linux: `sudo apt install libmpv2`), or place libmpv-2.dll next to the program (Windows)."
            );
            eprintln!("{msg}");
            rfd::MessageDialog::new().set_title("IINA-RS").set_description(&msg).set_level(rfd::MessageLevel::Error).show();
            std::process::exit(1);
        }
    };
    event_loop.run_app(&mut app).expect("event loop error");
}
