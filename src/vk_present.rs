//! HDR output through Vulkan.
//!
//! libmpv only renders through OpenGL, and an OpenGL window can only present
//! SDR. For HDR displays the window is instead a Vulkan swapchain in the HDR10
//! (PQ / BT.2020) color space, and OpenGL renders off-screen:
//!
//! 1. mpv renders the video into a float texture (PQ/BT.2020 for HDR video,
//!    sRGB/BT.709 for SDR video);
//! 2. egui draws the UI into a separate texture;
//! 3. a composite pass converts both to PQ/BT.2020 (SDR content at the OS's
//!    SDR white level) and writes an image whose memory is shared with Vulkan
//!    (EXT_memory_object + VK_KHR_external_memory);
//! 4. Vulkan blits that image into the swapchain and presents it. GL and
//!    Vulkan hand the image back and forth with shared semaphores.
//!
//! Windows uses opaque Win32 handles, Linux opaque file descriptors.

use ash::vk;
use std::ffi::{c_void, CStr};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

// ------------------------------------------------------------------ GL side

const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGB10_A2: u32 = 0x8059;
const GL_RGBA16F: u32 = 0x881A;
const GL_TEXTURE_TILING_EXT: u32 = 0x9580;
const GL_DEDICATED_MEMORY_OBJECT_EXT: u32 = 0x9581;
const GL_OPTIMAL_TILING_EXT: i32 = 0x9584;
const GL_HANDLE_TYPE_OPAQUE_FD_EXT: u32 = 0x9586;
const GL_HANDLE_TYPE_OPAQUE_WIN32_EXT: u32 = 0x9587;
const GL_DEVICE_UUID_EXT: u32 = 0x9597;
const GL_LAYOUT_TRANSFER_SRC_EXT: u32 = 0x9592;

type FnCreateMemoryObjects = unsafe extern "system" fn(i32, *mut u32);
type FnDeleteMemoryObjects = unsafe extern "system" fn(i32, *const u32);
type FnMemoryObjectParameteriv = unsafe extern "system" fn(u32, u32, *const i32);
type FnImportMemoryWin32Handle = unsafe extern "system" fn(u32, u64, u32, *mut c_void);
type FnImportMemoryFd = unsafe extern "system" fn(u32, u64, u32, i32);
type FnTexStorageMem2D = unsafe extern "system" fn(u32, i32, u32, i32, i32, u32, u64);
type FnGenSemaphores = unsafe extern "system" fn(i32, *mut u32);
type FnDeleteSemaphores = unsafe extern "system" fn(i32, *const u32);
type FnImportSemaphoreWin32Handle = unsafe extern "system" fn(u32, u32, *mut c_void);
type FnImportSemaphoreFd = unsafe extern "system" fn(u32, u32, i32);
type FnSemaphoreOp = unsafe extern "system" fn(u32, u32, *const u32, u32, *const u32, *const u32);
type FnGetUnsignedBytei = unsafe extern "system" fn(u32, u32, *mut u8);

/// GL_EXT_memory_object / GL_EXT_semaphore entry points (not in glow).
pub struct GlExt {
    create_memory_objects: FnCreateMemoryObjects,
    delete_memory_objects: FnDeleteMemoryObjects,
    memory_object_parameteriv: FnMemoryObjectParameteriv,
    import_memory_win32: Option<FnImportMemoryWin32Handle>,
    import_memory_fd: Option<FnImportMemoryFd>,
    tex_storage_mem_2d: FnTexStorageMem2D,
    gen_semaphores: FnGenSemaphores,
    delete_semaphores: FnDeleteSemaphores,
    import_semaphore_win32: Option<FnImportSemaphoreWin32Handle>,
    import_semaphore_fd: Option<FnImportSemaphoreFd>,
    signal_semaphore: FnSemaphoreOp,
    wait_semaphore: FnSemaphoreOp,
    get_unsigned_bytei: FnGetUnsignedBytei,
}

