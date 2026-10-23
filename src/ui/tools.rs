//! Secondary windows: key bindings editor, filters, mpv log viewer, the
//! screenshot preview and the update notice.

use super::*;
use crate::keys::KeyRow;
use crate::settings::SavedFilter;
use crate::update::Status;

pub struct LogLine {
    pub level: String,
    pub prefix: String,
    pub text: String,
}

pub const LOG_LIMIT: usize = 5000;

pub struct KeyEditor {
    pub rows: Vec<KeyRow>,
    pub filter: String,
    /// Row waiting for a key press (the next key goes into it).
    pub capture: Option<usize>,
    pub dirty: bool,
}

pub struct ShotPreview {
    pub path: std::path::PathBuf,
    pub tex: egui::TextureHandle,
    pub copied: bool,
    pub at: Instant,
}

fn tool_window(ctx: &egui::Context, title: &str, open: &mut bool, size: egui::Vec2, add: impl FnOnce(&mut Ui)) {
    let mut header_close = false;
    egui::Window::new(t(title))
        .title_bar(!is_rtl())
        .id(egui::Id::new(format!("win-{title}")))
        .open(open)
        .default_size(size)
        .show(ctx, |ui| {
            rtl_window(ui, size.x, |ui| {
                if rtl_header(ui, &t(title)) {
                    header_close = true;
                }
                add(ui)
            })
        });
    if header_close {
        *open = false;
    }
}

// ------------------------------------------------------------ key bindings

