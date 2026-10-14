//! egui overlay that mimics IINA's floating on-screen controller, sidebar
//! (playlist / chapters / quick settings) and welcome window.

use crate::mpv::Mpv;
use crate::state::{Player, Track};
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
}

impl UiState {
    pub fn new(recent: Vec<String>) -> Self {
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
        }
    }

    pub fn toggle_tab(&mut self, tab: Tab) {
        self.sidebar = if self.sidebar == Some(tab) { None } else { Some(tab) };
    }
}

const OSC_HIDE_AFTER: Duration = Duration::from_millis(2500);
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

/// IINA-style thin progress bar. Returns `(value, finished)` while the user interacts.
fn bar_slider(
    ui: &mut Ui,
    width: f32,
    value: f64,
    max: f64,
    marks: &[f64],
    hover_label: Option<&dyn Fn(f64) -> String>,
) -> (Response, Option<(f64, bool)>) {
    let height = 18.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let active = resp.hovered() || resp.dragged();
    let thick = if active { 5.0 } else { 3.5 };
    let bar = Rect::from_center_size(rect.center(), vec2(rect.width(), thick));
    let frac = if max > 0.0 { (value / max).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let p = ui.painter();
    p.rect_filled(bar, thick / 2.0, TRACK_BG);
    p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, thick)), thick / 2.0, FG);
    if max > 0.0 {
        for &m in marks {
            let x = bar.left() + bar.width() * (m / max).clamp(0.0, 1.0) as f32;
            p.line_segment([pos2(x, bar.top() - 1.0), pos2(x, bar.bottom() + 1.0)], Stroke::new(1.5_f32, Color32::from_black_alpha(160)));
        }
    }
    if active {
        p.circle_filled(pos2(bar.left() + bar.width() * frac, bar.center().y), 6.0, Color32::WHITE);
    }
    let pointer_value = resp
        .interact_pointer_pos()
        .or(resp.hover_pos())
        .map(|pos| (((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64) * max);
    if let (Some(v), Some(label)) = (pointer_value, hover_label) {
        if resp.hovered() || resp.dragged() {
            let x = bar.left() + bar.width() * (v / max.max(1e-9)) as f32;
            let text = label(v);
            let galley = p.layout_no_wrap(text, FontId::proportional(12.0), Color32::WHITE);
            let tr = Rect::from_center_size(pos2(x, rect.top() - 14.0), galley.size() + vec2(10.0, 4.0));
            p.rect_filled(tr, 4.0, Color32::from_black_alpha(200));
            p.galley(tr.min + vec2(5.0, 2.0), galley, Color32::WHITE);
        }
    }
    let mut out = None;
    if let Some(v) = pointer_value {
        if resp.dragged() || resp.drag_started() {
            out = Some((v, false));
        }
        if resp.drag_stopped() || resp.clicked() {
            out = Some((v, true));
        }
    }
    (resp, out)
}

// ------------------------------------------------------------- top-level ---

/// Draws the whole overlay. Returns app-level commands (same names as the
/// `#@iina` bindings) to be executed by the window owner.
pub fn draw(ctx: &egui::Context, ui_state: &mut UiState, pl: &Player, mpv: &Mpv) -> Vec<String> {
    let mut cmds = Vec::new();
    let now = Instant::now();

    if pl.idle && !ui_state.pip {
        welcome(ctx, ui_state, &mut cmds);
    } else {
        let recently_active = now.duration_since(ui_state.last_activity) < OSC_HIDE_AFTER;
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
            osc(ctx, ui_state, pl, mpv, alpha, &mut cmds);
        }
        if want_osc && !pl.pause {
            ctx.request_repaint_after(OSC_HIDE_AFTER.saturating_sub(now.duration_since(ui_state.last_activity)) + Duration::from_millis(30));
        }
    }

    if let Some(tab) = ui_state.sidebar {
        sidebar(ctx, ui_state, tab, pl, mpv, &mut cmds);
    }
    context_menu(ctx, ui_state, pl, &mut cmds);
    url_dialog(ctx, ui_state, mpv);
    about(ctx, ui_state);

    ui_state.pointer_over_ui = ctx.is_pointer_over_area();
    cmds
}

