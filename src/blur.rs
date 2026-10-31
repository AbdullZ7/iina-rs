//! Frosted glass behind the controller, title bar and OSD (IINA's
//! NSVisualEffectView): the video frame is blurred with the dual Kawase
//! filter at half resolution and below, once per new video frame, and the
//! HUD panels show that blurred copy through rounded masks.

use glow::HasContext;

/// A panel to show blurred video under, in window pixels (top-left origin).
#[derive(Clone, Copy, Debug)]
pub struct BlurRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub radius: f32,
    pub alpha: f32,
}

/// At most this many panels at once (shader uniform arrays).
pub const MAX_RECTS: usize = 8;

/// Levels: 1/2, 1/4, 1/8, 1/16 of the video. The result is level 0.
const LEVELS: usize = 4;

const VS: &str = r#"#version 330 core
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const DOWN_FS: &str = r#"#version 330 core
uniform sampler2D u_src;
uniform vec2 u_half; // half a source texel, in uv
uniform vec2 u_size; // target size
out vec4 frag;
void main() {
    vec2 uv = gl_FragCoord.xy / u_size;
    vec4 s = texture(u_src, uv) * 4.0;
    s += texture(u_src, uv - u_half);
    s += texture(u_src, uv + u_half);
    s += texture(u_src, uv + vec2(u_half.x, -u_half.y));
    s += texture(u_src, uv - vec2(u_half.x, -u_half.y));
    frag = vec4(s.rgb / 8.0, 1.0);
}
"#;

const UP_FS: &str = r#"#version 330 core
uniform sampler2D u_src;
uniform vec2 u_half;
uniform vec2 u_size;
out vec4 frag;
void main() {
    vec2 uv = gl_FragCoord.xy / u_size;
    vec2 h = u_half;
    vec4 s = texture(u_src, uv + vec2(-h.x * 2.0, 0.0));
    s += texture(u_src, uv + vec2(-h.x, h.y)) * 2.0;
    s += texture(u_src, uv + vec2(0.0, h.y * 2.0));
    s += texture(u_src, uv + vec2(h.x, h.y)) * 2.0;
    s += texture(u_src, uv + vec2(h.x * 2.0, 0.0));
    s += texture(u_src, uv + vec2(h.x, -h.y)) * 2.0;
    s += texture(u_src, uv + vec2(0.0, -h.y * 2.0));
    s += texture(u_src, uv + vec2(-h.x, -h.y)) * 2.0;
    frag = vec4(s.rgb / 12.0, 1.0);
}
"#;

/// Shared with the HDR composite shader (vk_present.rs): the blurred copy's
/// coverage at window pixel `p` (top-left origin).
pub const MASK_GLSL: &str = r#"
uniform vec4 u_rects[8];   // x0, y0, x1, y1 (window pixels, top-left origin)
uniform vec2 u_rect_ra[8]; // corner radius, alpha
uniform int u_nrects;
float blur_mask(vec2 p) {
    float m = 0.0;
    for (int i = 0; i < u_nrects; i++) {
        vec4 r = u_rects[i];
        float rad = u_rect_ra[i].x;
        vec2 c = (r.xy + r.zw) * 0.5;
        vec2 q = abs(p - c) - (r.zw - r.xy) * 0.5 + rad;
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - rad;
        m = max(m, clamp(0.5 - d, 0.0, 1.0) * u_rect_ra[i].y);
    }
    return m;
}
"#;

/// Draws the blurred copy into the bound framebuffer (SDR output).
fn draw_fs() -> String {
    format!(
        r#"#version 330 core
uniform sampler2D u_blur;
uniform vec2 u_size;
out vec4 frag;
{MASK_GLSL}
void main() {{
    vec2 uv = gl_FragCoord.xy / u_size;
    float m = blur_mask(vec2(gl_FragCoord.x, u_size.y - gl_FragCoord.y));
    if (m <= 0.0) discard;
    frag = vec4(texture(u_blur, uv).rgb, m);
}}
"#
    )
}

struct Level {
    tex: glow::Texture,
    fbo: glow::Framebuffer,
    w: i32,
    h: i32,
}

pub struct Blur {
    down: glow::Program,
    up: glow::Program,
    draw: glow::Program,
    vao: glow::VertexArray,
    levels: Vec<Level>,
    src_size: (i32, i32),
    /// The blurred copy matches the current video frame.
    pub valid: bool,
}

unsafe fn program(gl: &glow::Context, fs: &str) -> Result<glow::Program, String> {
    let p = gl.create_program()?;
    for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, fs)] {
        let sh = gl.create_shader(kind)?;
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            return Err(format!("blur shader: {}", gl.get_shader_info_log(sh)));
        }
        gl.attach_shader(p, sh);
        gl.delete_shader(sh);
    }
    gl.link_program(p);
    if !gl.get_program_link_status(p) {
        return Err(format!("blur program: {}", gl.get_program_info_log(p)));
    }
    Ok(p)
}