impl GlExt {
    pub fn load(get: impl Fn(&CStr) -> *const c_void) -> Result<Self, String> {
        macro_rules! req {
            ($name:literal) => {{
                let p = get($name);
                if p.is_null() {
                    return Err(format!("missing GL function {}", $name.to_string_lossy()));
                }
                unsafe { std::mem::transmute::<*const c_void, _>(p) }
            }};
        }
        macro_rules! opt {
            ($name:literal) => {{
                let p = get($name);
                (!p.is_null()).then(|| unsafe { std::mem::transmute::<*const c_void, _>(p) })
            }};
        }
        Ok(Self {
            create_memory_objects: req!(c"glCreateMemoryObjectsEXT"),
            delete_memory_objects: req!(c"glDeleteMemoryObjectsEXT"),
            memory_object_parameteriv: req!(c"glMemoryObjectParameterivEXT"),
            import_memory_win32: opt!(c"glImportMemoryWin32HandleEXT"),
            import_memory_fd: opt!(c"glImportMemoryFdEXT"),
            tex_storage_mem_2d: req!(c"glTexStorageMem2DEXT"),
            gen_semaphores: req!(c"glGenSemaphoresEXT"),
            delete_semaphores: req!(c"glDeleteSemaphoresEXT"),
            import_semaphore_win32: opt!(c"glImportSemaphoreWin32HandleEXT"),
            import_semaphore_fd: opt!(c"glImportSemaphoreFdEXT"),
            signal_semaphore: req!(c"glSignalSemaphoreEXT"),
            wait_semaphore: req!(c"glWaitSemaphoreEXT"),
            get_unsigned_bytei: req!(c"glGetUnsignedBytei_vEXT"),
        })
    }

    /// UUID of the GPU the GL context runs on (to pick the same Vulkan device).
    pub fn device_uuid(&self) -> [u8; 16] {
        let mut uuid = [0u8; 16];
        unsafe { (self.get_unsigned_bytei)(GL_DEVICE_UUID_EXT, 0, uuid.as_mut_ptr()) };
        uuid
    }
}

/// An OS handle exported from Vulkan, imported by GL.
pub enum ExternalHandle {
    #[cfg_attr(not(windows), allow(dead_code))]
    Win32(*mut c_void),
    #[cfg_attr(windows, allow(dead_code))]
    Fd(i32),
}

const COMPOSITE_VS: &str = r#"#version 330 core
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

/// Video + UI → PQ / BT.2020. Rows are flipped because GL's bottom-left
/// origin becomes Vulkan's top-left in the shared image.
const COMPOSITE_FS: &str = r#"#version 330 core
uniform sampler2D u_video;
uniform sampler2D u_ui;
uniform vec2 u_size;
uniform int u_video_hdr;
uniform float u_sdr_white; // nits
uniform int u_output_pq;   // 0: SDR surface (test mode), output sRGB
out vec4 frag;

const float m1 = 0.1593017578125;
const float m2 = 78.84375;
const float c1 = 0.8359375;
const float c2 = 18.8515625;
const float c3 = 18.6875;

vec3 pq_to_linear(vec3 e) { // -> nits / 10000
    vec3 p = pow(clamp(e, 0.0, 1.0), vec3(1.0 / m2));
    return pow(max(p - c1, 0.0) / (c2 - c3 * p), vec3(1.0 / m1));
}
vec3 linear_to_pq(vec3 l) {
    vec3 p = pow(clamp(l, 0.0, 1.0), vec3(m1));
    return pow((c1 + c2 * p) / (1.0 + c3 * p), vec3(m2));
}
vec3 srgb_to_linear(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(0.04045, c));
}
// BT.709 -> BT.2020 primaries (column-major).
const mat3 to2020 = mat3(
    0.6274, 0.0691, 0.0164,
    0.3293, 0.9195, 0.0880,
    0.0433, 0.0114, 0.8956);

void main() {
    vec2 uv = vec2(gl_FragCoord.x / u_size.x, 1.0 - gl_FragCoord.y / u_size.y);
    vec3 v = texture(u_video, uv).rgb;
    if (u_output_pq == 0) {
        vec4 u = texture(u_ui, uv);
        vec3 c = u.a > 0.0 ? mix(v, u.rgb / u.a, clamp(u.a, 0.0, 1.0)) : v;
        frag = vec4(clamp(c, 0.0, 1.0), 1.0);
        return;
    }
    vec3 lin = u_video_hdr == 1 ? pq_to_linear(v) : to2020 * srgb_to_linear(v) * (u_sdr_white / 10000.0);
    vec4 ui = texture(u_ui, uv);
    if (ui.a > 0.0) {
        vec3 ul = to2020 * srgb_to_linear(ui.rgb / ui.a) * (u_sdr_white / 10000.0);
        lin = mix(lin, ul, clamp(ui.a, 0.0, 1.0));
    }
    frag = vec4(linear_to_pq(max(lin, 0.0)), 1.0);
}
"#;

/// Off-screen GL targets for the Vulkan presenter.
pub struct GlTargets {
    ext: GlExt,
    pub video_fbo: u32,
    pub ui_fbo: u32,
    video_tex: u32,
    ui_tex: u32,
    shared_mem: u32,
    shared_tex: u32,
    pub shared_fbo: u32,
    sem_gl_done: u32,
    sem_vk_done: u32,
    program: glow::Program,
    vao: glow::VertexArray,
    size: (i32, i32),
    frames: u64,
}

