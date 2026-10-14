//! Online subtitle search via the OpenSubtitles.com REST API (what IINA's
//! "Find Online Subtitles" uses). Needs a free API key from
//! https://www.opensubtitles.com/consumers, set in Preferences.

use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const API: &str = "https://api.opensubtitles.com/api/v1";
const USER_AGENT: &str = concat!("IINA-RS v", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug)]
pub struct SubResult {
    pub file_id: u64,
    pub language: String,
    pub release: String,
    pub file_name: String,
    pub downloads: u64,
    /// Matched by the file's OpenSubtitles hash (i.e. synced to this exact file).
    pub hash_match: bool,
}

/// State of the search window, shared with the worker threads.
#[derive(Default)]
pub struct Search {
    pub query: String,
    pub languages: String,
    pub busy: bool,
    pub status: String,
    pub results: Vec<SubResult>,
    /// The media file being searched for (used for the hash and save folder).
    pub media_path: String,
    /// Downloaded subtitle files waiting to be loaded by the player.
    pub ready: Vec<PathBuf>,
}

pub type Shared = Arc<Mutex<Search>>;

/// OpenSubtitles hash: file size + 64-bit little-endian word sums of the first
/// and last 64 KiB, with wrapping arithmetic.
pub fn movie_hash(path: &Path) -> std::io::Result<String> {
    const CHUNK: u64 = 65536;
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    if size < CHUNK {
        return Err(std::io::Error::other("file too small"));
    }
    let mut hash = size;
    let mut buf = vec![0u8; CHUNK as usize];
    for offset in [0, size - CHUNK] {
        f.seek(SeekFrom::Start(offset))?;
        f.read_exact(&mut buf)?;
        for word in buf.chunks_exact(8) {
            hash = hash.wrapping_add(u64::from_le_bytes(word.try_into().unwrap()));
        }
    }
    Ok(format!("{hash:016x}"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent(USER_AGENT).build()
}

fn api_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(401 | 403, _) => "OpenSubtitles rejected the API key. Check it in Preferences.".into(),
        ureq::Error::Status(406, r) | ureq::Error::Status(429, r) => {
            let msg = r.into_string().unwrap_or_default();
            format!("Download limit reached or request refused: {}", msg.chars().take(200).collect::<String>())
        }
        ureq::Error::Status(code, r) => format!("OpenSubtitles error {code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
        ureq::Error::Transport(t) => format!("Network error: {t}"),
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<SearchItem>,
}
#[derive(Deserialize)]
struct SearchItem {
    attributes: Attributes,
}
#[derive(Deserialize)]
struct Attributes {
    language: Option<String>,
    release: Option<String>,
    download_count: Option<u64>,
    moviehash_match: Option<bool>,
    files: Vec<FileInfo>,
}
#[derive(Deserialize)]
struct FileInfo {
    file_id: u64,
    file_name: Option<String>,
}

fn search_blocking(key: &str, query: &str, languages: &str, hash: Option<&str>) -> Result<Vec<SubResult>, String> {
    let mut req = agent().get(&format!("{API}/subtitles")).set("Api-Key", key);
    // Parameters must be lowercase and alphabetically ordered (API guideline).
    let langs = languages.to_lowercase().replace(' ', "");
    if !langs.is_empty() {
        req = req.query("languages", &langs);
    }
    if let Some(h) = hash {
        req = req.query("moviehash", h);
    }
    if !query.trim().is_empty() {
        req = req.query("query", &query.trim().to_lowercase());
    }
    let resp: SearchResponse = req.call().map_err(api_error)?.into_json().map_err(|e| e.to_string())?;
    let mut out: Vec<SubResult> = resp
        .data
        .into_iter()
        .filter_map(|item| {
            let a = item.attributes;
            let f = a.files.into_iter().next()?;
            Some(SubResult {
                file_id: f.file_id,
                language: a.language.unwrap_or_default(),
                release: a.release.unwrap_or_default(),
                file_name: f.file_name.unwrap_or_default(),
                downloads: a.download_count.unwrap_or(0),
                hash_match: a.moviehash_match.unwrap_or(false),
            })
        })
        .collect();
    out.sort_by(|a, b| b.hash_match.cmp(&a.hash_match).then(b.downloads.cmp(&a.downloads)));
    Ok(out)
}

#[derive(Deserialize)]
struct DownloadResponse {
    link: String,
    file_name: Option<String>,
    remaining: Option<i64>,
}

fn download_blocking(key: &str, file_id: u64, dest_dir: &Path) -> Result<(PathBuf, Option<i64>), String> {
    let resp: DownloadResponse = agent()
        .post(&format!("{API}/download"))
        .set("Api-Key", key)
        .send_json(serde_json::json!({ "file_id": file_id }))
        .map_err(api_error)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let name = resp.file_name.unwrap_or_else(|| format!("{file_id}.srt"));
    // Keep only a safe file name component.
    let name: String = Path::new(&name)
        .file_name()
        .map(|n| n.to_string_lossy().chars().filter(|c| !"<>:\"/\\|?*".contains(*c)).collect())
        .unwrap_or_else(|| format!("{file_id}.srt"));
    let mut body = Vec::new();
    agent().get(&resp.link).call().map_err(api_error)?.into_reader().take(20 << 20).read_to_end(&mut body).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let path = dest_dir.join(name);
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok((path, resp.remaining))
}

/// Starts a search on a worker thread; `wake` is called when it finishes.
pub fn start_search(shared: Shared, key: String, wake: impl Fn() + Send + 'static) {
    let (query, langs, media) = {
        let mut s = shared.lock().unwrap();
        s.busy = true;
        s.status = "Searching…".into();
        s.results.clear();
        (s.query.clone(), s.languages.clone(), s.media_path.clone())
    };
    std::thread::spawn(move || {
        let hash = movie_hash(Path::new(&media)).ok();
        let mut result = search_blocking(&key, &query, &langs, hash.as_deref());
        // A hash-only search can be empty for rare files; retry by name alone.
        if matches!(&result, Ok(r) if r.is_empty()) && hash.is_some() && !query.trim().is_empty() {
            result = search_blocking(&key, &query, &langs, None);
        }
        let mut s = shared.lock().unwrap();
        s.busy = false;
        match result {
            Ok(r) => {
                s.status = if r.is_empty() { "No subtitles found.".into() } else { format!("{} results", r.len()) };
                s.results = r;
            }
            Err(e) => s.status = e,
        }
        drop(s);
        wake();
    });
}

/// Downloads a subtitle next to the cache folder; on success it's queued in `ready`.
pub fn start_download(shared: Shared, key: String, file_id: u64, dest_dir: PathBuf, wake: impl Fn() + Send + 'static) {
    shared.lock().unwrap().busy = true;
    shared.lock().unwrap().status = "Downloading…".into();
    std::thread::spawn(move || {
        let r = download_blocking(&key, file_id, &dest_dir);
        let mut s = shared.lock().unwrap();
        s.busy = false;
        match r {
            Ok((path, remaining)) => {
                s.status = match remaining {
                    Some(n) => format!("Loaded {} ({n} downloads left today)", path.file_name().unwrap_or_default().to_string_lossy()),
                    None => format!("Loaded {}", path.file_name().unwrap_or_default().to_string_lossy()),
                };
                s.ready.push(path);
            }
            Err(e) => s.status = e,
        }
        drop(s);
        wake();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_reference_algorithm() {
        // 128 KiB + 8 bytes of a repeating pattern; compare with a direct
        // re-implementation of the published algorithm.
        let dir = std::env::temp_dir().join("iina-rs-hash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sample.bin");
        let data: Vec<u8> = (0..(131072 + 8)).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        let mut expect = data.len() as u64;
        for chunk in [&data[..65536], &data[data.len() - 65536..]] {
            for w in chunk.chunks_exact(8) {
                expect = expect.wrapping_add(u64::from_le_bytes(w.try_into().unwrap()));
            }
        }
        assert_eq!(movie_hash(&p).unwrap(), format!("{expect:016x}"));
    }
}