pub fn key_bindings(ctx: &egui::Context, st: &mut UiState, cmds: &mut Vec<String>) {
    let Some(mut ed) = st.key_editor.take() else { return };
    let mut open = true;
    let screen_h = ctx.screen_rect().height();
    tool_window(ctx, "Key Bindings", &mut open, vec2(620.0, 480.0), |ui| {
        row(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut ed.filter).hint_text(t("Search…")).desired_width(220.0));
            if ui.button(t("Add")).clicked() {
                ed.rows.insert(0, KeyRow { key: String::new(), command: String::new(), app: false });
                ed.capture = Some(0);
                ed.filter.clear();
                ed.dirty = true;
            }
            if ui.button(t("Restore defaults")).clicked() {
                ed.rows = crate::keys::Bindings::load(None).rows();
                ed.capture = None;
                ed.dirty = true;
            }
            if ui.add_enabled(ed.dirty, egui::Button::new(t("Save"))).clicked() {
                cmds.push("keybindings-save".into());
            }
        });
        ui.label(RichText::new(t("Click a key to change it, then press the new key. Commands use mpv's input.conf syntax; ✓ app marks IINA-RS commands.")).size(11.0).color(FG_DIM));
        ui.separator();
        let filter = ed.filter.to_lowercase();
        let mut remove = None;
        let max_h = (screen_h - 260.0).max(160.0);
        scroll_v(ui, "keys", Some(max_h), false, |ui| {
            for (i, r) in ed.rows.iter_mut().enumerate() {
                if !filter.is_empty() && !r.key.to_lowercase().contains(&filter) && !r.command.to_lowercase().contains(&filter) {
                    continue;
                }
                row(ui, |ui| {
                    let capturing = ed.capture == Some(i);
                    let label = if capturing { t("Press a key…") } else if r.key.is_empty() { t("(none)") } else { crate::ui::shortcut_label(&r.key) };
                    let b = egui::Button::new(RichText::new(label).monospace()).selected(capturing).min_size(vec2(130.0, 0.0));
                    if ui.add(b).clicked() {
                        ed.capture = if capturing { None } else { Some(i) };
                    }
                    let w = (ui.available_width() - 110.0).max(120.0);
                    if ui.add(egui::TextEdit::singleline(&mut r.command).desired_width(w).code_editor()).changed() {
                        ed.dirty = true;
                    }
                    if ui.checkbox(&mut r.app, t("app")).changed() {
                        ed.dirty = true;
                    }
                    if ui.small_button("✕").on_hover_text(t("Remove")).clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some(i) = remove {
            ed.rows.remove(i);
            ed.capture = None;
            ed.dirty = true;
        }
    });
    if open {
        st.key_editor = Some(ed);
    }
}

/// Puts a pressed key into the row being captured. Returns true if used.
pub fn capture_key(st: &mut UiState, spec: &str) -> bool {
    let Some(ed) = st.key_editor.as_mut() else { return false };
    let Some(i) = ed.capture.take() else { return false };
    if spec == "ESC" {
        return true;
    }
    if let Some(r) = ed.rows.get_mut(i) {
        r.key = spec.to_string();
        // A key can only have one binding: drop other rows using it.
        let key = spec.to_string();
        let mut idx = 0;
        ed.rows.retain(|r| {
            let keep = idx == i || r.key != key;
            idx += 1;
            keep
        });
        ed.dirty = true;
    }
    true
}

// ------------------------------------------------------------------ filters

/// Splits mpv's `vf`/`af` string at top-level commas.
pub fn split_filters(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut quote) = (Vec::new(), String::new(), 0i32, false);
    for c in s.chars() {
        match c {
            '"' => quote = !quote,
            '[' | '(' if !quote => depth += 1,
            ']' | ')' if !quote => depth -= 1,
            ',' if !quote && depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

pub fn filters(ctx: &egui::Context, st: &mut UiState, pl: &Player, mpv: &Mpv) {
    if !st.show_filters {
        return;
    }
    let mut open = true;
    let mut saved = st.settings.saved_filters.clone();
    let (mut kind, mut name, mut value) = (st.filter_kind.clone(), std::mem::take(&mut st.filter_name), std::mem::take(&mut st.filter_value));
    tool_window(ctx, "Filters", &mut open, vec2(560.0, 440.0), |ui| {
        for (k, title, current) in [("vf", "Video filters", &pl.vf), ("af", "Audio filters", &pl.af)] {
            ui.label(RichText::new(t(title)).strong());
            let list = split_filters(current);
            if list.is_empty() {
                ui.label(RichText::new(t("None")).color(FG_DIM));
            }
            for f in list {
                row(ui, |ui| {
                    if ui.small_button("✕").on_hover_text(t("Remove")).clicked() {
                        // Labelled filters are removed by label.
                        let target = f.split_once(':').filter(|(l, _)| l.starts_with('@')).map(|(l, _)| l.to_string()).unwrap_or_else(|| f.clone());
                        mpv.command(&[k, "remove", &target]);
                    }
                    ui.add(egui::Label::new(RichText::new(&f).monospace().size(12.0)).truncate());
                });
            }
            ui.add_space(4.0);
        }
        ui.separator();
        ui.label(RichText::new(t("Saved filters")).strong());
        let mut delete = None;
        for (i, f) in saved.iter().enumerate() {
            let list = if f.kind == "af" { &pl.af } else { &pl.vf };
            let label = f.label();
            let mut on = list.contains(&format!("@{label}:"));
            row(ui, |ui| {
                if check(ui, &mut on, v(&f.name)).changed() {
                    if on {
                        mpv.command(&[&f.kind, "add", &format!("@{label}:{}", f.value)]);
                    } else {
                        mpv.command(&[&f.kind, "remove", &format!("@{label}")]);
                    }
                }
                ui.label(RichText::new(if f.kind == "af" { t("audio") } else { t("video") }).size(11.0).color(FG_DIM));
                ui.add(egui::Label::new(RichText::new(&f.value).monospace().size(11.0).color(FG_DIM)).truncate());
                if ui.small_button("🗑").on_hover_text(t("Delete")).clicked() {
                    delete = Some(i);
                }
            });
        }
        if let Some(i) = delete {
            let f = saved.remove(i);
            mpv.command(&[&f.kind, "remove", &format!("@{}", f.label())]);
        }
        ui.separator();
        ui.label(RichText::new(t("New filter")).strong());
        row(ui, |ui| {
            for (k, l) in [("vf", "Video"), ("af", "Audio")] {
                if ui.selectable_label(kind == k, t(l)).clicked() {
                    kind = k.to_string();
                }
            }
        });
        field(ui, "Name", |ui| ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0)));
        field(ui, "Filter", |ui| {
            ui.add(egui::TextEdit::singleline(&mut value).desired_width(260.0).code_editor().hint_text(if kind == "af" { "lavfi=[loudnorm]" } else { "lavfi=[hqdn3d]" }))
        });
        ui.label(RichText::new(t("mpv filter syntax, e.g. lavfi=[unsharp], hflip, scale=1280:-2.")).size(11.0).color(FG_DIM));
        row(ui, |ui| {
            let ok = !value.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new(t("Add"))).clicked() {
                mpv.command(&[&kind, "add", value.trim()]);
                value.clear();
            }
            if ui.add_enabled(ok, egui::Button::new(t("Add and Save"))).clicked() {
                let f = SavedFilter { name: if name.trim().is_empty() { value.trim().to_string() } else { name.trim().to_string() }, kind: kind.clone(), value: value.trim().to_string() };
                mpv.command(&[&f.kind, "add", &format!("@{}:{}", f.label(), f.value)]);
                saved.push(f);
                name.clear();
                value.clear();
            }
        });
    });
    st.filter_kind = kind;
    st.filter_name = name;
    st.filter_value = value;
    if saved != st.settings.saved_filters {
        st.settings.saved_filters = saved;
        st.settings_dirty = true;
    }
    st.show_filters = open;
}

// --------------------------------------------------------------------- logs

pub fn logs(ctx: &egui::Context, st: &mut UiState) {
    if !st.show_logs {
        return;
    }
    let mut open = true;
    let screen_h = ctx.screen_rect().height();
    let mut level = st.log_level;
    let mut filter = std::mem::take(&mut st.log_filter);
    let logs = &st.logs;
    tool_window(ctx, "Logs", &mut open, vec2(720.0, 460.0), |ui| {
        row(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text(t("Search…")).desired_width(220.0));
            for (i, l) in ["All", "Warnings", "Errors"].iter().enumerate() {
                if ui.selectable_label(level == i as u8, t(l)).clicked() {
                    level = i as u8;
                }
            }
        });
        let f = filter.to_lowercase();
        let keep = |l: &LogLine| {
            let lv = match l.level.as_str() {
                "fatal" | "error" => 2,
                "warn" => 1,
                _ => 0,
            };
            lv >= level && (f.is_empty() || l.text.to_lowercase().contains(&f) || l.prefix.contains(&f))
        };
        let text = |l: &LogLine| format!("[{}] {}: {}", l.level, l.prefix, l.text);
        row(ui, |ui| {
            if ui.button(t("Copy")).clicked() {
                let all: Vec<String> = logs.iter().filter(|l| keep(l)).map(text).collect();
                ui.ctx().copy_text(all.join("\n"));
            }
            if ui.button(t("Save…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_file_name("iina-rs.log").save_file() {
                    let all: Vec<String> = logs.iter().map(text).collect();
                    let _ = std::fs::write(p, all.join("\n"));
                }
            }
            ui.label(RichText::new(tf("{} lines", &[&logs.len().to_string()])).size(11.0).color(FG_DIM));
        });
        ui.separator();
        // Log lines are technical (mostly English): keep them left-to-right.
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            egui::ScrollArea::vertical().max_height((screen_h - 250.0).max(150.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for l in logs.iter().filter(|l| keep(l)) {
                    let color = match l.level.as_str() {
                        "fatal" | "error" => Color32::from_rgb(255, 120, 110),
                        "warn" => Color32::from_rgb(240, 200, 100),
                        _ => FG,
                    };
                    ui.add(egui::Label::new(RichText::new(text(l)).monospace().size(11.5).color(color)).wrap());
                }
            });
        });
    });
    st.log_level = level;
    st.log_filter = filter;
    st.show_logs = open;
}

