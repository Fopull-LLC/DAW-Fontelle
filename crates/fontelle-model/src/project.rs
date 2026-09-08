use fontelle_types::{ChannelId, ClipId, LaneId, PPQN, PrefabId, Sample, Tick};

use crate::arena::Arena;

use crate::asset_table::AssetTable;
use crate::channel::Channel;
use crate::clip::{Clip, ClipSource};
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

/// One constant-tempo stretch of the timeline, in force from `start_tick`
/// until the next segment begins.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TempoSegment {
    pub start_tick: Tick,
    pub bpm: f64,
}

/// The slowest tempo the map will hold.
///
/// A malformed file can carry a tempo of zero, and dividing by it gives an
/// infinite sample position that poisons every conversion after it — including
/// the ones for notes before the bad segment, once a prefix sum has picked it
/// up. Clamping is the only behaviour that keeps a bad file playable.
const MIN_BPM: f64 = 0.01;

/// Piecewise tick -> BPM function with a cached prefix-sum table, so both
/// directions of tick/sample conversion are O(log n) (TDD §6.2). Automatable —
/// the tempo map is itself a compiled artefact of tempo automation, not an
/// independent structure kept in sync by hand.
///
/// **Scope cut:** every segment is constant. TDD §6.2 also asks for
/// interpolated ramps, and those are a bounded addition rather than a
/// redesign: with BPM linear in tick the elapsed time over a ramp is
/// `(1/m) * ln(b1 / b0)` where `m` is the slope in BPM per tick, and its
/// inverse is `(b0/m) * (exp(m * t) - 1)`. They are not built because nothing
/// can create one yet — MIDI carries only constant tempo events, and tempo
/// automation (§12) does not exist. A time-signature track is likewise still
/// missing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(from = "TempoMapData", into = "TempoMapData")]
pub struct TempoMap {
    /// Sorted by `start_tick`, always non-empty, and the first always starts
    /// at or before tick 0 — see `from_segments`.
    segments: Vec<TempoSegment>,
    /// Seconds elapsed at each segment's `start_tick`, so a conversion is a
    /// binary search plus one multiply rather than a walk from the beginning.
    ///
    /// In seconds rather than samples because the sample rate is a runtime
    /// property: changing it rescales every answer without rebuilding this.
    prefix_seconds: Vec<f64>,
    /// Not project data — the runtime audio device's rate. Kept here (rather
    /// than threaded through every call site) because both conversion
    /// directions need it and this is the one piece of state both share.
    sample_rate_hz: f64,
}

/// What a `TempoMap` actually saves: its segments, and nothing derived.
///
/// The prefix table and the sample rate are both reconstructed on load — one
/// because a cache that can disagree with its source is a bug waiting to
/// happen, the other because it belongs to the audio device rather than to the
/// document.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TempoMapData {
    segments: Vec<TempoSegment>,
}

impl From<TempoMapData> for TempoMap {
    fn from(data: TempoMapData) -> Self {
        Self::from_segments(data.segments, default_sample_rate_hz())
    }
}

impl From<TempoMap> for TempoMapData {
    fn from(map: TempoMap) -> Self {
        Self {
            segments: map.segments,
        }
    }
}

fn default_sample_rate_hz() -> f64 {
    48_000.0
}

/// How long one tick lasts at `bpm`.
fn seconds_per_tick(bpm: f64) -> f64 {
    60.0 / (bpm * PPQN as f64)
}

impl TempoMap {
    /// A map with one constant tempo for the whole project.
    pub fn new(bpm: f64, sample_rate_hz: f64) -> Self {
        Self::from_segments(vec![TempoSegment { start_tick: 0, bpm }], sample_rate_hz)
    }

    /// Builds a map from tempo changes in any order.
    ///
    /// They arrive in file order rather than musical order — a MIDI file may
    /// put tempo events in any track — so they are sorted here, and where two
    /// land on the same tick the last one wins, which is what a sequencer
    /// writing over its own event does. A file with no tempo at tick 0 has its
    /// first segment reach back to cover the opening rather than leaving the
    /// bars before it undefined.
    pub fn from_segments(mut segments: Vec<TempoSegment>, sample_rate_hz: f64) -> Self {
        segments.retain(|s| s.bpm.is_finite());
        for segment in &mut segments {
            segment.bpm = segment.bpm.max(MIN_BPM);
        }
        segments.sort_by_key(|s| s.start_tick);
        segments.dedup_by_key(|s| s.start_tick);

        match segments.first_mut() {
            Some(first) if first.start_tick > 0 => first.start_tick = 0,
            Some(_) => {}
            None => segments.push(TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            }),
        }

