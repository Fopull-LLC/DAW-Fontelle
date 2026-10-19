//! The Fontelle Singer's voicebanks: installed into one folder, recognised,
//! described, their terms kept and acknowledged, and removed.
//!
//! # The folder
//!
//! `<data>/fontelle/voicebanks/` by the app's folder rule
//! ([`voicebanks_dir`]): one folder per bank, named after the bank's own
//! folder in its package, holding the bank exactly as its author laid it out
//! (`oto.ini`, pitch folders, `character.txt` / `character.yaml`,
//! `dsconfig.yaml`, …) — the layout OpenUtau's `Singers` folder uses, so a
//! runtime reads a bank here the way OpenUtau would. Dependencies (a
//! DiffSinger vocoder, say) go in [`DEPENDENCIES`] beside them, one folder
//! each, as OpenUtau's `Dependencies` does. A folder whose name starts with a
//! dot is Fontelle's own (an install in progress) and is not a bank.
//!
//! # What a package is
//!
//! A `.zip`, an `.oudep` (a zip), a `.7z`, a `.uar` (UTAU's installer
//! archive: a zip with an `install.txt` saying `folder=` and `contentsdir=`,
//! which are honoured in any package) or a folder — told apart by the
//! file's first bytes, not its name. Inside it, the **bank's root**
//! is found wherever the author put it — under one or more wrapping
//! folders, beside a readme:
//!
//! - a **DiffSinger** bank: the folder with `dsconfig.yaml` (and usually a
//!   `character.yaml` saying `singer_type: diffsinger`);
//! - a **classic UTAU** bank: the folder with `character.txt` or
//!   `character.yaml` that has an `oto.ini` in it or under it (a multi-pitch
//!   bank's pitch folders), else the folder holding all the `oto.ini`s;
//! - a **dependency**: the folder with `vocoder.yaml` or `oudep.yaml`.
//!
//! # Terms
//!
//! A bank's terms are what its author wrote: the readme (`readme.txt`,
//! `README.md`, `利用規約.txt`, a `terms` or `license` file) at its root, read
//! in whichever encoding it was written. Before a bank first sings, the
//! window shows them and asks for an acknowledgement, recorded beside the
//! bank with a hash of the text — changed terms are asked again.

use std::hash::Hasher;
use std::path::{Path, PathBuf};

use crate::zip::{ZipArchive, safe_path};

pub use crate::zip::decode_text;

/// The folder dependencies are installed in, under the voicebanks folder.
pub const DEPENDENCIES: &str = "Dependencies";

/// The file a bank's acknowledged terms are recorded in.
const ACKNOWLEDGED: &str = ".fontelle-terms";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageKind {
    Utau,
    DiffSinger,
    /// Not a singer: something a bank needs (a vocoder).
    Dependency,
}

impl PackageKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utau => "UTAU",
            Self::DiffSinger => "DiffSinger",
            Self::Dependency => "Dependency",
        }
    }
}

