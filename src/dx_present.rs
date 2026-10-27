//! HDR output through Direct3D 11 (Windows).
//!
//! Same idea as vk_present.rs, with a DXGI swapchain in the HDR10 colour
//! space (PQ / BT.2020) instead of a Vulkan one: GL composites into a D3D11
//! texture it shares through WGL_NV_DX_interop2, and D3D11 copies that
//! texture to the back buffer and presents it. The interop extension is
//! older and more widely supported than GL/Vulkan memory sharing (AMD,
//! NVIDIA and Intel drivers all ship it), so this path is tried first.

use std::ffi::{c_void, CStr};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, LUID};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Swapchain and shared texture format (same layout as GL_RGB10_A2).
const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R10G10B10A2_UNORM;

pub struct DxPresenter {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    /// The texture GL composites into.
    pub(crate) shared: Option<ID3D11Texture2D>,
    /// The previous size's texture, still registered in GL until
    /// `GlTargets::recreate` runs.
    _retired: Option<ID3D11Texture2D>,
    pub extent: (u32, u32),
    pub description: String,
}

impl DxPresenter {
    /// Creates the swapchain on `window`, on the GPU the GL context runs on
    /// (`gl_luid`, else the first adapter of `gl_vendor`). Fails if the
    /// swapchain can't use the HDR10 colour space.
    pub fn new(window: &Window, gl_luid: Option<[u8; 8]>, gl_vendor: &str, peak_nits: f32) -> Result<Self, String> {
        let RawWindowHandle::Win32(h) = window.window_handle().map_err(|e| e.to_string())?.as_raw() else {
            return Err("not a Win32 window".into());
        };
        let hwnd = HWND(h.hwnd.get() as *mut c_void);
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory1().map_err(|e| format!("DXGI factory: {e}"))?;
            let vendor_id = match gl_vendor.to_ascii_lowercase() {
                v if v.contains("nvidia") => 0x10de,
                v if v.contains("ati") || v.contains("amd") => 0x1002,
                v if v.contains("intel") => 0x8086,
                _ => 0,
            };
            let mut by_vendor = None;
            let mut by_luid = None;
            for i in 0.. {
                let Ok(a) = factory.EnumAdapters1(i) else { break };
                let Ok(desc) = a.GetDesc1() else { continue };
                if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let luid = luid_bytes(desc.AdapterLuid);
                let name = String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').to_string();
                if gl_luid == Some(luid) {
                    by_luid = Some((a.clone(), name.clone()));
                }
                if by_vendor.is_none() && desc.VendorId == vendor_id {
                    by_vendor = Some((a, name));
                }
            }
            let (adapter, gpu_name) = by_luid.or(by_vendor).ok_or("no Direct3D adapter matches the OpenGL GPU")?;

            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| format!("D3D11CreateDevice: {e}"))?;
            let (device, context) = (device.ok_or("no D3D11 device")?, context.ok_or("no D3D11 context")?);

