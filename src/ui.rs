//! egui overlay that mimics IINA's floating on-screen controller, sidebar
//! (playlist / chapters / quick settings) and welcome window.

use crate::mpv::Mpv;
use crate::settings::Settings;
use crate::state::{Player, Track};
use crate::thumbs::Thumbnails;
use std::collections::HashMap;
use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Painter, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, Ui,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Playlist,
    Chapters,
    Video,
    Audio,
    Subtitles,
}

pub struct UiState {
    pub sidebar: Option<Tab>,
    pub last_activity: Instant,
    pub url_input: Option<String>,
    pub context_menu: Option<Pos2>,
    pub show_about: bool,
    pub music_mode: bool,
    pub pip: bool,
    pub seek_preview: Option<f64>,
    pub pointer_over_ui: bool,
    pub recent: Vec<String>,
    pub fullscreen: bool,
    pub osc_visible: bool,
    pub settings: Settings,
    /// Set when the UI changed a setting; the app applies and saves it.
    pub settings_dirty: bool,
    pub show_prefs: bool,
    pub show_inspector: bool,
    pub jump_input: Option<String>,
    /// Audio equalizer gains (dB) for `EQ_FREQS`.
    pub eq: [f32; 10],
    /// Where the seek bar was last drawn (used by the debug hover aid).
    pub seek_bar_rect: Option<Rect>,
    thumb_path: String,
    thumb_tex: HashMap<usize, egui::TextureHandle>,
}

impl UiState {
    pub fn new(recent: Vec<String>, settings: Settings) -> Self {
        Self {
            sidebar: None,
            last_activity: Instant::now(),
            url_input: None,
            context_menu: None,
            show_about: false,
            music_mode: false,
            pip: false,
            seek_preview: None,
            pointer_over_ui: false,
            recent,
            fullscreen: false,
            osc_visible: true,
            settings,
            settings_dirty: false,
            show_prefs: false,
            show_inspector: false,
            jump_input: None,
            eq: [0.0; 10],
            seek_bar_rect: None,
            thumb_path: String::new(),
            thumb_tex: HashMap::new(),
        }
    }

    pub fn toggle_tab(&mut self, tab: Tab) {
        self.sidebar = if self.sidebar == Some(tab) { None } else { Some(tab) };
    }
}

const HUD_BG: Color32 = Color32::from_rgba_premultiplied(24, 24, 26, 225);
const FG: Color32 = Color32::from_gray(235);
const FG_DIM: Color32 = Color32::from_gray(150);
const TRACK_BG: Color32 = Color32::from_rgba_premultiplied(110, 110, 110, 160);

pub fn fmt_time(t: f64) -> String {
    if !t.is_finite() || t < 0.0 {
        return "--:--".into();
    }
    let s = t as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub fn file_name(path: &str) -> String {
    if path.contains("://") {
        return path.to_string();
    }
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

pub fn setup_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(28, 28, 30);
    visuals.window_fill = Color32::from_rgb(36, 36, 38);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.selection.bg_fill = Color32::from_rgb(64, 120, 220);
    ctx.set_visuals(visuals);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 4.0);
        s.interaction.tooltip_delay = 0.4;
    });
    install_fallback_fonts(ctx);
}

/// egui's bundled font only covers Latin; add system fonts so Arabic, CJK,
/// Cyrillic etc. file names and track titles render.
fn install_fallback_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = if cfg!(windows) {
        &[
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\YuGothR.ttc",
            "C:\\Windows\\Fonts\\malgun.ttf",
            "C:\\Windows\\Fonts\\seguisym.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        ]
    };
    let mut fonts = egui::FontDefinitions::default();
    for (i, path) in candidates.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(path) {
            let name = format!("system-{i}");
            fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes).into());
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.clone());
            }
        }
    }
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------- icons ---

type IconFn = fn(&Painter, Rect, Color32);

fn icon_play(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.22);
    p.add(Shape::convex_polygon(
        vec![r.left_top() + vec2(r.width() * 0.1, 0.0), pos2(r.right(), r.center().y), r.left_bottom() + vec2(r.width() * 0.1, 0.0)],
        c,
        Stroke::NONE,
    ));
}

fn icon_pause(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.25);
    let w = r.width() * 0.32;
    p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), 1.5, c);
    p.rect_filled(Rect::from_min_size(pos2(r.right() - w, r.top()), vec2(w, r.height())), 1.5, c);
}

fn double_triangle(p: &Painter, r: Rect, c: Color32, left: bool) {
    let r = r.shrink(r.width() * 0.24);
    let h = r.width() / 2.0;
    for i in 0..2 {
        let x0 = r.left() + i as f32 * h;
        let pts = if left {
            vec![pos2(x0 + h, r.top()), pos2(x0, r.center().y), pos2(x0 + h, r.bottom())]
        } else {
            vec![pos2(x0, r.top()), pos2(x0 + h, r.center().y), pos2(x0, r.bottom())]
        };
        p.add(Shape::convex_polygon(pts, c, Stroke::NONE));
    }
}

fn icon_prev(p: &Painter, r: Rect, c: Color32) {
    double_triangle(p, r, c, true)
}

fn icon_next(p: &Painter, r: Rect, c: Color32) {
    double_triangle(p, r, c, false)
}

fn speaker(p: &Painter, r: Rect, c: Color32) -> Rect {
    let r = r.shrink(r.width() * 0.2);
    let cy = r.center().y;
    let h = r.height();
    p.add(Shape::convex_polygon(
        vec![
            pos2(r.left(), cy - h * 0.18),
            pos2(r.left() + h * 0.22, cy - h * 0.18),
            pos2(r.left() + h * 0.48, cy - h * 0.42),
            pos2(r.left() + h * 0.48, cy + h * 0.42),
            pos2(r.left() + h * 0.22, cy + h * 0.18),
            pos2(r.left(), cy + h * 0.18),
        ],
        c,
        Stroke::NONE,
    ));
    r
}

fn arc(p: &Painter, center: Pos2, radius: f32, stroke: Stroke) {
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = -0.85 + 1.7 * i as f32 / 12.0;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    p.add(Shape::line(pts, stroke));
}

