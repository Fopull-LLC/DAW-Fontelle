use fontelle_types::{ChannelId, ClipId, LaneId, PrefabId, Tick};
use slotmap::SlotMap;

use crate::asset_table::AssetTable;
use crate::channel::Channel;
use crate::clip::Clip;
use crate::lane::Lane;
use crate::mixer::Mixer;
use crate::prefab::Prefab;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub created: String, // ISO-8601; kept as a plain string to avoid a chrono dep here
    pub app_version: String,
    pub format_version: u32,
}

/// Piecewise tick -> BPM function with a cached prefix-sum table, so both
/// directions of tick/sample conversion are O(log n) (TDD §6.2). Automatable —
/// the tempo map is itself a compiled artefact of tempo automation, not an
/// independent structure kept in sync by hand.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TempoMap {
    // Segment list + prefix-sum cache land with the sequencer compiler (M3).
}

impl TempoMap {
    pub fn tick_to_sample(&self, _tick: Tick) -> fontelle_types::Sample {
        todo!("integrate over tempo segments, TDD §6.2")
    }

    pub fn sample_to_tick(&self, _sample: fontelle_types::Sample) -> Tick {
        todo!("inverse of tick_to_sample via the same prefix-sum table")
    }

    pub fn tempo_at(&self, _tick: Tick) -> f64 {
        todo!("BPM at a tick, accounting for ramp segments")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Marker {
    pub name: String,
    pub tick: Tick,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ViewState {
    pub zoom: f32,
    pub scroll: f32,
}

/// The whole document (TDD §10.1). Owned exclusively by the model thread; every
/// mutation goes through a `Command` (INVARIANT 9).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub meta: ProjectMeta,
    pub tempo_map: TempoMap,
    pub channels: SlotMap<ChannelId, Channel>,
    pub mixer: Mixer,
    /// Visual only — TDD §10.3.
    pub lanes: Vec<Lane>,
    #[serde(skip)]
    pub lane_ids: Vec<LaneId>,
    pub clips: SlotMap<ClipId, Clip>,
    pub prefabs: SlotMap<PrefabId, Prefab>,
    pub assets: AssetTable,
    pub markers: Vec<Marker>,
    pub view_state: ViewState,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            meta: ProjectMeta {
                name: name.into(),
                created: String::new(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                format_version: 0,
            },
            tempo_map: TempoMap::default(),
            channels: SlotMap::default(),
            mixer: Mixer::default(),
            lanes: Vec::new(),
            lane_ids: Vec::new(),
            clips: SlotMap::default(),
            prefabs: SlotMap::default(),
            assets: AssetTable::default(),
            markers: Vec::new(),
            view_state: ViewState::default(),
        }
    }
}
