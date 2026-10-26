//! Windows taskbar thumbnail toolbar: Previous / Play·Pause / Next buttons
//! under the window's taskbar preview (ITaskbarList3), as media players have.

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, TaskbarList, THBF_ENABLED, THBN_CLICKED, THB_FLAGS, THB_ICON, THB_TOOLTIP, THUMBBUTTON,
};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, GetSystemMetrics, RegisterWindowMessageW, HICON, ICONINFO, SM_CXSMICON, WM_COMMAND, WM_SIZING};

/// Width / height to keep while the user resizes the window (PiP), as
/// f64 bits; 0 = free.
pub static KEEP_ASPECT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Adjusts the rectangle being dragged (WM_SIZING) to the aspect ratio,
/// moving the edge the user isn't holding.
fn keep_aspect(edge: usize, r: &mut RECT, aspect: f64) {
    const LEFT: usize = 1;
    const RIGHT: usize = 2;
    const TOP: usize = 3;
    const TOPLEFT: usize = 4;
    const TOPRIGHT: usize = 5;
    const BOTTOM: usize = 6;
    let (w, h) = ((r.right - r.left) as f64, (r.bottom - r.top) as f64);
    match edge {
        // Dragging a side: the height follows the width.
        LEFT | RIGHT => r.bottom = r.top + (w / aspect).round() as i32,
        // Top / bottom: the width follows the height.
        TOP | BOTTOM => r.right = r.left + (h * aspect).round() as i32,
        // Corners: the larger change wins; the opposite corner stays put.
        _ => {
            let (nw, nh) = if w / h > aspect { (w, w / aspect) } else { (h * aspect, h) };
            if matches!(edge, TOPLEFT | TOPRIGHT) {
                r.top = r.bottom - nh.round() as i32;
            } else {
                r.bottom = r.top + nh.round() as i32;
            }
            if matches!(edge, TOPLEFT) || edge == 7 {
                r.left = r.right - nw.round() as i32;
            } else {
                r.right = r.left + nw.round() as i32;
            }
        }
    }
}

/// PotPlayer's thumbnail toolbar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Previous = 1,
    Rewind = 2,
    Stop = 3,
    PlayPause = 4,
    Forward = 5,
    Next = 6,
    Fullscreen = 7,
}

impl Button {
    const ALL: [Button; 7] = [Button::Previous, Button::Rewind, Button::Stop, Button::PlayPause, Button::Forward, Button::Next, Button::Fullscreen];
}

struct Hook {
    created_msg: u32,
    on_click: Box<dyn Fn(Button)>,
    on_created: Box<dyn Fn()>,
}