/// Sets the mask uniforms of `prog` (which includes `MASK_GLSL`).
pub fn set_rects(gl: &glow::Context, prog: glow::Program, rects: &[BlurRect]) {
    let n = rects.len().min(MAX_RECTS);
    let mut r = [0f32; MAX_RECTS * 4];
    let mut ra = [0f32; MAX_RECTS * 2];
    for (i, b) in rects.iter().take(n).enumerate() {
        r[i * 4..i * 4 + 4].copy_from_slice(&[b.min[0], b.min[1], b.max[0], b.max[1]]);
        ra[i * 2..i * 2 + 2].copy_from_slice(&[b.radius, b.alpha]);
    }
    unsafe {
        let loc = |s: &str| gl.get_uniform_location(prog, s);
        gl.uniform_4_f32_slice(loc("u_rects").as_ref(), &r);
        gl.uniform_2_f32_slice(loc("u_rect_ra").as_ref(), &ra);
        gl.uniform_1_i32(loc("u_nrects").as_ref(), n as i32);
    }
}

impl Blur {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            Ok(Self {
                down: program(gl, DOWN_FS)?,
                up: program(gl, UP_FS)?,
                draw: program(gl, &draw_fs())?,
                vao: gl.create_vertex_array()?,
                levels: Vec::new(),
                src_size: (0, 0),
                valid: false,
            })
        }
    }

    /// The blurred copy (half the video's size), once `update` ran.
    pub fn texture(&self) -> Option<glow::Texture> {
        self.levels.first().map(|l| l.tex).filter(|_| self.valid)
    }

    fn resize(&mut self, gl: &glow::Context, w: i32, h: i32) -> Result<(), String> {
        if self.src_size == (w, h) && !self.levels.is_empty() {
            return Ok(());
        }
        self.free_levels(gl);
        unsafe {
            let (mut lw, mut lh) = (w, h);
            for _ in 0..LEVELS {
                lw = (lw / 2).max(1);
                lh = (lh / 2).max(1);
                let tex = gl.create_texture()?;
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA16F as i32, lw, lh, 0, glow::RGBA, glow::HALF_FLOAT, glow::PixelUnpackData::Slice(None));
                for (p, v) in [
                    (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                    (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                    (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                    (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
                ] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
                }
                let fbo = gl.create_framebuffer()?;
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
                let ok = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
                self.levels.push(Level { tex, fbo, w: lw, h: lh });
                if !ok {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                    gl.bind_texture(glow::TEXTURE_2D, None);
                    self.free_levels(gl);
                    return Err("blur framebuffer incomplete".into());
                }
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        self.src_size = (w, h);
        Ok(())
    }

    fn free_levels(&mut self, gl: &glow::Context) {
        unsafe {
            for l in self.levels.drain(..) {
                gl.delete_framebuffer(l.fbo);
                gl.delete_texture(l.tex);
            }
        }
        self.src_size = (0, 0);
        self.valid = false;
    }

    /// Blurs `src` (a `w`×`h` texture). Leaves framebuffer 0 bound.
    pub fn update(&mut self, gl: &glow::Context, src: glow::Texture, w: i32, h: i32) -> Result<(), String> {
        self.resize(gl, w, h)?;
        unsafe {
            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            let pass = |prog: glow::Program, from: glow::Texture, fw: i32, fh: i32, to: &Level| {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(to.fbo));
                gl.viewport(0, 0, to.w, to.h);
                gl.use_program(Some(prog));
                gl.bind_texture(glow::TEXTURE_2D, Some(from));
                let loc = |s: &str| gl.get_uniform_location(prog, s);
                gl.uniform_1_i32(loc("u_src").as_ref(), 0);
                gl.uniform_2_f32(loc("u_half").as_ref(), 0.5 / fw as f32 * 2.0, 0.5 / fh as f32 * 2.0);
                gl.uniform_2_f32(loc("u_size").as_ref(), to.w as f32, to.h as f32);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            };
            // The video texture may use nearest filtering: sample it linearly.
            gl.bind_texture(glow::TEXTURE_2D, Some(src));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            pass(self.down, src, w, h, &self.levels[0]);
            gl.bind_texture(glow::TEXTURE_2D, Some(src));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            for i in 1..LEVELS {
                let (a, b) = (&self.levels[i - 1], &self.levels[i]);
                pass(self.down, a.tex, a.w, a.h, b);
            }
            for i in (0..LEVELS - 1).rev() {
                let (a, b) = (&self.levels[i + 1], &self.levels[i]);
                pass(self.up, a.tex, a.w, a.h, b);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        self.valid = true;
        Ok(())
    }

    /// Draws the blurred copy under `rects` into the bound framebuffer
    /// (`w`×`h`, bottom-left origin, e.g. the window).
    pub fn draw(&self, gl: &glow::Context, rects: &[BlurRect], w: i32, h: i32) {
        let Some(tex) = self.texture() else { return };
        if rects.is_empty() {
            return;
        }
        unsafe {
            gl.viewport(0, 0, w, h);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.draw));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            let loc = |s: &str| gl.get_uniform_location(self.draw, s);
            gl.uniform_1_i32(loc("u_blur").as_ref(), 0);
            gl.uniform_2_f32(loc("u_size").as_ref(), w as f32, h as f32);
            set_rects(gl, self.draw, rects);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::BLEND);
        }
    }
}