// --------------------------------------------------------- screenshot preview

pub fn screenshot_preview(ctx: &egui::Context, st: &mut UiState, cmds: &mut Vec<String>) {
    let Some(p) = st.shot_preview.as_ref() else { return };
    const SHOW: f32 = 5.0;
    let age = p.at.elapsed().as_secs_f32();
    let hovered = ctx.memory(|m| m.area_rect(egui::Id::new("shot-preview"))).is_some_and(|r| ctx.pointer_hover_pos().is_some_and(|h| r.contains(h)));
    if age > SHOW && !hovered {
        st.shot_preview = None;
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(250));
    let size = p.tex.size_vec2();
    let w = 220.0;
    let h = w * size.y / size.x.max(1.0);
    let (path, tex, copied) = (p.path.clone(), p.tex.clone(), p.copied);
    let (align, off) = if is_rtl() { (Align2::LEFT_BOTTOM, vec2(16.0, -110.0)) } else { (Align2::RIGHT_BOTTOM, vec2(-16.0, -110.0)) };
    egui::Area::new(egui::Id::new("shot-preview")).anchor(align, off).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new().fill(HUD_BG).corner_radius(10).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(w);
            let r = ui.add(egui::Image::new((tex.id(), vec2(w, h))).corner_radius(6).sense(Sense::click()));
            if r.on_hover_text(t("Open")).clicked() {
                cmds.push(format!("open-external {}", path.display()));
            }
            row(ui, |ui| {
                if ui.small_button(t("Show in Folder")).clicked() {
                    cmds.push(format!("reveal {}", path.display()));
                }
                if copied {
                    ui.label(RichText::new(t("Copied to clipboard")).size(11.0).color(FG_DIM));
                } else if ui.small_button(t("Copy")).clicked() {
                    cmds.push(format!("copy-image {}", path.display()));
                }
            });
        });
    });
}

