//! **Find singers**: the catalogue of free voicebanks, and getting one.
//!
//! The index is a JSON file (schema in `docs/vocal-plan.md` §4.1; a seed in
//! `docs/singers-index.json`), read leniently: unknown keys are ignored, an
//! entry with no way to get it is skipped and named. Each entry records what
//! its author allows; nothing is rehosted, so a download URL is the
//! author's own, and where the author wants their page visited the entry
//! has only a page.
//!
//! - **Install**: a direct `https` download with its size and sha256 in the
//!   index — fetched (the app's `curl`, as the updater does), checked with
//!   [`verify`], installed.
//! - **Get it**: the author's page opens in the browser, and a
//!   [`DownloadWatch`] on the Downloads folder installs the file when it
//!   lands.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;
use sha2::Digest;

/// The index format this build reads.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogueKind {
    Utau,
    DiffSinger,
    Dependency,
}

impl CatalogueKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utau => "UTAU",
            Self::DiffSinger => "DiffSinger",
            Self::Dependency => "Dependency",
        }
    }
}

/// What an author's terms say about one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Permission {
    Yes,
    No,
    /// Allowed on conditions, or by asking.
    Ask,
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for Permission {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Ok(match text.to_ascii_lowercase().as_str() {
            "yes" | "allowed" | "true" => Self::Yes,
            "no" | "forbidden" | "false" => Self::No,
            "ask" | "conditional" | "conditions" => Self::Ask,
            _ => Self::Unknown,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Terms {
    /// One or two sentences: what the author allows.
    #[serde(default)]
    pub summary: String,
    /// The author's full terms.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub commercial: Permission,
    /// Whether the voice may be used to train other voices.
    #[serde(default)]
    pub ai_training: Permission,
    /// Whether the author must be credited.
    #[serde(default)]
    pub credit: Permission,
    /// Whether the bank may be passed on as it is — what a shared song
    /// needs to send a friend the singer it uses. Only `yes` lets a studio
    /// send it (`docs/collab-plan.md` §18, F93); anything else, unknown
    /// included, sends the friend to `page` or `download` instead.
    #[serde(default)]
    pub redistribute: Permission,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

impl Download {
    fn valid(&self) -> bool {
        self.url.starts_with("https://")
            && self.size > 0
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// One singer in the catalogue.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CatalogueSinger {
    pub id: String,
    pub name: String,
    pub kind: CatalogueKind,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// An image URL.
    #[serde(default)]
    pub portrait: Option<String>,
    /// An audio URL to listen to.
    #[serde(default)]
    pub sample: Option<String>,
    #[serde(default)]
    pub terms: Terms,
    #[serde(default)]
    pub download: Option<Download>,
    /// The author's page for it.
    #[serde(default)]
    pub page: Option<String>,
    /// Names its bank installs as — the `character.txt` name, the folder
    /// the package makes — for telling an installed bank is this one when
    /// its own name is in another script (see [`match_bank`]). Format 1
    /// grows by keys, so an index without it reads as before.
    #[serde(default)]
    pub installs_as: Vec<String>,
}

/// How a singer is got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Get {
    /// Fetched, verified and installed here.
    Download(Download),
    /// The author's page, opened in the browser; the file is installed when
    /// it lands in Downloads.
    Page(String),
}

impl CatalogueSinger {
    pub fn get(&self) -> Get {
        match &self.download {
            Some(d) if d.valid() => Get::Download(d.clone()),
            _ => Get::Page(self.page.clone().unwrap_or_default()),
        }
    }

    fn reachable(&self) -> bool {
        self.download.as_ref().is_some_and(Download::valid)
            || self
                .page
                .as_deref()
                .is_some_and(|p| p.starts_with("https://"))
    }
}

/// A read index.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Catalogue {
    pub updated: Option<String>,
    pub singers: Vec<CatalogueSinger>,
    /// Entries left out, and why, for whoever keeps the index.
    pub skipped: Vec<String>,
}

#[derive(Deserialize)]
struct Raw {
    format: u32,
    #[serde(default)]
    updated: Option<String>,
    #[serde(default)]
    singers: Vec<serde_json::Value>,
}

impl Catalogue {
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| format!("not an index: {e}"))?;
        if raw.format != FORMAT {
            return Err(format!(
                "index format {} — this Fontelle reads {FORMAT}",
                raw.format
            ));
        }
        let mut out = Catalogue {
            updated: raw.updated,
            ..Catalogue::default()
        };
        for value in raw.singers {
            let label = value
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("an entry")
                .to_string();
            match serde_json::from_value::<CatalogueSinger>(value) {
                Ok(s) if !s.reachable() => out
                    .skipped
                    .push(format!("{label}: no download and no page")),
                Ok(s) if s.terms.summary.trim().is_empty() => {
                    out.skipped.push(format!("{label}: no terms"))
                }
                Ok(s) => out.singers.push(s),
                Err(e) => out.skipped.push(format!("{label}: {e}")),
            }
        }
        Ok(out)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        Self::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }
}

fn sha256_of(path: &Path) -> Result<(String, u64), String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    let digest = hasher.finalize();
    Ok((digest.iter().map(|b| format!("{b:02x}")).collect(), size))
}