/// Why a package was not installed.
#[derive(Debug)]
pub enum InstallError {
    NotAVoicebank,
    /// A file name that would land outside the bank's folder.
    Unsafe(String),
    /// The archive could not be read.
    Archive(String),
    Io(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAVoicebank => f.write_str(
                "this is not a voicebank: no oto.ini (UTAU), dsconfig.yaml (DiffSinger) \
                 or vocoder.yaml (a dependency) in it",
            ),
            Self::Unsafe(name) => write!(
                f,
                "the package names a file outside its own folder ({name}), so it was not unpacked"
            ),
            Self::Archive(why) => write!(f, "the archive could not be read: {why}"),
            Self::Io(why) => write!(f, "it could not be written: {why}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// What a bank says about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BankInfo {
    pub name: String,
    pub author: Option<String>,
    pub web: Option<String>,
    /// Its picture, when it has one.
    pub portrait: Option<PathBuf>,
    /// A sample to listen to, when it has one.
    pub sample: Option<PathBuf>,
    /// BCP 47 tags, where the bank says (a UTAU bank usually does not).
    pub languages: Vec<String>,
}

/// An installed bank or dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    /// Its folder's name: what [`uninstall`] takes.
    pub id: String,
    pub kind: PackageKind,
    /// What it calls itself.
    pub name: String,
    pub path: PathBuf,
    /// The folder inside it with the bank's own files (`character.txt`,
    /// `oto.ini`, `dsconfig.yaml`): `path` itself, or a folder under it.
    pub root: PathBuf,
    /// Its author's terms, as written.
    pub terms: Option<String>,
    pub info: BankInfo,
}

/// Fontelle's voicebanks folder for an environment — see the module doc.
pub fn voicebanks_dir_from(
    env: &dyn Fn(&str) -> Option<String>,
    windows: bool,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    fontelle_host::fontelle_data_dir_from(env, windows, exists).map(|d| d.join("voicebanks"))
}

/// Fontelle's voicebanks folder.
pub fn voicebanks_dir() -> Option<PathBuf> {
    fontelle_host::fontelle_data_dir().map(|d| d.join("voicebanks"))
}

/// [`voicebanks_dir_from`], or — on a machine with no data folder at all —
/// one in the temporary folder.
///
/// The fallback used to be a bare `voicebanks`, under the working
/// directory: the install folder when Steam starts the program, which Steam
/// verifies file by file (hub card 0429). Banks installed there would be
/// lost at the next reboot, and that is the lesser harm.
pub fn voicebanks_dir_or_scratch_from(
    env: &dyn Fn(&str) -> Option<String>,
    windows: bool,
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    voicebanks_dir_from(env, windows, exists)
        .unwrap_or_else(|| std::env::temp_dir().join("fontelle-voicebanks"))
}

/// [`voicebanks_dir_or_scratch_from`] for this process.
pub fn voicebanks_dir_or_scratch() -> PathBuf {
    voicebanks_dir_or_scratch_from(&|key| std::env::var(key).ok(), cfg!(windows), &|path| {
        path.is_dir()
    })
}

// ------------------------------------------------------------ installing

static STAGING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Installs the package at `source` (a `.zip`, `.oudep` or folder) into
/// `voicebanks`. Nothing is left behind when it fails.
pub fn install(source: &Path, voicebanks: &Path) -> Result<Installed, InstallError> {
    std::fs::create_dir_all(voicebanks)?;
    let staging = voicebanks.join(format!(
        ".installing-{}-{}",
        std::process::id(),
        STAGING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let result = install_from(source, voicebanks, &staging);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn install_from(
    source: &Path,
    voicebanks: &Path,
    staging: &Path,
) -> Result<Installed, InstallError> {
    if source.is_dir() {
        let into = staging.join(file_name(source).unwrap_or_else(|| "bank".into()));
        copy_dir(source, &into)?;
    } else {
        unpack(source, staging)?;
    }
    let oudep = source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("oudep"));
    // UTAU's installer file, when the package has one, says where the bank
    // is in it and what its folder is called.
    let hint = install_txt(staging);
    let search = hint
        .as_ref()
        .and_then(|h| h.contents.as_ref())
        .map(|c| staging.join(c))
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| staging.to_path_buf());
    let (kind, root) = find_root(&search, oudep).ok_or(InstallError::NotAVoicebank)?;
    let info = read_info(&root, kind);
    // install.txt's folder, else the folder's own name when the package had
    // one, else what the bank calls itself.
    let folder = if let Some(folder) = hint.and_then(|h| h.folder) {
        folder
    } else if root == staging {
        info.name.clone()
    } else {
        file_name(&root).unwrap_or_else(|| info.name.clone())
    };
    let parent = match kind {
        PackageKind::Dependency => voicebanks.join(DEPENDENCIES),
        _ => voicebanks.to_path_buf(),
    };
    std::fs::create_dir_all(&parent)?;
    let folder = if kind == PackageKind::Dependency {
        sanitize(&info.name)
    } else {
        sanitize(&folder)
    };
    let mut dest = parent.join(&folder);
    let mut n = 2;
    while dest.exists() {
        // A dependency of the same name is the same dependency, newer.
        if kind == PackageKind::Dependency {
            std::fs::remove_dir_all(&dest)?;
            break;
        }
        dest = parent.join(format!("{folder} ({n})"));
        n += 1;
    }
    std::fs::rename(&root, &dest).or_else(|_| copy_dir(&root, &dest))?;
    let terms = read_terms(&dest);
    let info = read_info(&dest, kind);
    Ok(Installed {
        id: file_name(&dest).unwrap_or_default(),
        kind,
        name: info.name.clone(),
        root: dest.clone(),
        path: dest,
        terms,
        info,
    })
}

fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// A folder name anybody's file system takes.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c))
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        "Voicebank".into()
    } else if cleaned.eq_ignore_ascii_case(DEPENDENCIES) {
        format!("{cleaned} bank")
    } else {
        cleaned
    }
}