// ------------------------------------------------------------ update notice

/// A small banner (welcome window / About) when a newer release exists.
pub fn update_notice(ui: &mut Ui, st: &UiState) {
    if let Status::Available { version, url } = &*st.update.lock().unwrap() {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(80, 120, 220, 60)).corner_radius(6).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            row(ui, |ui| {
                ui.label(tf("IINA-RS {} is available.", &[version]));
                ui.hyperlink_to(t("Download"), url);
            });
        });
    }
}

pub fn update_status_line(ui: &mut Ui, st: &UiState, cmds: &mut Vec<String>) {
    let status = st.update.lock().unwrap().clone();
    row(ui, |ui| {
        let busy = status == Status::Checking;
        if ui.add_enabled(!busy, egui::Button::new(t("Check for Updates"))).clicked() {
            cmds.push("check-updates".into());
        }
        let msg = match &status {
            Status::Idle => String::new(),
            Status::Checking => t("Checking…"),
            Status::UpToDate => t("You have the latest version."),
            Status::Available { version, .. } => tf("IINA-RS {} is available.", &[version]),
            Status::Failed(_) => t("Couldn't check for updates."),
        };
        ui.label(RichText::new(msg).size(11.5).color(FG_DIM));
    });
    if let Status::Available { url, .. } = &status {
        ui.hyperlink_to(t("Download"), url);
    }
}

// -------------------------------------------------------------- font dialog

pub const PREVIEW_FAMILY: &str = "sub-preview";
/// The UI's font set, kept so the sample's font can be swapped in.
pub static BASE_FONTS: std::sync::OnceLock<egui::FontDefinitions> = std::sync::OnceLock::new();

pub struct FontDialog {
    pub filter: String,
    /// Subtitle style when the dialog opened (Cancel restores it).
    pub before: crate::settings::SubStyle,
    /// Face currently loaded for the sample: (path, index).
    loaded: Option<(std::path::PathBuf, u32)>,
    /// Scroll the family list to the selection once.
    scroll_to_selection: bool,
    /// libass rendering of the sample (exactly as on the video).
    preview: crate::sub_preview::Preview,
    sent: Option<crate::settings::SubStyle>,
    tex: Option<(u64, egui::TextureHandle)>,
}

impl FontDialog {
    pub fn new(before: crate::settings::SubStyle, preview: crate::sub_preview::Preview) -> Self {
        FontDialog { filter: String::new(), before, loaded: None, scroll_to_selection: true, preview, sent: None, tex: None }
    }
}

const SIZES: [f64; 16] = [16.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 55.0, 60.0, 66.0, 72.0, 80.0, 96.0, 120.0];

fn style_label(style: &str) -> String {
    match style.to_lowercase().as_str() {
        "regular" | "normal" | "book" | "roman" => t("Regular"),
        "bold" => t("Bold"),
        "italic" | "oblique" => t("Italic"),
        "bold italic" | "bold oblique" => t("Bold Italic"),
        _ => style.to_string(),
    }
}

/// Puts the chosen face first in the sample's font family.
fn load_preview_font(ctx: &egui::Context, face: Option<&crate::fonts::Face>) {
    let Some(base) = BASE_FONTS.get() else { return };
    let mut defs = base.clone();
    if let Some(face) = face {
        if let Ok(bytes) = std::fs::read(&face.path) {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = face.index;
            defs.font_data.insert("sub-preview-face".into(), std::sync::Arc::new(data));
            defs.families.entry(egui::FontFamily::Name(PREVIEW_FAMILY.into())).or_default().insert(0, "sub-preview-face".into());
        }
    }
    ctx.set_fonts(defs);
}