/// Whether the file at `path` is the one the index describes.
pub fn verify(path: &Path, sha256: &str, size: u64) -> Result<(), String> {
    let (actual, len) = sha256_of(path)?;
    if len != size {
        return Err(format!(
            "the download is {len} bytes and the catalogue says {size}: wrong size"
        ));
    }
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err("the download's sha256 is not the catalogue's: it was not installed".into());
    }
    Ok(())
}

/// The Downloads folder: the desktop's `XDG_DOWNLOAD_DIR` (from
/// `user-dirs.dirs`, given as `user_dirs`), else `~/Downloads`; on Windows
/// `%USERPROFILE%\Downloads`.
pub fn downloads_dir_from(
    env: &dyn Fn(&str) -> Option<String>,
    windows: bool,
    user_dirs: Option<&str>,
) -> Option<PathBuf> {
    if windows {
        return env("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads"));
    }
    let home = env("HOME").filter(|h| !h.is_empty())?;
    if let Some(dirs) = user_dirs {
        for line in dirs.lines() {
            let Some(value) = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=") else {
                continue;
            };
            let value = value.trim().trim_matches('"').replace("$HOME", &home);
            if value.starts_with('/') {
                return Some(PathBuf::from(value));
            }
        }
    }
    Some(PathBuf::from(home).join("Downloads"))
}

/// [`downloads_dir_from`] for this machine.
pub fn downloads_dir() -> Option<PathBuf> {
    let env = |key: &str| std::env::var(key).ok();
    let config = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")));
    let dirs = config.and_then(|c| std::fs::read_to_string(c.join("user-dirs.dirs")).ok());
    downloads_dir_from(&env, cfg!(windows), dirs.as_deref())
}

/// What a voicebank's download is called.
fn is_package(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["zip", "oudep", "7z", "uar"].contains(&e.to_ascii_lowercase().as_str()))
}

/// Watches a Downloads folder for the file a "Get it" is waiting for: a new
/// voicebank package (not a browser's partial file), once its size has held
/// still for one poll — and, when the index knows its sha256, only that file.
pub struct DownloadWatch {
    dir: PathBuf,
    since: SystemTime,
    sha256: Option<String>,
    /// Present when the watch began: not what it waits for.
    before: HashMap<PathBuf, u64>,
    /// Seen once, with the size it had.
    seen: HashMap<PathBuf, u64>,
    done: Vec<PathBuf>,
}

impl DownloadWatch {
    pub fn new(dir: &Path, since: SystemTime, sha256: Option<String>) -> Self {
        let before = Self::scan(dir).into_iter().collect();
        Self {
            dir: dir.to_path_buf(),
            since,
            sha256,
            before,
            seen: HashMap::new(),
            done: Vec::new(),
        }
    }

    fn scan(dir: &Path) -> Vec<(PathBuf, u64)> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let meta = e.metadata().ok()?;
                meta.is_file().then_some((path, meta.len()))
            })
            .collect()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file, once it has landed whole.
    pub fn poll(&mut self) -> Option<PathBuf> {
        let mut found = None;
        for (path, size) in Self::scan(&self.dir) {
            if !is_package(&path) || self.done.contains(&path) {
                continue;
            }
            if self.before.get(&path) == Some(&size) {
                continue;
            }
            let fresh = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= self.since);
            if !fresh {
                continue;
            }
            match self.seen.insert(path.clone(), size) {
                Some(last) if last == size && size > 0 && found.is_none() => {
                    let wanted = match &self.sha256 {
                        Some(sha) => {
                            sha256_of(&path).is_ok_and(|(s, _)| s.eq_ignore_ascii_case(sha))
                        }
                        None => true,
                    };
                    self.done.push(path.clone());
                    if wanted {
                        found = Some(path);
                    }
                }
                _ => {}
            }
        }
        found
    }
}

// ------------------------------------------------------- reading terms
//
// > Ty agreed to Kasane Teto's terms, which are Japanese, without being
// > able to read them.
//
// A bank's own terms are kept and shown as its author wrote them; when
// they are not in the reader's language, the index's English summary of
// the same singer is put beside them, and a Translate link hands the text
// to a translation site in the browser — only when asked.

