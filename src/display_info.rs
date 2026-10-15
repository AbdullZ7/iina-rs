//! What the display under the window can do: whether the OS is currently
//! outputting HDR (PQ / BT.2020), its peak luminance, and the brightness the
//! OS uses for SDR content (used for the UI and SDR video in HDR mode).

#[derive(Clone, Copy, Debug)]
pub struct HdrDisplay {
    pub hdr_active: bool,
    /// Peak luminance in nits.
    pub max_nits: f32,
    /// Luminance of SDR white in HDR mode, in nits.
    pub sdr_white_nits: f32,
}

impl Default for HdrDisplay {
    fn default() -> Self {
        Self { hdr_active: false, max_nits: 1000.0, sdr_white_nits: 203.0 }
    }
}

#[cfg(windows)]
pub fn query(window: &winit::window::Window) -> HdrDisplay {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else { return HdrDisplay::default() };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return HdrDisplay::default() };
    windows_impl::query(h.hwnd.get()).unwrap_or_default()
}

#[cfg(not(windows))]
pub fn query(_window: &winit::window::Window) -> HdrDisplay {
    // Linux: HDR availability is decided by the Vulkan surface (Wayland color
    // management); peak luminance isn't exposed generically.
    HdrDisplay::default()
}

#[cfg(windows)]
mod windows_impl {
    use super::HdrDisplay;
    use windows::core::Interface;
    use windows::Win32::Devices::Display::*;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
    use windows::Win32::Graphics::Dxgi::*;
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};

    pub fn query(hwnd: isize) -> Option<HdrDisplay> {
        unsafe {
            let monitor = MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTONEAREST);
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            for a in 0.. {
                let Ok(adapter) = factory.EnumAdapters1(a) else { break };
                for o in 0.. {
                    let Ok(output) = adapter.EnumOutputs(o) else { break };
                    let Ok(output6) = output.cast::<IDXGIOutput6>() else { continue };
                    let Ok(desc) = output6.GetDesc1() else { continue };
                    if desc.Monitor != monitor {
                        continue;
                    }
                    let hdr_active = desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
                    let sdr_white_nits = sdr_white_level(&desc.DeviceName).unwrap_or(203.0);
                    return Some(HdrDisplay {
                        hdr_active,
                        max_nits: if desc.MaxLuminance > 0.0 { desc.MaxLuminance } else { 1000.0 },
                        sdr_white_nits,
                    });
                }
            }
            None
        }
    }

    /// Windows' "SDR content brightness" for the display with this GDI name.
    unsafe fn sdr_white_level(gdi_name: &[u16; 32]) -> Option<f32> {
        let (mut npaths, mut nmodes) = (0u32, 0u32);
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut npaths, &mut nmodes).ok().ok()?;
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); npaths as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nmodes as usize];
        QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut npaths, paths.as_mut_ptr(), &mut nmodes, modes.as_mut_ptr(), None).ok().ok()?;
        for path in paths.iter().take(npaths as usize) {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 || &source.viewGdiDeviceName != gdi_name {
                continue;
            }
            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            white.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            white.header.size = std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
            white.header.adapterId = path.targetInfo.adapterId;
            white.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut white.header) == 0 {
                // Units of 1/1000 of 80 nits.
                return Some(white.SDRWhiteLevel as f32 / 1000.0 * 80.0);
            }
        }
        None
    }
}
