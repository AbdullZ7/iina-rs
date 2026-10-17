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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_filter_lists() {
        assert_eq!(split_filters("@iina-flip:vflip,lavfi=[eq=gamma=1.2,unsharp],hflip"), ["@iina-flip:vflip", "lavfi=[eq=gamma=1.2,unsharp]", "hflip"]);
        assert!(split_filters("").is_empty());
    }
}
