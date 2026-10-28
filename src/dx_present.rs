//! HDR output through Direct3D 11 (Windows).
//!
//! Same idea as vk_present.rs, with a DXGI swapchain in the HDR10 colour
//! space (PQ / BT.2020) instead of a Vulkan one: GL composites into a D3D11
//! texture it shares through WGL_NV_DX_interop2, and D3D11 copies that
//! texture to the back buffer and presents it. The interop extension is
//! older and more widely supported than GL/Vulkan memory sharing (AMD,
//! NVIDIA and Intel drivers all ship it), so this path is tried first.
//!
//! The same device also runs NVIDIA RTX Video (`Enhancer`): mpv renders the
//! video at about its own resolution, and the D3D11 video processor scales it
//! to the window with RTX Video Super Resolution and/or turns SDR into HDR
//! with RTX Video HDR, the way browsers apply them. Without an RTX GPU, or
//! with "RTX Video Enhancement" off in the NVIDIA Control Panel, the driver
//! accepts the request and scales normally.

use std::ffi::{c_void, CStr};
use windows::core::{Interface, GUID};
use windows::Win32::Foundation::{HMODULE, HWND, LUID, RECT};
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
    /// HDR10 swapchain (else SDR, used for RTX Video on SDR displays).
    pub hdr: bool,
    /// PCI vendor of the adapter (0x10de: NVIDIA).
    pub vendor_id: u32,
}

pub const VENDOR_NVIDIA: u32 = 0x10de;

/// The GPUs and their driver versions, as a key for `render_memory`.
pub fn gpu_fingerprint() -> String {
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return "unknown".into() };
        (0..)
            .map_while(|i| factory.EnumAdapters1(i).ok())
            .filter_map(|a| {
                let d = a.GetDesc1().ok()?;
                if (d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    return None;
                }
                let driver = a.CheckInterfaceSupport(&IDXGIDevice::IID).unwrap_or(0) as u64;
                let name = String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string();
                Some(format!(
                    "{name} {:04x}:{:04x} {}.{}.{}.{}",
                    d.VendorId,
                    d.DeviceId,
                    driver >> 48,
                    (driver >> 32) & 0xffff,
                    (driver >> 16) & 0xffff,
                    driver & 0xffff
                ))
            })
            .collect::<std::collections::BTreeSet<_>>() // an adapter can be listed twice
            .into_iter()
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

/// Whether the computer has an NVIDIA GPU (RTX Video is worth trying).
pub fn has_nvidia_gpu() -> bool {
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return false };
        (0..).map_while(|i| factory.EnumAdapters1(i).ok()).any(|a| a.GetDesc1().is_ok_and(|d| d.VendorId == VENDOR_NVIDIA))
    }
}

/// Asks Windows to run this program on the high-performance GPU, as the
/// per-app choice in Settings > Display > Graphics does. Applies at the next
/// start.
pub fn prefer_high_performance_gpu() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    set_string_value(windows::core::w!("Software\\Microsoft\\DirectX\\UserGpuPreferences"), exe.as_os_str(), "GpuPreference=2;")
}

/// HKCU\`key`: value `name` = `data` (REG_SZ).
fn set_string_value(key: windows::core::PCWSTR, name: &std::ffi::OsStr, data: &str) -> Result<(), String> {
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let name: Vec<u16> = name.encode_wide_z();
    let data: Vec<u16> = data.encode_utf16().chain([0]).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            windows::core::PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr() as *const c_void),
            (data.len() * 2) as u32,
        )
        .ok()
        .map_err(|e| e.to_string())
    }
}

trait EncodeWideZ {
    fn encode_wide_z(&self) -> Vec<u16>;
}

impl EncodeWideZ for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}