pub struct Taskbar {
    hwnd: HWND,
    list: ITaskbarList3,
    added: bool,
    paused: Option<bool>,
    /// Previous, rewind, stop, play, pause, forward, next, fullscreen.
    icons: [HICON; 8],
    tips: [String; 8],
    /// Right-to-left order (Arabic UI), as Windows shows RTL apps.
    rtl: bool,
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    let hook = unsafe { &*(data as *const Hook) };
    if msg == WM_COMMAND && ((wp.0 >> 16) & 0xffff) as u32 == THBN_CLICKED {
        let id = (wp.0 & 0xffff) as u32;
        let b = Button::ALL.into_iter().find(|b| *b as u32 == id);
        if let Some(b) = b {
            (hook.on_click)(b);
            return LRESULT(0);
        }
    }
    if msg == WM_SIZING {
        let aspect = f64::from_bits(KEEP_ASPECT.load(std::sync::atomic::Ordering::Relaxed));
        if aspect > 0.0 && lp.0 != 0 {
            keep_aspect(wp.0, unsafe { &mut *(lp.0 as *mut RECT) }, aspect);
            return LRESULT(1);
        }
    }
    if msg == hook.created_msg {
        (hook.on_created)();
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

/// White glyph on transparent, `size`×`size`, as an HICON.
fn icon(size: i32, inside: impl Fn(f32, f32) -> bool) -> Option<HICON> {
    unsafe {
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        // 4×4 supersampling for smooth edges.
        for y in 0..size {
            for x in 0..size {
                let mut hit = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let (fx, fy) = ((x as f32 + (sx as f32 + 0.5) / 4.0) / size as f32, (y as f32 + (sy as f32 + 0.5) / 4.0) / size as f32);
                        hit += inside(fx, fy) as u32;
                    }
                }
                let a = hit * 255 / 16;
                // Premultiplied white.
                px[(y * size + x) as usize] = (a << 24) | (a << 16) | (a << 8) | a;
            }
        }
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

/// Inside a triangle pointing right (`dir` = 1) or left (-1) in [x0,x1]×[y0,y1].
fn triangle(x: f32, y: f32, x0: f32, x1: f32, y0: f32, y1: f32, dir: f32) -> bool {
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let t = if dir > 0.0 { (x - x0) / (x1 - x0) } else { (x1 - x) / (x1 - x0) };
    let half = (y1 - y0) / 2.0 * (1.0 - t);
    (y - (y0 + y1) / 2.0).abs() <= half
}

fn rect(x: f32, y: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> bool {
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

impl Taskbar {
    /// Installs the toolbar on `hwnd`. `on_created` runs when Explorer
    /// (re)creates the taskbar button; call `add()` then.
    /// `tips`: previous, rewind, stop, play, pause, forward, next, fullscreen.
    pub fn new(hwnd: *mut std::ffi::c_void, tips: [String; 8], rtl: bool, on_click: impl Fn(Button) + 'static, on_created: impl Fn() + 'static) -> Option<Taskbar> {
        let hwnd = HWND(hwnd);
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()?;
            list.HrInit().ok()?;
            let hook = Box::new(Hook { created_msg: RegisterWindowMessageW(w!("TaskbarButtonCreated")), on_click: Box::new(on_click), on_created: Box::new(on_created) });
            // Lives as long as the window (the subclass is never removed).
            let data = Box::into_raw(hook) as usize;
            if !SetWindowSubclass(hwnd, Some(subclass), 0x1194A, data).as_bool() {
                drop(Box::from_raw(data as *mut Hook));
                return None;
            }
            let s = GetSystemMetrics(SM_CXSMICON).max(16);
            // Corner brackets for fullscreen.
            let corner = |x: f32, y: f32| {
                let (t, l) = (0.1, 0.32);
                let near = |v: f32, a: f32| v >= a && v <= a + t;
                let edge_x = near(x, 0.14) || near(x, 0.76);
                let edge_y = near(y, 0.14) || near(y, 0.76);
                let in_x = x <= 0.14 + l || x >= 0.86 - l;
                let in_y = y <= 0.14 + l || y >= 0.86 - l;
                (edge_x && in_y && (0.14..=0.86).contains(&y)) || (edge_y && in_x && (0.14..=0.86).contains(&x))
            };
            let icons = [
                icon(s, |x, y| rect(x, y, 0.18, 0.28, 0.22, 0.78) || triangle(x, y, 0.28, 0.82, 0.22, 0.78, -1.0)),
                icon(s, |x, y| triangle(x, y, 0.12, 0.5, 0.24, 0.76, -1.0) || triangle(x, y, 0.5, 0.88, 0.24, 0.76, -1.0)),
                icon(s, |x, y| rect(x, y, 0.24, 0.76, 0.24, 0.76)),
                icon(s, |x, y| triangle(x, y, 0.26, 0.84, 0.18, 0.82, 1.0)),
                icon(s, |x, y| rect(x, y, 0.24, 0.42, 0.2, 0.8) || rect(x, y, 0.58, 0.76, 0.2, 0.8)),
                icon(s, |x, y| triangle(x, y, 0.12, 0.5, 0.24, 0.76, 1.0) || triangle(x, y, 0.5, 0.88, 0.24, 0.76, 1.0)),
                icon(s, |x, y| triangle(x, y, 0.18, 0.72, 0.22, 0.78, 1.0) || rect(x, y, 0.72, 0.82, 0.22, 0.78)),
                icon(s, corner),
            ];
            if icons.iter().any(|i| i.is_none()) {
                return None;
            }
            Some(Taskbar { hwnd, list, added: false, paused: None, icons: icons.map(|i| i.unwrap()), tips, rtl })
        }
    }

    fn buttons(&self, paused: bool) -> Vec<THUMBBUTTON> {
        let make = |id: Button, icon: HICON, tip: &str| {
            let mut b = THUMBBUTTON { dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS, iId: id as u32, hIcon: icon, dwFlags: THBF_ENABLED, ..Default::default() };
            for (i, c) in tip.encode_utf16().take(259).enumerate() {
                b.szTip[i] = c;
            }
            b
        };
        let mut v: Vec<THUMBBUTTON> = Button::ALL
            .into_iter()
            .map(|b| {
                let i = match b {
                    Button::Previous => 0,
                    Button::Rewind => 1,
                    Button::Stop => 2,
                    Button::PlayPause if paused => 3,
                    Button::PlayPause => 4,
                    Button::Forward => 5,
                    Button::Next => 6,
                    Button::Fullscreen => 7,
                };
                make(b, self.icons[i], &self.tips[i])
            })
            .collect();
        if self.rtl {
            v.reverse();
        }
        v
    }

    /// Adds the buttons (once the taskbar button exists).
    pub fn add(&mut self, paused: bool) {
        if self.added {
            return;
        }
        let b = self.buttons(paused);
        match unsafe { self.list.ThumbBarAddButtons(self.hwnd, &b) } {
            Ok(()) => {
                self.added = true;
                self.paused = Some(paused);
                crate::dbg_log("taskbar buttons added".into());
            }
            Err(e) => crate::dbg_log(format!("taskbar buttons not added yet: {e}")),
        }
    }

    /// Shows Play or Pause on the middle button.
    pub fn set_paused(&mut self, paused: bool) {
        if !self.added {
            self.add(paused);
            return;
        }
        if self.paused == Some(paused) {
            return;
        }
        let b = self.buttons(paused);
        match unsafe { self.list.ThumbBarUpdateButtons(self.hwnd, &b) } {
            Ok(()) => {
                self.paused = Some(paused);
                crate::dbg_log(format!("taskbar: {}", if paused { "play" } else { "pause" }));
            }
            Err(e) => crate::dbg_log(format!("taskbar update failed: {e}")),
        }
    }

    /// Explorer restarted / the button was recreated: add again.
    pub fn recreated(&mut self, paused: bool) {
        self.added = false;
        self.add(paused);
        // If the buttons already existed (the first "created" message often
        // arrives after our early add), Windows keeps their old icons:
        // push the current state explicitly.
        self.paused = None;
        self.set_paused(paused);
    }
}

impl Drop for Taskbar {
    fn drop(&mut self) {
        for i in self.icons {
            unsafe {
                let _ = DestroyIcon(i);
            }
        }
    }
}
