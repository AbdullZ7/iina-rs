//! Windows taskbar thumbnail toolbar: Previous / Play·Pause / Next buttons
//! under the window's taskbar preview (ITaskbarList3), as media players have.

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, TaskbarList, THBF_ENABLED, THBN_CLICKED, THB_FLAGS, THB_ICON, THB_TOOLTIP, THUMBBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, GetSystemMetrics, RegisterWindowMessageW, HICON, ICONINFO, SM_CXSMICON, WM_COMMAND};

#[derive(Clone, Copy, Debug)]
pub enum Button {
    Previous = 1,
    PlayPause = 2,
    Next = 3,
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
    icons: [HICON; 4], // previous, play, pause, next
    tips: [String; 4],
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    let hook = unsafe { &*(data as *const Hook) };
    if msg == WM_COMMAND && ((wp.0 >> 16) & 0xffff) as u32 == THBN_CLICKED {
        let b = match wp.0 & 0xffff {
            1 => Some(Button::Previous),
            2 => Some(Button::PlayPause),
            3 => Some(Button::Next),
            _ => None,
        };
        if let Some(b) = b {
            (hook.on_click)(b);
            return LRESULT(0);
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
    pub fn new(hwnd: *mut std::ffi::c_void, tips: [String; 4], on_click: impl Fn(Button) + 'static, on_created: impl Fn() + 'static) -> Option<Taskbar> {
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
            let icons = [
                icon(s, |x, y| rect(x, y, 0.18, 0.28, 0.22, 0.78) || triangle(x, y, 0.28, 0.82, 0.22, 0.78, -1.0)),
                icon(s, |x, y| triangle(x, y, 0.26, 0.84, 0.18, 0.82, 1.0)),
                icon(s, |x, y| rect(x, y, 0.24, 0.42, 0.2, 0.8) || rect(x, y, 0.58, 0.76, 0.2, 0.8)),
                icon(s, |x, y| triangle(x, y, 0.18, 0.72, 0.22, 0.78, 1.0) || rect(x, y, 0.72, 0.82, 0.22, 0.78)),
            ];
            if icons.iter().any(|i| i.is_none()) {
                return None;
            }
            Some(Taskbar { hwnd, list, added: false, paused: None, icons: icons.map(|i| i.unwrap()), tips })
        }
    }

    fn buttons(&self, paused: bool) -> [THUMBBUTTON; 3] {
        let make = |id: Button, icon: HICON, tip: &str| {
            let mut b = THUMBBUTTON { dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS, iId: id as u32, hIcon: icon, dwFlags: THBF_ENABLED, ..Default::default() };
            for (i, c) in tip.encode_utf16().take(259).enumerate() {
                b.szTip[i] = c;
            }
            b
        };
        let (play_icon, play_tip) = if paused { (self.icons[1], &self.tips[1]) } else { (self.icons[2], &self.tips[2]) };
        [make(Button::Previous, self.icons[0], &self.tips[0]), make(Button::PlayPause, play_icon, play_tip), make(Button::Next, self.icons[3], &self.tips[3])]
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
        if unsafe { self.list.ThumbBarUpdateButtons(self.hwnd, &b) }.is_ok() {
            self.paused = Some(paused);
        }
    }

    /// Explorer restarted / the button was recreated: add again.
    pub fn recreated(&mut self, paused: bool) {
        self.added = false;
        self.add(paused);
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