        let mut prefix_seconds = Vec::with_capacity(segments.len());
        let mut elapsed = 0.0;
        let mut previous: Option<&TempoSegment> = None;
        for segment in &segments {
            if let Some(previous) = previous {
                elapsed += (segment.start_tick - previous.start_tick) as f64
                    * seconds_per_tick(previous.bpm);
            }
            prefix_seconds.push(elapsed);
            previous = Some(segment);
        }

        Self {
            segments,
            prefix_seconds,
            sample_rate_hz,
        }
    }

    /// The tempo changes, in order. Always at least one, always starting at
    /// tick 0.
    pub fn segments(&self) -> &[TempoSegment] {
        &self.segments
    }

    /// Points the map at a different audio device rate. The prefix table is in
    /// seconds, so nothing has to be rebuilt.
    pub fn set_sample_rate(&mut self, sample_rate_hz: f64) {
        self.sample_rate_hz = sample_rate_hz;
    }

    pub fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }

    /// The index of the segment in force at `tick`. A segment owns its own
    /// start, and ticks before the first one use it too — a count-in runs
    /// backwards at the opening tempo rather than collapsing onto the downbeat.
    fn segment_at(&self, tick: Tick) -> usize {
        match self.segments.binary_search_by_key(&tick, |s| s.start_tick) {
            Ok(index) => index,
            Err(0) => 0,
            Err(index) => index - 1,
        }
    }

    pub fn tick_to_sample(&self, tick: Tick) -> Sample {
        let index = self.segment_at(tick);
        let segment = &self.segments[index];
        let seconds = self.prefix_seconds[index]
            + (tick - segment.start_tick) as f64 * seconds_per_tick(segment.bpm);
        (seconds * self.sample_rate_hz).round() as Sample
    }

    pub fn sample_to_tick(&self, sample: Sample) -> Tick {
        let seconds = sample as f64 / self.sample_rate_hz;
        // The prefix table is ascending, so the same search works on it.
        let index = match self
            .prefix_seconds
            .binary_search_by(|s| s.partial_cmp(&seconds).unwrap_or(std::cmp::Ordering::Less))
        {
            Ok(index) => index,
            Err(0) => 0,
            Err(index) => index - 1,
        };
        let segment = &self.segments[index];
        let into_segment = (seconds - self.prefix_seconds[index]) / seconds_per_tick(segment.bpm);
        segment.start_tick + into_segment.round() as Tick
    }

    pub fn tempo_at(&self, tick: Tick) -> f64 {
        self.segments[self.segment_at(tick)].bpm
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

/// 4/4 — what a project is in until somebody says otherwise, and what a file
/// written before the field existed was in.
fn default_beats_per_bar() -> u32 {
    4
}

/// The whole document (TDD §10.1). Owned exclusively by the model thread; every
/// mutation goes through a `Command` (INVARIANT 9).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub meta: ProjectMeta,
    pub tempo_map: TempoMap,
    /// The time signature's numerator: how many beats there are in a bar.
    ///
    /// **The denominator is always four and is not stored**, because [`PPQN`]
    /// is ticks per *quarter* note — a denominator other than four is a change
    /// to what a tick means in every conversion in this crate, not a document
    /// field. A time-signature *track* (§6.2's remaining gap) is where a piece
    /// that changes metre mid-song goes; this is the one signature the grid,
    /// the bar numbers and the read-out all count by.
    ///
    /// Defaulted rather than required, so a project written before this field
    /// existed opens in the 4/4 it was made in.
    #[serde(default = "default_beats_per_bar")]
    pub beats_per_bar: u32,
    pub channels: Arena<ChannelId, Channel>,
    pub mixer: Mixer,
    /// Visual only — TDD §10.3.
    pub lanes: Arena<LaneId, Lane>,
    pub clips: Arena<ClipId, Clip>,
    pub prefabs: Arena<PrefabId, Prefab>,
    pub assets: AssetTable,
    pub markers: Vec<Marker>,
    /// The loop region, in ticks (TDD §6.3: "loop points are ticks").
    ///
    /// Document state rather than transport state: a project reopens to the
    /// section you were working on. The `Transport` holds the same range in
    /// samples as well, because the RT thread cannot run a `TempoMap` lookup
    /// against a map the model thread may be editing — the two halves are
    /// published together for exactly that reason.
    pub loop_range: Option<(Tick, Tick)>,
    pub view_state: ViewState,
}

impl Project {
    /// The lanes, **in the order the arrangement stacks them**.
    ///
    /// One answer to "what is row 3", here rather than in whatever draws the
    /// arrangement, because a command that moves a row and a canvas that draws
    /// one have to agree about which row is which — and two sorts is one to
    /// forget. See [`Lane::order`](crate::Lane::order).
    ///
    /// The sort is **stable**, which is what keeps a project written before
    /// rows could be ordered stacking the way it always did: every lane in it
    /// carries the same default, so the tie falls back to the arena's own
    /// order.
    pub fn lane_ids(&self) -> Vec<LaneId> {
        let mut ids: Vec<LaneId> = self.lanes.keys().collect();
        ids.sort_by_key(|id| self.lanes.get(*id).map_or(0, |lane| lane.order));
        ids
    }