fn gl_name<T>(raw: T) -> u32
where
    T: Into<std::num::NonZeroU32>,
{
    raw.into().get()
}

impl GlTargets {
    pub fn new(gl: &glow::Context, ext: GlExt, vk: &VkPresenter) -> Result<Self, String> {
        use glow::HasContext;
        unsafe {
            let program = gl.create_program()?;
            for (kind, src) in [(glow::VERTEX_SHADER, COMPOSITE_VS), (glow::FRAGMENT_SHADER, COMPOSITE_FS)] {
                let sh = gl.create_shader(kind)?;
                gl.shader_source(sh, src);
                gl.compile_shader(sh);
                if !gl.get_shader_compile_status(sh) {
                    return Err(format!("composite shader: {}", gl.get_shader_info_log(sh)));
                }
                gl.attach_shader(program, sh);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(format!("composite program: {}", gl.get_program_info_log(program)));
            }
            let vao = gl.create_vertex_array()?;

            // Semaphores are created once and live as long as the presenter.
            let (gl_done_handle, vk_done_handle) = vk.semaphore_handles()?;
            let mut sems = [0u32; 2];
            (ext.gen_semaphores)(2, sems.as_mut_ptr());
            for (sem, h) in [(sems[0], gl_done_handle), (sems[1], vk_done_handle)] {
                import_semaphore(&ext, sem, h)?;
            }

            let mut t = Self {
                ext,
                video_fbo: 0,
                ui_fbo: 0,
                video_tex: 0,
                ui_tex: 0,
                shared_mem: 0,
                shared_tex: 0,
                shared_fbo: 0,
                sem_gl_done: sems[0],
                sem_vk_done: sems[1],
                program,
                vao,
                size: (0, 0),
                frames: 0,
            };
            t.recreate(gl, vk)?;
            Ok(t)
        }
    }