fn icon_volume(p: &Painter, r: Rect, c: Color32) {
    let s = speaker(p, r, c);
    let center = pos2(s.left() + s.height() * 0.42, s.center().y);
    arc(p, center, s.height() * 0.32, Stroke::new(1.6_f32, c));
    arc(p, center, s.height() * 0.55, Stroke::new(1.6_f32, c));
}

fn icon_muted(p: &Painter, r: Rect, c: Color32) {
    let s = speaker(p, r, c);
    let x = s.left() + s.height() * 0.65;
    let d = s.height() * 0.18;
    let cy = s.center().y;
    let st = Stroke::new(1.6_f32, c);
    p.line_segment([pos2(x, cy - d), pos2(x + 2.0 * d, cy + d)], st);
    p.line_segment([pos2(x, cy + d), pos2(x + 2.0 * d, cy - d)], st);
}

fn icon_fullscreen(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.26);
    let l = r.width() * 0.35;
    let st = Stroke::new(1.8_f32, c);
    for (corner, dx, dy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        p.line_segment([corner, corner + vec2(dx * l, 0.0)], st);
        p.line_segment([corner, corner + vec2(0.0, dy * l)], st);
    }
}

fn icon_playlist(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.25);
    let st = Stroke::new(1.8_f32, c);
    for i in 0..3 {
        let y = r.top() + r.height() * (0.15 + 0.35 * i as f32);
        p.circle_filled(pos2(r.left() + 1.5, y), 1.6, c);
        p.line_segment([pos2(r.left() + 5.5, y), pos2(r.right(), y)], st);
    }
}

fn icon_settings(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.25);
    let st = Stroke::new(1.6_f32, c);
    for (i, k) in [0.7f32, 0.3, 0.55].iter().enumerate() {
        let y = r.top() + r.height() * (0.15 + 0.35 * i as f32);
        p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], st);
        p.circle_filled(pos2(r.left() + r.width() * k, y), 2.6, c);
    }
}

fn icon_subtitle(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.22);
    let b = Rect::from_min_max(pos2(r.left(), r.top() + r.height() * 0.12), pos2(r.right(), r.bottom() - r.height() * 0.12));
    p.rect_stroke(b, 2.5, Stroke::new(1.5_f32, c), egui::StrokeKind::Inside);
    let st = Stroke::new(1.6_f32, c);
    let y1 = b.bottom() - b.height() * 0.42;
    let y2 = b.bottom() - b.height() * 0.22;
    p.line_segment([pos2(b.left() + 4.0, y1), pos2(b.right() - 7.0, y1)], st);
    p.line_segment([pos2(b.left() + 7.0, y2), pos2(b.right() - 4.0, y2)], st);
}

fn icon_pip(p: &Painter, r: Rect, c: Color32) {
    let r = r.shrink(r.width() * 0.22);
    let b = Rect::from_min_max(pos2(r.left(), r.top() + r.height() * 0.12), pos2(r.right(), r.bottom() - r.height() * 0.12));
    p.rect_stroke(b, 2.0, Stroke::new(1.5_f32, c), egui::StrokeKind::Inside);
    let inner = Rect::from_min_max(b.center() + vec2(1.0, 1.0), b.right_bottom() - vec2(3.0, 3.0));
    p.rect_filled(inner, 1.0, c);
}

fn icon_button(ui: &mut Ui, size: f32, tip: &str, icon: IconFn) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let color = if resp.is_pointer_button_down_on() {
        FG_DIM
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        FG
    };
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, Color32::from_white_alpha(18));
    }
    icon(ui.painter(), rect, color);
    resp.on_hover_text(tip)
}

/// What the pointer is doing over a bar slider.
struct BarHover {
    value: f64,
    /// Point on top of the bar under the pointer (screen coordinates).
    anchor: Pos2,
}

