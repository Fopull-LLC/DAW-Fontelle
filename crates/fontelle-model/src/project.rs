use fontelle_types::{ChannelId, ClipId, LaneId, PPQN, PrefabId, Sample, Tick};
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
///
/// **Scope cut:** only a single constant-tempo segment is implemented right
/// now — `bpm` never changes across the project. The full piecewise
/// segment list + prefix-sum cache (ramps, a time-signature track) is real
/// work that lands with M3's timeline; this is enough for `fontelle-sequencer`
/// to convert a note's tick position to samples honestly (not by ad-hoc
/// arithmetic elsewhere) for the M0 vertical slice. See `PROGRESS.md`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TempoMap {
    bpm: f64,
    /// Not project data — the runtime audio device's rate. Kept here (rather
    /// than threaded through every call site) because both conversion
    /// directions need it and this is the one piece of state both share.
    /// `#[serde(skip)]`: never belongs in a saved project.
    #[serde(skip, default = "default_sample_rate_hz")]
    sample_rate_hz: f64,
}

fn default_sample_rate_hz() -> f64 {
    48_000.0
}

impl TempoMap {
    pub fn new(bpm: f64, sample_rate_hz: f64) -> Self {
        Self {
            bpm,
            sample_rate_hz,
        }
    }

    pub fn tick_to_sample(&self, tick: Tick) -> Sample {
        let samples_per_tick = self.sample_rate_hz * 60.0 / (self.bpm * PPQN as f64);
        (tick as f64 * samples_per_tick).round() as Sample
    }

    pub fn sample_to_tick(&self, sample: Sample) -> Tick {
        let ticks_per_sample = self.bpm * PPQN as f64 / (self.sample_rate_hz * 60.0);
        (sample as f64 * ticks_per_sample).round() as Tick
    }

    pub fn tempo_at(&self, _tick: Tick) -> f64 {
        self.bpm
    }
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::new(120.0, default_sample_rate_hz())
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
    pub lanes: SlotMap<LaneId, Lane>,
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
            lanes: SlotMap::default(),
            clips: SlotMap::default(),
            prefabs: SlotMap::default(),
            assets: AssetTable::default(),
            markers: Vec::new(),
            view_state: ViewState::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_note_at_120bpm_48khz_is_exactly_half_a_second() {
        let map = TempoMap::new(120.0, 48_000.0);
        assert_eq!(map.tick_to_sample(PPQN), 24_000);
    }

    #[test]
    fn tick_zero_is_sample_zero() {
        let map = TempoMap::new(120.0, 48_000.0);
        assert_eq!(map.tick_to_sample(0), 0);
    }

    #[test]
    fn round_trip_is_exact_across_many_ticks_at_120bpm_48khz() {
        // At this particular (bpm, sample_rate) pair, samples-per-tick is an
        // exact integer (25), so the round trip has no rounding error to
        // paper over — a stronger claim than "close enough" property tests
        // usually get to make.
        let map = TempoMap::new(120.0, 48_000.0);
        for tick in (0..100_000).step_by(37) {
            assert_eq!(map.sample_to_tick(map.tick_to_sample(tick)), tick);
        }
    }

    #[test]
    fn tempo_at_returns_the_constant_bpm_regardless_of_tick() {
        let map = TempoMap::new(140.0, 44_100.0);
        assert_eq!(map.tempo_at(0), 140.0);
        assert_eq!(map.tempo_at(1_000_000), 140.0);
    }
}