    /// **What `clip` actually holds.**
    ///
    /// An ordinary clip holds its own notes and this hands them back. A clip
    /// that follows a prefab holds *nothing* — its notes are the prefab's, and
    /// there is exactly one copy of them — so this is the only correct way to
    /// ask a clip what it plays.
    ///
    /// # Read this before reaching for `clip.source`
    ///
    /// `Clip::source` is still there, still holds notes for every ordinary
    /// clip, and reads correctly for all of them. It is wrong **only** for the
    /// clips the prefab feature makes, which is exactly the shape of bug that
    /// ships: the code looks right, the tests that predate prefabs pass, and
    /// the failure is "my prefab instances are silent". The compiler
    /// (`fontelle-sequencer`), the piano roll and the arrangement's captions
    /// all come through here.
    ///
    /// `Cow`, and not a plain reference, because that is the shape the answer
    /// really has: mirroring borrows the prefab's own content and costs
    /// nothing, and an instance with overrides on it has to be *materialised*
    /// before it can be read (see [`prefab::resolve`](crate::resolve)).
    /// Borrowing today and cloning when there is something to apply keeps the
    /// hot path free without the signature having to change later.
    ///
    /// `None` only when `clip` is not in this project. A link naming a prefab
    /// that has gone reads as the clip's own (empty) source — a half-migrated
    /// or hand-edited file is a project that still opens.
    pub fn clip_source(&self, clip: ClipId) -> Option<std::borrow::Cow<'_, ClipSource>> {
        use std::borrow::Cow;
        let clip = self.clips.get(clip)?;
        let Some(link) = &clip.prefab_link else {
            return Some(Cow::Borrowed(&clip.source));
        };
        let Some(prefab) = self.prefabs.get(link.prefab) else {
            return Some(Cow::Borrowed(&clip.source));
        };
        // The common case by far, and the one worth not cloning for: a mirror
        // instance of a prefab that is not itself a variant *is* the prefab's
        // content, with nothing to apply over it.
        if prefab.base.is_none() && link.overrides.props.is_empty() {
            return Some(Cow::Borrowed(&prefab.source));
        }
        match crate::prefab::resolve(&self.prefabs, link.prefab, Some(link)) {
            Some(source) => Some(Cow::Owned(source)),
            None => Some(Cow::Borrowed(&clip.source)),
        }
    }

    /// **Where an edit to `clip` belongs.**
    ///
    /// > *"editing a prefab clip basically works like just editing a normal
    /// > clip except you dont have to only be selecting it in the
    /// > arrangement."*
    ///
    /// An ordinary clip answers itself. A clip that follows a prefab answers
    /// the *prefab*, which is what makes an edit made through one instance
    /// show up in every other one — the thing the whole feature is for.
    ///
    /// One function rather than a rule each caller applies, because "which of
    /// the two did you mean" is a question the piano roll, the arrangement and
    /// the keyboard shortcuts would each get to answer differently otherwise.
    ///
    /// `None` when `clip` is not in this project.
    pub fn note_home(&self, clip: ClipId) -> Option<crate::note::NoteHome> {
        use crate::note::NoteHome;
        let found = self.clips.get(clip)?;
        Some(match &found.prefab_link {
            // A link naming a prefab that has gone edits the clip itself:
            // there is nothing else it could edit, and refusing the edit would
            // make a corrupt file into a clip nobody can fix.
            Some(link) if self.prefabs.contains_key(link.prefab) => NoteHome::Prefab(link.prefab),
            _ => NoteHome::Clip(clip),
        })
    }

    /// The prefabs, in a stable order for a list to draw.
    ///
    /// Arena order, which is insertion order — the order somebody made them
    /// in, which is the order the panel that lists them should show. Its own
    /// function for the same reason [`lane_ids`](Self::lane_ids) is: "prefab
    /// 3" has to mean the same thing to the list and to the click on it.
    pub fn prefab_ids(&self) -> Vec<PrefabId> {
        self.prefabs.keys().collect()
    }

    /// Every clip that follows `prefab`.
    ///
    /// What says whether deleting one is going to change the arrangement, and
    /// what a panel counts to say "used in 4 places".
    pub fn prefab_instances(&self, prefab: PrefabId) -> Vec<ClipId> {
        self.clips
            .iter()
            .filter(|(_, clip)| {
                clip.prefab_link
                    .as_ref()
                    .is_some_and(|link| link.prefab == prefab)
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// A new, empty document — with a master mixer track, because every
    /// project has one (TDD §13.1) and a mixer with nothing to sum into is not
    /// a state any command should have to handle.
    pub fn new(name: impl Into<String>) -> Self {
        let mut mixer = Mixer::default();
        mixer.master = Some(mixer.tracks.insert(crate::mixer::MixerTrack::new("Master")));
        Self {
            meta: ProjectMeta {
                name: name.into(),
                created: String::new(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                format_version: 0,
            },
            tempo_map: TempoMap::default(),
            beats_per_bar: default_beats_per_bar(),
            channels: Arena::default(),
            mixer,
            lanes: Arena::default(),
            clips: Arena::default(),
            prefabs: Arena::default(),
            assets: AssetTable::default(),
            markers: Vec::new(),
            loop_range: None,
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

    // --- Piecewise tempo ----------------------------------------------------

    /// 120 bpm for the first bar, then 60 bpm. At 48 kHz a quarter note is
    /// 24 000 samples at the first tempo and 48 000 at the second, so every
    /// number below is exact.
    fn two_tempo_map() -> TempoMap {
        TempoMap::from_segments(
            vec![
                TempoSegment {
                    start_tick: 0,
                    bpm: 120.0,
                },
                TempoSegment {
                    start_tick: PPQN * 4,
                    bpm: 60.0,
                },
            ],
            48_000.0,
        )
    }

    #[test]
    fn a_tempo_change_shifts_everything_after_it() {
        let map = two_tempo_map();
        // Before the change: unchanged.
        assert_eq!(map.tick_to_sample(PPQN), 24_000);
        // Exactly at it: four quarter notes at 120 bpm.
        assert_eq!(map.tick_to_sample(PPQN * 4), 96_000);
        // One quarter note past it, now at half the speed.
        assert_eq!(map.tick_to_sample(PPQN * 5), 96_000 + 48_000);
        // Reading the file's first tempo and holding it — what the importer
        // used to do — would put this at 120 000 instead, and every note after
        // the change would drift further out.
        assert_ne!(map.tick_to_sample(PPQN * 5), 120_000);
    }

    #[test]
    fn sample_to_tick_inverts_tick_to_sample_across_a_change() {
        let map = two_tempo_map();
        for tick in (0..PPQN * 12).step_by(37) {
            assert_eq!(
                map.sample_to_tick(map.tick_to_sample(tick)),
                tick,
                "round trip failed at tick {tick}"
            );
        }
    }

    #[test]
    fn tempo_at_reports_the_segment_in_force() {
        let map = two_tempo_map();
        assert_eq!(map.tempo_at(0), 120.0);
        assert_eq!(map.tempo_at(PPQN * 4 - 1), 120.0);
        assert_eq!(map.tempo_at(PPQN * 4), 60.0, "a segment owns its own start");
        assert_eq!(map.tempo_at(PPQN * 400), 60.0);
    }

    #[test]
    fn segments_are_sorted_and_the_project_always_starts_somewhere() {
        // A MIDI file may put its tempo events in any track, so they arrive in
        // file order rather than musical order — and it need not have one at
        // tick 0 at all.
        let map = TempoMap::from_segments(
            vec![
                TempoSegment {
                    start_tick: PPQN * 8,
                    bpm: 90.0,
                },
                TempoSegment {
                    start_tick: PPQN * 2,
                    bpm: 150.0,
                },
            ],
            48_000.0,
        );
        assert_eq!(
            map.tempo_at(0),
            150.0,
            "with nothing at tick 0, the first segment reaches back to it              rather than leaving the opening bars undefined"
        );
        assert_eq!(map.tempo_at(PPQN * 8), 90.0);
        assert!(map.tick_to_sample(PPQN * 8) < map.tick_to_sample(PPQN * 9));
    }

    #[test]
    fn a_tempo_of_zero_cannot_stop_time() {
        // A malformed file can carry one, and dividing by it produces an
        // infinite sample position that poisons every conversion after it.
        let map = TempoMap::from_segments(
            vec![TempoSegment {
                start_tick: 0,
                bpm: 0.0,
            }],
            48_000.0,
        );
        assert!(map.tempo_at(0) > 0.0);
        assert!(map.tick_to_sample(PPQN).is_positive());
    }

    #[test]
    fn changing_the_sample_rate_rescales_every_segment() {
        let mut map = two_tempo_map();
        map.set_sample_rate(96_000.0);
        assert_eq!(map.tick_to_sample(PPQN * 5), (96_000 + 48_000) * 2);
    }

    #[test]
    fn negative_ticks_run_backwards_at_the_opening_tempo() {
        // A count-in sits before tick 0, and it has to land somewhere sensible
        // rather than clamping every pre-roll note onto the downbeat.
        let map = two_tempo_map();
        assert_eq!(map.tick_to_sample(-PPQN), -24_000);
    }
}