/// IINA-style thin progress bar. Returns `(value, finished)` while the user
/// interacts, plus the hovered value for previews.
fn bar_slider(
    ui: &mut Ui,
    width: f32,
    value: f64,
    max: f64,
    marks: &[f64],
    ranges: &[(f64, f64, Color32)],
) -> (Response, Option<(f64, bool)>, Option<BarHover>) {
    let height = 18.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let active = resp.hovered() || resp.dragged();
    let thick = if active { 5.0 } else { 3.5 };
    let bar = Rect::from_center_size(rect.center(), vec2(rect.width(), thick));
    let frac = |v: f64| if max > 0.0 { (v / max).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let p = ui.painter();
    p.rect_filled(bar, thick / 2.0, TRACK_BG);
    for &(a, b, color) in ranges {
        let (x0, x1) = (bar.left() + bar.width() * frac(a), bar.left() + bar.width() * frac(b));
        if x1 > x0 {
            p.rect_filled(Rect::from_min_max(pos2(x0, bar.top()), pos2(x1, bar.bottom())), thick / 2.0, color);
        }
    }
    p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac(value), thick)), thick / 2.0, FG);
    if max > 0.0 {
        for &m in marks {
            let x = bar.left() + bar.width() * frac(m);
            p.line_segment([pos2(x, bar.top() - 1.0), pos2(x, bar.bottom() + 1.0)], Stroke::new(1.5_f32, Color32::from_black_alpha(160)));
        }
    }
    if active {
        p.circle_filled(pos2(bar.left() + bar.width() * frac(value), bar.center().y), 6.0, Color32::WHITE);
    }
    let pointer_value = resp
        .interact_pointer_pos()
        .or(resp.hover_pos())
        .map(|pos| (((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64) * max);
    let hover = pointer_value.filter(|_| active && max > 0.0).map(|v| BarHover {
        value: v,
        anchor: pos2(bar.left() + bar.width() * frac(v), rect.top()),
    });
    let mut out = None;
    if let Some(v) = pointer_value {
        if resp.dragged() || resp.drag_started() {
            out = Some((v, false));
        }
        if resp.drag_stopped() || resp.clicked() {
            out = Some((v, true));
        }
    }
    (resp, out, hover)
}

/// Floating preview above the seek bar: thumbnail (when ready) + time + chapter.
fn seek_preview(ctx: &egui::Context, st: &mut UiState, pl: &Player, thumbs: Option<&Thumbnails>, hover: &BarHover) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("seek-preview")));
    let screen = ctx.screen_rect();
    let chapter = pl.chapters.iter().rev().find(|c| c.time <= hover.value).map(|c| c.title.clone()).unwrap_or_default();
    let label = if chapter.is_empty() { fmt_time(hover.value) } else { format!("{}  ·  {chapter}", fmt_time(hover.value)) };
    let galley = painter.layout_no_wrap(label, FontId::proportional(12.5), Color32::WHITE);

    // Thumbnail texture, uploaded lazily and cached per file.
    let mut tex: Option<egui::TextureHandle> = None;
    if let Some(t) = thumbs.filter(|_| st.settings.thumbnails) {
        if t.path != st.thumb_path {
            st.thumb_path = t.path.clone();
            st.thumb_tex.clear();
        }
        if let Some(i) = t.nearest(hover.value) {
            tex = Some(
                st.thumb_tex
                    .entry(i)
                    .or_insert_with(|| ctx.load_texture(format!("thumb-{i}"), t.images[i].1.clone(), egui::TextureOptions::LINEAR))
                    .clone(),
            );
        }
    }

    let pad = 4.0;
    let (img_size, w) = match &tex {
        Some(t) => {
            let [tw, th] = t.size();
            let width = (screen.width() / 5.0).clamp(140.0, 260.0);
            let s = vec2(width, width * th as f32 / tw.max(1) as f32);
            (Some(s), s.x.max(galley.size().x) + pad * 2.0)
        }
        None => (None, galley.size().x + 12.0),
    };
    let h = img_size.map_or(0.0, |s| s.y + pad) + galley.size().y + 6.0 + pad;
    let x = (hover.anchor.x - w / 2.0).clamp(screen.left() + 4.0, screen.right() - w - 4.0);
    let frame = Rect::from_min_size(pos2(x, hover.anchor.y - h - 8.0), vec2(w, h));
    painter.rect_filled(frame, 6.0, Color32::from_black_alpha(215));
    painter.rect_stroke(frame, 6.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
    let mut y = frame.top() + pad;
    if let (Some(t), Some(s)) = (&tex, img_size) {
        let r = Rect::from_min_size(pos2(frame.center().x - s.x / 2.0, y), s);
        painter.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        y += s.y + pad;
    }
    painter.galley(pos2(frame.center().x - galley.size().x / 2.0, y + 2.0), galley, Color32::WHITE);
}

// ------------------------------------------------------------- top-level ---

/// Draws the whole overlay. Returns app-level commands (same names as the
/// `#@iina` bindings) to be executed by the window owner.
pub fn draw(ctx: &egui::Context, ui_state: &mut UiState, pl: &Player, mpv: &Mpv, thumbs: Option<&Thumbnails>) -> Vec<String> {
    let mut cmds = Vec::new();
    let now = Instant::now();
    let hide_after = Duration::from_secs_f32(ui_state.settings.osc_hide_secs.max(0.5));

    if pl.idle && !ui_state.pip {
        welcome(ctx, ui_state, &mut cmds);
    } else {
        let recently_active = now.duration_since(ui_state.last_activity) < hide_after;
        let want_osc = pl.pause
            || recently_active
            || ui_state.pointer_over_ui
            || ui_state.music_mode
            || ui_state.seek_preview.is_some()
            || ui_state.context_menu.is_some();
        ui_state.osc_visible = want_osc;
        let alpha = ctx.animate_bool_with_time(egui::Id::new("osc-alpha"), want_osc, 0.18);
        if alpha > 0.0 {
            if ui_state.fullscreen || ui_state.music_mode {
                title_bar(ctx, pl, alpha);
            }
            osc(ctx, ui_state, pl, mpv, thumbs, alpha, &mut cmds);
        }
        if want_osc && !pl.pause {
            ctx.request_repaint_after(hide_after.saturating_sub(now.duration_since(ui_state.last_activity)) + Duration::from_millis(30));
        }
    }

    if let Some(tab) = ui_state.sidebar {
        sidebar(ctx, ui_state, tab, pl, mpv, &mut cmds);
    }
    context_menu(ctx, ui_state, pl, &mut cmds);
    url_dialog(ctx, ui_state, mpv);
    jump_dialog(ctx, ui_state, pl, mpv);
    inspector(ctx, ui_state, pl, mpv);
    preferences(ctx, ui_state);
    about(ctx, ui_state);

    ui_state.pointer_over_ui = ctx.is_pointer_over_area();
    cmds
}

fn title_bar(ctx: &egui::Context, pl: &Player, alpha: f32) {
    let max_w = ctx.screen_rect().width() - 80.0;
    egui::Area::new(egui::Id::new("title"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 10.0))
        .default_width(max_w)
        .interactable(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            egui::Frame::new()
                .fill(HUD_BG)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 6))
                .show(ui, |ui| {
                    ui.set_max_width(max_w);
                    ui.add(egui::Label::new(RichText::new(&pl.title).color(FG).size(14.0)).wrap_mode(egui::TextWrapMode::Truncate));
                });
        });
}