/// What UTAU's `install.txt` says (`type=voiceset`, `folder=`,
/// `contentsdir=`), from the shallowest one in the package.
struct InstallTxt {
    folder: Option<String>,
    /// The bank's files, relative to the install.txt's own folder, checked.
    contents: Option<PathBuf>,
}

fn install_txt(staging: &Path) -> Option<InstallTxt> {
    let file = walk(staging, 2)
        .into_iter()
        .find(|f| named(f, &["install.txt"]))?;
    let text = read(&file)?;
    if ini_value(&text, "type").is_some_and(|t| !t.eq_ignore_ascii_case("voiceset")) {
        return None;
    }
    let base = file.parent()?.strip_prefix(staging).ok()?.to_path_buf();
    let contents = ini_value(&text, "contentsdir")
        .and_then(|c| safe_path(&c).ok().flatten())
        .map(|c| base.join(c));
    // The last part of a path given as the folder; sanitized like any other.
    let folder = ini_value(&text, "folder").and_then(|f| {
        f.replace('\\', "/")
            .rsplit('/')
            .find(|p| !p.is_empty())
            .map(sanitize)
    });
    Some(InstallTxt { folder, contents })
}

/// What an archive is, by its first bytes: a `.uar` is a zip, and a `.7z`
/// saved as `.zip` happens.
fn unpack(source: &Path, into: &Path) -> Result<(), InstallError> {
    let mut magic = [0u8; 8];
    let n = {
        use std::io::Read;
        let mut f = std::fs::File::open(source)?;
        f.read(&mut magic)?
    };
    let magic = &magic[..n];
    if magic.starts_with(b"7z\xBC\xAF\x27\x1C") {
        un7z(source, into)
    } else if magic.starts_with(b"Rar!") {
        Err(InstallError::Archive(
            "this is a RAR archive, which Fontelle cannot unpack; extract it with \
             another tool and drop the folder in instead"
                .into(),
        ))
    } else {
        unzip(source, into)
    }
}

fn un7z(source: &Path, into: &Path) -> Result<(), InstallError> {
    use sevenz_rust2::{ArchiveReader, Password};
    let archive_error = |e: sevenz_rust2::Error| InstallError::Archive(e.to_string());
    let mut reader = ArchiveReader::open(source, Password::empty()).map_err(archive_error)?;
    // Every name is checked before anything is written. 7z names are
    // UTF-16 in the archive, so there is no encoding to guess.
    for entry in &reader.archive().files {
        if let Err(name) = safe_path(&entry.name) {
            return Err(InstallError::Unsafe(name));
        }
    }
    let mut failed: Option<InstallError> = None;
    reader
        .for_each_entries(|entry, data| {
            let Ok(Some(path)) = safe_path(&entry.name) else {
                return Ok(true);
            };
            let to = into.join(path);
            let result = (|| -> std::io::Result<()> {
                if entry.is_directory {
                    return std::fs::create_dir_all(&to);
                }
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out = std::fs::File::create(&to)?;
                std::io::copy(data, &mut out)?;
                Ok(())
            })();
            match result {
                Ok(()) => Ok(true),
                Err(e) => {
                    failed = Some(e.into());
                    Ok(false)
                }
            }
        })
        .map_err(archive_error)?;
    failed.map_or(Ok(()), Err)
}

