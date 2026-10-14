//! Online subtitle search and download. Two providers:
//! - SubDL (https://subdl.com/api-doc): free API key for search; downloads are
//!   ZIP archives from dl.subdl.com and need no key (300/day per IP).
//! - OpenSubtitles.com REST API: free API key; searches by file hash + name.

use serde::Deserialize;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const USER_AGENT: &str = concat!("IINA-RS v", env!("CARGO_PKG_VERSION"));
const OPENSUBTITLES_API: &str = "https://api.opensubtitles.com/api/v1";
const SUBDL_API: &str = "https://api.subdl.com/api/v1/subtitles";
const SUBDL_DL: &str = "https://dl.subdl.com";
const SUB_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "smi", "sup", "idx"];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Provider {
    #[default]
    SubDL,
    OpenSubtitles,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::SubDL, Provider::OpenSubtitles];

    pub fn name(self) -> &'static str {
        match self {
            Provider::SubDL => "SubDL",
            Provider::OpenSubtitles => "OpenSubtitles",
        }
    }

    pub fn key_url(self) -> &'static str {
        match self {
            Provider::SubDL => "https://subdl.com/panel/api",
            Provider::OpenSubtitles => "https://www.opensubtitles.com/consumers",
        }
    }

    pub fn from_name(s: &str) -> Self {
        if s.eq_ignore_ascii_case("opensubtitles") { Provider::OpenSubtitles } else { Provider::SubDL }
    }
}

#[derive(Clone, Debug)]
pub struct SubResult {
    pub provider: Provider,
    /// OpenSubtitles: file_id. SubDL: download path ("/subtitle/…zip").
    pub download: String,
    pub language: String,
    pub release: String,
    pub author: String,
    pub downloads: Option<u64>,
    pub hearing_impaired: bool,
    /// Synced to this exact file (OpenSubtitles hash match, or SubDL release
    /// name that closely matches the file name).
    pub good_match: bool,
    score: f32,
}

/// State of the search window, shared with the worker threads.
#[derive(Default)]
pub struct Search {
    pub provider: Provider,
    pub query: String,
    pub languages: String,
    pub busy: bool,
    pub status: String,
    pub results: Vec<SubResult>,
    /// The media file being searched for (hash, release matching, episode).
    pub media_path: String,
    /// Downloaded subtitle files waiting to be loaded by the player.
    pub ready: Vec<PathBuf>,
}

pub type Shared = Arc<Mutex<Search>>;

// ------------------------------------------------------------ file-name parsing

/// What can be inferred from a media file name like
/// "Show.Name.S02E05.1080p.WEB.mkv" or "Movie (2010) 1080p.mkv".
#[derive(Debug, Default, PartialEq)]
pub struct NameInfo {
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(|t| t.to_lowercase()).collect()
}

/// Parses "S01E02" / "s1e2" from a token.
fn season_episode(tok: &str) -> Option<(u32, u32)> {
    let t = tok.strip_prefix('s')?;
    let (s, e) = t.split_once('e')?;
    Some((s.parse().ok()?, e.parse().ok()?))
}

pub fn parse_name(file_name: &str) -> NameInfo {
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut info = NameInfo::default();
    let mut title = Vec::new();
    for tok in tokens(&stem) {
        if let Some((s, e)) = season_episode(&tok) {
            info.season = Some(s);
            info.episode = Some(e);
            break;
        }
        if tok.len() == 4 && (tok.starts_with("19") || tok.starts_with("20")) && tok.parse::<u32>().is_ok() && !title.is_empty() {
            info.year = tok.parse().ok();
            break;
        }
        // Stop at typical release tags.
        if ["1080p", "720p", "2160p", "480p", "4k", "8k", "bluray", "webrip", "web", "hdtv", "x264", "x265", "hevc", "dvdrip", "brrip"].contains(&tok.as_str()) {
            break;
        }
        title.push(tok);
    }
    info.title = title.join(" ");
    info
}

/// Share of the file's tokens that also appear in the release name (0..1).
fn release_score(file_name: &str, release: &str) -> f32 {
    let f = tokens(Path::new(file_name).file_stem().map(|s| s.to_string_lossy()).unwrap_or_default().as_ref());
    if f.is_empty() {
        return 0.0;
    }
    let r = tokens(release);
    f.iter().filter(|t| r.contains(t)).count() as f32 / f.len() as f32
}

// ------------------------------------------------------------------ helpers

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
    ureq::AgentBuilder::new().timeout(Duration::from_secs(25)).user_agent(USER_AGENT).build()
}