/// Text with outline and shadow, roughly as libass draws it.
fn sample_text(painter: &Painter, center: Pos2, text: &str, font: FontId, s: &crate::settings::SubStyle) {
    let c = |c: [u8; 4]| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    let galley = |color| painter.layout_no_wrap(text.to_string(), font.clone(), color);
    let main = galley(c(s.color));
    let origin = center - main.size() / 2.0;
    let scale = font.size / 40.0;
    let outline = s.outline_size as f32 * scale;
    let shadow = s.shadow_offset as f32 * scale;
    if s.border_style == "outline-and-shadow" {
        if shadow > 0.0 {
            painter.galley(origin + vec2(shadow, shadow), galley(c(s.shadow_color)), Color32::WHITE);
        }
        if outline > 0.0 {
            let g = galley(c(s.outline_color));
            let steps = 16;
            for i in 0..steps {
                let a = i as f32 / steps as f32 * std::f32::consts::TAU;
                painter.galley(origin + vec2(a.cos(), a.sin()) * outline, g.clone(), Color32::WHITE);
            }
        }
    } else {
        let pad = vec2(outline.max(2.0) + 4.0, outline.max(2.0));
        painter.rect_filled(Rect::from_min_size(origin - pad, main.size() + pad * 2.0), 2.0, c(s.box_color));
    }
    painter.galley(origin, main, Color32::WHITE);
}

fn column<R>(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let align = if is_rtl() { Align::Max } else { Align::Min };
    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(align), |ui| {
        ui.set_width(width);
        add(ui)
    })
    .inner
}

fn list<R>(ui: &mut Ui, id: &str, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let align = if is_rtl() { Align::Max } else { Align::Min };
    egui::Frame::new().fill(Color32::from_black_alpha(60)).corner_radius(4).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, false]).show(ui, |ui| ui.with_layout(Layout::top_down_justified(align), add).inner).inner
    })
    .inner
}

