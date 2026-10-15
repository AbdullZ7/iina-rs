//! Key bindings. IINA ships mpv-style `input.conf` files where `Meta` is the
//! macOS Command key; on Windows/Linux that maps to `Ctrl`. Lines starting with
//! `#@iina` bind IINA-specific (non-mpv) actions, which the app handles itself.

use std::collections::HashMap;
use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

pub const IINA_DEFAULT: &str = include_str!("../assets/iina-default-input.conf");

/// Extra bindings for actions that live in the macOS menu bar in IINA.
const DESKTOP_EXTRAS: &str = "
#@iina Meta+o open-file
#@iina Meta+u open-url
#@iina Meta+w close-file
#@iina Meta+i inspector
#@iina Meta+j jump-to
#@iina Meta+h history
#@iina Z sub-delay-step 0 -
#@iina X sub-delay-step 0 +
#@iina Shift+Meta+Z sub-delay-step 1 -
#@iina Shift+Meta+X sub-delay-step 1 +
#@iina Ctrl+Meta+Shift+Z sub-delay-step 2 -
#@iina Ctrl+Meta+Shift+X sub-delay-step 2 +
#@iina Meta+, preferences
Meta+q quit
#@iina F11 toggle-fullscreen
";

#[derive(Clone, Debug)]
pub enum Binding {
    Mpv(String),
    App(String),
}

#[derive(Default)]
pub struct Bindings {
    map: HashMap<String, Binding>,
}

/// Canonical form: modifiers in fixed order, `Meta` folded into `Ctrl`, and
/// `Shift` dropped for printable characters (the char is already shifted).
pub fn normalize(spec: &str) -> String {
    let spec = spec.trim();
    // Split on '+' but allow the key itself to be '+' (e.g. "Ctrl++").
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = spec;
    while let Some(idx) = rest.find('+') {
        if idx == 0 {
            break;
        }
        parts.push(&rest[..idx]);
        rest = &rest[idx + 1..];
    }
    let key = rest;
    let (mut ctrl, mut meta, mut alt, mut shift) = (false, false, false, false);
    for m in parts {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" => ctrl = true,
            "meta" | "cmd" => meta = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => {}
        }
    }
    // Cmd becomes Ctrl. IINA's few Ctrl+Cmd bindings would then collide with
    // Cmd ones (e.g. Ctrl+Cmd+S vs Cmd+S), so they become Ctrl+Alt instead.
    if ctrl && meta {
        alt = true;
    }
    let ctrl = ctrl || meta;
    let printable = key.chars().count() == 1;
    let key = if !printable {
        key.to_ascii_uppercase()
    } else if shift {
        key.to_uppercase()
    } else {
        key.to_string()
    };
    let mut out = String::new();
    if ctrl {
        out.push_str("Ctrl+");
    }
    if alt {
        out.push_str("Alt+");
    }
    if shift && !printable {
        out.push_str("Shift+");
    }
    out.push_str(&key);
    out
}

impl Bindings {
    pub fn load(user_conf: Option<String>) -> Self {
        let mut b = Bindings::default();
        b.parse(IINA_DEFAULT);
        b.parse(DESKTOP_EXTRAS);
        if let Some(c) = user_conf {
            b.parse(&c);
        }
        b
    }

    fn parse(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            let (is_app, body) = if let Some(rest) = line.strip_prefix("#@iina") {
                (true, rest.trim())
            } else if line.is_empty() || line.starts_with('#') {
                continue;
            } else {
                (false, line)
            };
            let Some((key, cmd)) = body.split_once(char::is_whitespace) else { continue };
            let mut cmd = cmd.trim().to_string();
            // Strip trailing comments, but keep '#' inside quotes.
            let mut in_quote = false;
            for (i, c) in cmd.char_indices() {
                match c {
                    '"' => in_quote = !in_quote,
                    '#' if !in_quote => {
                        cmd.truncate(i);
                        break;
                    }
                    _ => {}
                }
            }
            let cmd = cmd.trim().to_string();
            if cmd.is_empty() {
                continue;
            }
            let binding = if is_app { Binding::App(cmd) } else { Binding::Mpv(cmd) };
            self.map.insert(normalize(key), binding);
        }
    }

    pub fn lookup(&self, key: &str) -> Option<&Binding> {
        self.map.get(key)
    }
}

