use std::path::PathBuf;

use crate::AssetId;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum AssetKind {
    Sf2,
    Sf3,
    Sfz,
    Sample,
}

/// A reference to sample/soundfont data on disk (TDD §17.4). Referenced by
/// `fontelle-core::Layer::Source`, resolved and imported by `fontelle-assets`, and
/// found in a song by `fontelle_model::Project::files` — living here keeps
/// `fontelle-core` from depending on either of those crates (INVARIANT 4).
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct AssetRef {
    pub id: AssetId,
    /// Absolute, or project-relative if the asset was copied in on import.
    pub path: PathBuf,
    /// The low 64 bits of the whole file's SHA-256
    /// (`fontelle_assets::content_hash`), used to relink broken references
    /// (TDD §17.4) and to find a shared song's files on another machine.
    /// Songs saved before that change carry an older xxhash here, which no
    /// file will match.
    pub content_hash: u64,
    pub size: u64,
    pub kind: AssetKind,
}