fn title_bar(ctx: &egui::Context, pl: &Player, alpha: f32) {
    egui::Area::new(egui::Id::new("title"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            egui::Frame::new()
                .fill(HUD_BG)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 6))
                .show(ui, |ui| {
                    ui.label(RichText::new(&pl.title).color(FG).size(14.0));
                });
        });
}

fn osc(ctx: &egui::Context, st: &mut UiState, pl: &Player, mpv: &Mpv, alpha: f32, cmds: &mut Vec<String>) {
    let screen = ctx.screen_rect();
    let sidebar_w = if st.sidebar.is_some() { 340.0 } else { 0.0 };
    let avail = (screen.width() - sidebar_w - 24.0).max(200.0);
    let width = avail.min(560.0);
    let compact = st.pip || screen.height() < 160.0;

    egui::Area::new(egui::Id::new("osc"))
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
                                let (_, v) = bar_slider(ui, (side_w - 40.0).clamp(30.0, 90.0), pl.volume, 100.0, &[], None);
                                if let Some((v, _)) = v {
                                    mpv.set_double("volume", v.round());
                                }
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
                            }
                        });
                    });
                    // Row 2: time | seek bar | duration
                    ui.horizontal(|ui| {
                        let shown = st.seek_preview.unwrap_or(pl.time_pos);
                        let label_w = 54.0;
                        ui.allocate_ui_with_layout(vec2(label_w, 18.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.label(RichText::new(fmt_time(shown)).monospace().size(11.5).color(FG));
                        });
                        let marks: Vec<f64> = pl.chapters.iter().map(|c| c.time).filter(|t| *t > 0.5).collect();
                        let chapters = pl.chapters.clone();
                        let label = move |v: f64| {
                            let ch = chapters.iter().rev().find(|c| c.time <= v).map(|c| c.title.clone()).unwrap_or_default();
                            if ch.is_empty() { fmt_time(v) } else { format!("{}  ·  {ch}", fmt_time(v)) }
                        };
                        let bar_w = ui.available_width() - label_w - 8.0;
                        let (_, v) = bar_slider(ui, bar_w, shown, pl.duration, &marks, Some(&label));
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
                            ui.label(RichText::new(fmt_time(pl.duration)).monospace().size(11.5).color(FG));
                        });
                    });
                });
        });
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

