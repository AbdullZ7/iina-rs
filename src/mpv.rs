//! Minimal libmpv binding, loaded at runtime with `libloading` so the same
//! binary works with whichever libmpv the system (or the bundle) provides.

use libloading::Library;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

#[allow(non_camel_case_types)]
type mpv_handle = c_void;
#[allow(non_camel_case_types)]
type mpv_render_context = c_void;

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_START_FILE: c_int = 6;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_VIDEO_RECONFIG: c_int = 17;
pub const EVENT_SEEK: c_int = 20;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;
pub const EVENT_HOOK: c_int = 25;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

const RENDER_PARAM_INVALID: c_int = 0;
const RENDER_PARAM_API_TYPE: c_int = 1;
const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const RENDER_PARAM_OPENGL_FBO: c_int = 3;
const RENDER_PARAM_FLIP_Y: c_int = 4;
const RENDER_PARAM_SW_SIZE: c_int = 17;
const RENDER_PARAM_SW_FORMAT: c_int = 18;
const RENDER_PARAM_SW_STRIDE: c_int = 19;
const RENDER_PARAM_SW_POINTER: c_int = 20;

#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct RawEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct RawEventHook {
    name: *const c_char,
    id: u64,
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

type GetProcAddress = unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: GetProcAddress,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

struct Api {
    create: unsafe extern "C" fn() -> *mut mpv_handle,
    initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    set_option_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    set_property: unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    get_property: unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    command_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> c_int,
    observe_property: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut RawEvent,
    set_wakeup_callback:
        unsafe extern "C" fn(*mut mpv_handle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    free: unsafe extern "C" fn(*mut c_void),
    hook_add: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    hook_continue: unsafe extern "C" fn(*mut mpv_handle, u64) -> c_int,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    render_context_create:
        unsafe extern "C" fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut RenderParam) -> c_int,
    render_context_set_update_callback:
        unsafe extern "C" fn(*mut mpv_render_context, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    render_context_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    render_context_render: unsafe extern "C" fn(*mut mpv_render_context, *mut RenderParam) -> c_int,
    render_context_report_swap: unsafe extern "C" fn(*mut mpv_render_context),
    render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
    _lib: Library,
}

fn library_candidates() -> Vec<std::path::PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["libmpv-2.dll", "mpv-2.dll", "mpv-1.dll"]
    } else if cfg!(target_os = "macos") {
        &["libmpv.2.dylib", "libmpv.dylib"]
    } else {
        &["libmpv.so.2", "libmpv.so.1", "libmpv.so"]
    };
    let mut out = Vec::new();
    // Prefer a libmpv shipped next to the executable (portable bundle).
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())) {
        for n in names {
            out.push(dir.join(n));
            out.push(dir.join("lib").join(n));
            // macOS .app bundle: Contents/MacOS/iina-rs -> Contents/Frameworks.
            out.push(dir.join("../Frameworks").join(n));
        }
    }
    for n in names {
        out.push(n.into());
    }
    out
}

impl Api {
    fn load() -> Result<Self, String> {
        let mut last_err = String::from("no candidates");
        for path in library_candidates() {
            if path.is_absolute() && !path.exists() {
                continue;
            }
            match unsafe { Library::new(&path) } {
                Ok(lib) => return unsafe { Self::from_lib(lib) }.map_err(|e| e.to_string()),
                Err(e) => last_err = format!("{}: {e}", path.display()),
            }
        }
        Err(format!("could not load libmpv ({last_err})"))
    }