/// Lower-case letters and digits only: how two spellings of one name are
/// compared ("Kasane Teto" and "KasaneTeto_tougou").
fn squashed(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The catalogue entry an installed bank is, by the names it goes by — its
/// display name, its folder, its author's name for it. An entry's
/// [`installs_as`](CatalogueSinger::installs_as) names match whole; else
/// its name before any bracket matches when the bank's name or folder
/// starts with or contains its first two words run together ("Hitsuboku
/// Kumi" in `Hitsuboku_Kumi_CHN`). Only an entry of the same kind: a
/// singer's UTAU bank does not borrow its DiffSinger bank's terms.
pub fn match_bank<'a>(
    catalogue: &'a Catalogue,
    kind: CatalogueKind,
    names: &[&str],
) -> Option<&'a CatalogueSinger> {
    let names: Vec<String> = names
        .iter()
        .map(|n| squashed(n))
        .filter(|n| !n.is_empty())
        .collect();
    let same_kind = || catalogue.singers.iter().filter(move |s| s.kind == kind);
    if let Some(s) = same_kind().find(|s| {
        s.installs_as
            .iter()
            .map(|a| squashed(a))
            .any(|a| !a.is_empty() && names.contains(&a))
    }) {
        return Some(s);
    }
    // The longest key that fits wins, so "Kasane Teto English" would beat
    // "Kasane Teto" for an English bank if the index had both.
    same_kind()
        .filter_map(|s| {
            let base = s.name.split(['(', '[']).next().unwrap_or(&s.name);
            let words: Vec<&str> = base.split_whitespace().collect();
            let key = squashed(&words[..words.len().min(2)].concat());
            // A one-letter key would match half the folder names there are.
            (key.chars().count() >= 4 && names.iter().any(|n| n.contains(&key)))
                .then_some((key.len(), s))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, s)| s)
}

/// Whether `text` reads as written in `language` (a BCP 47 tag), judged by
/// its script: English is Latin letters, Japanese kana or kanji, Chinese
/// Han, Korean hangul. A text with nothing to read is read. A name in
/// another script inside an English sentence is still English: it is the
/// share of the letters that decides.
pub fn reads_as(text: &str, language: &str) -> bool {
    #[derive(PartialEq)]
    enum Script {
        Latin,
        Kana,
        Han,
        Hangul,
        Other,
    }
    let script = |c: char| match c as u32 {
        0x3040..=0x30ff | 0x31f0..=0x31ff | 0xff66..=0xff9f => Script::Kana,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2fa1f => Script::Han,
        0x1100..=0x11ff | 0x3130..=0x318f | 0xac00..=0xd7af => Script::Hangul,
        _ if c.is_ascii_alphabetic() => Script::Latin,
        0xc0..=0x24f => Script::Latin,
        _ => Script::Other,
    };
    let letters: Vec<Script> = text
        .chars()
        .filter(|c| c.is_alphabetic())
        .map(script)
        .collect();
    if letters.is_empty() {
        return true;
    }
    let share = |want: &[Script]| {
        letters.iter().filter(|s| want.contains(s)).count() as f32 / letters.len() as f32
    };
    let primary = language.split(['-', '_']).next().unwrap_or(language);
    let ours = match primary {
        "ja" => share(&[Script::Kana, Script::Han]),
        "zh" => share(&[Script::Han]),
        "ko" => share(&[Script::Hangul, Script::Han]),
        // Every language written in Latin letters: English, Spanish, ...
        _ => share(&[Script::Latin]),
    };
    ours >= 0.6
}

/// The longest URL [`translate_url`] makes: browsers and the site take
/// more, but a desktop's "open this link" goes through a command line.
pub const TRANSLATE_URL_MAX: usize = 7000;

/// A link that opens `text` translated into `to` on a translation site
/// (Google Translate), the original language detected there. Cut to what a
/// URL can carry, never inside a character.
pub fn translate_url(text: &str, to: &str) -> String {
    let head = format!(
        "https://translate.google.com/?sl=auto&tl={}&op=translate&text=",
        to.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>()
    );
    let mut url = head;
    for c in text.trim().chars() {
        let mut buf = [0u8; 4];
        let mut piece = String::new();
        for b in c.encode_utf8(&mut buf).bytes() {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                piece.push(b as char);
            } else {
                piece.push_str(&format!("%{b:02X}"));
            }
        }
        if url.len() + piece.len() > TRANSLATE_URL_MAX {
            break;
        }
        url.push_str(&piece);
    }
    url
}
