use fontelle_types::{ClipId, LaneId, Tick};

use crate::automation::AutomationData;
use crate::note::NoteData;
use crate::prefab::PrefabLink;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioClipData {
    // Fleshed out in TDD §15 (M6). Left as a marker variant until then so
    // `ClipSource` has its full v1 shape from commit one.
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ClipSource {
    Notes(NoteData),
    Automation(AutomationData),
    Audio(AudioClipData),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub lane: LaneId,
    pub start: Tick,
    pub length: Tick,
    pub source: ClipSource,
    pub prefab_link: Option<PrefabLink>,
    /// Overrides the source's own colour.
    pub color: Option<[u8; 4]>,
    pub muted: bool,
}

pub type ClipMap = crate::arena::Arena<ClipId, Clip>;
