//! The Supporter Pack: content for people who want to support Fontelle —
//! never features (hub card 0429, `docs/steam.md` §5).
//!
//! On Steam it is a DLC whose depot installs a `supporter-pack/` folder into
//! the app's install folder. Anybody else drops the same folder into
//! Fontelle's data folder. Either way the free program works identically
//! without it: a pack adds read-only folders of content, and nothing in the
//! program asks whether one is there to decide what it may do. Its manifest
//! is read for a name and a version and nothing else — a field a later
//! manifest adds is ignored, never obeyed.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The pack's folder, wherever it is looked for.
pub const PACK_FOLDER: &str = "supporter-pack";

/// What makes a folder a pack rather than somebody's folder of soundfonts.
pub const MANIFEST: &str = "fontelle-pack.json";

/// The newest manifest format this build reads. A newer one is not guessed
/// at: it may describe a shape this build would read wrongly.
pub const MANIFEST_FORMAT: u32 = 1;

/// A pack found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupporterPack {
    /// The pack's folder.
    pub root: PathBuf,
    /// What the soundfont bank calls its shelf.
    pub name: String,
    pub version: String,
}

#[derive(Deserialize)]
struct Manifest {
    format: u32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

impl SupporterPack {
    /// The pack at `root`, if `root` holds a manifest this build reads.
    pub fn read(root: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(root.join(MANIFEST)).ok()?;
        let manifest: Manifest = serde_json::from_str(&text).ok()?;
        if manifest.format == 0 || manifest.format > MANIFEST_FORMAT {
            return None;
        }
        Some(Self {
            root: root.to_path_buf(),
            name: manifest
                .name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "Supporter Pack".to_string()),
            version: manifest.version.unwrap_or_default(),
        })
    }

    /// Its `.sf2` files, which the soundfont bank lists read-only.
    pub fn soundfonts(&self) -> PathBuf {
        self.root.join("soundfonts")
    }

    /// Its presets. The shape is reserved; nothing reads it yet.
    pub fn presets(&self) -> PathBuf {
        self.root.join("presets")
    }

    /// Its project templates. The shape is reserved; nothing reads it yet.
    pub fn templates(&self) -> PathBuf {
        self.root.join("templates")
    }
}

/// Where a pack may be, in order: beside the install (where Steam puts a
/// DLC's files — beside a macOS app, never inside it, where it would break
/// the bundle's signature), then Fontelle's data folder.
pub fn pack_places(exe: Option<&Path>, data_dir: Option<&Path>) -> Vec<PathBuf> {
    exe.and_then(crate::distribution::install_dir)
        .into_iter()
        .chain(data_dir.map(Path::to_path_buf))
        .map(|dir| dir.join(PACK_FOLDER))
        .collect()
}

/// The packs at `places`, each folder once however it was reached.
pub fn find_packs(places: &[PathBuf]) -> Vec<SupporterPack> {
    let mut seen = Vec::new();
    let mut packs = Vec::new();
    for place in places {
        let key = place.canonicalize().unwrap_or_else(|_| place.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        packs.extend(SupporterPack::read(place));
    }
    packs
}

/// This process's packs: beside the program it was launched as, and in the
/// data folder.
pub fn installed_packs() -> Vec<SupporterPack> {
    let exe = crate::updates::launched_exe();
    let data = crate::settings::Settings::data_dir();
    find_packs(&pack_places(exe.as_deref(), data.as_deref()))
}