fn osc(ctx: &egui::Context, st: &mut UiState, pl: &Player, mpv: &Mpv, thumbs: Option<&Thumbnails>, alpha: f32, cmds: &mut Vec<String>) {
    let screen = ctx.screen_rect();
    let sidebar_w = if st.sidebar.is_some() { 340.0 } else { 0.0 };
    let avail = (screen.width() - sidebar_w - 24.0).max(200.0);
    let width = avail.min(560.0);
    let compact = st.pip || screen.height() < 160.0;
    let mut hover: Option<BarHover> = None;

    let area = egui::Area::new(egui::Id::new("osc"))
        .anchor(Align2::CENTER_BOTTOM, vec2(-sidebar_w / 2.0, if compact { -6.0 } else { -22.0 }))
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            egui::Frame::new()
                .fill(HUD_BG)
                .corner_radius(12)
                .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(18)))
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    // Row 1: volume | transport | toggles
                    ui.horizontal(|ui| {
                        let side_w = (width - 28.0 - 3.0 * 34.0) / 2.0 - 8.0;
                        ui.allocate_ui_with_layout(vec2(side_w, 30.0), Layout::left_to_right(Align::Center), |ui| {
                            let icon: IconFn = if pl.mute || pl.volume <= 0.0 { icon_muted } else { icon_volume };
                            if icon_button(ui, 26.0, "Mute", icon).clicked() {
                                mpv.command(&["cycle", "mute"]);
                            }
                            if !compact {
                                let (r, v, _) = bar_slider(ui, (side_w - 40.0).clamp(30.0, 90.0), pl.volume, 100.0, &[], &[]);
                                if let Some((v, _)) = v {
                                    mpv.set_double("volume", v.round());
                                }
                                r.on_hover_text(format!("Volume {:.0}", pl.volume));
                            }
                        });
                        if icon_button(ui, 34.0, "Previous", icon_prev).clicked() {
                            if pl.playlist_count > 1 {
                                mpv.command(&["playlist-prev"]);
                            } else {
                                mpv.command(&["seek", "-10"]);
                            }
                        }
                        let (icon, tip): (IconFn, &str) = if pl.pause { (icon_play, "Play") } else { (icon_pause, "Pause") };
                        if icon_button(ui, 34.0, tip, icon).clicked() {
                            mpv.command(&["cycle", "pause"]);
                        }
                        if icon_button(ui, 34.0, "Next", icon_next).clicked() {
                            if pl.playlist_count > 1 {
                                mpv.command(&["playlist-next"]);
                            } else {
                                mpv.command(&["seek", "10"]);
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !st.pip && icon_button(ui, 26.0, "Fullscreen", icon_fullscreen).clicked() {
                                cmds.push("toggle-fullscreen".into());
                            }
                            if icon_button(ui, 26.0, "Picture-in-Picture", icon_pip).clicked() {
                                cmds.push("toggle-pip".into());
                            }
                            if !compact {
                                if icon_button(ui, 26.0, "Playlist", icon_playlist).clicked() {
                                    cmds.push("playlist-panel".into());
                                }
                                if icon_button(ui, 26.0, "Subtitles", icon_subtitle).clicked() {
                                    cmds.push("sub-panel".into());
                                }
                                if icon_button(ui, 26.0, "Video & Audio settings", icon_settings).clicked() {
                                    cmds.push("video-panel".into());
                                }
                                if (pl.speed - 1.0).abs() > 1e-3 {
                                    ui.label(RichText::new(format!("{:.2}x", pl.speed)).size(11.5).color(FG_DIM));
                                }
                            }
                        });
                    });
                    // Row 2: time | seek bar | duration (click to toggle remaining)
                    ui.horizontal(|ui| {
                        let shown = st.seek_preview.unwrap_or(pl.time_pos);
                        let label_w = 58.0;
                        ui.allocate_ui_with_layout(vec2(label_w, 18.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.label(RichText::new(fmt_time(shown)).monospace().size(11.5).color(FG));
                        });
                        let marks: Vec<f64> = pl.chapters.iter().map(|c| c.time).filter(|t| *t > 0.5).collect();
                        let mut ranges = Vec::new();
                        // Buffered range (network streams), like IINA's cache indicator.
                        if pl.cache_time > pl.time_pos {
                            ranges.push((pl.time_pos, pl.cache_time, Color32::from_white_alpha(70)));
                        }
                        if let (Some(a), Some(b)) = (pl.ab_loop_a, pl.ab_loop_b) {
                            ranges.push((a.min(b), a.max(b), Color32::from_rgba_unmultiplied(80, 150, 255, 140)));
                        }
                        let bar_w = ui.available_width() - label_w - 8.0;
                        let (r, v, h) = bar_slider(ui, bar_w, shown, pl.duration, &marks, &ranges);
                        st.seek_bar_rect = Some(r.rect);
                        hover = h;
                        if let Some((v, finished)) = v {
                            if finished {
                                st.seek_preview = None;
                                mpv.command(&["seek", &format!("{v:.3}"), "absolute+exact"]);
                            } else {
                                st.seek_preview = Some(v);
                                mpv.command(&["seek", &format!("{v:.3}"), "absolute+keyframes"]);
                            }
                        }
                        ui.allocate_ui_with_layout(vec2(label_w, 18.0), Layout::right_to_left(Align::Center), |ui| {
                            let text = if st.settings.show_remaining && pl.duration > 0.0 {
                                format!("-{}", fmt_time((pl.duration - shown).max(0.0)))
                            } else {
                                fmt_time(pl.duration)
                            };
                            let r = ui.add(egui::Label::new(RichText::new(text).monospace().size(11.5).color(FG)).sense(Sense::click()));
                            if r.on_hover_text("Click to toggle remaining time").clicked() {
                                st.settings.show_remaining = !st.settings.show_remaining;
                                st.settings_dirty = true;
                            }
                        });
                    });
                });
        });
    if let Some(mut h) = hover {
        // Float the preview above the whole controller, like IINA.
        h.anchor.y = area.response.rect.top();
        seek_preview(ctx, st, pl, thumbs, &h);
    }
}

fn welcome(ctx: &egui::Context, st: &mut UiState, cmds: &mut Vec<String>) {
    egui::Area::new(egui::Id::new("welcome")).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        egui::Frame::new()
            .fill(Color32::from_rgba_premultiplied(30, 30, 32, 240))
            .corner_radius(14)
            .inner_margin(egui::Margin::same(24))
            .show(ui, |ui| {
                ui.set_width(560.0_f32.min(ctx.screen_rect().width() - 40.0));
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(vec2(190.0, 0.0), Layout::top_down_justified(Align::Center), |ui| {
                        ui.add(egui::Image::new(egui::include_image!("../assets/iina-rs.png")).fit_to_exact_size(vec2(110.0, 110.0)));
                        ui.label(RichText::new("IINA-RS").size(26.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(FG_DIM));
                        ui.add_space(10.0);
                        if ui.button("Open File…").clicked() {
                            cmds.push("open-file".into());
                        }
                        if ui.button("Open URL…").clicked() {
                            cmds.push("open-url".into());
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Recent").strong().color(FG));
                        ui.add_space(4.0);
                        if st.recent.is_empty() {
                            ui.label(RichText::new("Drop a video here, or open a file.").color(FG_DIM));
                        }
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for path in st.recent.clone() {
                                let r = ui.add(egui::Button::new(file_name(&path)).frame(false).truncate());
                                if r.clicked() {
                                    cmds.push(format!("open-path {path}"));
                                }
                                r.on_hover_text(&path);
                            }
                        });
                        if !st.recent.is_empty() && ui.small_button("Clear recent").clicked() {
                            cmds.push("clear-recent".into());
                        }
                    });
                });
            });
    });
}