    /// (Re)creates the size-dependent textures; call after `VkPresenter::resize`.
    pub fn recreate(&mut self, gl: &glow::Context, vk: &VkPresenter) -> Result<(), String> {
        use glow::HasContext;
        unsafe {
            self.delete_sized(gl);
            let (w, h) = (vk.extent.width as i32, vk.extent.height as i32);
            self.size = (w, h);

            let make = |internal: u32, format: u32, ty: u32| -> Result<(u32, u32), String> {
                let tex = gl.create_texture()?;
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, internal as i32, w, h, 0, format, ty, glow::PixelUnpackData::Slice(None));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
                let fbo = gl.create_framebuffer()?;
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
                Ok((gl_name(tex.0), gl_name(fbo.0)))
            };
            (self.video_tex, self.video_fbo) = make(GL_RGBA16F, glow::RGBA, glow::HALF_FLOAT)?;
            (self.ui_tex, self.ui_fbo) = make(glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE)?;

            // Shared image: memory allocated by Vulkan, used as a GL texture.
            let (handle, mem_size) = vk.shared_memory_handle()?;
            let mut mem = 0u32;
            (self.ext.create_memory_objects)(1, &mut mem);
            let dedicated = 1i32;
            (self.ext.memory_object_parameteriv)(mem, GL_DEDICATED_MEMORY_OBJECT_EXT, &dedicated);
            match handle {
                ExternalHandle::Win32(h) => {
                    let f = self.ext.import_memory_win32.ok_or("glImportMemoryWin32HandleEXT missing")?;
                    f(mem, mem_size, GL_HANDLE_TYPE_OPAQUE_WIN32_EXT, h);
                    close_handle(h);
                }
                ExternalHandle::Fd(fd) => {
                    let f = self.ext.import_memory_fd.ok_or("glImportMemoryFdEXT missing")?;
                    f(mem, mem_size, GL_HANDLE_TYPE_OPAQUE_FD_EXT, fd);
                }
            }
            self.shared_mem = mem;
            let tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, GL_TEXTURE_TILING_EXT, GL_OPTIMAL_TILING_EXT);
            (self.ext.tex_storage_mem_2d)(GL_TEXTURE_2D, 1, GL_RGB10_A2, w, h, mem, 0);
            let fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            self.shared_tex = gl_name(tex.0);
            self.shared_fbo = gl_name(fbo.0);
            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("shared framebuffer incomplete: 0x{status:x}"));
            }
            if let Some(e) = Some(gl.get_error()).filter(|e| *e != glow::NO_ERROR) {
                return Err(format!("GL error 0x{e:x} importing the shared image"));
            }
            self.frames = 0;
            Ok(())
        }
    }

    fn delete_sized(&mut self, gl: &glow::Context) {
        use glow::HasContext;
        unsafe {
            for fbo in [self.video_fbo, self.ui_fbo, self.shared_fbo] {
                if let Some(n) = std::num::NonZeroU32::new(fbo) {
                    gl.delete_framebuffer(glow::NativeFramebuffer(n));
                }
            }
            for tex in [self.video_tex, self.ui_tex, self.shared_tex] {
                if let Some(n) = std::num::NonZeroU32::new(tex) {
                    gl.delete_texture(glow::NativeTexture(n));
                }
            }
            if self.shared_mem != 0 {
                (self.ext.delete_memory_objects)(1, &self.shared_mem);
            }
            (self.video_fbo, self.ui_fbo, self.shared_fbo) = (0, 0, 0);
            (self.video_tex, self.ui_tex, self.shared_tex, self.shared_mem) = (0, 0, 0, 0);
        }
    }

    pub fn size(&self) -> (i32, i32) {
        self.size
    }

    /// Waits until Vulkan has finished reading the shared image.
    pub fn begin_frame(&mut self) {
        if self.frames > 0 {
            let layout = GL_LAYOUT_TRANSFER_SRC_EXT;
            unsafe { (self.ext.wait_semaphore)(self.sem_vk_done, 0, std::ptr::null(), 1, &self.shared_tex, &layout) };
        }
    }

    /// Composites video + UI into the shared image as PQ / BT.2020.
    pub fn composite(&mut self, gl: &glow::Context, video_hdr: bool, sdr_white_nits: f32, output_pq: bool) {
        use glow::HasContext;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, std::num::NonZeroU32::new(self.shared_fbo).map(glow::NativeFramebuffer));
            gl.viewport(0, 0, self.size.0, self.size.1);
            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, std::num::NonZeroU32::new(self.video_tex).map(glow::NativeTexture));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, std::num::NonZeroU32::new(self.ui_tex).map(glow::NativeTexture));
            let loc = |n: &str| gl.get_uniform_location(self.program, n);
            gl.uniform_1_i32(loc("u_video").as_ref(), 0);
            gl.uniform_1_i32(loc("u_ui").as_ref(), 1);
            gl.uniform_2_f32(loc("u_size").as_ref(), self.size.0 as f32, self.size.1 as f32);
            gl.uniform_1_i32(loc("u_video_hdr").as_ref(), video_hdr as i32);
            gl.uniform_1_f32(loc("u_sdr_white").as_ref(), sdr_white_nits);
            gl.uniform_1_i32(loc("u_output_pq").as_ref(), output_pq as i32);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }

    /// Hands the shared image to Vulkan.
    pub fn end_frame(&mut self, gl: &glow::Context) {
        use glow::HasContext;
        let layout = GL_LAYOUT_TRANSFER_SRC_EXT;
        unsafe {
            (self.ext.signal_semaphore)(self.sem_gl_done, 0, std::ptr::null(), 1, &self.shared_tex, &layout);
            gl.flush();
        }
        self.frames += 1;
    }

    pub fn destroy(&mut self, gl: &glow::Context) {
        use glow::HasContext;
        self.delete_sized(gl);
        unsafe {
            (self.ext.delete_semaphores)(2, [self.sem_gl_done, self.sem_vk_done].as_ptr());
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
        }
    }
}

unsafe fn import_semaphore(ext: &GlExt, sem: u32, h: ExternalHandle) -> Result<(), String> {
    match h {
        ExternalHandle::Win32(h) => {
            let f = ext.import_semaphore_win32.ok_or("glImportSemaphoreWin32HandleEXT missing")?;
            f(sem, GL_HANDLE_TYPE_OPAQUE_WIN32_EXT, h);
            close_handle(h);
        }
        ExternalHandle::Fd(fd) => {
            let f = ext.import_semaphore_fd.ok_or("glImportSemaphoreFdEXT missing")?;
            f(sem, GL_HANDLE_TYPE_OPAQUE_FD_EXT, fd);
        }
    }
    Ok(())
}

/// Opaque Win32 handles stay owned by the app after import; close ours.
fn close_handle(h: *mut c_void) {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(h));
    }
    #[cfg(not(windows))]
    let _ = h;
}

// -------------------------------------------------------------- Vulkan side

#[cfg(windows)]
const MEM_HANDLE: vk::ExternalMemoryHandleTypeFlags = vk::ExternalMemoryHandleTypeFlags::OPAQUE_WIN32;
#[cfg(not(windows))]
const MEM_HANDLE: vk::ExternalMemoryHandleTypeFlags = vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD;
#[cfg(windows)]
const SEM_HANDLE: vk::ExternalSemaphoreHandleTypeFlags = vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_WIN32;
#[cfg(not(windows))]
const SEM_HANDLE: vk::ExternalSemaphoreHandleTypeFlags = vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD;