pub fn font_dialog(ctx: &egui::Context, st: &mut UiState, cmds: &mut Vec<String>) {
    let Some(mut dlg) = st.font_dialog.take() else { return };
    let mut open = true;
    let mut done: Option<bool> = None; // Some(true) = OK, Some(false) = Cancel
    let all = st.fonts.lock().unwrap().clone();
    if all.is_none() {
        cmds.push("scan-fonts".into());
    }
    let mut s = st.settings.subs.clone();

    // The sample shows the face libass would pick for these flags.
    let family = all.as_ref().and_then(|l| l.iter().find(|f| f.name.eq_ignore_ascii_case(&s.font)).cloned());
    let face = family.as_ref().and_then(|f| f.face(s.bold, s.italic)).cloned();
    let want = face.as_ref().map(|f| (f.path.clone(), f.index));
    if want != dlg.loaded {
        load_preview_font(ctx, face.as_ref());
        dlg.loaded = want;
        ctx.request_repaint();
    }

    tool_window(ctx, "Font", &mut open, vec2(640.0, 560.0), |ui| {
        let list_h = 220.0;
        row(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            column(ui, 300.0, list_h + 64.0, |ui| {
                ui.label(RichText::new(t("Font")).strong());
                ui.add(egui::TextEdit::singleline(&mut dlg.filter).hint_text(t("Search fonts…")).desired_width(300.0));
                let f = dlg.filter.to_lowercase();
                // Scroll to the current font once the list has loaded.
                let scroll = dlg.scroll_to_selection && all.is_some();
                if scroll {
                    dlg.scroll_to_selection = false;
                }
                list(ui, "font-families", list_h, |ui| {
                    if f.is_empty() && ui.selectable_label(s.font.is_empty(), t("Default font")).clicked() {
                        s.font.clear();
                    }
                    match &all {
                        None => {
                            ui.label(RichText::new(t("Loading fonts…")).color(FG_DIM));
                        }
                        Some(fams) => {
                            for fam in fams.iter().filter(|n| f.is_empty() || n.name.to_lowercase().contains(&f)) {
                                let sel = fam.name.eq_ignore_ascii_case(&s.font);
                                let r = ui.selectable_label(sel, &fam.name);
                                if sel && scroll {
                                    r.scroll_to_me(Some(Align::Center));
                                }
                                if r.clicked() {
                                    s.font = fam.name.clone();
                                }
                            }
                        }
                    }
                });
            });
            column(ui, 150.0, list_h + 64.0, |ui| {
                ui.label(RichText::new(t("Font style")).strong());
                ui.add_space(ui.spacing().interact_size.y + ui.spacing().item_spacing.y);
                list(ui, "font-styles", list_h, |ui| {
                    let faces: Vec<(String, bool, bool)> = match &family {
                        Some(fam) => fam.faces.iter().map(|f| (f.style.clone(), f.bold, f.italic)).collect(),
                        None => vec![("Regular".into(), false, false), ("Bold".into(), true, false), ("Italic".into(), false, true), ("Bold Italic".into(), true, true)],
                    };
                    for (style, bold, italic) in faces {
                        let sel = face.as_ref().map_or(s.bold == bold && s.italic == italic, |f| f.style == style);
                        if ui.selectable_label(sel, style_label(&style)).clicked() {
                            s.bold = bold;
                            s.italic = italic;
                        }
                    }
                });
            });
            column(ui, 100.0, list_h + 64.0, |ui| {
                ui.label(RichText::new(t("Size")).strong());
                ui.add(egui::DragValue::new(&mut s.size).range(8.0..=200.0).speed(0.5).max_decimals(0));
                list(ui, "font-sizes", list_h, |ui| {
                    for size in SIZES {
                        if ui.selectable_label((s.size - size).abs() < 0.5, format!("{size:.0}")).clicked() {
                            s.size = size;
                        }
                    }
                });
            });
        });
        ui.add_space(6.0);
        ui.label(RichText::new(t("Sample")).strong());
        // libass's own rendering when ready; drawn here meanwhile (or if
        // the preview player can't start).
        if dlg.sent.as_ref() != Some(&s) {
            dlg.preview.show(&s);
            dlg.sent = Some(s.clone());
        }
        {
            let out = dlg.preview.out.lock().unwrap();
            if let Some(img) = out.image.as_ref().filter(|_| dlg.tex.as_ref().is_none_or(|(g, _)| *g != out.generation)) {
                let tex = ui.ctx().load_texture("sub-sample", img.clone(), egui::TextureOptions::LINEAR);
                dlg.tex = Some((out.generation, tex));
            }
        }
        if let Some((_, tex)) = &dlg.tex {
            let w = ui.available_width();
            let size = tex.size_vec2();
            ui.add(egui::Image::new((tex.id(), vec2(w, w * size.y / size.x))).corner_radius(6));
        } else {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
        let painter = ui.painter_at(rect);
        // A video-like backdrop, so outline and shadow read as on screen.
        painter.rect_filled(rect, 6.0, Color32::from_rgb(70, 82, 96));
        painter.rect_filled(Rect::from_min_max(pos2(rect.left(), rect.center().y), rect.max), 0.0, Color32::from_rgb(40, 44, 50));
        let size = (s.size as f32 * 0.7).clamp(12.0, 70.0);
        let font = FontId::new(size, egui::FontFamily::Name(PREVIEW_FAMILY.into()));
        sample_text(&painter, pos2(rect.center().x, rect.top() + rect.height() * 0.3), "AaBbYyZz 0123", font.clone(), &s);
        sample_text(&painter, pos2(rect.center().x, rect.top() + rect.height() * 0.72), &v("أبجد هوز حطي كلمن"), font, &s);
        }
        ui.label(RichText::new(t("The subtitle on the video changes as you choose.")).size(11.0).color(FG_DIM));
        ui.add_space(4.0);
        row(ui, |ui| {
            if ui.button(t("OK")).clicked() {
                done = Some(true);
            }
            if ui.button(t("Cancel")).clicked() {
                done = Some(false);
            }
        });
    });

    if !open && done.is_none() {
        done = Some(false); // Closing the window cancels, as in PotPlayer.
    }
    if done == Some(false) {
        s = dlg.before.clone();
    }
    if s != st.settings.subs {
        st.settings.subs = s;
        st.settings_dirty = true;
    }
    if done.is_none() {
        st.font_dialog = Some(dlg);
    } else {
        // Back to the plain UI fonts.
        load_preview_font(ctx, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_filter_lists() {
        assert_eq!(split_filters("@iina-flip:vflip,lavfi=[eq=gamma=1.2,unsharp],hflip"), ["@iina-flip:vflip", "lavfi=[eq=gamma=1.2,unsharp]", "hflip"]);
        assert!(split_filters("").is_empty());
    }
}