fn track_list(ui: &mut Ui, mpv: &Mpv, tracks: &[&Track], prop: &str, allow_none: bool, selected: impl Fn(&Track) -> bool) {
    let none_selected = !tracks.iter().any(|t| selected(t));
    if allow_none && ui.selectable_label(none_selected, "None").clicked() {
        mpv.set_string(prop, "no");
    }
    for t in tracks {
        let mut label = format!("#{}", t.id);
        if let Some(title) = &t.title {
            label.push_str(&format!("  {title}"));
        }
        if let Some(lang) = &t.lang {
            label.push_str(&format!("  [{lang}]"));
        }
        if let Some(codec) = &t.codec {
            label.push_str(&format!("  {codec}"));
        }
        if let Some(ch) = t.channels {
            label.push_str(&format!("  {ch}ch"));
        }
        if t.external {
            label.push_str("  (external)");
        }
        if ui.selectable_label(selected(t), label).clicked() {
            mpv.set_string(prop, &t.id.to_string());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn labeled_slider(ui: &mut Ui, mpv: &Mpv, label: &str, prop: &str, value: f64, range: std::ops::RangeInclusive<f64>, default: f64, suffix: &str) {
    ui.label(RichText::new(label).color(FG_DIM));
    ui.horizontal(|ui| {
        let mut v = value;
        let r = ui.add(egui::Slider::new(&mut v, range).suffix(suffix).max_decimals(2));
        if r.changed() {
            mpv.set_double(prop, v);
        }
        if ui.small_button("Reset").clicked() {
            mpv.set_double(prop, default);
        }
    });
}

/// IINA's 10-band audio equalizer (ISO octave bands), as one labelled lavfi filter.
pub const EQ_FREQS: [u32; 10] = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const EQ_PRESETS: &[(&str, [f32; 10])] = &[
    ("Flat", [0.0; 10]),
    ("Bass Boost", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Treble Boost", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0]),
    ("Vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    ("Rock", [5.0, 4.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 4.0, 5.0]),
    ("Loudness", [6.0, 4.0, 0.0, 0.0, -2.0, 0.0, -1.0, -5.0, 5.0, 1.0]),
];

pub fn apply_audio_eq(mpv: &Mpv, gains: &[f32; 10]) {
    mpv.command(&["af", "remove", "@iina-eq"]);
    if gains.iter().all(|g| g.abs() < 0.05) {
        return;
    }
    let bands: Vec<String> = EQ_FREQS
        .iter()
        .zip(gains)
        .map(|(f, g)| format!("equalizer=f={f}:t=o:w=1:g={g:.1}"))
        .collect();
    mpv.command(&["af", "add", &format!("@iina-eq:lavfi=[{}]", bands.join(","))]);
}

fn sidebar(ctx: &egui::Context, st: &mut UiState, tab: Tab, pl: &Player, mpv: &Mpv, cmds: &mut Vec<String>) {
    egui::SidePanel::right("sidebar")
        .exact_width(340.0)
        .resizable(false)
        .frame(egui::Frame::new().fill(Color32::from_rgba_premultiplied(26, 26, 28, 245)).inner_margin(egui::Margin::same(12)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tabs: &[(Tab, &str)] = if matches!(tab, Tab::Playlist | Tab::Chapters) {
                    &[(Tab::Playlist, "Playlist"), (Tab::Chapters, "Chapters")]
                } else {
                    &[(Tab::Video, "Video"), (Tab::Audio, "Audio"), (Tab::Subtitles, "Subtitles")]
                };
                for (t, name) in tabs {
                    if ui.selectable_label(tab == *t, RichText::new(*name).size(14.0)).clicked() {
                        st.sidebar = Some(*t);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        st.sidebar = None;
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match tab {
                Tab::Playlist => playlist_tab(ui, pl, mpv, cmds),
                Tab::Chapters => {
                    if pl.chapters.is_empty() {
                        ui.label(RichText::new("No chapters").color(FG_DIM));
                    }
                    for (i, c) in pl.chapters.iter().enumerate() {
                        let text = format!("{}   {}", fmt_time(c.time), if c.title.is_empty() { format!("Chapter {}", i + 1) } else { c.title.clone() });
                        if ui.selectable_label(pl.chapter == i as i64, text).clicked() {
                            mpv.set_string("chapter", &i.to_string());
                        }
                    }
                }
                Tab::Video => video_tab(ui, pl, mpv),
                Tab::Audio => audio_tab(ui, st, pl, mpv, cmds),
                Tab::Subtitles => {
                    ui.label(RichText::new("Subtitle track").strong());
                    let tracks: Vec<&Track> = pl.tracks.iter().filter(|t| t.kind == "sub").collect();
                    let sid = pl.sid.clone();
                    track_list(ui, mpv, &tracks, "sid", true, |t| t.id.to_string() == sid);
                    if ui.button("Load external subtitle…").clicked() {
                        cmds.push("add-sub".into());
                    }
                    ui.separator();
                    ui.label(RichText::new("Secondary subtitle").strong());
                    let sec = pl.secondary_sid.clone();
                    track_list(ui, mpv, &tracks, "secondary-sid", true, |t| t.id.to_string() == sec);
                    ui.separator();
                    let mut vis = pl.sub_visible;
                    if ui.checkbox(&mut vis, "Show subtitles").changed() {
                        mpv.set_flag("sub-visibility", vis);
                    }
                    labeled_slider(ui, mpv, "Subtitle delay", "sub-delay", pl.sub_delay, -10.0..=10.0, 0.0, " s");
                    labeled_slider(ui, mpv, "Text size", "sub-scale", pl.sub_scale, 0.3..=3.0, 1.0, "x");
                    labeled_slider(ui, mpv, "Position", "sub-pos", pl.sub_pos, 0.0..=150.0, 100.0, "");
                    ui.label(RichText::new("Text color").color(FG_DIM));
                    ui.horizontal(|ui| {
                        for (name, c) in [("White", "#FFFFFF"), ("Yellow", "#FFEB3B"), ("Cyan", "#4DD0E1"), ("Green", "#8BC34A")] {
                            if ui.small_button(name).clicked() {
                                mpv.set_string("sub-color", c);
                            }
                        }
                    });
                }
            });
        });
}

fn playlist_tab(ui: &mut Ui, pl: &Player, mpv: &Mpv, cmds: &mut Vec<String>) {
    ui.horizontal(|ui| {
        if ui.button("Add files…").clicked() {
            cmds.push("add-files".into());
        }
        if ui.button("Shuffle").clicked() {
            mpv.command(&["playlist-shuffle"]);
        }
        let mut looping = pl.loop_playlist;
        if ui.checkbox(&mut looping, "Loop").changed() {
            mpv.set_string("loop-playlist", if looping { "inf" } else { "no" });
        }
    });
    ui.add_space(4.0);
    let n = pl.playlist.len();
    for (i, item) in pl.playlist.iter().enumerate() {
        let name = item.title.clone().unwrap_or_else(|| file_name(&item.filename));
        let text = format!("{:>2}. {}", i + 1, name);
        let r = ui.add(egui::Button::selectable(item.current, text).truncate());
        if r.clicked() {
            mpv.set_string("playlist-pos", &i.to_string());
        }
        r.context_menu(|ui| {
            if ui.button("Play").clicked() {
                mpv.set_string("playlist-pos", &i.to_string());
                ui.close();
            }
            if i > 0 && ui.button("Move Up").clicked() {
                mpv.command(&["playlist-move", &i.to_string(), &(i - 1).to_string()]);
                ui.close();
            }
            if i + 1 < n && ui.button("Move Down").clicked() {
                mpv.command(&["playlist-move", &i.to_string(), &(i + 2).to_string()]);
                ui.close();
            }
            if ui.button("Remove").clicked() {
                mpv.command(&["playlist-remove", &i.to_string()]);
                ui.close();
            }
            if ui.button("Show in Folder").clicked() {
                cmds.push(format!("reveal {}", item.filename));
                ui.close();
            }
        });
    }
}

fn video_tab(ui: &mut Ui, pl: &Player, mpv: &Mpv) {
    ui.label(RichText::new("Video track").strong());
    let tracks: Vec<&Track> = pl.tracks.iter().filter(|t| t.kind == "video").collect();
    let vid = pl.vid.clone();
    track_list(ui, mpv, &tracks, "vid", true, |t| t.id.to_string() == vid);
    ui.separator();
    labeled_slider(ui, mpv, "Speed", "speed", pl.speed, 0.25..=4.0, 1.0, "x");
    ui.horizontal(|ui| {
        for s in [0.5, 1.0, 1.25, 1.5, 2.0] {
            if ui.small_button(format!("{s}x")).clicked() {
                mpv.set_double("speed", s);
            }
        }
    });
    ui.separator();
    ui.label(RichText::new("Aspect ratio").color(FG_DIM));
    ui.horizontal_wrapped(|ui| {
        for (label, val) in [("Default", "no"), ("4:3", "4:3"), ("16:9", "16:9"), ("16:10", "16:10"), ("21:9", "2.33"), ("1:1", "1:1"), ("5:4", "5:4")] {
            let sel = pl.aspect == val || (val == "no" && (pl.aspect.is_empty() || pl.aspect.starts_with('-')));
            if ui.selectable_label(sel, label).clicked() {
                mpv.set_string("video-aspect-override", val);
            }
        }
    });
    ui.label(RichText::new("Rotation").color(FG_DIM));
    ui.horizontal(|ui| {
        for deg in [0, 90, 180, 270] {
            if ui.selectable_label(pl.rotate == deg, format!("{deg}°")).clicked() {
                mpv.set_string("video-rotate", &deg.to_string());
            }
        }
    });
    ui.horizontal(|ui| {
        let mut flip = pl.vf.contains("@iina-flip");
        if ui.checkbox(&mut flip, "Flip").changed() {
            mpv.command(&["vf", "toggle", "@iina-flip:vflip"]);
        }
        let mut mirror = pl.vf.contains("@iina-mirror");
        if ui.checkbox(&mut mirror, "Mirror").changed() {
            mpv.command(&["vf", "toggle", "@iina-mirror:hflip"]);
        }
        let mut deint = pl.deinterlace;
        if ui.checkbox(&mut deint, "Deinterlace").changed() {
            mpv.set_string("deinterlace", if deint { "yes" } else { "no" });
        }
    });
    ui.separator();
    ui.label(RichText::new("Video equalizer").strong());
    for (label, prop, v) in [
        ("Brightness", "brightness", pl.brightness),
        ("Contrast", "contrast", pl.contrast),
        ("Saturation", "saturation", pl.saturation),
        ("Gamma", "gamma", pl.gamma),
        ("Hue", "hue", pl.hue),
    ] {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(70.0, 18.0), egui::Label::new(RichText::new(label).color(FG_DIM)));
            let mut x = v as f64;
            if ui.add(egui::Slider::new(&mut x, -100.0..=100.0).integer()).changed() {
                mpv.set_string(prop, &format!("{}", x.round() as i64));
            }
        });
    }
    if ui.small_button("Reset equalizer").clicked() {
        for p in ["brightness", "contrast", "saturation", "gamma", "hue"] {
            mpv.set_string(p, "0");
        }
    }
    ui.separator();
    let mut hw = pl.hwdec_enabled;
    if ui.checkbox(&mut hw, "Hardware decoding").changed() {
        mpv.set_string("hwdec", if hw { "auto-safe" } else { "no" });
    }
    ui.label(RichText::new(format!("Decoder: {}", if pl.hwdec_current.is_empty() { "software" } else { &pl.hwdec_current })).color(FG_DIM).size(11.0));
    if ui.button("Take screenshot").clicked() {
        mpv.command(&["screenshot"]);
    }
}

fn audio_tab(ui: &mut Ui, st: &mut UiState, pl: &Player, mpv: &Mpv, cmds: &mut Vec<String>) {
    ui.label(RichText::new("Audio track").strong());
    let tracks: Vec<&Track> = pl.tracks.iter().filter(|t| t.kind == "audio").collect();
    let aid = pl.aid.clone();
    track_list(ui, mpv, &tracks, "aid", true, |t| t.id.to_string() == aid);
    if ui.button("Load external audio…").clicked() {
        cmds.push("add-audio".into());
    }
    ui.separator();
    labeled_slider(ui, mpv, "Volume", "volume", pl.volume, 0.0..=130.0, 100.0, "");
    labeled_slider(ui, mpv, "Audio delay", "audio-delay", pl.audio_delay, -5.0..=5.0, 0.0, " s");
    ui.separator();
    ui.label(RichText::new("Equalizer").strong());
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (name, gains) in EQ_PRESETS {
            if ui.small_button(*name).clicked() {
                st.eq = *gains;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        for (i, f) in EQ_FREQS.iter().enumerate() {
            ui.vertical(|ui| {
                ui.spacing_mut().slider_width = 110.0;
                if ui.add(egui::Slider::new(&mut st.eq[i], -12.0..=12.0).vertical().show_value(false)).changed() {
                    changed = true;
                }
                let label = if *f >= 1000 { format!("{}k", f / 1000) } else { f.to_string() };
                ui.label(RichText::new(label).size(9.5).color(FG_DIM));
            });
        }
    });
    if changed {
        apply_audio_eq(mpv, &st.eq);
    }
}

fn context_menu(ctx: &egui::Context, st: &mut UiState, pl: &Player, cmds: &mut Vec<String>) {
    let Some(pos) = st.context_menu else { return };
    let resp = egui::Area::new(egui::Id::new("ctx-menu"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(210.0);
                let mut item = |ui: &mut Ui, label: &str, shortcut: &str, cmd: &str| {
                    let b = egui::Button::new(label).frame(false).shortcut_text(shortcut);
                    if ui.add(b).clicked() {
                        cmds.push(cmd.to_string());
                    }
                };
                item(ui, "Open File…", "Ctrl+O", "open-file");
                item(ui, "Open URL…", "Ctrl+U", "open-url");
                ui.separator();
                if !pl.idle {
                    item(ui, if pl.pause { "Play" } else { "Pause" }, "Space", "mpv cycle pause");
                    item(ui, "Jump to…", "Ctrl+J", "jump-to");
                    item(ui, "Take Screenshot", "Ctrl+S", "mpv screenshot");
                    item(ui, "Stop", "Ctrl+W", "close-file");
                    ui.separator();
                }
                item(ui, "Playlist", "Ctrl+Shift+P", "playlist-panel");
                item(ui, "Chapters", "Ctrl+Shift+C", "chapter-panel");
                item(ui, "Video Settings", "Ctrl+Shift+V", "video-panel");
                item(ui, "Audio Settings", "Ctrl+Shift+A", "audio-panel");
                item(ui, "Subtitle Settings", "Ctrl+Shift+S", "sub-panel");
                ui.separator();
                item(ui, if st.fullscreen { "Exit Fullscreen" } else { "Enter Fullscreen" }, "F", "toggle-fullscreen");
                item(ui, if pl.ontop { "✓ Float on Top" } else { "Float on Top" }, "Ctrl+Alt+T", "mpv cycle ontop");
                item(ui, if st.pip { "✓ Picture-in-Picture" } else { "Picture-in-Picture" }, "Ctrl+Alt+P", "toggle-pip");
                item(ui, if st.music_mode { "✓ Music Mode" } else { "Music Mode" }, "Ctrl+Alt+M", "toggle-music-mode");
                if !pl.path.is_empty() && !pl.path.contains("://") {
                    item(ui, "Show in Folder", "Ctrl+Shift+R", "show-current-file-in-finder");
                }
                ui.separator();
                item(ui, "Inspector", "Ctrl+I", "inspector");
                item(ui, "Preferences…", "Ctrl+,", "preferences");
                item(ui, "About IINA-RS", "", "about");
                item(ui, "Quit", "Ctrl+Q", "mpv quit");
            });
        })
        .response;
    let clicked_elsewhere = ctx.input(|i| i.pointer.any_pressed()) && !resp.contains_pointer() && !ctx.input(|i| i.pointer.secondary_pressed());
    if !cmds.is_empty() || clicked_elsewhere || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.context_menu = None;
    }
}