fn track_list(ui: &mut Ui, mpv: &Mpv, tracks: &[&Track], prop: &str, allow_none: bool) {
    let none_selected = !tracks.iter().any(|t| t.selected);
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
        if t.external {
            label.push_str("  (external)");
        }
        if ui.selectable_label(t.selected, label).clicked() {
            mpv.set_string(prop, &t.id.to_string());
        }
    }
}

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
                Tab::Playlist => {
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
                    for (i, item) in pl.playlist.iter().enumerate() {
                        let name = item.title.clone().unwrap_or_else(|| file_name(&item.filename));
                        let text = format!("{:>2}. {}", i + 1, name);
                        let r = ui.selectable_label(item.current, text);
                        if r.double_clicked() || r.clicked() {
                            mpv.set_string("playlist-pos", &i.to_string());
                        }
                        r.context_menu(|ui| {
                            if ui.button("Play").clicked() {
                                mpv.set_string("playlist-pos", &i.to_string());
                                ui.close();
                            }
                            if ui.button("Remove").clicked() {
                                mpv.command(&["playlist-remove", &i.to_string()]);
                                ui.close();
                            }
                            if ui.button("Show in folder").clicked() {
                                cmds.push(format!("reveal {}", item.filename));
                                ui.close();
                            }
                        });
                    }
                }
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
                Tab::Video => {
                    ui.label(RichText::new("Video track").strong());
                    let tracks: Vec<&Track> = pl.tracks.iter().filter(|t| t.kind == "video").collect();
                    track_list(ui, mpv, &tracks, "vid", true);
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
                    ui.separator();
                    let mut deint = pl.deinterlace;
                    if ui.checkbox(&mut deint, "Deinterlace").changed() {
                        mpv.set_string("deinterlace", if deint { "yes" } else { "no" });
                    }
                    let mut hw = pl.hwdec_enabled;
                    if ui.checkbox(&mut hw, "Hardware decoding").changed() {
                        mpv.set_string("hwdec", if hw { "auto-safe" } else { "no" });
                    }
                    ui.label(RichText::new(format!("Decoder: {}", if pl.hwdec_current.is_empty() { "software" } else { &pl.hwdec_current })).color(FG_DIM).size(11.0));
                    ui.separator();
                    if ui.button("Take screenshot").clicked() {
                        mpv.command(&["screenshot"]);
                    }
                }
                Tab::Audio => {
                    ui.label(RichText::new("Audio track").strong());
                    let tracks: Vec<&Track> = pl.tracks.iter().filter(|t| t.kind == "audio").collect();
                    track_list(ui, mpv, &tracks, "aid", true);
                    if ui.button("Load external audio…").clicked() {
                        cmds.push("add-audio".into());
                    }
                    ui.separator();
                    labeled_slider(ui, mpv, "Volume", "volume", pl.volume, 0.0..=130.0, 100.0, "");
                    labeled_slider(ui, mpv, "Audio delay", "audio-delay", pl.audio_delay, -5.0..=5.0, 0.0, " s");
                }
                Tab::Subtitles => {
                    ui.label(RichText::new("Subtitle track").strong());
                    let tracks: Vec<&Track> = pl.tracks.iter().filter(|t| t.kind == "sub").collect();
                    track_list(ui, mpv, &tracks, "sid", true);
                    if ui.button("Load external subtitle…").clicked() {
                        cmds.push("add-sub".into());
                    }
                    ui.separator();
                    let mut vis = pl.sub_visible;
                    if ui.checkbox(&mut vis, "Show subtitles").changed() {
                        mpv.set_flag("sub-visibility", vis);
                    }
                    labeled_slider(ui, mpv, "Subtitle delay", "sub-delay", pl.sub_delay, -10.0..=10.0, 0.0, " s");
                    labeled_slider(ui, mpv, "Text size", "sub-scale", pl.sub_scale, 0.3..=3.0, 1.0, "x");
                    labeled_slider(ui, mpv, "Position", "sub-pos", pl.sub_pos, 0.0..=150.0, 100.0, "");
                }
            });
        });
}

fn context_menu(ctx: &egui::Context, st: &mut UiState, pl: &Player, cmds: &mut Vec<String>) {
    let Some(pos) = st.context_menu else { return };
    let resp = egui::Area::new(egui::Id::new("ctx-menu"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(200.0);
                let mut item = |ui: &mut Ui, label: &str, cmd: &str| {
                    if ui.add(egui::Button::new(label).frame(false)).clicked() {
                        cmds.push(cmd.to_string());
                    }
                };
                item(ui, "Open File…", "open-file");
                item(ui, "Open URL…", "open-url");
                ui.separator();
                if !pl.idle {
                    item(ui, if pl.pause { "Play" } else { "Pause" }, "mpv cycle pause");
                    item(ui, "Stop", "close-file");
                    item(ui, "Take Screenshot", "mpv screenshot");
                    ui.separator();
                }
                item(ui, "Playlist", "playlist-panel");
                item(ui, "Chapters", "chapter-panel");
                item(ui, "Video Settings", "video-panel");
                item(ui, "Audio Settings", "audio-panel");
                item(ui, "Subtitle Settings", "sub-panel");
                ui.separator();
                item(ui, if st.fullscreen { "Exit Fullscreen" } else { "Enter Fullscreen" }, "toggle-fullscreen");
                item(ui, if pl.ontop { "✓ Float on Top" } else { "Float on Top" }, "mpv cycle ontop");
                item(ui, if st.pip { "✓ Picture-in-Picture" } else { "Picture-in-Picture" }, "toggle-pip");
                item(ui, if st.music_mode { "✓ Music Mode" } else { "Music Mode" }, "toggle-music-mode");
                if !pl.path.is_empty() && !pl.path.contains("://") {
                    item(ui, "Show in Folder", "show-current-file-in-finder");
                }
                ui.separator();
                item(ui, "About IINA-RS", "about");
                item(ui, "Quit", "mpv quit");
            });
        })
        .response;
    let clicked_elsewhere = ctx.input(|i| i.pointer.any_pressed()) && !resp.contains_pointer() && !resp.ctx.input(|i| i.pointer.secondary_pressed());
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