impl DxPresenter {
    /// Creates the swapchain on `window`, on the GPU the GL context runs on
    /// (`gl_luid`, else the first adapter of `gl_vendor`). With `hdr`, fails
    /// if the swapchain can't use the HDR10 colour space.
    pub fn new(window: &Window, gl_luid: Option<[u8; 8]>, gl_vendor: &str, peak_nits: f32, hdr: bool) -> Result<Self, String> {
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
                    by_luid = Some((a.clone(), name.clone(), desc.VendorId));
                }
                if by_vendor.is_none() && desc.VendorId == vendor_id {
                    by_vendor = Some((a, name, desc.VendorId));
                }
            }
            let (adapter, gpu_name, adapter_vendor) = by_luid.or(by_vendor).ok_or("no Direct3D adapter matches the OpenGL GPU")?;

            // Video support for the RTX Video processor; retried without it.
            let mut device = None;
            let mut context = None;
            let mut created = Err(windows::core::Error::empty());
            for flags in [D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_CREATE_DEVICE_FLAG(0)] {
                created = D3D11CreateDevice(
                    &adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    flags,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                );
                if created.is_ok() {
                    break;
                }
            }
            created.map_err(|e| format!("D3D11CreateDevice: {e}"))?;
            let (device, context) = (device.ok_or("no D3D11 device")?, context.ok_or("no D3D11 context")?);
            // The GL interop driver uses the device from its own threads.
            if let Ok(mt) = context.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }

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
            let cs = if hdr { DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 } else { DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709 };
            let support = sc3.CheckColorSpaceSupport(cs).unwrap_or(0);
            if support & DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT.0 as u32 == 0 {
                return Err(format!("the swapchain can't use the {} colour space", if hdr { "HDR10" } else { "sRGB" }));
            }
            sc3.SetColorSpace1(cs).map_err(|e| format!("SetColorSpace1: {e}"))?;
            if let Some(sc4) = swapchain.cast::<IDXGISwapChain4>().ok().filter(|_| hdr) {
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
                description: format!("{} · Direct3D 11 · {gpu_name}", if hdr { "HDR10" } else { "SDR" }),
                hdr,
                vendor_id: adapter_vendor,
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

// ---------------------------------------------------------- NVIDIA RTX Video

// From mpv's vf_d3d11vpp.c (also used by Chromium).
const NVIDIA_PPE_INTERFACE: GUID = GUID::from_u128(0xd43ce1b3_1f4b_48ac_baee_c3c25375e6f7);
const NVIDIA_TRUE_HDR_INTERFACE: GUID = GUID::from_u128(0xfdd62bb4_620b_4fd7_9ab3_1e59d0d544b3);

#[repr(C)]
struct NvidiaExt {
    version: u32,
    method: u32,
    enable: u32,
}

/// RGB video at (about) its own resolution in, the window's size out.
pub const ENHANCE_IN_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;

/// The D3D11 video processor running RTX Video Super Resolution (scaling
/// `input` to `output`) and/or RTX Video HDR (SDR in, PQ / BT.2020 out).
pub struct Enhancer {
    context: ID3D11DeviceContext,
    vctx: ID3D11VideoContext1,
    vp: ID3D11VideoProcessor,
    _enumerator: ID3D11VideoProcessorEnumerator,
    in_view: ID3D11VideoProcessorInputView,
    out_view: ID3D11VideoProcessorOutputView,
    /// The textures GL shares (mpv renders into `input`, the composite
    /// reads `output`). The video processor works on its own copies: run
    /// on interop textures directly, it crashed the NVIDIA driver.
    pub input: ID3D11Texture2D,
    pub output: ID3D11Texture2D,
    vp_input: ID3D11Texture2D,
    vp_output: ID3D11Texture2D,
    pub in_size: (u32, u32),
    pub out_size: (u32, u32),
    /// What the driver accepted.
    pub super_resolution: bool,
    pub true_hdr: bool,
}

impl DxPresenter {
    pub fn enhancer(&self, in_size: (u32, u32), out_size: (u32, u32), super_resolution: bool, true_hdr: bool) -> Result<Enhancer, String> {
        let e = |what: &'static str| move |err: windows::core::Error| format!("{what}: {err}");
        unsafe {
            let vdev: ID3D11VideoDevice = self.device.cast().map_err(e("video device"))?;
            let vctx: ID3D11VideoContext1 = self.context.cast().map_err(e("video context"))?;
            let rate = DXGI_RATIONAL { Numerator: 60, Denominator: 1 };
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: rate,
                InputWidth: in_size.0,
                InputHeight: in_size.1,
                OutputFrameRate: rate,
                OutputWidth: out_size.0,
                OutputHeight: out_size.1,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let enumerator = vdev.CreateVideoProcessorEnumerator(&content).map_err(e("video processor enumerator"))?;
            let vp = vdev.CreateVideoProcessor(&enumerator, 0).map_err(e("video processor"))?;
            let texture = |size: (u32, u32), format: DXGI_FORMAT| -> Result<ID3D11Texture2D, String> {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: size.0,
                    Height: size.1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut t = None;
                self.device.CreateTexture2D(&desc, None, Some(&mut t)).map_err(e("video texture"))?;
                t.ok_or_else(|| "no video texture".to_string())
            };
            // 8-bit out for SDR: with a 10-bit output the processor shifted
            // the colours (washed out). RTX Video HDR needs 10-bit PQ.
            let out_format = if true_hdr { FORMAT } else { ENHANCE_IN_FORMAT };
            let input = texture(in_size, ENHANCE_IN_FORMAT)?;
            let output = texture(out_size, out_format)?;
            let vp_input = texture(in_size, ENHANCE_IN_FORMAT)?;
            let vp_output = texture(out_size, out_format)?;
            let mut in_view = None;
            let ivd = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 } },
            };
            vdev.CreateVideoProcessorInputView(&vp_input, &enumerator, &ivd, Some(&mut in_view)).map_err(e("input view"))?;
            let mut out_view = None;
            let ovd = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
            };
            vdev.CreateVideoProcessorOutputView(&vp_output, &enumerator, &ovd, Some(&mut out_view)).map_err(e("output view"))?;

            // mpv's output is display-referred SDR; RTX Video HDR needs a
            // PQ / BT.2020 output, otherwise nothing is converted.
            vctx.VideoProcessorSetStreamColorSpace1(&vp, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
            vctx.VideoProcessorSetOutputColorSpace1(
                &vp,
                if true_hdr { DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 } else { DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709 },
            );
            vctx.VideoProcessorSetStreamFrameFormat(&vp, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            vctx.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, false);
            let src = RECT { left: 0, top: 0, right: in_size.0 as i32, bottom: in_size.1 as i32 };
            let dst = RECT { left: 0, top: 0, right: out_size.0 as i32, bottom: out_size.1 as i32 };
            vctx.VideoProcessorSetStreamSourceRect(&vp, 0, true, Some(&src));
            vctx.VideoProcessorSetStreamDestRect(&vp, 0, true, Some(&dst));
            vctx.VideoProcessorSetOutputTargetRect(&vp, true, Some(&dst));
            let ext = |guid: &GUID, version: u32, method: u32| {
                let data = NvidiaExt { version, method, enable: 1 };
                vctx.VideoProcessorSetStreamExtension(&vp, 0, guid, std::mem::size_of::<NvidiaExt>() as u32, &data as *const _ as *const c_void) == 0
            };
            let super_resolution = super_resolution && ext(&NVIDIA_PPE_INTERFACE, 1, 2);
            let true_hdr = true_hdr && ext(&NVIDIA_TRUE_HDR_INTERFACE, 4, 3);
            Ok(Enhancer {
                context: self.context.clone(),
                vctx,
                vp,
                _enumerator: enumerator,
                in_view: in_view.ok_or("no input view")?,
                out_view: out_view.ok_or("no output view")?,
                input,
                output,
                vp_input,
                vp_output,
                in_size,
                out_size,
                super_resolution,
                true_hdr,
            })
        }
    }
}