fn unzip(source: &Path, into: &Path) -> Result<(), InstallError> {
    let mut zip = ZipArchive::open(source).map_err(InstallError::Archive)?;
    // Every name is checked before anything is written.
    let mut plan = Vec::new();
    for (i, entry) in zip.entries.iter().enumerate() {
        match safe_path(&entry.name) {
            Err(name) => return Err(InstallError::Unsafe(name)),
            Ok(None) => {}
            Ok(Some(path)) => plan.push((i, path, entry.is_dir)),
        }
    }
    for (i, path, is_dir) in plan {
        let to = into.join(&path);
        if is_dir {
            std::fs::create_dir_all(&to)?;
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        zip.extract(i, &to).map_err(InstallError::Archive)?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Every file under `dir`, at most `depth` folders down, shallowest first.
fn walk(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..=depth {
        let mut next = Vec::new();
        for d in &level {
            let Ok(entries) = std::fs::read_dir(d) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    next.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        level = next;
    }
    out
}

fn named(path: &Path, names: &[&str]) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| names.iter().any(|w| n.eq_ignore_ascii_case(w)))
}

/// What kind of package is under `dir`, and where its root is.
fn find_root(dir: &Path, oudep: bool) -> Option<(PackageKind, PathBuf)> {
    let files = walk(dir, 6);
    let parent = |p: &PathBuf| p.parent().map(Path::to_path_buf);
    // A dependency first when the package says it is one.
    let dep = files
        .iter()
        .find(|f| named(f, &["vocoder.yaml", "oudep.yaml"]))
        .and_then(parent);
    if oudep && let Some(root) = dep.clone() {
        return Some((PackageKind::Dependency, root));
    }
    if let Some(root) = files
        .iter()
        .find(|f| named(f, &["dsconfig.yaml"]))
        .and_then(parent)
    {
        return Some((PackageKind::DiffSinger, root));
    }
    let otos: Vec<&PathBuf> = files.iter().filter(|f| named(f, &["oto.ini"])).collect();
    if !otos.is_empty() {
        // The shallowest character file with an oto.ini in or under it.
        if let Some(root) = files
            .iter()
            .filter(|f| named(f, &["character.txt", "character.yaml"]))
            .filter_map(parent)
            .find(|d| otos.iter().any(|o| o.starts_with(d)))
        {
            return Some((PackageKind::Utau, root));
        }
        // Else the folder all the oto.ini files share.
        let mut root = parent(otos[0])?;
        while !otos.iter().all(|o| o.starts_with(&root)) {
            root = root.parent()?.to_path_buf();
        }
        // A pitch folder's parent is the bank (A3/, C4/ … beside each other).
        if otos.len() == 1 && root != dir && root.parent() != Some(dir) {
            // One oto.ini in a lone folder: that folder is the bank.
        }
        return Some((PackageKind::Utau, root));
    }
    dep.map(|root| (PackageKind::Dependency, root))
}

/// `key: value` lines of a simple YAML file (top level, scalars only).
fn yaml_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            if line.starts_with(' ') || line.starts_with('\t') {
                return None;
            }
            let (k, v) = line.split_once(':')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').trim_matches('\'').to_string())
        })
        .filter(|v| !v.is_empty())
}

/// `key=value` lines of a UTAU `character.txt`.
fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            k.trim()
                .eq_ignore_ascii_case(key)
                .then(|| v.trim().to_string())
        })
        .filter(|v| !v.is_empty())
}

fn read(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| decode_text(&b))
}