fn named(k: &NamedKey) -> Option<&'static str> {
    use NamedKey::*;
    Some(match k {
        ArrowLeft => "LEFT",
        ArrowRight => "RIGHT",
        ArrowUp => "UP",
        ArrowDown => "DOWN",
        Space => "SPACE",
        Enter => "ENTER",
        Escape => "ESC",
        Backspace => "BS",
        Tab => "TAB",
        Delete => "DEL",
        Insert => "INS",
        Home => "HOME",
        End => "END",
        PageUp => "PGUP",
        PageDown => "PGDWN",
        MediaPlayPause => "PLAYPAUSE",
        MediaPlay => "PLAY",
        MediaPause => "PAUSE",
        MediaStop => "STOP",
        MediaTrackNext => "NEXT",
        MediaTrackPrevious => "PREV",
        MediaFastForward => "FORWARD",
        MediaRewind => "REWIND",
        AudioVolumeUp => "VOLUME_UP",
        AudioVolumeDown => "VOLUME_DOWN",
        AudioVolumeMute => "MUTE",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        _ => return None,
    })
}

/// Converts a winit key press into the canonical binding string.
pub fn event_to_spec(ev: &KeyEvent, mods: ModifiersState) -> Option<String> {
    // Cmd/Win counts as Ctrl (IINA's Meta). Ctrl+Cmd together is IINA's
    // Ctrl+Meta, which `normalize` maps to Ctrl+Alt.
    let (ctrl, alt, shift) = (
        mods.control_key() || mods.super_key(),
        mods.alt_key() || (mods.control_key() && mods.super_key()),
        mods.shift_key(),
    );
    let key = match &ev.logical_key {
        Key::Named(n) => named(n)?.to_string(),
        Key::Character(s) => {
            let c = s.chars().next()?;
            if c == ' ' {
                "SPACE".to_string()
            } else if c.is_control() || ctrl || alt {
                // With Ctrl/Alt held the logical key may be a control char or
                // layout-dependent; fall back to the unmodified key.
                let base = match ev.key_without_modifiers() {
                    Key::Character(b) => b.to_string(),
                    Key::Named(n) => named(&n)?.to_string(),
                    _ => return None,
                };
                if shift && base.chars().all(|c| c.is_ascii_alphabetic()) {
                    base.to_uppercase()
                } else if !c.is_control() && !c.is_ascii_alphanumeric() {
                    s.to_string()
                } else {
                    base
                }
            } else {
                s.to_string()
            }
        }
        _ => return None,
    };
    let mut spec = String::new();
    if ctrl {
        spec.push_str("Ctrl+");
    }
    if alt {
        spec.push_str("Alt+");
    }
    if shift {
        spec.push_str("Shift+");
    }
    spec.push_str(&key);
    Some(normalize(&spec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_iina_specs() {
        assert_eq!(normalize("Meta+L"), "Ctrl+L");
        assert_eq!(normalize("Shift+Meta+>"), "Ctrl+>");
        assert_eq!(normalize("Alt+Meta+["), "Ctrl+Alt+[");
        assert_eq!(normalize("Shift+PGUP"), "Shift+PGUP");
        assert_eq!(normalize("Ctrl++"), "Ctrl++");
        assert_eq!(normalize("Shift+_"), "_");
        assert_eq!(normalize("Ctrl+Meta+f"), "Ctrl+Alt+f");
    }

    #[test]
    fn parses_default_conf() {
        let b = Bindings::load(None);
        assert!(matches!(b.lookup("SPACE"), Some(Binding::Mpv(c)) if c == "cycle pause"));
        assert!(matches!(b.lookup("Ctrl+Shift+P"), None));
        assert!(matches!(b.lookup("Ctrl+P"), Some(Binding::App(c)) if c == "playlist-panel"));
        assert!(matches!(b.lookup("f"), Some(Binding::Mpv(c)) if c == "cycle fullscreen"));
        assert!(matches!(b.lookup("Ctrl+o"), Some(Binding::App(c)) if c == "open-file"));
        assert!(matches!(b.lookup("Ctrl+s"), Some(Binding::Mpv(c)) if c == "screenshot"));
        assert!(matches!(b.lookup("Ctrl+Alt+s"), Some(Binding::Mpv(c)) if c == "cycle sub"));
        assert!(matches!(b.lookup("Ctrl+Alt+p"), Some(Binding::App(c)) if c == "toggle-pip"));
    }
}