impl Enhancer {
    /// `input` → `output` (GL has unlocked `input`).
    pub fn process(&self) -> Result<(), String> {
        unsafe {
            let mut streams = [D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(self.in_view.clone())),
                ..Default::default()
            }];
            self.context.CopyResource(&self.vp_input, &self.input);
            let r = self.vctx.VideoProcessorBlt(&self.vp, &self.out_view, 0, &streams);
            std::mem::ManuallyDrop::drop(&mut streams[0].pInputSurface);
            r.map_err(|e| format!("VideoProcessorBlt: {e}"))?;
            self.context.CopyResource(&self.output, &self.vp_output);
            Ok(())
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

pub const WGL_ACCESS_READ_ONLY_NV: u32 = 0x0000;
pub const WGL_ACCESS_WRITE_DISCARD_NV: u32 = 0x0002;
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

    /// Registers another D3D11 texture as GL texture `tex`; returns its
    /// interop handle (for `lock_other`, `unlock_other`, `unregister_other`).
    pub fn register_other(&self, d3d: &ID3D11Texture2D, tex: u32, access: u32) -> Result<*mut c_void, String> {
        let h = unsafe { (self.register)(self.device, d3d.as_raw(), tex, GL_TEXTURE_2D, access) };
        if h.is_null() {
            return Err(format!("wglDXRegisterObjectNV failed ({})", std::io::Error::last_os_error()));
        }
        Ok(h)
    }

    pub fn lock_other(&self, mut h: *mut c_void) {
        unsafe { (self.lock)(self.device, 1, &mut h) };
    }

    pub fn unlock_other(&self, mut h: *mut c_void) {
        unsafe { (self.unlock)(self.device, 1, &mut h) };
    }

    pub fn unregister_other(&self, h: *mut c_void) {
        unsafe { (self.unregister)(self.device, h) };
    }

    pub fn close(&mut self) {
        self.unregister();
        if !self.device.is_null() {
            unsafe { (self.close_device)(self.device) };
            self.device = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{RegDeleteTreeW, RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    #[test]
    fn writes_string_value() {
        let key = windows::core::w!("Software\\IINA-RS-test");
        let name = std::ffi::OsStr::new("C:\\Some Folder\\iina-rs.exe");
        set_string_value(key, name, "GpuPreference=2;").unwrap();
        let wname = name.encode_wide_z();
        let mut buf = [0u16; 64];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key, windows::core::PCWSTR(wname.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut size))
        };
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key) };
        r.ok().unwrap();
        let got = String::from_utf16_lossy(&buf[..size as usize / 2 - 1]);
        assert_eq!(got, "GpuPreference=2;");
    }
}