/// Format GL renders to (GL_RGB10_A2 has this memory layout).
const SHARED_FORMAT: vk::Format = vk::Format::A2B10G10R10_UNORM_PACK32;

struct SharedImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
    size: u64,
}

pub struct VkPresenter {
    _entry: ash::Entry,
    instance: ash::Instance,
    surface_fn: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    pdev: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    qfi: u32,
    swapchain_fn: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    swap_images: Vec<vk::Image>,
    surface_format: vk::SurfaceFormatKHR,
    pub extent: vk::Extent2D,
    hdr_metadata_fn: Option<ash::ext::hdr_metadata::Device>,
    #[cfg(windows)]
    mem_export: ash::khr::external_memory_win32::Device,
    #[cfg(windows)]
    sem_export: ash::khr::external_semaphore_win32::Device,
    #[cfg(not(windows))]
    mem_export: ash::khr::external_memory_fd::Device,
    #[cfg(not(windows))]
    sem_export: ash::khr::external_semaphore_fd::Device,
    shared: Option<SharedImage>,
    gl_done: vk::Semaphore,
    vk_done: vk::Semaphore,
    acquired: vk::Semaphore,
    render_done: Vec<vk::Semaphore>,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    peak_nits: f32,
    /// The swapchain is HDR10 (PQ). False only in the SDR test mode.
    pub hdr: bool,
    pub description: String,
}

fn vkerr(what: &str) -> impl Fn(vk::Result) -> String + '_ {
    move |e| format!("{what}: {e:?}")
}