            let size = window.inner_size();
            let extent = (size.width.max(1), size.height.max(1));
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: extent.0,
                Height: extent.1,
                Format: FORMAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            let swapchain = factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None).map_err(|e| format!("swapchain: {e}"))?;
            // winit handles fullscreen itself.
            let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);

            let sc3: IDXGISwapChain3 = swapchain.cast().map_err(|e| format!("IDXGISwapChain3: {e}"))?;
            let cs = DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
            let support = sc3.CheckColorSpaceSupport(cs).unwrap_or(0);
            if support & DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT.0 as u32 == 0 {
                return Err("the swapchain can't use the HDR10 colour space".into());
            }
            sc3.SetColorSpace1(cs).map_err(|e| format!("SetColorSpace1: {e}"))?;
            if let Ok(sc4) = swapchain.cast::<IDXGISwapChain4>() {
                // BT.2020 primaries and the display's peak, as in the Vulkan path.
                let xy = |x: f32, y: f32| [(x * 50000.0) as u16, (y * 50000.0) as u16];
                let md = DXGI_HDR_METADATA_HDR10 {
                    RedPrimary: xy(0.708, 0.292),
                    GreenPrimary: xy(0.170, 0.797),
                    BluePrimary: xy(0.131, 0.046),
                    WhitePoint: xy(0.3127, 0.3290),
                    MaxMasteringLuminance: peak_nits as u32,
                    MinMasteringLuminance: 0,
                    MaxContentLightLevel: peak_nits as u16,
                    MaxFrameAverageLightLevel: peak_nits as u16,
                };
                let bytes = std::slice::from_raw_parts(&md as *const _ as *const u8, std::mem::size_of_val(&md));
                let _ = sc4.SetHDRMetaData(DXGI_HDR_METADATA_TYPE_HDR10, Some(bytes));
            }

            let mut p = Self {
                device,
                context,
                swapchain,
                shared: None,
                _retired: None,
                extent,
                description: format!("HDR10 · Direct3D 11 · {gpu_name}"),
            };
            p.shared = Some(p.create_shared()?);
            Ok(p)
        }
    }

    fn create_shared(&self) -> Result<ID3D11Texture2D, String> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.extent.0,
            Height: self.extent.1,
            MipLevels: 1,
            ArraySize: 1,
            Format: FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut tex = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex)) }.map_err(|e| format!("shared texture: {e}"))?;
        tex.ok_or_else(|| "no shared texture".into())
    }

    /// The device pointer WGL_NV_DX_interop opens.
    pub(crate) fn device_ptr(&self) -> *mut c_void {
        self.device.as_raw()
    }

    /// New back buffers and shared texture for a new window size. GL must
    /// re-register afterwards (`GlTargets::recreate`).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Ok(()); // minimized
        }
        unsafe {
            self.context.ClearState();
            self.context.Flush();
            self.swapchain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(|e| format!("ResizeBuffers: {e}"))?;
        }
        self.extent = (width, height);
        self._retired = self.shared.take();
        self.shared = Some(self.create_shared()?);
        Ok(())
    }

    /// Copies the shared texture (GL has unlocked it) to the back buffer
    /// and presents it with vsync.
    pub fn present(&mut self) -> Result<bool, String> {
        unsafe {
            let shared = self.shared.as_ref().ok_or("no shared texture")?;
            let back: ID3D11Texture2D = self.swapchain.GetBuffer(0).map_err(|e| format!("back buffer: {e}"))?;
            self.context.CopyResource(&back, shared);
            drop(back);
            self.swapchain.Present(1, DXGI_PRESENT(0)).ok().map_err(|e| format!("Present: {e}"))?;
        }
        Ok(true)
    }

    /// The centre pixel of the shared texture (raw R10G10B10A2).
    pub fn read_shared_pixel(&self) -> Result<u32, String> {
        unsafe {
            let shared = self.shared.as_ref().ok_or("no shared texture")?;
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 1,
                Height: 1,
                MipLevels: 1,
                ArraySize: 1,
                Format: FORMAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            self.device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(|e| format!("staging texture: {e}"))?;
            let staging = staging.ok_or("no staging texture")?;
            let (x, y) = (self.extent.0 / 2, self.extent.1 / 2);
            let bx = D3D11_BOX { left: x, top: y, front: 0, right: x + 1, bottom: y + 1, back: 1 };
            self.context.CopySubresourceRegion(&staging, 0, 0, 0, 0, shared, 0, Some(&bx));
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| format!("Map: {e}"))?;
            let px = std::ptr::read_unaligned(mapped.pData as *const u32);
            self.context.Unmap(&staging, 0);
            Ok(px)
        }
    }
}

fn luid_bytes(l: LUID) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&l.LowPart.to_le_bytes());
    b[4..].copy_from_slice(&l.HighPart.to_le_bytes());
    b
}

// ------------------------------------------------------------------ GL side

const WGL_ACCESS_WRITE_DISCARD_NV: u32 = 0x0002;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_DEVICE_LUID_EXT: u32 = 0x9599;