    unsafe fn from_lib(lib: Library) -> Result<Self, libloading::Error> {
        macro_rules! sym {
            ($name:literal) => {
                *lib.get(concat!($name, "\0").as_bytes())?
            };
        }
        Ok(Self {
            create: sym!("mpv_create"),
            initialize: sym!("mpv_initialize"),
            terminate_destroy: sym!("mpv_terminate_destroy"),
            set_option_string: sym!("mpv_set_option_string"),
            set_property: sym!("mpv_set_property"),
            get_property: sym!("mpv_get_property"),
            command: sym!("mpv_command"),
            command_string: sym!("mpv_command_string"),
            observe_property: sym!("mpv_observe_property"),
            wait_event: sym!("mpv_wait_event"),
            set_wakeup_callback: sym!("mpv_set_wakeup_callback"),
            free: sym!("mpv_free"),
            hook_add: sym!("mpv_hook_add"),
            hook_continue: sym!("mpv_hook_continue"),
            error_string: sym!("mpv_error_string"),
            render_context_create: sym!("mpv_render_context_create"),
            render_context_set_update_callback: sym!("mpv_render_context_set_update_callback"),
            render_context_update: sym!("mpv_render_context_update"),
            render_context_render: sym!("mpv_render_context_render"),
            render_context_report_swap: sym!("mpv_render_context_report_swap"),
            render_context_free: sym!("mpv_render_context_free"),
            _lib: lib,
        })
    }
}

#[derive(Debug, Clone)]
pub enum PropValue {
    None,
    Str(String),
    Flag(bool),
    Int(i64),
    Double(f64),
}

#[derive(Debug)]
pub enum Event {
    Shutdown,
    StartFile,
    EndFile,
    FileLoaded,
    VideoReconfig,
    PlaybackRestart,
    Seek,
    /// A hook registered with `hook_add`; call `hook_continue(id)` when done.
    Hook(String, u64),
    Property(String, PropValue),
    Other,
}

pub struct Mpv {
    api: Api,
    handle: *mut mpv_handle,
    render: *mut mpv_render_context,
    // Keeps the boxed closures alive for as long as mpv may call them.
    _wakeup: Option<Box<Box<dyn Fn() + Send + Sync>>>,
    _update: Option<Box<Box<dyn Fn() + Send + Sync>>>,
    _proc: Option<Box<Box<dyn Fn(&CStr) -> *const c_void>>>,
}

unsafe extern "C" fn trampoline(ctx: *mut c_void) {
    let f = &*(ctx as *const Box<dyn Fn() + Send + Sync>);
    f();
}

unsafe extern "C" fn proc_trampoline(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let f = &*(ctx as *const Box<dyn Fn(&CStr) -> *const c_void>);
    f(CStr::from_ptr(name)) as *mut c_void
}

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap()
}

impl Mpv {
    /// Creates the core. Options must be set via `set_option` before `init`.
    pub fn new() -> Result<Self, String> {
        let api = Api::load()?;
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        Ok(Self { api, handle, render: ptr::null_mut(), _wakeup: None, _update: None, _proc: None })
    }

    fn err(&self, code: c_int) -> String {
        unsafe { CStr::from_ptr((self.api.error_string)(code)) }.to_string_lossy().into_owned()
    }