impl VkPresenter {
    /// Creates the presenter on `window`, on the same GPU as the GL context
    /// (`gl_uuid`). Fails if the surface can't do HDR10.
    pub fn new(window: &Window, gl_uuid: [u8; 16], peak_nits: f32) -> Result<Self, String> {
        unsafe {
            let entry = ash::Entry::load().map_err(|e| format!("Vulkan loader: {e}"))?;
            let display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
            let win = window.window_handle().map_err(|e| e.to_string())?.as_raw();

            let mut inst_exts: Vec<*const std::ffi::c_char> =
                ash_window::enumerate_required_extensions(display).map_err(vkerr("surface extensions"))?.to_vec();
            let available = entry.enumerate_instance_extension_properties(None).map_err(vkerr("instance extensions"))?;
            let has_inst = |n: &CStr| available.iter().any(|e| e.extension_name_as_c_str() == Ok(n));
            if !has_inst(ash::ext::swapchain_colorspace::NAME) {
                return Err("VK_EXT_swapchain_colorspace unavailable".into());
            }
            inst_exts.push(ash::ext::swapchain_colorspace::NAME.as_ptr());
            let app = vk::ApplicationInfo::default().application_name(c"IINA-RS").api_version(vk::API_VERSION_1_1);
            let instance = entry
                .create_instance(&vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&inst_exts), None)
                .map_err(vkerr("vkCreateInstance"))?;

            // From here on, clean up the instance on failure.
            let result = (|| -> Result<Self, String> {
                let surface_fn = ash::khr::surface::Instance::new(&entry, &instance);
                let surface = ash_window::create_surface(&entry, &instance, display, win, None).map_err(vkerr("create surface"))?;

                // Same physical device as the GL context.
                let mut chosen = None;
                for pdev in instance.enumerate_physical_devices().map_err(vkerr("enumerate devices"))? {
                    let mut id = vk::PhysicalDeviceIDProperties::default();
                    let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
                    instance.get_physical_device_properties2(pdev, &mut props2);
                    let name = props2.properties.device_name_as_c_str().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
                    if id.device_uuid != gl_uuid {
                        continue;
                    }
                    let families = instance.get_physical_device_queue_family_properties(pdev);
                    for (i, f) in families.iter().enumerate() {
                        if f.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                            && surface_fn.get_physical_device_surface_support(pdev, i as u32, surface).unwrap_or(false)
                        {
                            chosen = Some((pdev, i as u32, name.clone()));
                            break;
                        }
                    }
                }
                let Some((pdev, qfi, gpu_name)) = chosen else {
                    surface_fn.destroy_surface(surface, None);
                    return Err("no Vulkan device matches the OpenGL GPU".into());
                };

                // HDR10 surface format.
                let formats = surface_fn.get_physical_device_surface_formats(pdev, surface).map_err(vkerr("surface formats"))?;
                let hdr10 = formats
                    .iter()
                    .find(|f| f.color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT && f.format == SHARED_FORMAT)
                    .or_else(|| formats.iter().find(|f| f.color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT))
                    .copied();
                // Test mode (IINA_RS_FORCE_VULKAN): run the whole pipeline on a
                // plain SDR surface when HDR is off.
                let sdr_test = || {
                    std::env::var_os("IINA_RS_FORCE_VULKAN")?;
                    formats.iter().find(|f| f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR && f.format == vk::Format::B8G8R8A8_UNORM).copied()
                };
                let Some(surface_format) = hdr10.or_else(sdr_test) else {
                    surface_fn.destroy_surface(surface, None);
                    return Err("the display surface offers no HDR10 format".into());
                };
                let hdr = surface_format.color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT;

                let dev_exts_avail = instance.enumerate_device_extension_properties(pdev).map_err(vkerr("device extensions"))?;
                let has_dev = |n: &CStr| dev_exts_avail.iter().any(|e| e.extension_name_as_c_str() == Ok(n));
                #[cfg(windows)]
                let (mem_ext, sem_ext) = (ash::khr::external_memory_win32::NAME, ash::khr::external_semaphore_win32::NAME);
                #[cfg(not(windows))]
                let (mem_ext, sem_ext) = (ash::khr::external_memory_fd::NAME, ash::khr::external_semaphore_fd::NAME);
                let mut dev_exts = vec![ash::khr::swapchain::NAME.as_ptr()];
                for n in [mem_ext, sem_ext] {
                    if !has_dev(n) {
                        surface_fn.destroy_surface(surface, None);
                        return Err(format!("{} unavailable", n.to_string_lossy()));
                    }
                    dev_exts.push(n.as_ptr());
                }
                let hdr_meta = has_dev(ash::ext::hdr_metadata::NAME);
                if hdr_meta {
                    dev_exts.push(ash::ext::hdr_metadata::NAME.as_ptr());
                }
                let prio = [1.0f32];
                let qinfo = [vk::DeviceQueueCreateInfo::default().queue_family_index(qfi).queue_priorities(&prio)];
                let device = instance
                    .create_device(pdev, &vk::DeviceCreateInfo::default().queue_create_infos(&qinfo).enabled_extension_names(&dev_exts), None)
                    .map_err(vkerr("vkCreateDevice"))?;
                let queue = device.get_device_queue(qfi, 0);

                let export_sem = |device: &ash::Device| -> Result<vk::Semaphore, String> {
                    let mut export = vk::ExportSemaphoreCreateInfo::default().handle_types(SEM_HANDLE);
                    device.create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut export), None).map_err(vkerr("export semaphore"))
                };
                let gl_done = export_sem(&device)?;
                let vk_done = export_sem(&device)?;
                let acquired = device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).map_err(vkerr("semaphore"))?;
                let pool = device
                    .create_command_pool(
                        &vk::CommandPoolCreateInfo::default().queue_family_index(qfi).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                        None,
                    )
                    .map_err(vkerr("command pool"))?;
                let cmd = device
                    .allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(pool).command_buffer_count(1))
                    .map_err(vkerr("command buffer"))?[0];
                let fence = device
                    .create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None)
                    .map_err(vkerr("fence"))?;

                let size = window.inner_size();
                let mut p = Self {
                    swapchain_fn: ash::khr::swapchain::Device::new(&instance, &device),
                    hdr_metadata_fn: hdr_meta.then(|| ash::ext::hdr_metadata::Device::new(&instance, &device)),
                    #[cfg(windows)]
                    mem_export: ash::khr::external_memory_win32::Device::new(&instance, &device),
                    #[cfg(windows)]
                    sem_export: ash::khr::external_semaphore_win32::Device::new(&instance, &device),
                    #[cfg(not(windows))]
                    mem_export: ash::khr::external_memory_fd::Device::new(&instance, &device),
                    #[cfg(not(windows))]
                    sem_export: ash::khr::external_semaphore_fd::Device::new(&instance, &device),
                    _entry: entry.clone(),
                    instance: instance.clone(),
                    surface_fn,
                    surface,
                    pdev,
                    device,
                    queue,
                    qfi,
                    swapchain: vk::SwapchainKHR::null(),
                    swap_images: vec![],
                    surface_format,
                    extent: vk::Extent2D { width: size.width.max(1), height: size.height.max(1) },
                    shared: None,
                    gl_done,
                    vk_done,
                    acquired,
                    render_done: vec![],
                    pool,
                    cmd,
                    fence,
                    peak_nits,
                    hdr,
                    description: format!("{} · Vulkan · {gpu_name} · {:?}", if hdr { "HDR10" } else { "SDR (test)" }, surface_format.format),
                };
                p.resize(size.width, size.height)?;
                Ok(p)
            })();
            if result.is_err() {
                instance.destroy_instance(None);
            }
            result
        }
    }

    pub fn semaphore_handles(&self) -> Result<(ExternalHandle, ExternalHandle), String> {
        Ok((self.export_semaphore(self.gl_done)?, self.export_semaphore(self.vk_done)?))
    }

    #[cfg(windows)]
    fn export_semaphore(&self, s: vk::Semaphore) -> Result<ExternalHandle, String> {
        let info = vk::SemaphoreGetWin32HandleInfoKHR::default().semaphore(s).handle_type(SEM_HANDLE);
        unsafe { self.sem_export.get_semaphore_win32_handle(&info) }.map(|h| ExternalHandle::Win32(h as *mut c_void)).map_err(vkerr("export semaphore handle"))
    }

    #[cfg(not(windows))]
    fn export_semaphore(&self, s: vk::Semaphore) -> Result<ExternalHandle, String> {
        let info = vk::SemaphoreGetFdInfoKHR::default().semaphore(s).handle_type(SEM_HANDLE);
        unsafe { self.sem_export.get_semaphore_fd(&info) }.map(ExternalHandle::Fd).map_err(vkerr("export semaphore fd"))
    }

    /// Handle + allocation size of the current shared image's memory.
    pub fn shared_memory_handle(&self) -> Result<(ExternalHandle, u64), String> {
        let shared = self.shared.as_ref().ok_or("no shared image")?;
        #[cfg(windows)]
        let h = {
            let info = vk::MemoryGetWin32HandleInfoKHR::default().memory(shared.memory).handle_type(MEM_HANDLE);
            ExternalHandle::Win32(unsafe { self.mem_export.get_memory_win32_handle(&info) }.map_err(vkerr("export memory handle"))? as *mut c_void)
        };
        #[cfg(not(windows))]
        let h = {
            let info = vk::MemoryGetFdInfoKHR::default().memory(shared.memory).handle_type(MEM_HANDLE);
            ExternalHandle::Fd(unsafe { self.mem_export.get_memory_fd(&info) }.map_err(vkerr("export memory fd"))?)
        };
        Ok((h, shared.size))
    }

    /// Recreates the swapchain and the shared image for a new window size.
    /// The GL side must re-import afterwards (`GlTargets::recreate`).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        unsafe {
            let _ = self.device.device_wait_idle();
            let caps = self
                .surface_fn
                .get_physical_device_surface_capabilities(self.pdev, self.surface)
                .map_err(vkerr("surface capabilities"))?;
            let extent = if caps.current_extent.width != u32::MAX {
                caps.current_extent
            } else {
                vk::Extent2D {
                    width: width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                    height: height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
                }
            };
            if extent.width == 0 || extent.height == 0 {
                return Ok(()); // minimized
            }
            let image_count = (caps.min_image_count + 1).min(if caps.max_image_count > 0 { caps.max_image_count } else { u32::MAX });
            let old = self.swapchain;
            let info = vk::SwapchainCreateInfoKHR::default()
                .surface(self.surface)
                .min_image_count(image_count)
                .image_format(self.surface_format.format)
                .image_color_space(self.surface_format.color_space)
                .image_extent(extent)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(caps.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(vk::PresentModeKHR::FIFO)
                .clipped(true)
                .old_swapchain(old);
            self.swapchain = self.swapchain_fn.create_swapchain(&info, None).map_err(vkerr("create swapchain"))?;
            if old != vk::SwapchainKHR::null() {
                self.swapchain_fn.destroy_swapchain(old, None);
            }
            self.swap_images = self.swapchain_fn.get_swapchain_images(self.swapchain).map_err(vkerr("swapchain images"))?;
            for s in self.render_done.drain(..) {
                self.device.destroy_semaphore(s, None);
            }
            for _ in &self.swap_images {
                self.render_done.push(self.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).map_err(vkerr("semaphore"))?);
            }
            self.extent = extent;
            self.set_hdr_metadata();
            self.recreate_shared()?;
            Ok(())
        }
    }

    fn set_hdr_metadata(&self) {
        let Some(f) = self.hdr_metadata_fn.as_ref().filter(|_| self.hdr) else { return };
        // Describe the content as mastered for this display (no extra mapping).
        let xy = |x, y| vk::XYColorEXT { x, y };
        let md = vk::HdrMetadataEXT::default()
            .display_primary_red(xy(0.708, 0.292))
            .display_primary_green(xy(0.170, 0.797))
            .display_primary_blue(xy(0.131, 0.046))
            .white_point(xy(0.3127, 0.3290))
            .max_luminance(self.peak_nits)
            .min_luminance(0.0)
            .max_content_light_level(self.peak_nits)
            .max_frame_average_light_level(self.peak_nits);
        unsafe { f.set_hdr_metadata(&[self.swapchain], &[md]) };
    }

    unsafe fn recreate_shared(&mut self) -> Result<(), String> {
        if let Some(s) = self.shared.take() {
            self.device.destroy_image(s.image, None);
            self.device.free_memory(s.memory, None);
        }
        let mut ext = vk::ExternalMemoryImageCreateInfo::default().handle_types(MEM_HANDLE);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(SHARED_FORMAT)
            .extent(vk::Extent3D { width: self.extent.width, height: self.extent.height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut ext);
        let image = self.device.create_image(&info, None).map_err(vkerr("shared image"))?;
        let req = self.device.get_image_memory_requirements(image);
        let props = self.instance.get_physical_device_memory_properties(self.pdev);
        let type_index = (0..props.memory_type_count)
            .find(|&i| req.memory_type_bits & (1 << i) != 0 && props.memory_types[i as usize].property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL))
            .ok_or("no device-local memory type")?;
        let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(MEM_HANDLE);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(type_index)
            .push_next(&mut export)
            .push_next(&mut dedicated);
        let memory = self.device.allocate_memory(&alloc, None).map_err(vkerr("shared memory"))?;
        self.device.bind_image_memory(image, memory, 0).map_err(vkerr("bind shared memory"))?;
        self.shared = Some(SharedImage { image, memory, size: req.size });
        Ok(())
    }

    /// Copies the shared image (rendered by GL, signalled with `gl_done`) to
    /// the swapchain and presents it. Returns false when the swapchain is out
    /// of date (caller resizes).
    pub fn present(&mut self) -> Result<bool, String> {
        unsafe {
            let d = &self.device;
            d.wait_for_fences(&[self.fence], true, u64::MAX).map_err(vkerr("wait fence"))?;
            let (index, _suboptimal) = match self.swapchain_fn.acquire_next_image(self.swapchain, u64::MAX, self.acquired, vk::Fence::null()) {
                Ok(r) => r,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(false),
                Err(e) => return Err(format!("acquire: {e:?}")),
            };
            d.reset_fences(&[self.fence]).map_err(vkerr("reset fence"))?;
            let shared = self.shared.as_ref().ok_or("no shared image")?;
            let swap = self.swap_images[index as usize];
            let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
            d.reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty()).map_err(vkerr("reset cmd"))?;
            d.begin_command_buffer(self.cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))
                .map_err(vkerr("begin cmd"))?;
            let acquire_shared = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                .dst_queue_family_index(self.qfi)
                .image(shared.image)
                .subresource_range(range);
            let to_dst = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(swap)
                .subresource_range(range);
            d.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[acquire_shared, to_dst],
            );
            let layers = vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1);
            let corner = vk::Offset3D { x: self.extent.width as i32, y: self.extent.height as i32, z: 1 };
            let blit = vk::ImageBlit::default()
                .src_subresource(layers)
                .src_offsets([vk::Offset3D::default(), corner])
                .dst_subresource(layers)
                .dst_offsets([vk::Offset3D::default(), corner]);
            d.cmd_blit_image(
                self.cmd,
                shared.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                swap,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[blit],
                vk::Filter::NEAREST,
            );
            let release_shared = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::empty())
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(self.qfi)
                .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                .image(shared.image)
                .subresource_range(range);
            let to_present = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::empty())
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(swap)
                .subresource_range(range);
            d.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[release_shared, to_present],
            );
            d.end_command_buffer(self.cmd).map_err(vkerr("end cmd"))?;

            let wait = [self.acquired, self.gl_done];
            let stages = [vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER];
            let signal = [self.render_done[index as usize], self.vk_done];
            let cmds = [self.cmd];
            let submit = vk::SubmitInfo::default().wait_semaphores(&wait).wait_dst_stage_mask(&stages).command_buffers(&cmds).signal_semaphores(&signal);
            d.queue_submit(self.queue, &[submit], self.fence).map_err(vkerr("submit"))?;

            let swapchains = [self.swapchain];
            let indices = [index];
            let present_wait = [self.render_done[index as usize]];
            let present = vk::PresentInfoKHR::default().wait_semaphores(&present_wait).swapchains(&swapchains).image_indices(&indices);
            match self.swapchain_fn.queue_present(self.queue, &present) {
                Ok(false) => Ok(true),
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(false),
                Err(e) => Err(format!("present: {e:?}")),
            }
        }
    }
}

impl Drop for VkPresenter {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(s) = self.shared.take() {
                self.device.destroy_image(s.image, None);
                self.device.free_memory(s.memory, None);
            }
            for s in self.render_done.drain(..) {
                self.device.destroy_semaphore(s, None);
            }
            for s in [self.gl_done, self.vk_done, self.acquired] {
                self.device.destroy_semaphore(s, None);
            }
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.pool, None);
            self.swapchain_fn.destroy_swapchain(self.swapchain, None);
            self.device.destroy_device(None);
            self.surface_fn.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}
