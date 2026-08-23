use std::path::PathBuf;

use crate::AssetId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AssetKind {
    Sf2,
    Sf3,
    Sfz,
    Sample,
}

/// A reference to sample/soundfont data on disk (TDD §17.4). Referenced by
/// `fontelle-core::Layer::Source`, resolved and imported by `fontelle-assets`, and
/// tracked in the document's `AssetTable` by `fontelle-model` — living here keeps
/// `fontelle-core` from depending on either of those crates (INVARIANT 4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssetRef {
    pub id: AssetId,
    /// Absolute, or project-relative if the asset was copied in on import.
    pub path: PathBuf,
    /// xxhash of the first 1MB + file size — cheap enough to compute on every load,
    /// used to relink broken references (TDD §17.4).
    pub content_hash: u64,
    pub size: u64,
    pub kind: AssetKind,
}
