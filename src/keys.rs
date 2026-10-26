//! Key bindings. IINA ships mpv-style `input.conf` files where `Meta` is the
//! macOS Command key; on Windows/Linux that maps to `Ctrl`. Lines starting with
//! `#@iina` bind IINA-specific (non-mpv) actions, which the app handles itself.

use std::collections::HashMap;
use winit::event::KeyEvent;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
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
#@iina F1 about
#@iina F3 open-file
#@iina F4 close-file
#@iina F5 preferences
#@iina F6 playlist-panel
#@iina F7 audio-panel
PGUP playlist-prev
PGDWN playlist-next
BS seek 0 absolute
#@iina g jump-to
l cycle sub
a cycle audio
c add speed 0.1
x add speed -0.1
> add sub-delay 0.5
< add sub-delay -0.5
/ set sub-delay 0
#@iina Ctrl+e screenshot
#@iina Ctrl+c screenshot-clipboard
#@iina Ctrl+z toggle-mirror
#@iina Ctrl+v toggle-flip
j cycle-values video-aspect-override 16:9 4:3 2.35:1 -1
w add brightness -1
e add brightness 1
y add saturation -1
u add saturation 1
[ set ab-loop-a ${time-pos}
] set ab-loop-b ${time-pos}
\\ ab-loop
d frame-back-step
#@iina Shift+N toggle-normalize
#@iina ` window-size min
#@iina 1 window-size 0.5
#@iina 2 window-size 1
#@iina 3 window-size 1.5
#@iina 4 window-size 2
#@iina 5 window-size max
#@iina 6 fit-to-screen
#@iina 7 toggle-fullscreen
#@iina 8 window-fit-aspect
#@iina Ctrl+Alt+KP_ADD bigger-window
#@iina Ctrl+Alt+KP_SUBTRACT smaller-window
#@iina Ctrl+Alt+KP4 move-window -1 0
#@iina Ctrl+Alt+KP6 move-window 1 0
#@iina Ctrl+Alt+KP8 move-window 0 -1
#@iina Ctrl+Alt+KP2 move-window 0 1
#@iina Ctrl+Alt+KP5 place-window center
#@iina Ctrl+Alt+KP7 place-window top-left
#@iina Ctrl+Alt+KP9 place-window top-right
#@iina Ctrl+Alt+KP1 place-window bottom-left
#@iina Ctrl+Alt+KP3 place-window bottom-right
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

    /// Editable rows (key, command, is app command), sorted by key; keys
    /// bound to `ignore` are left out.
    pub fn rows(&self) -> Vec<KeyRow> {
        let mut v: Vec<KeyRow> = self
            .map
            .iter()
            .filter_map(|(k, b)| {
                let (cmd, app) = match b {
                    Binding::Mpv(c) => (c.clone(), false),
                    Binding::App(c) => (c.clone(), true),
                };
                (cmd != "ignore").then(|| KeyRow { key: k.clone(), command: cmd, app })
            })
            .collect();
        v.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()).then(a.key.cmp(&b.key)));
        v
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    pub key: String,
    pub command: String,
    /// An IINA-RS command (`#@iina` line) rather than an mpv one.
    pub app: bool,
}

