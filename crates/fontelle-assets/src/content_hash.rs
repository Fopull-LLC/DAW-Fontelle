//! What a file's bytes are called (`docs/collab-plan.md` §7.1).
//!
//! **The low 64 bits of the file's SHA-256** — the last eight bytes of the
//! digest, read big-endian — for every kind of file a song uses. One name for
//! a file on disk, in `AssetRef::content_hash`, in a shared session's list of
//! what a song needs, and in the file name a collected copy is kept under
//! (`assets/<the whole digest in hex>.<ext>`), so a file already on a machine
//! is found by what is in it and never sent twice.
//!
//! Hashing a 325 MB soundfont takes about a second, and the browser imports a
//! preset on every click, so a file is hashed **once per version**: the answer
//! is kept against its path, size and modification time, in memory and — once
//! [`set_cache_file`] has named one — in a small JSON file beside the
//! settings, so a bank is hashed once per machine rather than once per run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sha2::Digest;

/// A file's name by its contents.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileHash {
    /// What `AssetRef::content_hash` holds.
    pub low: u64,
    /// The whole digest, for a file name.
    pub hex: String,
    pub size: u64,
}

/// The hash of `bytes`.
pub fn hash_bytes(bytes: &[u8]) -> FileHash {
    let digest = sha2::Sha256::digest(bytes);
    FileHash {
        low: u64::from_be_bytes(digest[24..32].try_into().expect("a SHA-256 is 32 bytes")),
        hex: digest.iter().map(|b| format!("{b:02x}")).collect(),
        size: bytes.len() as u64,
    }
}

/// The hash of the file at `path`, from the cache when the file has not
/// changed since it was last hashed.
pub fn hash_file(path: &Path) -> std::io::Result<FileHash> {
    let meta = std::fs::metadata(path)?;
    let stamp = Stamp::of(&meta);
    if let Some(known) = cache().lock().expect("hash cache").get(path, &stamp) {
        return Ok(known);
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let digest = hasher.finalize();
    let hash = FileHash {
        low: u64::from_be_bytes(digest[24..32].try_into().expect("a SHA-256 is 32 bytes")),
        hex: digest.iter().map(|b| format!("{b:02x}")).collect(),
        size: meta.len(),
    };
    cache()
        .lock()
        .expect("hash cache")
        .put(path, stamp, hash.clone());
    Ok(hash)
}

/// [`hash_file`] for a file whose bytes are already in hand — what an import
/// that read the whole file uses, so nothing is read twice.
pub fn hash_file_bytes(path: &Path, bytes: &[u8]) -> FileHash {
    let stamp = std::fs::metadata(path).map(|meta| Stamp::of(&meta));
    if let Ok(stamp) = &stamp
        && let Some(known) = cache().lock().expect("hash cache").get(path, stamp)
    {
        return known;
    }
    let hash = hash_bytes(bytes);
    if let Ok(stamp) = stamp {
        cache()
            .lock()
            .expect("hash cache")
            .put(path, stamp, hash.clone());
    }
    hash
}

/// The first file under `dirs` (and their folders) whose contents are
/// `hash` and whose size is `size`.
///
/// The size is checked first, so a search through a bank of a hundred
/// soundfonts hashes only the ones that could possibly be the file.
pub fn find_by_hash(dirs: &[PathBuf], hash: u64, size: u64) -> Option<PathBuf> {
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(listing) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in listing.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.len() == size && hash_file(&path).is_ok_and(|found| found.low == hash) {
                return Some(path);
            }
        }
    }
    None
}

/// Keeps the cache in `path` from now on, reading what is already there.
///
/// A file that will not read is a cache that starts empty, not an error:
/// the worst it costs is hashing a bank again.
pub fn set_cache_file(path: PathBuf) {
    let mut cache = cache().lock().expect("hash cache");
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(entries) = serde_json::from_str::<Vec<Entry>>(&text)
    {
        for entry in entries {
            cache
                .known
                .insert(entry.path.clone(), (entry.stamp, entry.hash));
        }
    }
    cache.file = Some(path);
}

/// When a file was last changed, and how big it was: a file whose stamp has
/// moved is hashed again.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Stamp {
    size: u64,
    modified_nanos: u128,
}

impl Stamp {
    fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            size: meta.len(),
            modified_nanos: meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos()),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    path: PathBuf,
    stamp: Stamp,
    hash: FileHash,
}

#[derive(Default)]
struct Cache {
    known: HashMap<PathBuf, (Stamp, FileHash)>,
    file: Option<PathBuf>,
}

impl Cache {
    fn get(&self, path: &Path, stamp: &Stamp) -> Option<FileHash> {
        self.known
            .get(path)
            .filter(|(known, _)| known == stamp)
            .map(|(_, hash)| hash.clone())
    }

    fn put(&mut self, path: &Path, stamp: Stamp, hash: FileHash) {
        self.known.insert(path.to_path_buf(), (stamp, hash));
        let Some(file) = &self.file else {
            return;
        };
        let entries: Vec<Entry> = self
            .known
            .iter()
            .map(|(path, (stamp, hash))| Entry {
                path: path.clone(),
                stamp: stamp.clone(),
                hash: hash.clone(),
            })
            .collect();
        // Written beside and renamed over, so a crash mid-write leaves the
        // old cache rather than half of one; a failure costs a re-hash.
        let temp = file.with_extension("json.tmp");
        if let Ok(text) = serde_json::to_string(&entries)
            && std::fs::write(&temp, text).is_ok()
        {
            let _ = std::fs::rename(&temp, file);
        }
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_the_low_end_of_the_sha256() {
        // SHA-256 of "abc" is ba7816bf…f20015ad; its last eight bytes are
        // b410ff61f20015ad.
        let hash = hash_bytes(b"abc");
        assert_eq!(hash.low, 0xb410_ff61_f200_15ad);
        assert!(hash.hex.starts_with("ba7816bf"));
        assert!(hash.hex.ends_with("b410ff61f20015ad"));
        assert_eq!(hash.size, 3);
    }

    #[test]
    fn a_file_is_found_by_what_is_in_it_whatever_it_is_called() {
        let dir = std::env::temp_dir().join(format!("fontelle-hash-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("deeper")).unwrap();
        std::fs::write(dir.join("deeper").join("renamed.sf2"), b"the same bytes").unwrap();
        std::fs::write(dir.join("other.sf2"), b"different bytes").unwrap();
        let wanted = hash_bytes(b"the same bytes");
        assert_eq!(
            find_by_hash(std::slice::from_ref(&dir), wanted.low, wanted.size),
            Some(dir.join("deeper").join("renamed.sf2"))
        );
        assert_eq!(
            find_by_hash(std::slice::from_ref(&dir), 1, wanted.size),
            None
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
