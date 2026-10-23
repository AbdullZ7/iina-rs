//! Installed font families, for the subtitle font picker. Reads only each
//! file's table directory and `name` table, so scanning stays fast even with
//! large CJK fonts.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// One face of a family (e.g. "Bold Italic"), as libass selects it: by
/// family name plus the bold / italic flags.
#[derive(Clone, Debug)]
pub struct Face {
    pub style: String,
    pub path: PathBuf,
    /// Face index inside a .ttc collection.
    pub index: u32,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug)]
pub struct Family {
    pub name: String,
    pub faces: Vec<Face>,
}

impl Family {
    /// The face closest to the bold / italic flags.
    pub fn face(&self, bold: bool, italic: bool) -> Option<&Face> {
        self.faces
            .iter()
            .min_by_key(|f| (f.bold != bold) as u8 * 2 + (f.italic != italic) as u8 + (!f.style.eq_ignore_ascii_case("regular") && !f.bold && !f.italic) as u8)
    }
}

pub type Shared = Arc<Mutex<Option<Vec<Family>>>>;

fn font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        v.push(PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts"));
        if let Some(local) = dirs::data_local_dir() {
            v.push(local.join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        v.push("/System/Library/Fonts".into());
        v.push("/Library/Fonts".into());
        if let Some(home) = dirs::home_dir() {
            v.push(home.join("Library/Fonts"));
        }
    } else {
        v.push("/usr/share/fonts".into());
        v.push("/usr/local/share/fonts".into());
        if let Some(home) = dirs::home_dir() {
            v.push(home.join(".local/share/fonts"));
            v.push(home.join(".fonts"));
        }
    }
    v
}

fn read_at(f: &mut File, off: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut b = vec![0u8; len];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// (family, style, face index) of each face in a .ttf/.otf/.ttc file
/// (name IDs 1 and 2, Windows platform).
pub fn faces(path: &Path) -> Vec<(String, String, u32)> {
    let Ok(mut f) = File::open(path) else { return vec![] };
    let Some(head) = read_at(&mut f, 0, 12) else { return vec![] };
    let offsets: Vec<u64> = if &head[..4] == b"ttcf" {
        let n = u32_at(&head, 8).min(64) as usize;
        let Some(list) = read_at(&mut f, 12, n * 4) else { return vec![] };
        (0..n).map(|i| u32_at(&list, i * 4) as u64).collect()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for (i, off) in offsets.into_iter().enumerate() {
        if let Some(name) = name_at(&mut f, off, 1) {
            let style = name_at(&mut f, off, 2).unwrap_or_else(|| "Regular".into());
            out.push((name, style, i as u32));
        }
    }
    out
}

fn name_at(f: &mut File, off: u64, wanted: u16) -> Option<String> {
    let dir = read_at(f, off, 12)?;
    let ntables = u16_at(&dir, 4) as usize;
    let records = read_at(f, off + 12, ntables.min(512) * 16)?;
    let (name_off, name_len) = (0..ntables.min(512)).find_map(|i| {
        let r = &records[i * 16..i * 16 + 16];
        (&r[..4] == b"name").then(|| (u32_at(r, 8) as u64, u32_at(r, 12) as usize))
    })?;
    let table = read_at(f, name_off, name_len.min(1 << 20))?;
    let count = u16_at(&table, 2) as usize;
    let strings = u16_at(&table, 4) as usize;
    let mut fallback = None;
    for i in 0..count {
        let r = 6 + i * 12;
        if r + 12 > table.len() {
            break;
        }
        let (platform, lang, name_id) = (u16_at(&table, r), u16_at(&table, r + 4), u16_at(&table, r + 6));
        let (len, offset) = (u16_at(&table, r + 8) as usize, u16_at(&table, r + 10) as usize);
        if name_id != wanted || platform != 3 {
            continue;
        }
        let start = strings + offset;
        let bytes = table.get(start..start + len)?;
        let utf16: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        let name = String::from_utf16_lossy(&utf16).trim().to_string();
        if name.is_empty() {
            continue;
        }
        // Prefer the English (US) name, as libass/DirectWrite match on it too.
        if lang == 0x0409 {
            return Some(name);
        }
        fallback.get_or_insert(name);
    }
    fallback
}

fn walk(dir: &Path, depth: usize, out: &mut std::collections::HashMap<String, Family>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                walk(&p, depth + 1, out);
            }
            continue;
        }
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
            for (name, style, index) in faces(&p) {
                let low = style.to_lowercase();
                let face = Face { bold: low.contains("bold"), italic: low.contains("italic") || low.contains("oblique"), style, path: p.clone(), index };
                let fam = out.entry(name.to_lowercase()).or_insert_with(|| Family { name: name.clone(), faces: Vec::new() });
                if !fam.faces.iter().any(|f| f.style.eq_ignore_ascii_case(&face.style)) {
                    fam.faces.push(face);
                }
            }
        }
    }
}

/// Scans installed fonts on a background thread; `wake` runs when done.
pub fn scan(shared: Shared, extra_dir: PathBuf, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let mut map = std::collections::HashMap::new();
        for d in font_dirs().into_iter().chain(std::iter::once(extra_dir)) {
            walk(&d, 0, &mut map);
        }
        let mut list: Vec<Family> = map.into_values().collect();
        for f in &mut list {
            f.faces.sort_by_key(|f| (f.bold, f.italic, f.style.to_lowercase()));
        }
        list.sort_by_key(|f| f.name.to_lowercase());
        *shared.lock().unwrap() = Some(list);
        wake();
    });
}