fn http_error(provider: Provider, e: ureq::Error) -> String {
    let name = provider.name();
    match e {
        ureq::Error::Status(401 | 403, r) => {
            let body = r.into_string().unwrap_or_default();
            if body.contains("not_authorized") || body.contains("api") || body.is_empty() {
                format!("{name} rejected the API key. Check it in Preferences.")
            } else {
                format!("{name}: access denied ({})", body.chars().take(160).collect::<String>())
            }
        }
        ureq::Error::Status(406 | 429, r) => format!("{name}: limit reached ({})", r.into_string().unwrap_or_default().chars().take(160).collect::<String>()),
        ureq::Error::Status(code, r) => format!("{name} error {code}: {}", r.into_string().unwrap_or_default().chars().take(160).collect::<String>()),
        ureq::Error::Transport(t) => format!("Network error: {t}"),
    }
}

fn safe_file_name(name: &str, fallback: &str) -> String {
    let n: String = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().chars().filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control()).collect())
        .unwrap_or_default();
    if n.trim().is_empty() { fallback.to_string() } else { n }
}

fn read_body(resp: ureq::Response) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    resp.into_reader().take(50 << 20).read_to_end(&mut body).map_err(|e| e.to_string())?;
    Ok(body)
}

// ------------------------------------------------------------------ SubDL

#[derive(Deserialize)]
struct SubdlResponse {
    status: bool,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    subtitles: Vec<SubdlSub>,
}

#[derive(Deserialize)]
struct SubdlSub {
    #[serde(default)]
    release_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    hi: Option<bool>,
    #[serde(default)]
    releases: Option<Vec<String>>,
}

fn subdl_search(key: &str, query: &str, languages: &str, media: &str) -> Result<Vec<SubResult>, String> {
    let file_name = Path::new(media).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let info = parse_name(&file_name);
    let title = if query.trim().is_empty() { info.title.clone() } else { query.trim().to_string() };
    let mut req = agent()
        .get(SUBDL_API)
        .set("Accept", "application/json")
        .query("api_key", key)
        .query("film_name", &title)
        .query("subs_per_page", "30")
        .query("hi", "1")
        .query("releases", "1")
        .query("client", "custom_integration");
    let langs = languages.to_uppercase().replace(' ', "");
    if !langs.is_empty() {
        req = req.query("languages", &langs);
    }
    if let (Some(s), Some(e)) = (info.season, info.episode) {
        req = req.query("type", "tv").query("season_number", &s.to_string()).query("episode_number", &e.to_string());
    } else if let Some(y) = info.year {
        req = req.query("year", &y.to_string());
    }
    let resp = match req.call() {
        Ok(r) => r,
        // SubDL reports bad keys / missing results as JSON with status=false.
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            if let Ok(j) = serde_json::from_str::<SubdlResponse>(&body) {
                let msg = j.message.or_else(|| j.error.map(|e| e.to_string())).unwrap_or_default();
                if msg.to_lowercase().contains("not authorized") || msg.contains("not_authorized") || code == 401 || code == 403 {
                    return Err("SubDL rejected the API key. Check it in Preferences.".into());
                }
                return Err(format!("SubDL: {msg}"));
            }
            return Err(format!("SubDL error {code}"));
        }
        Err(e) => return Err(http_error(Provider::SubDL, e)),
    };
    let j: SubdlResponse = resp.into_json().map_err(|e| e.to_string())?;
    if !j.status {
        let msg = j.message.or_else(|| j.error.map(|e| e.as_str().map(String::from).unwrap_or_else(|| e.to_string()))).unwrap_or_default();
        // "can't find movie or tv" style errors just mean no results.
        if msg.to_lowercase().contains("find") || msg.to_lowercase().contains("not found") {
            return Ok(vec![]);
        }
        return Err(format!("SubDL: {msg}"));
    }
    Ok(j.subtitles
        .into_iter()
        .filter_map(|s| {
            let url = s.url?;
            let release = s.release_name.clone().or(s.name.clone()).unwrap_or_default();
            // Score against the best of the release names the subtitle lists.
            let mut score = release_score(&file_name, &release);
            for r in s.releases.iter().flatten() {
                score = score.max(release_score(&file_name, r));
            }
            Some(SubResult {
                provider: Provider::SubDL,
                download: url,
                language: s.language.or(s.lang).unwrap_or_default(),
                release,
                author: s.author.unwrap_or_default(),
                downloads: None,
                hearing_impaired: s.hi.unwrap_or(false),
                good_match: score >= 0.75,
                score,
            })
        })
        .collect())
}