type FnOpenDevice = unsafe extern "system" fn(*mut c_void) -> *mut c_void;
type FnCloseDevice = unsafe extern "system" fn(*mut c_void) -> i32;
type FnRegisterObject = unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, u32) -> *mut c_void;
type FnUnregisterObject = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
type FnLockObjects = unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32;
type FnGetUnsignedBytev = unsafe extern "system" fn(u32, *mut u8);

/// LUID of the GPU the GL context runs on (GL_EXT_memory_object_win32),
/// to create the D3D11 device on the same adapter.
pub fn gl_luid(gl: &glow::Context, get: impl Fn(&CStr) -> *const c_void) -> Option<[u8; 8]> {
    use glow::HasContext;
    if !gl.supported_extensions().contains("GL_EXT_memory_object_win32") {
        return None;
    }
    let p = get(c"glGetUnsignedBytevEXT");
    if p.is_null() {
        return None;
    }
    let f: FnGetUnsignedBytev = unsafe { std::mem::transmute(p) };
    let mut luid = [0u8; 8];
    unsafe {
        f(GL_DEVICE_LUID_EXT, luid.as_mut_ptr());
        (gl.get_error() == glow::NO_ERROR && luid != [0; 8]).then_some(luid)
    }
}

/// The GL end of WGL_NV_DX_interop2: the D3D11 device opened in GL and the
/// shared texture registered as a GL texture.
pub struct GlShare {
    open_device: FnOpenDevice,
    close_device: FnCloseDevice,
    register: FnRegisterObject,
    unregister: FnUnregisterObject,
    lock: FnLockObjects,
    unlock: FnLockObjects,
    device: *mut c_void,
    object: *mut c_void,
}

impl GlShare {
    pub fn new(get: impl Fn(&CStr) -> *const c_void, p: &DxPresenter) -> Result<Self, String> {
        macro_rules! req {
            ($name:literal) => {{
                let f = get($name);
                if f.is_null() {
                    return Err(format!("missing {} (WGL_NV_DX_interop)", $name.to_string_lossy()));
                }
                unsafe { std::mem::transmute::<*const c_void, _>(f) }
            }};
        }
        let mut s = Self {
            open_device: req!(c"wglDXOpenDeviceNV"),
            close_device: req!(c"wglDXCloseDeviceNV"),
            register: req!(c"wglDXRegisterObjectNV"),
            unregister: req!(c"wglDXUnregisterObjectNV"),
            lock: req!(c"wglDXLockObjectsNV"),
            unlock: req!(c"wglDXUnlockObjectsNV"),
            device: std::ptr::null_mut(),
            object: std::ptr::null_mut(),
        };
        s.device = unsafe { (s.open_device)(p.device_ptr()) };
        if s.device.is_null() {
            return Err(format!("wglDXOpenDeviceNV failed ({})", std::io::Error::last_os_error()));
        }
        Ok(s)
    }

    /// Registers the presenter's current shared texture as GL texture `tex`.
    pub fn register(&mut self, tex: u32, p: &DxPresenter) -> Result<(), String> {
        let shared = p.shared.as_ref().ok_or("no shared texture")?;
        self.object = unsafe { (self.register)(self.device, shared.as_raw(), tex, GL_TEXTURE_2D, WGL_ACCESS_WRITE_DISCARD_NV) };
        if self.object.is_null() {
            return Err(format!("wglDXRegisterObjectNV failed ({})", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    pub fn unregister(&mut self) {
        if !self.object.is_null() {
            unsafe { (self.unregister)(self.device, self.object) };
            self.object = std::ptr::null_mut();
        }
    }

    /// GL may draw into the texture until `unlock`.
    pub fn lock(&mut self) -> bool {
        !self.object.is_null() && unsafe { (self.lock)(self.device, 1, &mut self.object) } != 0
    }

    /// Hands the texture back to D3D11 (flushes GL's drawing into it).
    pub fn unlock(&mut self) {
        if !self.object.is_null() {
            unsafe { (self.unlock)(self.device, 1, &mut self.object) };
        }
    }

    pub fn close(&mut self) {
        self.unregister();
        if !self.device.is_null() {
            unsafe { (self.close_device)(self.device) };
            self.device = std::ptr::null_mut();
        }
    }
}