    pub fn set_option(&self, name: &str, value: &str) {
        let (n, v) = (cstr(name), cstr(value));
        let r = unsafe { (self.api.set_option_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        if r < 0 {
            eprintln!("mpv: option {name}={value}: {}", self.err(r));
        }
    }

    pub fn init(&self) -> Result<(), String> {
        let r = unsafe { (self.api.initialize)(self.handle) };
        if r < 0 { Err(self.err(r)) } else { Ok(()) }
    }

    pub fn command(&self, args: &[&str]) {
        let owned: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        let r = unsafe { (self.api.command)(self.handle, ptrs.as_mut_ptr()) };
        if r < 0 {
            eprintln!("mpv: command {args:?}: {}", self.err(r));
        }
    }

    pub fn command_string(&self, cmd: &str) {
        let c = cstr(cmd);
        let r = unsafe { (self.api.command_string)(self.handle, c.as_ptr()) };
        if r < 0 {
            eprintln!("mpv: command `{cmd}`: {}", self.err(r));
        }
    }

    pub fn set_flag(&self, name: &str, v: bool) {
        let n = cstr(name);
        let mut val: c_int = v as c_int;
        unsafe { (self.api.set_property)(self.handle, n.as_ptr(), FORMAT_FLAG, &mut val as *mut _ as *mut c_void) };
    }

    pub fn set_double(&self, name: &str, v: f64) {
        let n = cstr(name);
        let mut val = v;
        let r = unsafe { (self.api.set_property)(self.handle, n.as_ptr(), FORMAT_DOUBLE, &mut val as *mut _ as *mut c_void) };
        if r < 0 {
            // Integer-typed options reject doubles; mpv parses strings for any type.
            let s = if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") };
            self.set_string(name, &s);
        }
    }

    pub fn set_string(&self, name: &str, v: &str) {
        let (n, s) = (cstr(name), cstr(v));
        let mut p = s.as_ptr();
        unsafe { (self.api.set_property)(self.handle, n.as_ptr(), FORMAT_STRING, &mut p as *mut _ as *mut c_void) };
    }

    pub fn get_string(&self, name: &str) -> Option<String> {
        let n = cstr(name);
        let mut p: *mut c_char = ptr::null_mut();
        let r = unsafe { (self.api.get_property)(self.handle, n.as_ptr(), FORMAT_STRING, &mut p as *mut _ as *mut c_void) };
        if r < 0 || p.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { (self.api.free)(p as *mut c_void) };
        Some(s)
    }

    pub fn observe(&self, name: &str, format: c_int) {
        let n = cstr(name);
        unsafe { (self.api.observe_property)(self.handle, 0, n.as_ptr(), format) };
    }

    /// Registers a hook (e.g. "on_load"); mpv waits until `hook_continue`.
    pub fn hook_add(&self, name: &str, priority: i32) {
        let n = cstr(name);
        unsafe { (self.api.hook_add)(self.handle, 0, n.as_ptr(), priority) };
    }

    pub fn hook_continue(&self, id: u64) {
        unsafe { (self.api.hook_continue)(self.handle, id) };
    }

    pub fn set_wakeup_callback(&mut self, f: impl Fn() + Send + Sync + 'static) {
        let b: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(f));
        let ctx = &*b as *const _ as *mut c_void;
        unsafe { (self.api.set_wakeup_callback)(self.handle, Some(trampoline), ctx) };
        self._wakeup = Some(b);
    }

    fn convert(&self, ev: &RawEvent) -> Option<Event> {
        Some(match ev.event_id {
            EVENT_NONE => return None,
            EVENT_SHUTDOWN => Event::Shutdown,
            EVENT_START_FILE => Event::StartFile,
            EVENT_END_FILE => Event::EndFile,
            EVENT_FILE_LOADED => Event::FileLoaded,
            EVENT_VIDEO_RECONFIG => Event::VideoReconfig,
            EVENT_PLAYBACK_RESTART => Event::PlaybackRestart,
            EVENT_SEEK => Event::Seek,
            EVENT_HOOK => {
                let h = unsafe { &*(ev.data as *const RawEventHook) };
                Event::Hook(unsafe { CStr::from_ptr(h.name) }.to_string_lossy().into_owned(), h.id)
            }
            EVENT_PROPERTY_CHANGE => {
                let p = unsafe { &*(ev.data as *const RawEventProperty) };
                let name = unsafe { CStr::from_ptr(p.name) }.to_string_lossy().into_owned();
                let val = unsafe {
                    match p.format {
                        FORMAT_STRING if !p.data.is_null() => {
                            PropValue::Str(CStr::from_ptr(*(p.data as *const *const c_char)).to_string_lossy().into_owned())
                        }
                        FORMAT_FLAG if !p.data.is_null() => PropValue::Flag(*(p.data as *const c_int) != 0),
                        FORMAT_INT64 if !p.data.is_null() => PropValue::Int(*(p.data as *const i64)),
                        FORMAT_DOUBLE if !p.data.is_null() => PropValue::Double(*(p.data as *const f64)),
                        _ => PropValue::None,
                    }
                };
                Event::Property(name, val)
            }
            _ => Event::Other,
        })
    }

    /// Drains all pending events without blocking.
    pub fn poll_events(&self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(ev) = self.wait_event(0.0) {
            if !matches!(ev, Event::Other) {
                out.push(ev);
            }
        }
        out
    }

    /// Waits up to `timeout` seconds for the next event; `None` on timeout.
    pub fn wait_event(&self, timeout: f64) -> Option<Event> {
        let ev = unsafe { &*(self.api.wait_event)(self.handle, timeout) };
        self.convert(ev)
    }

    /// Creates the OpenGL render context. The GL context must be current.
    pub fn create_render_context(
        &mut self,
        get_proc: impl Fn(&CStr) -> *const c_void + 'static,
        on_update: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), String> {
        let proc_box: Box<Box<dyn Fn(&CStr) -> *const c_void>> = Box::new(Box::new(get_proc));
        let mut init = OpenGlInitParams {
            get_proc_address: proc_trampoline,
            get_proc_address_ctx: &*proc_box as *const _ as *mut c_void,
        };
        let api_type = cstr("opengl");
        let mut params = [
            RenderParam { kind: RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
            RenderParam { kind: RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        let mut ctx: *mut mpv_render_context = ptr::null_mut();
        let r = unsafe { (self.api.render_context_create)(&mut ctx, self.handle, params.as_mut_ptr()) };
        if r < 0 {
            return Err(self.err(r));
        }
        self.render = ctx;
        self._proc = Some(proc_box);

        let b: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(on_update));
        let cb_ctx = &*b as *const _ as *mut c_void;
        unsafe { (self.api.render_context_set_update_callback)(ctx, Some(trampoline), cb_ctx) };
        self._update = Some(b);
        Ok(())
    }

    /// Creates a software (CPU memory) render context, used for thumbnails.
    pub fn create_sw_render_context(&mut self, on_update: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
        let api_type = cstr("sw");
        let mut params = [
            RenderParam { kind: RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        let mut ctx: *mut mpv_render_context = ptr::null_mut();
        let r = unsafe { (self.api.render_context_create)(&mut ctx, self.handle, params.as_mut_ptr()) };
        if r < 0 {
            return Err(self.err(r));
        }
        self.render = ctx;
        let b: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(on_update));
        let cb_ctx = &*b as *const _ as *mut c_void;
        unsafe { (self.api.render_context_set_update_callback)(ctx, Some(trampoline), cb_ctx) };
        self._update = Some(b);
        Ok(())
    }

    /// Renders the current frame as RGBX into `buf` (`stride * h` bytes).
    pub fn render_sw(&self, w: i32, h: i32, stride: usize, buf: &mut [u8]) -> bool {
        if self.render.is_null() || buf.len() < stride * h as usize {
            return false;
        }
        let mut size = [w as c_int, h as c_int];
        let format = cstr("rgb0");
        let mut stride = stride;
        let mut params = [
            RenderParam { kind: RENDER_PARAM_SW_SIZE, data: size.as_mut_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_SW_FORMAT, data: format.as_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_SW_STRIDE, data: &mut stride as *mut _ as *mut c_void },
            RenderParam { kind: RENDER_PARAM_SW_POINTER, data: buf.as_mut_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        unsafe { (self.api.render_context_render)(self.render, params.as_mut_ptr()) >= 0 }
    }

    /// Must be called after the update callback fired; returns true if a new frame should be drawn.
    pub fn render_update(&self) -> bool {
        if self.render.is_null() {
            return false;
        }
        unsafe { (self.api.render_context_update)(self.render) & 1 != 0 }
    }

    /// Renders into GL framebuffer `fbo` (0 = default) of size `w`×`h`;
    /// `internal_format` is its GL format (0 = unknown).
    pub fn render(&self, fbo: i32, w: i32, h: i32, internal_format: i32) {
        if self.render.is_null() {
            return;
        }
        let mut fbo = OpenGlFbo { fbo, w, h, internal_format };
        let mut flip: c_int = 1;
        let mut params = [
            RenderParam { kind: RENDER_PARAM_OPENGL_FBO, data: &mut fbo as *mut _ as *mut c_void },
            RenderParam { kind: RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            RenderParam { kind: RENDER_PARAM_INVALID, data: ptr::null_mut() },
        ];
        unsafe { (self.api.render_context_render)(self.render, params.as_mut_ptr()) };
    }

    pub fn report_swap(&self) {
        if !self.render.is_null() {
            unsafe { (self.api.render_context_report_swap)(self.render) };
        }
    }

    /// Frees the render context; the GL context must still be current.
    pub fn free_render_context(&mut self) {
        if !self.render.is_null() {
            unsafe { (self.api.render_context_free)(self.render) };
            self.render = ptr::null_mut();
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.free_render_context();
        unsafe { (self.api.terminate_destroy)(self.handle) };
    }
}
