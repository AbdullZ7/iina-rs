#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! IINA-RS — an IINA-inspired media player for Windows and Linux.
//! Video is rendered by libmpv's OpenGL render API into the window's default
//! framebuffer; the egui overlay (controller, sidebar, menus) is drawn on top.

mod assoc;
mod display_info;
mod fonts;
mod history;
mod i18n;
mod instance;
mod keys;
mod mpv;
mod settings;
mod state;
mod sub_finder;
mod subtitles;
mod thumbs;
mod ui;
mod vk_present;

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
use winit::window::{CursorIcon, Fullscreen, Icon, ResizeDirection, Window, WindowAttributes, WindowLevel};

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
    /// GL surface: the main window (OpenGL mode) or the hidden helper window
    /// (Vulkan HDR mode, where GL renders off-screen).
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    egui: egui_glow::EguiGlow,
    vk: Option<VkMode>,
}

/// HDR presentation through Vulkan (see vk_present.rs).
struct VkMode {
    _helper: Window,
    presenter: vk_present::VkPresenter,
    targets: vk_present::GlTargets,
    display: display_info::HdrDisplay,
    video_hdr: Option<bool>,
}

const GL_RGBA16F: i32 = 0x881A;

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
    sub_margin_applied: Option<i64>,
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
    /// Pending single-click action (delayed so a double click can cancel it).
    click_pause_at: Option<(Instant, String)>,
    /// Save settings (the last volume) after this time.
    save_settings_at: Option<Instant>,
    /// A resize cursor is shown on a PiP window edge.
    pip_cursor_set: bool,
    /// The PiP window moved/resized: remember its rect (in about_to_wait).
    pip_moved: bool,
    /// The window has been centred once (later resizes keep its centre).
    window_centred: bool,
    /// Last state requested from the OS for keeping the display awake.
    keep_awake: bool,
    /// Fullscreen-on-open applied for the current file.
    fullscreen_applied: bool,
    history: history::History,
    /// Last known (path, position, duration) of the current file, for history.
    last_pos: Option<(String, f64, f64)>,
    debug_cmds_done: bool,
    fonts_scanning: bool,
    /// Property-change OSD is suppressed until shortly after a file starts,
    /// so initial values don't flash messages.
    osd_armed_at: Option<Instant>,
    /// Show the position after the next seek (keyboard seeks, like IINA).
    osd_seek: bool,
    /// Show the chapter name after the next chapter change.
    osd_chapter: bool,
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
        i18n::set_language(i18n::resolve(&settings.language));
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
            ("watch-later-dir", &format!("{cfg}/watch_later")),
            // mpv's built-in OSD/OSC are replaced by IINA-RS's own UI.
            ("osd-level", "0"),
            ("osd-bar", "no"),
            ("osd-on-seek", "no"),
            // A-B loops are per file (IINA).
            ("reset-on-next-file", "ab-loop-a,ab-loop-b"),
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
        mpv.set_option("volume", &format!("{}", settings.volume.min(settings.volume_max.max(100.0)).round() as i64));
        // Advanced: the user's own mpv options, after ours.
        for (k, v) in settings.extra_mpv_options() {
            mpv.set_option(&k, &v);
        }
        // `--name=value` / `--flag` / `--no-flag` arguments are passed to mpv, like iina-cli.
        for (k, v) in &mpv_opts {
            mpv.set_option(k, v);
        }
        mpv.init()?;
        state::observe_all(&mpv);
        // Subfolder subtitles are added while the file loads (see on_load).
        mpv.hook_add("on_load", 50);
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
            sub_margin_applied: None,
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
            save_settings_at: None,
            pip_cursor_set: false,
            pip_moved: false,
            window_centred: false,
            keep_awake: false,
            fullscreen_applied: false,
            history: history::History::load(&config_dir_for_history),
            last_pos: None,
            debug_cmds_done: false,
            fonts_scanning: false,
            osd_armed_at: None,
            osd_seek: false,
            osd_chapter: false,
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
            .with_inner_size(LogicalSize::new(640.0, 360.0))
            .with_min_inner_size(LogicalSize::new(285.0, 120.0))
            .with_window_icon(icon)
            .with_visible(false);
        match self.try_create_vulkan(el, &attrs) {
            Ok(gfx) => {
                self.gfx = Some(gfx);
                self.after_window_created();
                return;
            }
            Err(reason) => {
                dbg_log(format!("OpenGL output: {reason}"));
                self.ui.output_mode = if reason.is_empty() { "SDR · OpenGL".into() } else { format!("SDR · OpenGL ({reason})") };
            }
        }
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
        let egui = self.setup_egui_and_mpv(el, gl, &display);

        let window = Arc::new(window);
        window.set_visible(true);
        if std::env::var_os("IINA_RS_DEBUG").is_some() {
            use glow::HasContext;
            let gl = egui.painter.gl();
            eprintln!("[gl] {} / {}", unsafe { gl.get_parameter_string(glow::VENDOR) }, unsafe { gl.get_parameter_string(glow::RENDERER) });
            let ext = gl.supported_extensions();
            for e in ["GL_EXT_memory_object", "GL_EXT_memory_object_win32", "GL_EXT_semaphore", "GL_EXT_semaphore_win32", "GL_ARB_color_buffer_float"] {
                eprintln!("[gl] {e}: {}", ext.contains(e));
            }
            for f in [c"wglDXOpenDeviceNV", c"wglDXRegisterObjectNV", c"wglDXLockObjectsNV"] {
                eprintln!("[gl] {}: {}", f.to_string_lossy(), !display.get_proc_address(f).is_null());
            }
        }
        self.gfx = Some(Gfx { window, surface, context, egui, vk: None });
        self.after_window_created();
    }

    /// egui + mpv's render context on the current GL context.
    fn setup_egui_and_mpv(&mut self, el: &ActiveEventLoop, gl: Arc<glow::Context>, display: &glutin::display::Display) -> egui_glow::EguiGlow {
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
        egui
    }

    /// HDR output: a Vulkan HDR10 swapchain on the main window, with GL
    /// rendering off-screen on a hidden helper window. Returns why it isn't
    /// used (empty when simply not applicable).
    fn try_create_vulkan(&mut self, el: &ActiveEventLoop, attrs: &WindowAttributes) -> Result<Gfx, String> {
        if cfg!(target_os = "macos") || !self.ui.settings.hdr_output || std::env::var_os("IINA_RS_NO_VULKAN").is_some() {
            return Err(String::new());
        }
        let window = el.create_window(attrs.clone()).map_err(|e| e.to_string())?;
        let mut display_info = display_info::query(&window);
        // The OS-reported peak can vary or exceed the panel's real ability;
        // a user value takes precedence.
        if self.ui.settings.hdr_peak_nits > 0.0 {
            display_info.max_nits = self.ui.settings.hdr_peak_nits;
        }
        dbg_log(format!("display: {display_info:?}"));
        if cfg!(windows) && !display_info.hdr_active && std::env::var_os("IINA_RS_FORCE_VULKAN").is_none() {
            return Err(String::new());
        }

        let helper_attrs = WindowAttributes::default().with_title("IINA-RS GL").with_inner_size(PhysicalSize::new(64, 64)).with_visible(false);
        let template = glutin::config::ConfigTemplateBuilder::new().with_depth_size(0).with_stencil_size(0);
        let (helper, config) = glutin_winit::DisplayBuilder::new()
            .with_window_attributes(Some(helper_attrs.clone()))
            .build(el, template, |mut it| it.next().expect("no OpenGL config"))
            .map_err(|e| format!("GL display: {e}"))?;
        let helper = match helper {
            Some(h) => h,
            None => glutin_winit::finalize_window(el, helper_attrs, &config).map_err(|e| e.to_string())?,
        };
        let raw = helper.window_handle().map_err(|e| e.to_string())?.as_raw();
        let display = config.display();
        let ctx_attrs = glutin::context::ContextAttributesBuilder::new().build(Some(raw));
        let not_current = unsafe { display.create_context(&config, &ctx_attrs) }.map_err(|e| format!("GL context: {e}"))?;
        let surf_attrs =
            glutin::surface::SurfaceAttributesBuilder::<WindowSurface>::new().build(raw, NonZeroU32::new(64).unwrap(), NonZeroU32::new(64).unwrap());
        let surface = unsafe { display.create_window_surface(&config, &surf_attrs) }.map_err(|e| format!("GL surface: {e}"))?;
        let context = not_current.make_current(&surface).map_err(|e| format!("make current: {e}"))?;
        let gl = Arc::new(unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) });

        let ext = vk_present::GlExt::load(|s| display.get_proc_address(s))?;
        let uuid = ext.device_uuid();
        let presenter = vk_present::VkPresenter::new(&window, uuid, display_info.max_nits)?;
        let targets = vk_present::GlTargets::new(&gl, ext, &presenter)?;

        // mpv starts in SDR; switched per file in `update_hdr_target`.
        self.mpv.set_string("target-trc", "srgb");
        self.mpv.set_string("target-prim", "bt.709");
        self.ui.output_mode = format!("{} · {:.0} nits peak · UI {:.0} nits", presenter.description, display_info.max_nits, display_info.sdr_white_nits);
        dbg_log(format!("Vulkan output: {}", self.ui.output_mode));
        let egui = self.setup_egui_and_mpv(el, gl, &display);
        let window = Arc::new(window);
        window.set_visible(true);
        Ok(Gfx {
            window,
            surface,
            context,
            egui,
            vk: Some(VkMode { _helper: helper, presenter, targets, display: display_info, video_hdr: None }),
        })
    }

    /// In HDR output mode, render HDR video as PQ/BT.2020 at the display's
    /// peak (mpv tone-maps brighter masters), SDR video as sRGB.
    fn update_hdr_target(&mut self) {
        let Some(vk) = self.gfx.as_mut().and_then(|g| g.vk.as_mut()) else { return };
        let gamma = self.mpv.get_string("video-params/gamma").unwrap_or_default();
        // PQ output only on an HDR10 swapchain (not in the SDR test mode).
        let hdr = matches!(gamma.as_str(), "pq" | "hlg") && vk.presenter.hdr;
        if vk.video_hdr == Some(hdr) {
            return;
        }
        vk.video_hdr = Some(hdr);
        if hdr {
            self.mpv.set_string("target-trc", "pq");
            self.mpv.set_string("target-prim", "bt.2020");
            self.mpv.set_string("target-peak", &format!("{:.0}", vk.display.max_nits));
        } else {
            self.mpv.set_string("target-trc", "srgb");
            self.mpv.set_string("target-prim", "bt.709");
            self.mpv.set_string("target-peak", "auto");
        }
        dbg_log(format!("video {} → mpv target {}", gamma, if hdr { "PQ/BT.2020" } else { "sRGB" }));
    }

    fn after_window_created(&mut self) {
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
            if self.ui.settings.pause_on_open {
                self.mpv.set_flag("pause", true);
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
            "toggle-pause" => self.toggle_pause(),
            "arrow-left" => self.arrow_button(false),
            "arrow-right" => self.arrow_button(true),
            "screenshot" => self.take_screenshot(),
            "toggle-flip" => self.mpv.command(&["vf", "toggle", "@iina-flip:vflip"]),
            "toggle-mirror" => self.mpv.command(&["vf", "toggle", "@iina-mirror:hflip"]),
            "save-playlist" => self.save_playlist(),
            "delete-current-file" => self.delete_current_file(),
            "open-path" => self.open(arg, true),
            "open-file" => {
                let files = self.pick_files("Open", &[("Media", MEDIA_EXTS), ("All files", &["*"])]);
                self.open_many(files);
            }
            "add-files" => {
                let files = self.pick_files("Add to playlist", &[("Media", MEDIA_EXTS), ("All files", &["*"])]);
                let n = files.len();
                for f in files {
                    let idle = self.player.idle;
                    self.open_ex(&f, idle, false);
                }
                if n > 0 {
                    self.ui.show_osd(i18n::tlf("Added {} files to playlist", &[&n]), None);
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
            "close-file" => {
                // Keep the resume position, like IINA does on stop.
                if !self.player.idle {
                    self.mpv.command(&["write-watch-later-config"]);
                }
                self.mpv.command(&["stop"]);
                self.ui.show_osd(i18n::tl("Stop"), None);
            }
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
            // Debug aid: open the context menu at the window center.
            "ctx-menu" => {
                if let Some(g) = self.gfx.as_ref() {
                    let ctx = g.egui.egui_ctx.clone();
                    // "ctx-menu fx fy": at a fraction of the window (debug).
                    let f: Vec<f32> = arg.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if let [fx, fy] = f[..] {
                        let r = ctx.screen_rect();
                        self.ui.context_menu = Some(egui::pos2(r.left() + r.width() * fx, r.top() + r.height() * fy));
                    } else {
                        self.ui.open_context_menu_center(&ctx);
                    }
                }
            }
            // Debug aid: run a key binding by its spec, e.g. "key RIGHT".
            "key" => match self.bindings.lookup(&keys::normalize(arg)).cloned() {
                Some(Binding::Mpv(c)) => self.run_mpv_binding(&c),
                Some(Binding::App(c)) => self.run_app_command(&c),
                None => eprintln!("no binding for {arg}"),
            },
            "inspector" => self.ui.show_inspector = !self.ui.show_inspector,
            "history" => self.ui.show_history = !self.ui.show_history,
            "history-remove" => self.history.remove(arg),
            "history-clear" => self.history.clear(),
            "find-online-subs" => self.open_subtitle_search(),
            "subs-search" => {
                let p = self.proxy.clone();
                let provider = self.ui.subs.lock().unwrap().provider;
                let key = self.ui.settings.subtitle_key(provider).to_string();
                subtitles::start_search(self.ui.subs.clone(), key, move || {
                    let _ = p.send_event(UserEvent::MpvRender);
                });
            }
            "subs-download" => {
                if let Ok(index) = arg.parse::<usize>() {
                    let p = self.proxy.clone();
                    let dest = self.config_dir.join("subtitles");
                    let keys = (self.ui.settings.subdl_key.trim().to_string(), self.ui.settings.opensubtitles_key.trim().to_string());
                    subtitles::start_download(self.ui.subs.clone(), index, keys, dest, move || {
                        let _ = p.send_event(UserEvent::MpvRender);
                    });
                }
            }
            "crop" => self.apply_crop(arg),
            "assoc-register" => {
                self.ui.assoc_status = match assoc::register(VIDEO_AUDIO_EXTS) {
                    Ok(()) => i18n::tl("Registered. Pick IINA-RS under “Open with”, or set it as default in Default apps."),
                    Err(e) => i18n::tlf("Failed: {}", &[&e]),
                };
            }
            "assoc-unregister" => {
                self.ui.assoc_status = match assoc::unregister(VIDEO_AUDIO_EXTS) {
                    Ok(()) => i18n::tl("Removed IINA-RS from “Open with”."),
                    Err(e) => i18n::tlf("Failed: {}", &[&e]),
                };
            }
            "open-default-apps" => assoc::open_default_apps_settings(),
            "open-config-dir" => open_folder(&self.config_dir),
            "open-fonts-dir" => {
                // mpv loads fonts from <config>/fonts for ASS subtitles.
                let dir = self.config_dir.join("fonts");
                let _ = std::fs::create_dir_all(&dir);
                open_folder(&dir);
            }
            "scan-fonts" => {
                // Requested every frame by the open picker until the list is
                // ready; scan only once.
                if !std::mem::replace(&mut self.fonts_scanning, true) {
                    let p = self.proxy.clone();
                    fonts::scan(self.ui.fonts.clone(), self.config_dir.join("fonts"), move || {
                        let _ = p.send_event(UserEvent::MpvRender);
                    });
                }
            }
            "sub-delay-step" => {
                let mut it = arg.split_whitespace();
                let idx = it.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(2);
                let sign = if it.next() == Some("-") { -1.0 } else { 1.0 };
                let step = self.ui.settings.subs.sync_steps[idx];
                self.mpv.command(&["add", "sub-delay", &format!("{:.3}", sign * step)]);
            }
            "preferences" => {
                self.ui.show_prefs = true;
                if arg == "subs" {
                    self.ui.prefs_tab = 2;
                } else if let Ok(tab) = arg.parse::<u8>() {
                    self.ui.prefs_tab = tab.min(5);
                }
            }
            "jump-to" => {
                if !self.player.idle {
                    self.ui.jump_input = Some(String::new());
                }
            }
            "live-text" => {
                self.ui.show_osd(i18n::tl("Not available on this platform yet"), None);
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
        let scale = scale * self.ui.settings.resize_scale.clamp(0.25, 4.0);
        let (mut tw, mut th) = (w * scale * dpi, h * scale * dpi);
        if let Some(mon) = window.current_monitor() {
            let ms = mon.size();
            let (mw, mh) = (ms.width as f64 * 0.85, ms.height as f64 * 0.85);
            let k = (mw / tw).min(mh / th).min(1.0);
            tw *= k;
            th *= k;
            let old_inner = window.inner_size();
            let old_pos = window.outer_position().ok();
            let _ = window.request_inner_size(PhysicalSize::new(tw as u32, th as u32));
            let mp = mon.position();
            let outer = window.outer_size();
            let dx = outer.width.saturating_sub(old_inner.width) as f64;
            let dy = outer.height.saturating_sub(old_inner.height) as f64;
            // Like IINA: centre on the screen the first time, afterwards keep
            // the window's centre where it is.
            let (cx, cy) = match old_pos.filter(|_| self.window_centred) {
                Some(p) => (p.x as f64 + (old_inner.width as f64 + dx) / 2.0, p.y as f64 + (old_inner.height as f64 + dy) / 2.0),
                None => (mp.x as f64 + ms.width as f64 / 2.0, mp.y as f64 + ms.height as f64 / 2.0),
            };
            self.window_centred = true;
            let x = (cx - (tw + dx) / 2.0).clamp(mp.x as f64, (mp.x as f64 + ms.width as f64 - tw - dx).max(mp.x as f64));
            let y = (cy - (th + dy) / 2.0).clamp(mp.y as f64, (mp.y as f64 + ms.height as f64 - th - dy).max(mp.y as f64));
            window.set_outer_position(PhysicalPosition::new(x as i32, y as i32));
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

    /// Leaves fullscreen right away (not when mpv's property change comes back,
    /// which would restore the old window over a PiP / music-mode layout).
    /// Returns the windowed geometry fullscreen had saved, if any.
    fn leave_fullscreen_now(&mut self) -> Option<Geometry> {
        if !self.player.fullscreen {
            return None;
        }
        self.player.fullscreen = false;
        self.ui.fullscreen = false;
        self.mpv.set_flag("fullscreen", false);
        let w = self.window().cloned()?;
        if cfg!(windows) {
            let saved = self.fs_saved.as_ref().map(|(g, _)| Geometry { pos: g.pos, size: g.size });
            self.set_fullscreen_windows(&w, false);
            saved
        } else {
            w.set_fullscreen(None);
            None
        }
    }

    /// The PiP window edge under the cursor (borderless windows have no
    /// system resize frame, so the edges are handled here).
    fn pip_edge(&self) -> Option<ResizeDirection> {
        let g = self.gfx.as_ref()?;
        pip_edge_at(self.ui.pip, &g.window, self.cursor)
    }

    fn remember_pip(&mut self) {
        let Some(w) = self.window() else { return };
        let (Ok(p), s) = (w.outer_position(), w.inner_size()) else { return };
        if s.width < 100 || s.height < 60 {
            return;
        }
        let rect = Some([p.x, p.y, s.width as i32, s.height as i32]);
        if self.ui.settings.pip_rect != rect {
            self.ui.settings.pip_rect = rect;
            self.save_settings_at = Some(Instant::now() + Duration::from_secs(2));
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
        let windowed = self.leave_fullscreen_now();
        if self.ui.music_mode {
            self.ui.music_mode = false;
        } else if windowed.is_some() {
            self.saved_geometry = windowed;
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
        if w.is_maximized() {
            w.set_maximized(false);
        }
        w.set_decorations(false);
        w.set_window_level(WindowLevel::AlwaysOnTop);
        if let Some([x, y, pw, _]) = self.ui.settings.pip_rect {
            let on_screen = w.available_monitors().any(|m| {
                let (p, s) = (m.position(), m.size());
                x >= p.x - 50 && y >= p.y - 50 && x < p.x + s.width as i32 - 50 && y < p.y + s.height as i32 - 50
            });
            if on_screen && pw >= 160 {
                // Same place and width as last time; height follows this video.
                let height = pw as f64 * vh.max(1.0) / vw.max(1.0);
                let _ = w.request_inner_size(PhysicalSize::new(pw as u32, height as u32));
                w.set_outer_position(PhysicalPosition::new(x, y));
                return;
            }
        }
        if let Some(mon) = w.current_monitor() {
            let ms = mon.size();
            let width = ms.width as f64 * 0.25;
            let height = width * vh.max(1.0) / vw.max(1.0);
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
        let windowed = self.leave_fullscreen_now();
        if self.ui.pip {
            self.ui.pip = false;
            if let Some(w) = self.window() {
                w.set_decorations(true);
            }
        } else if windowed.is_some() {
            self.saved_geometry = windowed;
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
                Event::Hook(name, id) => {
                    if name == "on_load" {
                        self.on_load_hook(id);
                    } else {
                        self.mpv.hook_continue(id);
                    }
                }
                Event::Seek => {
                    // IINA shows the position on every seek (keys, bar, mpv).
                    if self.osd_armed_at.is_some_and(|t| Instant::now() >= t) {
                        self.show_seek_osd();
                    }
                }
                Event::StartFile => {
                    self.fullscreen_applied = false;
                    self.resize_on_video = self.ui.settings.resize_to_video;
                    self.osd_armed_at = Some(Instant::now() + Duration::from_millis(1200));
                }
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
                    if self.ui.settings.fullscreen_on_open && !std::mem::replace(&mut self.fullscreen_applied, true) && !self.player.fullscreen {
                        self.mpv.set_flag("fullscreen", true);
                    }
                    if !title.is_empty() {
                        self.ui.show_osd(title.clone(), None);
                    }
                }
                Event::EndFile => self.flush_history(),
                Event::PlaybackRestart => {
                    if !self.debug_cmds_done && std::env::var_os("IINA_RS_CMD_DELAY").is_none() {
                        self.run_debug_cmds();
                    }
                }
                Event::VideoReconfig => {
                    self.update_hdr_target();
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
                    self.osd_for_property(&name);
                    if name == "chapter" && std::mem::take(&mut self.osd_chapter) {
                        let n = self.player.chapters.len();
                        if let Some(i) = usize::try_from(self.player.chapter).ok().filter(|i| *i < n) {
                            let title = self.player.chapters[i].title.clone();
                            self.ui.show_osd_ex(
                                i18n::tlf("Chapter: {}", &[&title]),
                                ui::OsdDetail::Time(format!("({}/{})", i + 1, n)),
                                ui::OsdBar::None,
                            );
                        }
                    }
                    if name == "volume" && self.ui.settings.remember_volume && self.osd_armed_at.is_some_and(|t| Instant::now() >= t) {
                        // Remember the last volume (IINA's softVolume).
                        self.ui.settings.volume = self.player.volume;
                        self.save_settings_at = Some(Instant::now() + Duration::from_secs(2));
                    }
                    if matches!(name.as_str(), "pause" | "idle-active" | "vid") {
                        self.update_keep_awake();
                    }
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
        if let Some(vk) = gfx.vk.as_mut() {
            let (w, h) = vk.targets.size();
            vk.targets.begin_frame();
            self.mpv.render(vk.targets.video_fbo as i32, w, h, GL_RGBA16F);
            // The UI goes into its own (transparent) texture.
            use glow::HasContext;
            let gl = gfx.egui.painter.gl();
            unsafe {
                gl.bind_framebuffer(glow::FRAMEBUFFER, NonZeroU32::new(vk.targets.ui_fbo).map(glow::NativeFramebuffer));
                gl.viewport(0, 0, w, h);
                gl.disable(glow::SCISSOR_TEST);
                gl.clear_color(0.0, 0.0, 0.0, 0.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
        } else {
            self.mpv.render(0, size.width as i32, size.height as i32, 0);
        }
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
                "[color] in: {}/{}/{} sig-peak={}  ->  out target: {}/{}  current-vo={} vo-gpu-api={}",
                g("video-params/primaries"), g("video-params/gamma"), g("video-params/pixelformat"), g("video-params/sig-peak"),
                g("video-target-params/primaries"), g("video-target-params/gamma"), g("current-vo"), g("gpu-api")
            );
            eprintln!(
                "[audio] {} {}ch {}  ->  ao={} out={}ch {} Hz",
                g("audio-codec-name"), g("audio-params/channel-count"), g("audio-params/hr-channels"),
                g("current-ao"), g("audio-out-params/channel-count"), g("audio-out-params/samplerate")
            );
            eprintln!(
                "[stats] frames={} t={} hwdec={} vo-drop={} dec-drop={} delayed={} vf-fps={} display-fps={} size={}x{}",
                std::mem::take(&mut self.frames), g("time-pos"), g("hwdec-current"), g("frame-drop-count"), g("decoder-frame-drop-count"),
                g("vo-delayed-frame-count"), g("estimated-vf-fps"), g("display-fps"), size.width, size.height
            );
        }
        let snap_after = std::env::var("IINA_RS_SNAPSHOT_AFTER").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
        if let Some(vk) = gfx.vk.as_mut() {
            let video_hdr = vk.video_hdr.unwrap_or(false);
            let gl = gfx.egui.painter.gl().clone();
            vk.targets.composite(&gl, video_hdr, vk.display.sdr_white_nits, vk.presenter.hdr);
        }
        let snapshot_due = self.snapshot.is_some() && self.started.elapsed() > Duration::from_secs(snap_after);
        if let Some(path) = self.snapshot.as_ref().filter(|_| snapshot_due) {
            if let Some(vk) = gfx.vk.as_ref() {
                // PQ-encoded HDR10 frame, already top-down.
                use glow::HasContext;
                let gl = gfx.egui.painter.gl();
                unsafe { gl.bind_framebuffer(glow::READ_FRAMEBUFFER, NonZeroU32::new(vk.targets.shared_fbo).map(glow::NativeFramebuffer)) };
                let (w, h) = vk.targets.size();
                save_snapshot_ex(gl, PhysicalSize::new(w as u32, h as u32), path, false);
            } else {
                save_snapshot(gfx.egui.painter.gl(), size, path);
            }
            self.snapshot = None;
            self.mpv.command(&["quit"]);
        }
        let t1 = Instant::now();
        if let Some(vk) = gfx.vk.as_mut() {
            let gl = gfx.egui.painter.gl().clone();
            vk.targets.end_frame(&gl);
            match vk.presenter.present() {
                Ok(true) => {}
                Ok(false) => {
                    let s = gfx.window.inner_size();
                    if let Err(e) = vk.presenter.resize(s.width, s.height).and_then(|_| vk.targets.recreate(&gl, &vk.presenter)) {
                        eprintln!("Vulkan resize: {e}");
                    }
                }
                Err(e) => eprintln!("Vulkan present: {e}"),
            }
        } else {
            let _ = gfx.surface.swap_buffers(&gfx.context);
        }
        self.mpv.report_swap();
        let t_swap = t1.elapsed();
        if t0.elapsed() > Duration::from_millis(60) {
            dbg_log(format!("slow frame: mpv={:?} ui={:?} swap={:?} size={}x{}", t_mpv, t1 - t0 - t_mpv, t_swap, size.width, size.height));
        }

        // Like IINA, lift subtitles above the controller while it is shown.
        // sub-margin-y is in units of a 720px-tall frame; the bar is ~130pt high.
        // The user's bottom margin is the base; the controller lifts it further.
        let lift = self.ui.osc_visible && !self.player.idle && !self.ui.pip;
        let base = self.ui.settings.subs.margin;
        let margin = if lift {
            let h_pt = size.height as f64 / window.scale_factor();
            (130.0 / h_pt * 720.0).clamp(22.0, 300.0).max(base)
        } else {
            base
        }
        .round() as i64;
        if Some(margin) != self.sub_margin_applied {
            self.sub_margin_applied = Some(margin);
            self.mpv.set_string("sub-margin-y", &margin.to_string());
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

    /// Runs an input.conf command, routing mpv's OSD commands to our OSD and
    /// arming OSD feedback for actions that don't change a watched property.
    fn run_mpv_binding(&mut self, cmd: &str) {
        let first = cmd.split_whitespace().next().unwrap_or("");
        if cmd.split_whitespace().collect::<Vec<_>>() == ["cycle", "pause"] {
            self.toggle_pause();
            return;
        }
        if cmd.trim() == "screenshot" {
            self.take_screenshot();
            return;
        }
        match first {
            "show-progress" => {
                self.osd_seek = true;
                self.show_seek_osd();
                return;
            }
            "show-text" => {
                let text = cmd["show-text".len()..].trim().trim_matches('"').to_string();
                self.ui.show_osd(text, None);
                return;
            }
            "screenshot" => self.ui.show_osd(i18n::tl("Screenshot Captured"), None),
            _ => {}
        }
        if cmd.contains("chapter") {
            self.osd_chapter = true;
        }
        self.mpv.command_string(cmd);
    }

    fn show_seek_osd(&mut self) {
        // The OSD shows the live position (updated while it is visible).
        self.ui.show_time_osd();
    }

    /// Play/pause; at the end of a file (keep-open), play restarts it (IINA).
    fn toggle_pause(&mut self) {
        if self.player.idle {
            return;
        }
        if self.player.pause && self.player.eof_reached {
            self.mpv.command(&["seek", "0", "absolute"]);
            self.mpv.set_flag("pause", false);
        } else {
            self.mpv.command(&["cycle", "pause"]);
        }
    }

    /// The controller's ⏪ ⏩ buttons (IINA: speed by default).
    fn arrow_button(&mut self, forward: bool) {
        match self.ui.settings.arrow_action.as_str() {
            "playlist" if self.player.playlist_count > 1 => self.mpv.command(&[if forward { "playlist-next" } else { "playlist-prev" }]),
            "playlist" | "seek" => {
                let step = self.ui.settings.arrow_seek_step.max(1.0);
                self.mpv.command(&["seek", &format!("{}", if forward { step } else { -step })]);
            }
            _ => {
                let speed = self.player.speed;
                let new = if forward {
                    if speed < 1.0 { 1.0 } else { (speed * 2.0).min(32.0) }
                } else if speed > 1.0 {
                    1.0
                } else {
                    (speed / 2.0).max(0.25)
                };
                self.mpv.set_double("speed", new);
            }
        }
    }

    fn take_screenshot(&mut self) {
        let mode = if self.ui.settings.screenshot_with_subs { "subtitles" } else { "video" };
        self.mpv.command(&["screenshot", mode]);
        self.ui.show_osd(i18n::tl("Screenshot Captured"), None);
    }

    /// Mouse actions from Preferences.
    fn mouse_action(&mut self, action: &str) {
        match action {
            "pause" => self.toggle_pause(),
            "fullscreen" if !self.player.idle && !self.ui.pip => self.mpv.command(&["cycle", "fullscreen"]),
            "fullscreen" if self.ui.pip => self.toggle_pip(),
            "hide-osc" => {
                // Hide controller and cursor until the mouse moves again.
                self.ui.last_activity = Instant::now() - Duration::from_secs(3600);
                self.redraw();
            }
            "pip" => self.toggle_pip(),
            "menu" => {
                if let Some(g) = self.gfx.as_ref() {
                    let ppp = g.egui.egui_ctx.pixels_per_point() as f64;
                    self.ui.context_menu = Some(egui::pos2((self.cursor.x / ppp) as f32, (self.cursor.y / ppp) as f32));
                    self.redraw();
                }
            }
            _ => {}
        }
    }

    /// Scroll action ("volume", "seek", "speed") by `steps` notches.
    fn scroll_action(&mut self, action: &str, steps: f64) {
        if steps == 0.0 {
            return;
        }
        match action {
            "volume" => self.mpv.command(&["add", "volume", &format!("{:.2}", steps * 2.0)]),
            "seek" => self.mpv.command(&["seek", &format!("{:.2}", steps * 2.0)]),
            "speed" => self.mpv.command(&["multiply", "speed", &format!("{:.4}", 1.05f64.powf(steps))]),
            _ => {}
        }
    }

    fn save_playlist(&mut self) {
        if self.player.playlist.is_empty() {
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("M3U8", &["m3u8"]).set_file_name("playlist.m3u8").save_file() else { return };
        let mut text = String::from("#EXTM3U\n");
        for item in &self.player.playlist {
            if let Some(t) = &item.title {
                text.push_str(&format!("#EXTINF:-1,{t}\n"));
            }
            text.push_str(&item.filename);
            text.push('\n');
        }
        let _ = std::fs::write(path, text);
    }

    /// Moves the playing file to the recycle bin / trash (after confirming).
    fn delete_current_file(&mut self) {
        let path = self.player.path.clone();
        if path.is_empty() || path.contains("://") {
            return;
        }
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ok = rfd::MessageDialog::new()
            .set_title("IINA-RS")
            .set_description(i18n::tlf("Move “{}” to the Recycle Bin?", &[&name]))
            .set_buttons(rfd::MessageButtons::YesNo)
            .set_level(rfd::MessageLevel::Warning)
            .show();
        if ok != rfd::MessageDialogResult::Yes {
            return;
        }
        let pos = self.mpv.get_string("playlist-pos").unwrap_or_default();
        if self.player.playlist_count > 1 {
            self.mpv.command(&["playlist-next", "force"]);
            self.mpv.command(&["playlist-remove", &pos]);
        } else {
            self.mpv.command(&["stop"]);
        }
        if let Err(e) = trash::delete(&path) {
            self.ui.show_osd(format!("{e}"), None);
        }
    }

    /// Keeps the display awake while video plays (IINA: prevent screensaver).
    fn update_keep_awake(&mut self) {
        let want = self.ui.settings.prevent_sleep && !self.player.idle && !self.player.pause && !self.player.vid.is_empty() && self.player.vid != "no";
        if want == self.keep_awake {
            return;
        }
        self.keep_awake = want;
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
            let flags = if want { ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
            SetThreadExecutionState(flags);
        }
    }

    /// on_load hook: add subtitles found in subfolders before mpv selects tracks.
    fn on_load_hook(&mut self, id: u64) {
        if self.ui.settings.subs.search_subfolders {
            let path = self.mpv.get_string("path").unwrap_or_default();
            if !path.is_empty() && !path.contains("://") {
                let found = sub_finder::find(Path::new(&path));
                if !found.is_empty() {
                    let sep = if cfg!(windows) { ";" } else { ":" };
                    let list: Vec<String> = found.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                    dbg_log(format!("subfolder subtitles: {list:?}"));
                    self.mpv.set_string("file-local-options/sub-files", &list.join(sep));
                }
            }
        }
        self.mpv.hook_continue(id);
    }

    fn track_desc(&self, kind: &str, id: &str) -> String {
        if id == "no" || id.is_empty() {
            return i18n::tl("None");
        }
        let Some(t) = self.player.tracks.iter().find(|t| t.kind == kind && t.id.to_string() == id) else { return format!("#{id}") };
        let mut s = format!("#{}", t.id);
        for part in [&t.title, &t.lang, &t.codec].into_iter().flatten() {
            s.push(' ');
            s.push_str(part);
        }
        s
    }

    /// IINA-style feedback for user-visible property changes.
    fn osd_for_property(&mut self, name: &str) {
        if self.osd_armed_at.is_none_or(|t| Instant::now() < t) {
            return;
        }
        use i18n::{tl, tlf};
        let p = &self.player;
        let onoff = |b: bool| tl(if b { "On" } else { "Off" });
        let delay = |d: f64| {
            if d.abs() < 0.0005 {
                tl("No Delay")
            } else if d > 0.0 {
                tlf("{}s Later", &[&trim_num(d)])
            } else {
                tlf("{}s Earlier", &[&trim_num(-d)])
            }
        };
        let vmax = self.ui.settings.volume_max.max(100.0);
        match name {
            "pause" => {
                self.ui.show_osd_ex(tl(if p.pause { "Pause" } else { "Resume" }), ui::OsdDetail::Time(String::new()), ui::OsdBar::None);
                return;
            }
            "sub-delay" | "audio-delay" => {
                let (label, d) = if name == "sub-delay" { ("Subtitle Delay: {}", p.sub_delay) } else { ("Audio Delay: {}", p.audio_delay) };
                self.ui.show_osd_ex(tlf(label, &[&delay(d)]), ui::OsdDetail::None, ui::OsdBar::Centered((d / 10.0).clamp(-1.0, 1.0) as f32));
                return;
            }
            _ => {}
        }
        let msg: Option<(String, Option<f32>)> = match name {
            "volume" => Some((tlf("Volume: {}", &[&format!("{:.0}", p.volume)]), Some((p.volume / vmax) as f32))),
            "mute" => Some((tl(if p.mute { "Mute" } else { "Unmute" }), None)),
            "speed" => Some((tlf("Speed: {}x", &[&trim_num(p.speed)]), None)),
            "sid" => Some((tlf("Subtitle: {}", &[&self.track_desc("sub", &p.sid)]), None)),
            "secondary-sid" => Some((tlf("Secondary Subtitle: {}", &[&self.track_desc("sub", &p.secondary_sid)]), None)),
            "aid" => Some((tlf("Audio: {}", &[&self.track_desc("audio", &p.aid)]), None)),
            "vid" => Some((tlf("Video: {}", &[&self.track_desc("video", &p.vid)]), None)),
            "sub-scale" => Some((tlf("Subtitle Scale: {}x", &[&format!("{:.2}", p.sub_scale)]), None)),
            "sub-pos" => Some((tlf("Subtitle Position: {}", &[&format!("{:.0}", p.sub_pos)]), Some((p.sub_pos / 150.0) as f32))),
            "sub-visibility" => Some((tl(if p.sub_visible { "Subtitles Visible" } else { "Subtitles Hidden" }), None)),
            "loop-file" => Some((tl(if p.loop_file { "Enable file looping" } else { "Disable loop" }), None)),
            "loop-playlist" => Some((tl(if p.loop_playlist { "Enable playlist looping" } else { "Disable loop" }), None)),
            "ab-loop-a" | "ab-loop-b" => Some((
                match (p.ab_loop_a, p.ab_loop_b) {
                    (Some(a), Some(b)) => tlf("A-B Loop: {} – {}", &[&ui::fmt_time(a), &ui::fmt_time(b)]),
                    (Some(a), None) => tlf("A-B Loop: A = {}", &[&ui::fmt_time(a)]),
                    _ => tl("A-B Loop: Off"),
                },
                None,
            )),
            "video-aspect-override" => Some((
                tlf("Aspect Ratio: {}", &[&if p.aspect.is_empty() || p.aspect.starts_with('-') || p.aspect == "no" { tl("Default") } else { aspect_name(&p.aspect) }]),
                None,
            )),
            "video-rotate" => Some((tlf("Rotation: {}°", &[&p.rotate]), None)),
            "deinterlace" => Some((tlf("Deinterlace: {}", &[&onoff(p.deinterlace)]), None)),
            "hwdec" => Some((tlf("Hardware Decoding: {}", &[&onoff(p.hwdec_enabled)]), None)),
            "brightness" | "contrast" | "saturation" | "gamma" | "hue" => {
                let (label, val) = match name {
                    "brightness" => ("Brightness", p.brightness),
                    "contrast" => ("Contrast", p.contrast),
                    "saturation" => ("Saturation", p.saturation),
                    "gamma" => ("Gamma", p.gamma),
                    _ => ("Hue", p.hue),
                };
                Some((format!("{}: {val}", tl(label)), Some(((val + 100) as f32) / 200.0)))
            }
            _ => None,
        };
        if let Some((text, progress)) = msg {
            self.ui.show_osd(text, progress);
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
        // Search text: the title parsed from the file name (no year / release tags).
        let file_name = Path::new(&path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let query = subtitles::parse_name(&file_name).title;
        {
            let mut s = self.ui.subs.lock().unwrap();
            if s.media_path != path {
                s.results.clear();
                s.status.clear();
            }
            s.media_path = path;
            s.query = query.split_whitespace().collect::<Vec<_>>().join(" ");
            s.languages = self.ui.settings.sub_languages.clone();
            s.provider = subtitles::Provider::from_name(&self.ui.settings.sub_provider);
        }
        self.ui.show_subs = true;
    }

    /// Crops the video to the aspect ratio in `arg` ("<ratio> <label>" or "none None").
    fn apply_crop(&mut self, arg: &str) {
        let (ratio, label) = arg.split_once(' ').unwrap_or((arg, arg));
        if ratio == "none" {
            self.mpv.set_string("video-crop", "");
            self.player.crop_label = "None".into();
            self.ui.show_osd(i18n::tlf("Crop: {}", &[&i18n::tl("None")]), None);
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
        self.ui.show_osd(i18n::tlf("Crop: {}", &[&label]), None);
    }

    fn apply_settings(&mut self) {
        let s = self.ui.settings.clone();
        i18n::set_language(i18n::resolve(&s.language));
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
        self.thumbs = Some(thumbs::start(path, self.config_dir.join("thumbnails"), move || {
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

/// 1.5 → "1.5", 2.0 → "2", 1.25 → "1.25" (IINA's number format).
fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn save_snapshot(gl: &glow::Context, size: PhysicalSize<u32>, path: &str) {
    save_snapshot_ex(gl, size, path, true)
}

fn save_snapshot_ex(gl: &glow::Context, size: PhysicalSize<u32>, path: &str, flip: bool) {
    use glow::HasContext;
    let (w, h) = (size.width as i32, size.height as i32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    unsafe {
        gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut px)));
    }
    let row = (w * 4) as usize;
    let flipped: Vec<u8> = if flip { px.chunks(row).rev().flatten().copied().collect() } else { px };
    if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, flipped) {
        let _ = img.save(path);
    }
}

/// The OS double-click time (Windows), else 400 ms.
fn double_click_interval() -> Duration {
    #[cfg(windows)]
    unsafe {
        let ms = windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime();
        if ms > 0 {
            return Duration::from_millis(ms as u64);
        }
    }
    Duration::from_millis(400)
}

fn open_folder(dir: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
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
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
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
        // Debug aid: IINA_RS_CMD_DELAY=secs runs IINA_RS_CMD after a delay.
        if let Some(d) = std::env::var("IINA_RS_CMD_DELAY").ok().and_then(|v| v.parse::<f64>().ok()) {
            if !self.debug_cmds_done {
                let at = self.started + Duration::from_secs_f64(d);
                if Instant::now() >= at {
                    self.run_debug_cmds();
                } else if self.repaint_at.is_none_or(|r| at < r) {
                    self.repaint_at = Some(at);
                }
            }
        }
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
        if let Some((t, action)) = self.click_pause_at.clone() {
            if t <= Instant::now() {
                self.click_pause_at = None;
                self.mouse_action(&action);
            } else if self.repaint_at.is_none_or(|r| t < r) {
                self.repaint_at = Some(t);
            }
        }
        if std::mem::take(&mut self.pip_moved) && self.ui.pip {
            self.remember_pip();
        }
        if let Some(t) = self.save_settings_at {
            if t <= Instant::now() {
                self.save_settings_at = None;
                self.ui.settings.save(&self.config_dir);
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

    fn window_event(&mut self, el: &ActiveEventLoop, id: winit::window::WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        // Only the main window matters (not the hidden GL helper window, nor a
        // window discarded when HDR output wasn't possible).
        if id != gfx.window.id() {
            return;
        }
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
            WindowEvent::Moved(_) if self.ui.pip => self.pip_moved = true,
            WindowEvent::Resized(size) => {
                if self.ui.pip {
                    self.pip_moved = true;
                }
                dbg_log(format!("Resized {}x{} win_fs={} mpv_fs={}", size.width, size.height, gfx.window.fullscreen().is_some(), self.player.fullscreen));
                if let Some(vk) = gfx.vk.as_mut() {
                    if size.width > 0 && size.height > 0 {
                        let gl = gfx.egui.painter.gl().clone();
                        if let Err(e) = vk.presenter.resize(size.width, size.height).and_then(|_| vk.targets.recreate(&gl, &vk.presenter)) {
                            eprintln!("Vulkan resize: {e}");
                        }
                    }
                } else if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
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
                        Some(Binding::Mpv(cmd)) => self.run_mpv_binding(&cmd),
                        Some(Binding::App(cmd)) => self.run_app_command(&cmd),
                        None => {}
                    }
                    // Keys show the OSD, not the controller (IINA).
                    self.redraw();
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
                if self.ui.pip {
                    // Resize cursor on the borderless PiP window's edges.
                    let icon = match pip_edge_at(true, &gfx.window, self.cursor) {
                        Some(ResizeDirection::East | ResizeDirection::West) => Some(CursorIcon::EwResize),
                        Some(ResizeDirection::North | ResizeDirection::South) => Some(CursorIcon::NsResize),
                        Some(ResizeDirection::NorthWest | ResizeDirection::SouthEast) => Some(CursorIcon::NwseResize),
                        Some(_) => Some(CursorIcon::NeswResize),
                        None => None,
                    };
                    if let Some(icon) = icon {
                        gfx.window.set_cursor(icon);
                    } else if self.pip_cursor_set {
                        gfx.window.set_cursor(CursorIcon::Default);
                    }
                    self.pip_cursor_set = icon.is_some();
                }
                if moved && self.cursor_hidden {
                    self.cursor_hidden = false;
                    gfx.window.set_cursor_visible(true);
                }
                if moved {
                    self.on_activity();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let over_ui = egui_ctx.is_pointer_over_area() || egui_ctx.wants_pointer_input();
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) if self.pip_edge().is_some() => {
                        if let (Some(dir), Some(g)) = (self.pip_edge(), self.gfx.as_ref()) {
                            let _ = g.window.drag_resize_window(dir);
                        }
                    }
                    (MouseButton::Left, ElementState::Pressed) if !over_ui => {
                        self.ui.context_menu = None;
                        let now = Instant::now();
                        let interval = double_click_interval();
                        let double = self.ui.settings.double_click.clone();
                        if self.last_click.is_some_and(|t| now.duration_since(t) < interval) && double != "none" {
                            self.last_click = None;
                            self.press = None;
                            self.click_pause_at = None;
                            self.mouse_action(&double);
                        } else {
                            self.last_click = Some(now);
                            self.press = Some((self.cursor, now));
                            let single = self.ui.settings.single_click.clone();
                            if single != "none" && !self.player.idle {
                                // Wait for a possible double click, unless there's none.
                                let delay = if double == "none" { Duration::ZERO } else { interval };
                                self.click_pause_at = Some((now + delay, single));
                            }
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => self.press = None,
                    (MouseButton::Right, ElementState::Pressed) if !over_ui => {
                        let a = self.ui.settings.right_click.clone();
                        self.mouse_action(&a);
                    }
                    (MouseButton::Middle, ElementState::Pressed) if !over_ui => {
                        let a = self.ui.settings.middle_click.clone();
                        self.mouse_action(&a);
                    }
                    (MouseButton::Back, ElementState::Pressed) => self.mpv.command(&["seek", "-10"]),
                    (MouseButton::Forward, ElementState::Pressed) => self.mpv.command(&["seek", "10"]),
                    _ => {}
                }
                self.on_activity();
            }
            WindowEvent::CursorLeft { .. } => {
                // Leaving the window hides the controller at once (IINA).
                if self.ui.settings.osc_autohide && !self.player.pause {
                    self.ui.last_activity = Instant::now() - Duration::from_secs(3600);
                }
                self.ui.pointer_over_ui = false;
                self.redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.player.idle {
                    return;
                }
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(p) => (p.x / 40.0, p.y / 40.0),
                };
                // Over the seek bar the wheel seeks, over the volume slider it
                // changes the volume (IINA); elsewhere on the UI it does nothing.
                let ppp = egui_ctx.pixels_per_point() as f64;
                let pos = egui::pos2((self.cursor.x / ppp) as f32, (self.cursor.y / ppp) as f32);
                let main = if dy.abs() >= dx.abs() { dy } else { dx };
                if self.ui.seek_bar_rect.is_some_and(|r| r.expand2(egui::vec2(0.0, 8.0)).contains(pos)) && self.ui.osc_visible {
                    self.scroll_action("seek", main);
                } else if self.ui.volume_bar_rect.is_some_and(|r| r.expand2(egui::vec2(4.0, 8.0)).contains(pos)) && self.ui.osc_visible {
                    self.scroll_action("volume", main);
                } else if egui_ctx.is_pointer_over_area() {
                    return;
                } else if dy.abs() >= dx.abs() {
                    let a = self.ui.settings.scroll_vertical.clone();
                    self.scroll_action(&a, dy);
                } else {
                    let a = self.ui.settings.scroll_horizontal.clone();
                    self.scroll_action(&a, dx);
                }
                self.redraw();
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        self.flush_history();
        if self.save_settings_at.take().is_some() {
            self.ui.settings.save(&self.config_dir);
        }
        self.ui.settings.prevent_sleep = false;
        self.update_keep_awake();
        if self.ui.settings.single_instance {
            instance::release(&self.config_dir);
        }
        // The render context must be freed while the GL context is current.
        self.mpv.free_render_context();
        if let Some(mut g) = self.gfx.take() {
            if let Some(vk) = g.vk.as_mut() {
                let gl = g.egui.painter.gl().clone();
                vk.targets.destroy(&gl);
            }
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

/// "1.777778" → "16:9"; other ratios with at most two decimals.
fn aspect_name(raw: &str) -> String {
    let Ok(x) = raw.trim().parse::<f64>() else { return raw.to_string() };
    const NAMES: [(f64, &str); 9] = [
        (4.0 / 3.0, "4:3"),
        (16.0 / 9.0, "16:9"),
        (16.0 / 10.0, "16:10"),
        (2.33, "21:9"),
        (21.0 / 9.0, "21:9"),
        (1.0, "1:1"),
        (5.0 / 4.0, "5:4"),
        (2.35, "2.35:1"),
        (1.85, "1.85:1"),
    ];
    NAMES.iter().find(|(r, _)| (r - x).abs() < 0.005).map(|(_, n)| n.to_string()).unwrap_or_else(|| format!("{}:1", trim_num(x)))
}

/// The PiP window edge under `cursor` (borderless windows have no system
/// resize frame, so the edges are handled by hand).
fn pip_edge_at(pip: bool, window: &Window, cursor: PhysicalPosition<f64>) -> Option<ResizeDirection> {
    if !pip {
        return None;
    }
    let size = window.inner_size();
    let m = 8.0 * window.scale_factor();
    let (x, y) = (cursor.x, cursor.y);
    let (w, h) = (size.width as f64, size.height as f64);
    let (l, r, t, b) = (x < m, x > w - m, y < m, y > h - m);
    Some(match (l, r, t, b) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (.., true) => ResizeDirection::South,
        _ => return None,
    })
}