/// Picks the subtitle file to use from a SubDL ZIP: the one matching the
/// episode for season packs, otherwise the first subtitle file.
pub fn pick_from_zip(data: &[u8], season: Option<u32>, episode: Option<u32>) -> Result<(String, Vec<u8>), String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("Bad archive: {e}"))?;
    let mut candidates: Vec<(usize, String)> = Vec::new();
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if SUB_EXTS.contains(&ext.as_str()) {
            candidates.push((i, name));
        }
    }
    if candidates.is_empty() {
        return Err("The archive contains no subtitle file.".into());
    }
    let wanted = match (season, episode) {
        (Some(s), Some(e)) => candidates
            .iter()
            .find(|(_, n)| tokens(n).iter().any(|t| season_episode(t) == Some((s, e))))
            .or_else(|| candidates.iter().find(|(_, n)| tokens(n).iter().any(|t| t.parse::<u32>().ok() == Some(e)))),
        _ => None,
    };
    let (idx, name) = wanted.unwrap_or(&candidates[0]).clone();
    let mut f = zip.by_index(idx).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    f.by_ref().take(20 << 20).read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok((name, out))
}

fn subdl_download(r: &SubResult, media: &str, dest_dir: &Path) -> Result<PathBuf, String> {
    let url = format!("{SUBDL_DL}{}", if r.download.starts_with('/') { r.download.clone() } else { format!("/{}", r.download) });
    let resp = agent().get(&url).call().map_err(|e| http_error(Provider::SubDL, e))?;
    let body = read_body(resp)?;
    let info = parse_name(&Path::new(media).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let (name, data) = if body.starts_with(b"PK") {
        pick_from_zip(&body, info.season, info.episode)?
    } else {
        // Raw subtitle (e.g. unpacked file URL).
        (format!("{}.srt", r.release), body)
    };
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let path = dest_dir.join(safe_file_name(&name, "subdl.srt"));
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(path)
}

// ---------------------------------------------------------- OpenSubtitles

#[derive(Deserialize)]
struct OsSearchResponse {
    data: Vec<OsItem>,
}
#[derive(Deserialize)]
struct OsItem {
    attributes: OsAttributes,
}
#[derive(Deserialize)]
struct OsAttributes {
    language: Option<String>,
    release: Option<String>,
    download_count: Option<u64>,
    moviehash_match: Option<bool>,
    hearing_impaired: Option<bool>,
    uploader: Option<OsUploader>,
    files: Vec<OsFile>,
}
#[derive(Deserialize)]
struct OsUploader {
    name: Option<String>,
}
#[derive(Deserialize)]
struct OsFile {
    file_id: u64,
}

fn os_search_once(key: &str, query: &str, languages: &str, hash: Option<&str>) -> Result<Vec<SubResult>, String> {
    let mut req = agent().get(&format!("{OPENSUBTITLES_API}/subtitles")).set("Api-Key", key);
    // Parameters lowercase and alphabetical, per the API guidelines.
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
    let resp: OsSearchResponse = req.call().map_err(|e| http_error(Provider::OpenSubtitles, e))?.into_json().map_err(|e| e.to_string())?;
    Ok(resp
        .data
        .into_iter()
        .filter_map(|item| {
            let a = item.attributes;
            let f = a.files.into_iter().next()?;
            let good = a.moviehash_match.unwrap_or(false);
            Some(SubResult {
                provider: Provider::OpenSubtitles,
                download: f.file_id.to_string(),
                language: a.language.unwrap_or_default(),
                release: a.release.unwrap_or_default(),
                author: a.uploader.and_then(|u| u.name).unwrap_or_default(),
                downloads: a.download_count,
                hearing_impaired: a.hearing_impaired.unwrap_or(false),
                good_match: good,
                score: if good { 1.0 } else { 0.0 },
            })
        })
        .collect())
}

fn os_search(key: &str, query: &str, languages: &str, media: &str) -> Result<Vec<SubResult>, String> {
    let hash = movie_hash(Path::new(media)).ok();
    let mut r = os_search_once(key, query, languages, hash.as_deref())?;
    // A hash-only search can be empty for rare files; retry by name alone.
    if r.is_empty() && hash.is_some() && !query.trim().is_empty() {
        r = os_search_once(key, query, languages, None)?;
    }
    Ok(r)
}

#[derive(Deserialize)]
struct OsDownloadResponse {
    link: String,
    file_name: Option<String>,
}

fn os_download(key: &str, r: &SubResult, dest_dir: &Path) -> Result<PathBuf, String> {
    let file_id: u64 = r.download.parse().map_err(|_| "bad file id".to_string())?;
    let resp: OsDownloadResponse = agent()
        .post(&format!("{OPENSUBTITLES_API}/download"))
        .set("Api-Key", key)
        .send_json(serde_json::json!({ "file_id": file_id }))
        .map_err(|e| http_error(Provider::OpenSubtitles, e))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let body = read_body(agent().get(&resp.link).call().map_err(|e| http_error(Provider::OpenSubtitles, e))?)?;
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let path = dest_dir.join(safe_file_name(resp.file_name.as_deref().unwrap_or(""), &format!("{file_id}.srt")));
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

// ------------------------------------------------------------ public API

/// Starts a search on a worker thread; `wake` is called when it finishes.
pub fn start_search(shared: Shared, key: String, wake: impl Fn() + Send + 'static) {
    let (provider, query, langs, media) = {
        let mut s = shared.lock().unwrap();
        s.busy = true;
        s.status = format!("Searching {}…", s.provider.name());
        s.results.clear();
        (s.provider, s.query.clone(), s.languages.clone(), s.media_path.clone())
    };
    std::thread::spawn(move || {
        let result = match provider {
            Provider::SubDL => subdl_search(&key, &query, &langs, &media),
            Provider::OpenSubtitles => os_search(&key, &query, &langs, &media),
        };
        let mut s = shared.lock().unwrap();
        s.busy = false;
        match result {
            Ok(mut r) => {
                r.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then(b.downloads.cmp(&a.downloads)));
                s.status = if r.is_empty() { "No subtitles found.".into() } else { format!("{} results from {}", r.len(), provider.name()) };
                s.results = r;
            }
            Err(e) => s.status = e,
        }
        drop(s);
        wake();
    });
}

/// Downloads result `index`; on success the file is queued in `ready`.
pub fn start_download(shared: Shared, index: usize, keys: (String, String), dest_dir: PathBuf, wake: impl Fn() + Send + 'static) {
    let (r, media) = {
        let mut s = shared.lock().unwrap();
        let Some(r) = s.results.get(index).cloned() else { return };
        s.busy = true;
        s.status = "Downloading…".into();
        (r, s.media_path.clone())
    };
    std::thread::spawn(move || {
        let result = match r.provider {
            Provider::SubDL => subdl_download(&r, &media, &dest_dir),
            Provider::OpenSubtitles => os_download(&keys.1, &r, &dest_dir),
        };
        let mut s = shared.lock().unwrap();
        s.busy = false;
        match result {
            Ok(path) => {
                s.status = format!("Loaded {}", path.file_name().unwrap_or_default().to_string_lossy());
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
    use std::io::Write;

    #[test]
    fn hash_matches_reference_algorithm() {
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

    #[test]
    fn parses_file_names() {
        let i = parse_name("Breaking.Bad.S02E05.1080p.BluRay.x264.mkv");
        assert_eq!((i.title.as_str(), i.season, i.episode), ("breaking bad", Some(2), Some(5)));
        let i = parse_name("Inception (2010) 1080p.mkv");
        assert_eq!((i.title.as_str(), i.year), ("inception", Some(2010)));
        let i = parse_name("YOUR ATTENTION PLEASE (2026) - Trailer [8K Ultra HD].mkv");
        assert_eq!((i.title.as_str(), i.year), ("your attention please", Some(2026)));
        let i = parse_name("2001.A.Space.Odyssey.1968.mkv");
        assert_eq!((i.title.as_str(), i.year), ("2001 a space odyssey", Some(1968)));
    }

    #[test]
    fn picks_episode_from_season_pack() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opt = zip::write::SimpleFileOptions::default();
            for (name, body) in [("readme.txt", "x"), ("Show.S01E01.srt", "one"), ("Show.S01E02.srt", "two")] {
                z.start_file(name, opt).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let data = buf.into_inner();
        let (name, body) = pick_from_zip(&data, Some(1), Some(2)).unwrap();
        assert_eq!((name.as_str(), body.as_slice()), ("Show.S01E02.srt", b"two".as_slice()));
        let (name, _) = pick_from_zip(&data, None, None).unwrap();
        assert_eq!(name, "Show.S01E01.srt");
    }

    /// Network test against the real dl.subdl.com (anonymous download):
    /// `cargo test -- --ignored subdl_live_download`
    #[test]
    #[ignore]
    fn subdl_live_download() {
        let r = SubResult {
            provider: Provider::SubDL,
            download: "/subtitle/97187-1191137.zip".into(),
            language: "EN".into(),
            release: "Inception".into(),
            author: String::new(),
            downloads: None,
            hearing_impaired: false,
            good_match: false,
            score: 0.0,
        };
        let dir = std::env::temp_dir().join("iina-rs-subdl-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = subdl_download(&r, "Inception.2010.1080p.BluRay.mkv", &dir).expect("download");
        let body = std::fs::read(&path).unwrap();
        println!("saved {} ({} bytes)", path.display(), body.len());
        let text = String::from_utf8_lossy(&body[..body.len().min(200)]).to_string();
        println!("{text}");
        assert!(body.len() > 1000);
        assert!(text.contains("-->") || text.contains("[Script Info]") || text.contains('{'));
    }

    #[test]
    fn release_matching() {
        assert!(release_score("Movie.2010.1080p.BluRay.x264-GRP.mkv", "Movie.2010.1080p.BluRay.x264-GRP") > 0.99);
        assert!(release_score("Movie.2010.1080p.BluRay.x264-GRP.mkv", "Movie.2010.720p.WEB") < 0.75);
    }
}