/// The `input.conf` that turns the built-in bindings into `rows`: only the
/// differences are written (changed/added keys, and `ignore` for removed ones).
pub fn user_conf(rows: &[KeyRow]) -> String {
    let defaults = Bindings::load(None);
    let mut out = String::from("# IINA-RS key bindings (written by the Key Bindings window).
# Format: KEY mpv-command, or \"#@iina KEY app-command\".
");
    let mut seen = std::collections::HashSet::new();
    for r in rows {
        let key = normalize(&r.key);
        if key.is_empty() || r.command.trim().is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        let same = match defaults.lookup(&key) {
            Some(Binding::Mpv(c)) => !r.app && c == r.command.trim(),
            Some(Binding::App(c)) => r.app && c == r.command.trim(),
            None => false,
        };
        if !same {
            out.push_str(&format!("{}{} {}
", if r.app { "#@iina " } else { "" }, key, r.command.trim()));
        }
    }
    let mut removed: Vec<&String> = defaults.map.keys().filter(|k| !seen.contains(*k)).collect();
    removed.sort();
    for k in removed {
        out.push_str(&format!("{k} ignore
"));
    }
    out
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
        BrowserBack => "BROWSER_BACK",
        BrowserForward => "BROWSER_FORWARD",
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

/// mpv's names for numpad keys.
fn numpad(key: PhysicalKey) -> Option<&'static str> {
    use KeyCode::*;
    let PhysicalKey::Code(code) = key else { return None };
    Some(match code {
        Numpad0 => "KP0",
        Numpad1 => "KP1",
        Numpad2 => "KP2",
        Numpad3 => "KP3",
        Numpad4 => "KP4",
        Numpad5 => "KP5",
        Numpad6 => "KP6",
        Numpad7 => "KP7",
        Numpad8 => "KP8",
        Numpad9 => "KP9",
        NumpadAdd => "KP_ADD",
        NumpadSubtract => "KP_SUBTRACT",
        NumpadMultiply => "KP_MULTIPLY",
        NumpadDivide => "KP_DIVIDE",
        NumpadDecimal => "KP_DEC",
        NumpadEnter => "KP_ENTER",
        _ => return None,
    })
}

/// A numpad key with no binding of its own acts as the plain key
/// ("KP1" -> "1", "Ctrl+KP_ADD" -> "Ctrl++").
pub fn numpad_fallback(spec: &str) -> Option<String> {
    let (mods, key) = match spec.rfind('+') {
        Some(i) if spec[i + 1..].starts_with("KP") => (&spec[..i + 1], &spec[i + 1..]),
        _ if spec.starts_with("KP") => ("", spec),
        _ => return None,
    };
    let plain = match key {
        "KP_ADD" => "+",
        "KP_SUBTRACT" => "-",
        "KP_MULTIPLY" => "*",
        "KP_DIVIDE" => "/",
        "KP_DEC" => ".",
        "KP_ENTER" => "ENTER",
        k => k.strip_prefix("KP")?,
    };
    Some(normalize(&format!("{mods}{plain}")))
}

/// The character a key gives on a US layout.
fn us_char(key: PhysicalKey, shift: bool) -> Option<char> {
    use KeyCode::*;
    let PhysicalKey::Code(code) = key else { return None };
    let letter = |c: char| Some(if shift { c.to_ascii_uppercase() } else { c });
    let pick = |plain: char, shifted: char| Some(if shift { shifted } else { plain });
    match code {
        KeyA => letter('a'), KeyB => letter('b'), KeyC => letter('c'), KeyD => letter('d'), KeyE => letter('e'),
        KeyF => letter('f'), KeyG => letter('g'), KeyH => letter('h'), KeyI => letter('i'), KeyJ => letter('j'),
        KeyK => letter('k'), KeyL => letter('l'), KeyM => letter('m'), KeyN => letter('n'), KeyO => letter('o'),
        KeyP => letter('p'), KeyQ => letter('q'), KeyR => letter('r'), KeyS => letter('s'), KeyT => letter('t'),
        KeyU => letter('u'), KeyV => letter('v'), KeyW => letter('w'), KeyX => letter('x'), KeyY => letter('y'),
        KeyZ => letter('z'),
        Digit1 => pick('1', '!'), Digit2 => pick('2', '@'), Digit3 => pick('3', '#'), Digit4 => pick('4', '$'),
        Digit5 => pick('5', '%'), Digit6 => pick('6', '^'), Digit7 => pick('7', '&'), Digit8 => pick('8', '*'),
        Digit9 => pick('9', '('), Digit0 => pick('0', ')'),
        Numpad0 => Some('0'), Numpad1 => Some('1'), Numpad2 => Some('2'), Numpad3 => Some('3'), Numpad4 => Some('4'),
        Numpad5 => Some('5'), Numpad6 => Some('6'), Numpad7 => Some('7'), Numpad8 => Some('8'), Numpad9 => Some('9'),
        Minus => pick('-', '_'), Equal => pick('=', '+'), BracketLeft => pick('[', '{'), BracketRight => pick(']', '}'),
        Backslash => pick('\\', '|'), Semicolon => pick(';', ':'), Quote => pick('\'', '"'), Comma => pick(',', '<'),
        Period => pick('.', '>'), Slash => pick('/', '?'), Backquote => pick('`', '~'),
        _ => None,
    }
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
    // With a non-Latin layout (e.g. Arabic) letter and digit keys produce
    // other characters; shortcuts then follow the key's place on a US
    // keyboard, as in other players.
    if let Some(kp) = numpad(ev.physical_key) {
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
        spec.push_str(kp);
        return Some(normalize(&spec));
    }
    let latin;
    let logical = match &ev.logical_key {
        Key::Character(s) if s.chars().next().is_some_and(|c| !c.is_ascii()) => match us_char(ev.physical_key, shift) {
            Some(c) => {
                latin = Key::Character(c.to_string().into());
                &latin
            }
            None => &ev.logical_key,
        },
        other => other,
    };
    let key = match logical {
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
    fn numpad_falls_back_to_plain_keys() {
        assert_eq!(numpad_fallback("KP1").as_deref(), Some("1"));
        assert_eq!(numpad_fallback("Ctrl+Alt+KP_ADD").as_deref(), Some("Ctrl+Alt++"));
        assert_eq!(numpad_fallback("KP_ENTER").as_deref(), Some("ENTER"));
        assert_eq!(numpad_fallback("1"), None);
        let b = Bindings::load(None);
        assert!(matches!(b.lookup("Ctrl+Alt+KP5"), Some(Binding::App(c)) if c == "place-window center"));
        assert!(matches!(b.lookup("5"), Some(Binding::App(c)) if c == "window-size max"));
    }

    #[test]
    fn user_conf_round_trips() {
        let mut rows = Bindings::load(None).rows();
        assert_eq!(user_conf(&rows).lines().filter(|l| !l.starts_with('#')).count(), 0);
        rows.retain(|r| r.key != "f");
        rows.push(KeyRow { key: "Ctrl+Alt+k".into(), command: "cycle mute".into(), app: false });
        let conf = user_conf(&rows);
        assert!(conf.contains("
f ignore
"));
        assert!(conf.contains("
Ctrl+Alt+k cycle mute
"));
        let b = Bindings::load(Some(conf));
        assert_eq!(b.rows(), rows.iter().cloned().map(|mut r| { r.key = normalize(&r.key); r }).collect::<Vec<_>>().tap_sort());
    }

    trait TapSort {
        fn tap_sort(self) -> Self;
    }
    impl TapSort for Vec<KeyRow> {
        fn tap_sort(mut self) -> Self {
            self.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()).then(a.key.cmp(&b.key)));
            self
        }
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