/// What a bank at `root` says about itself.
pub fn read_info(root: &Path, kind: PackageKind) -> BankInfo {
    let mut info = BankInfo {
        name: file_name(root).unwrap_or_default(),
        ..BankInfo::default()
    };
    let existing = |name: Option<String>| {
        name.map(|n| root.join(n.replace('\\', "/")))
            .filter(|p| p.is_file())
    };
    if kind == PackageKind::Dependency {
        for file in ["vocoder.yaml", "oudep.yaml"] {
            if let Some(name) = read(&root.join(file)).and_then(|t| yaml_value(&t, "name")) {
                info.name = name;
                break;
            }
        }
        return info;
    }
    if let Some(text) = read(&root.join("character.txt")) {
        if let Some(name) = ini_value(&text, "name") {
            info.name = name;
        }
        info.author = ini_value(&text, "author");
        info.web = ini_value(&text, "web");
        info.portrait = existing(ini_value(&text, "image"));
        info.sample = existing(ini_value(&text, "sample"));
    }
    if let Some(text) = read(&root.join("character.yaml")) {
        if let Some(name) = yaml_value(&text, "name") {
            info.name = name;
        }
        info.author = yaml_value(&text, "author").or(info.author);
        info.web = yaml_value(&text, "web").or(info.web);
        // `image` is the bank's icon; `portrait` (OpenUtau's) the large
        // picture behind the roll — a face for a card only when there is
        // no icon.
        info.portrait = existing(yaml_value(&text, "image"))
            .or(info.portrait)
            .or_else(|| existing(yaml_value(&text, "portrait")));
        info.sample = existing(yaml_value(&text, "sample")).or(info.sample);
        if let Some(list) = yaml_value(&text, "languages") {
            info.languages = list
                .trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|l| l.trim().trim_matches('"').to_string())
                .filter(|l| !l.is_empty())
                .collect();
        }
    }
    info
}

/// The terms its author wrote: the first readme-like file at the bank's
/// root, by name.
pub fn read_terms(root: &Path) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let rank = |p: &PathBuf| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let text = name.ends_with(".txt") || name.ends_with(".md") || !name.contains('.');
        if !text {
            return None;
        }
        if name.contains("規約") || name.starts_with("terms") || name.contains("利用") {
            Some(0)
        } else if name.starts_with("licen") {
            Some(1)
        } else if name.starts_with("readme")
            || name.starts_with("read me")
            || name.contains("お読み")
        {
            Some(2)
        } else {
            None
        }
    };
    let best = files
        .iter()
        .filter_map(|p| rank(p).map(|r| (r, p)))
        .min_by_key(|(r, _)| *r)?
        .1;
    let text = read(best)?;
    let text = text.trim().to_string();
    (!text.is_empty()).then(|| {
        // Enough to read and agree to; the whole file is on disk.
        text.chars().take(20_000).collect()
    })
}

fn terms_hash(terms: Option<&str>) -> String {
    let mut h = twox_hash::XxHash64::with_seed(7);
    h.write(terms.unwrap_or("").as_bytes());
    format!("{:016x}", h.finish())
}

/// Whether the bank's current terms were acknowledged (a bank with none
/// has nothing to acknowledge).
pub fn terms_acknowledged(bank: &Path) -> bool {
    let terms = read_terms(bank);
    if terms.is_none() {
        return true;
    }
    std::fs::read_to_string(bank.join(ACKNOWLEDGED))
        .is_ok_and(|seen| seen.trim() == terms_hash(terms.as_deref()))
}

/// Records that the bank's current terms were read and agreed to.
pub fn acknowledge_terms(bank: &Path) -> std::io::Result<()> {
    std::fs::write(
        bank.join(ACKNOWLEDGED),
        terms_hash(read_terms(bank).as_deref()),
    )
}

// --------------------------------------------------------------- listing

fn describe(path: &Path) -> Option<Installed> {
    // Described from the folder with its own files, as the runtimes that
    // sing it read it: a package whose files are a folder down (Canary's,
    // Teto's) keeps its name and picture there. *"the picture is missing
    // on some in the installed section"*.
    let (kind, root) = find_root(path, false)?;
    let info = read_info(&root, kind);
    Some(Installed {
        id: file_name(path)?,
        kind,
        name: info.name.clone(),
        path: path.to_path_buf(),
        root,
        terms: read_terms(path),
        info,
    })
}

fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| file_name(p).is_some_and(|n| !n.starts_with('.')))
        .collect();
    out.sort();
    out
}

/// Every bank installed in `voicebanks`, by folder name.
pub fn list(voicebanks: &Path) -> Vec<Installed> {
    folders(voicebanks)
        .into_iter()
        .filter(|p| file_name(p).as_deref() != Some(DEPENDENCIES))
        .filter_map(|p| describe(&p))
        .filter(|b| b.kind != PackageKind::Dependency)
        .collect()
}

/// Every dependency installed beside the banks.
pub fn dependencies(voicebanks: &Path) -> Vec<Installed> {
    folders(&voicebanks.join(DEPENDENCIES))
        .into_iter()
        .map(|path| {
            let info = read_info(&path, PackageKind::Dependency);
            Installed {
                id: file_name(&path).unwrap_or_default(),
                kind: PackageKind::Dependency,
                name: info.name.clone(),
                terms: read_terms(&path),
                root: path.clone(),
                path,
                info,
            }
        })
        .collect()
}

/// Which of `installed` a built-in runtime's voice `id` is (`image` its
/// picture, when it has one).
pub fn bank_for_voice<'a>(
    installed: &'a [Installed],
    id: &str,
    image: Option<&Path>,
) -> Option<&'a Installed> {
    // Its picture is in the bank's folder, wherever under it.
    if let Some(image) = image
        && let Some(bank) = installed.iter().find(|b| image.starts_with(&b.path))
    {
        return Some(bank);
    }
    if let Some(bank) = installed.iter().find(|b| b.id == id) {
        return Some(bank);
    }
    // A runtime names a bank by the folder with its files (UTAU's with
    // `utau/` before it, and that folder's parent in brackets when two
    // share a name).
    let named = id.strip_prefix("utau/").unwrap_or(id);
    let root_is = |b: &Installed, name: &str| file_name(&b.root).as_deref() == Some(name);
    if let Some(bank) = installed.iter().find(|b| root_is(b, named)) {
        return Some(bank);
    }
    let (name, parent) = named.strip_suffix(')')?.rsplit_once(" (")?;
    installed.iter().find(|b| {
        root_is(b, name) && b.root.parent().and_then(file_name).as_deref() == Some(parent)
    })
}

/// Removes the bank whose folder is `id` (or the dependency, as
/// `Dependencies/<name>`).
pub fn uninstall(voicebanks: &Path, id: &str) -> Result<(), String> {
    let path = match safe_path(id) {
        Ok(Some(p)) if p.components().count() <= 2 => voicebanks.join(p),
        _ => return Err(format!("{id:?} is not a voicebank here")),
    };
    if path == voicebanks.join(DEPENDENCIES) || !path.is_dir() {
        return Err(format!("{id:?} is not installed"));
    }
    std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
}

/// What to call each of `entries` — `(name, kind, folder)` — so no two
/// read alike: its own name, unless another has the same one (Teto's
/// 単独音 and 統合 libraries both call themselves 重音テト音声ライブラリー);
/// then the name with its kind in brackets when the kind alone tells the
/// ones of that name apart, else with its folder.
pub fn told_apart(entries: &[(&str, &str, &str)]) -> Vec<String> {
    entries
        .iter()
        .map(|(name, kind, folder)| {
            let name = name.trim();
            let same: Vec<&(&str, &str, &str)> =
                entries.iter().filter(|e| e.0.trim() == name).collect();
            if same.len() < 2 {
                return name.to_string();
            }
            let kinds_differ = same
                .iter()
                .all(|e| same.iter().filter(|o| o.1 == e.1).count() == 1);
            let what = if kinds_differ { kind } else { folder };
            format!("{name} ({what})")
        })
        .collect()
}