fn url_dialog(ctx: &egui::Context, st: &mut UiState, mpv: &Mpv) {
    let Some(url) = st.url_input.as_mut() else { return };
    let mut close = false;
    egui::Window::new("Open URL")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label("Enter a media URL (http, https, YouTube via yt-dlp, …):");
            let r = ui.add(egui::TextEdit::singleline(url).desired_width(380.0).hint_text("https://"));
            r.request_focus();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if (ui.button("Open").clicked() || enter) && !url.trim().is_empty() {
                    mpv.command(&["loadfile", url.trim(), "replace"]);
                    close = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
    if close {
        st.url_input = None;
    }
}

/// Parses "1:02:03", "62:03", "3723", "1:02:03.5" or "50%" into seconds.
pub fn parse_time(s: &str, duration: f64) -> Option<f64> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().map(|p| duration * p.clamp(0.0, 100.0) / 100.0);
    }
    let mut total = 0.0;
    for part in s.split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(total)
}

fn jump_dialog(ctx: &egui::Context, st: &mut UiState, pl: &Player, mpv: &Mpv) {
    let Some(text) = st.jump_input.as_mut() else { return };
    let mut close = false;
    egui::Window::new("Jump to")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(format!("Time (hh:mm:ss) or percentage — duration {}", fmt_time(pl.duration)));
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(240.0).hint_text("1:23:45  or  50%"));
            r.request_focus();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let parsed = parse_time(text, pl.duration);
            ui.horizontal(|ui| {
                if (ui.add_enabled(parsed.is_some(), egui::Button::new("Jump")).clicked() || enter) && parsed.is_some() {
                    mpv.command(&["seek", &format!("{:.3}", parsed.unwrap()), "absolute+exact"]);
                    close = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
    if close {
        st.jump_input = None;
    }
}

/// IINA's Inspector window: file, track and live playback information.
fn inspector(ctx: &egui::Context, st: &mut UiState, pl: &Player, mpv: &Mpv) {
    if !st.show_inspector {
        return;
    }
    let mut open = true;
    let g = |p: &str| mpv.get_string(p).filter(|s| !s.is_empty()).unwrap_or_else(|| "—".into());
    egui::Window::new("Inspector")
        .open(&mut open)
        .default_width(380.0)
        .default_pos(pos2(40.0, 40.0))
        .show(ctx, |ui| {
            let section = |ui: &mut Ui, title: &str, rows: &[(&str, String)]| {
                ui.label(RichText::new(title).strong().color(FG));
                egui::Grid::new(title).num_columns(2).striped(true).spacing(vec2(12.0, 3.0)).show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(RichText::new(*k).color(FG_DIM));
                        ui.add(egui::Label::new(v).wrap());
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            };
            let mb = |s: String| s.parse::<f64>().map(|b| format!("{:.2} MB", b / 1_048_576.0)).unwrap_or(s);
            let kbps = |s: String| s.parse::<f64>().map(|b| format!("{:.0} kbps", b / 1000.0)).unwrap_or(s);
            if pl.idle {
                ui.label(RichText::new("Nothing is playing.").color(FG_DIM));
                return;
            }
            section(ui, "General", &[
                ("Path", pl.path.clone()),
                ("Format", g("file-format")),
                ("Size", mb(g("file-size"))),
                ("Duration", fmt_time(pl.duration)),
                ("Chapters", pl.chapters.len().to_string()),
            ]);
            section(ui, "Video", &[
                ("Codec", g("video-codec")),
                ("Resolution", format!("{} × {}", g("video-params/w"), g("video-params/h"))),
                ("Display", format!("{} × {}", g("dwidth"), g("dheight"))),
                ("Pixel format", g("video-params/pixelformat")),
                ("Colors", format!("{} / {} / {}", g("video-params/colormatrix"), g("video-params/primaries"), g("video-params/gamma"))),
                ("Frame rate", format!("{} fps (container {})", g("estimated-vf-fps"), g("container-fps"))),
                ("Bitrate", kbps(g("video-bitrate"))),
                ("Hardware decoder", if pl.hwdec_current.is_empty() { "no".into() } else { pl.hwdec_current.clone() }),
            ]);
            section(ui, "Audio", &[
                ("Codec", g("audio-codec")),
                ("Sample rate", format!("{} Hz", g("audio-params/samplerate"))),
                ("Channels", g("audio-params/channel-count")),
                ("Bitrate", kbps(g("audio-bitrate"))),
            ]);
            section(ui, "Playback", &[
                ("Position", format!("{} / {}", fmt_time(pl.time_pos), fmt_time(pl.duration))),
                ("Dropped (VO / decoder)", format!("{} / {}", g("frame-drop-count"), g("decoder-frame-drop-count"))),
                ("A/V sync", g("avsync")),
                ("Cache", format!("{} s", g("demuxer-cache-duration"))),
            ]);
            ctx.request_repaint_after(Duration::from_millis(500));
        });
    st.show_inspector = open;
}

fn preferences(ctx: &egui::Context, st: &mut UiState) {
    if !st.show_prefs {
        return;
    }
    let mut open = true;
    let before = st.settings.clone();
    egui::Window::new("Preferences")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let s = &mut st.settings;
            ui.label(RichText::new("General").strong());
            ui.checkbox(&mut s.resume_playback, "Resume last playback position");
            ui.checkbox(&mut s.auto_add_folder, "Add files in the same folder to the playlist");
            ui.checkbox(&mut s.resize_to_video, "Resize window to video size when opening a file");
            ui.add_space(6.0);
            ui.label(RichText::new("UI").strong());
            ui.checkbox(&mut s.thumbnails, "Show thumbnail preview on the seek bar");
            ui.checkbox(&mut s.show_remaining, "Show remaining time");
            ui.checkbox(&mut s.click_to_pause, "Single click on video toggles pause");
            ui.horizontal(|ui| {
                ui.label("Hide controller after");
                ui.add(egui::Slider::new(&mut s.osc_hide_secs, 1.0..=10.0).suffix(" s").max_decimals(1));
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Video / Audio").strong());
            ui.checkbox(&mut s.hardware_decoding, "Enable hardware decoding");
            ui.horizontal(|ui| {
                ui.label("Initial volume");
                ui.add(egui::Slider::new(&mut s.volume, 0.0..=100.0).integer());
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Screenshots").strong());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.screenshot_dir).desired_width(260.0));
                if ui.button("Choose…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        s.screenshot_dir = d.to_string_lossy().into_owned();
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Custom key bindings: input.conf, mpv options: mpv.conf in the settings folder.").size(11.0).color(FG_DIM));
        });
    if st.settings != before {
        st.settings_dirty = true;
    }
    st.show_prefs = open;
}

fn about(ctx: &egui::Context, st: &mut UiState) {
    let mut open = st.show_about;
    egui::Window::new("About IINA-RS")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new(egui::include_image!("../assets/iina-rs.png")).fit_to_exact_size(vec2(80.0, 80.0)));
                ui.heading("IINA-RS");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("A Windows & Linux media player in the spirit of IINA,");
                ui.label("written in Rust and powered by mpv (mpv.io).");
                ui.add_space(6.0);
                ui.hyperlink_to("IINA (GPLv3)", "https://github.com/iina/iina");
                ui.hyperlink_to("mpv", "https://mpv.io");
            });
        });
    st.show_about = open;
}
