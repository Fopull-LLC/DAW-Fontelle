//! Two audio clips overlapping on one row compile to a crossfade (TDD §15.2).
//!
//! *"when audio clips are overlapping ... it should also blend together like
//! a transition the timing based on how long the overlap section is."*
//!
//! The compiler already owns every tick-to-sample conversion, and it can see
//! both clips at once, so this is where the overlap is measured: the earlier
//! clip fades out over it and the later fades in over it, in song samples.
//! `fontelle-types/tests/crossfade.rs` is the curve; this is the length.

use std::collections::HashMap;

use fontelle_model::{Clip, ClipSource, Project, TempoMap};
use fontelle_sequencer::NodeMaps;
use fontelle_types::{
    AssetKind, AssetRef, AudioClipData, ClipId, LaneId, MixerTrackId, NodeId, PPQN, Tick,
};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;
/// 120 BPM at 48 kHz is exactly 25 samples per tick.
const SAMPLES_PER_TICK: i64 = 25;
const BAR: Tick = PPQN * 4;

fn an_asset() -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

struct Rig {
    project: Project,
    track: MixerTrackId,
    player: NodeId,
    lane: LaneId,
}

fn rig() -> Rig {
    let mut project = Project::new("crossfade");
    project.tempo_map = TempoMap::new(BPM, SR);
    let track = project
        .mixer
        .tracks
        .insert(fontelle_model::MixerTrack::new("Mic"));
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "row".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    Rig {
        project,
        track,
        player: NodeId::from(KeyData::from_ffi(7)),
        lane,
    }
}

impl Rig {
    fn another_lane(&mut self) -> LaneId {
        self.project.lanes.insert(fontelle_model::Lane {
            name: "row 2".into(),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
            order: 1,
        })
    }

    fn place_on(&mut self, lane: LaneId, start: Tick, length: Tick) -> ClipId {
        let mut data = AudioClipData::whole(an_asset(), 100_000, 48_000);
        data.mixer_track = Some(self.track);
        self.project.clips.insert(Clip {
            lane,
            start,
            length,
            source: ClipSource::Audio(data),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        })
    }

    fn place(&mut self, start: Tick, length: Tick) -> ClipId {
        self.place_on(self.lane, start, length)
    }

    fn compile(&self) -> fontelle_types::CompiledTimeline {
        let audio: HashMap<Option<MixerTrackId>, NodeId> =
            [(Some(self.track), self.player)].into_iter().collect();
        fontelle_sequencer::compile_with(
            &self.project,
            &NodeMaps {
                audio: &audio,
                ..Default::default()
            },
            fontelle_sequencer::CompileScope::Song,
        )
    }

    /// `(crossfade_in, crossfade_out)` of the placement made from `clip`.
    fn fades(&self, clip: ClipId) -> (i64, i64) {
        let timeline = self.compile();
        let placed = timeline
            .audio
            .iter()
            .find(|p| p.clip == clip)
            .expect("the clip was placed");
        (placed.crossfade_in, placed.crossfade_out)
    }
}

#[test]
fn a_clip_on_its_own_has_no_crossfade() {
    let mut r = rig();
    let a = r.place(0, BAR * 2);
    assert_eq!(r.fades(a), (0, 0));
}

#[test]
fn the_earlier_clip_fades_out_over_the_overlap_and_the_later_fades_in() {
    let mut r = rig();
    let a = r.place(0, BAR * 2);
    let b = r.place(BAR + PPQN * 2, BAR * 2); // two beats of overlap
    let overlap = PPQN * 2 * SAMPLES_PER_TICK;
    assert_eq!(r.fades(a), (0, overlap));
    assert_eq!(r.fades(b), (overlap, 0));
}

#[test]
fn the_length_is_the_overlap_and_only_the_overlap() {
    // *"the timing based on how long the overlap section is."*
    for beats in [1, 3, 7] {
        let mut r = rig();
        let a = r.place(0, BAR * 2);
        let b = r.place(BAR * 2 - PPQN * beats, BAR * 2);
        assert_eq!(
            r.fades(a).1,
            PPQN * beats * SAMPLES_PER_TICK,
            "{beats} beats"
        );
        assert_eq!(
            r.fades(b).0,
            PPQN * beats * SAMPLES_PER_TICK,
            "{beats} beats"
        );
    }
}

#[test]
fn clips_that_only_touch_do_not_fade() {
    let mut r = rig();
    let a = r.place(0, BAR);
    let b = r.place(BAR, BAR);
    assert_eq!(r.fades(a), (0, 0));
    assert_eq!(r.fades(b), (0, 0));
}

#[test]
fn clips_on_different_rows_do_not_fade_however_they_line_up() {
    let mut r = rig();
    let other = r.another_lane();
    let a = r.place(0, BAR * 2);
    let b = r.place_on(other, BAR, BAR * 2);
    assert_eq!(r.fades(a), (0, 0));
    assert_eq!(r.fades(b), (0, 0));
}

#[test]
fn a_clip_between_two_others_fades_at_both_ends() {
    let mut r = rig();
    let a = r.place(0, BAR * 2);
    let b = r.place(BAR, BAR * 2);
    let c = r.place(BAR * 2 + PPQN * 2, BAR * 2);
    let bar = BAR * SAMPLES_PER_TICK;
    let two_beats = PPQN * 2 * SAMPLES_PER_TICK;
    assert_eq!(r.fades(a), (0, bar));
    assert_eq!(r.fades(b), (bar, two_beats));
    assert_eq!(r.fades(c), (two_beats, 0));
}

#[test]
fn a_muted_clip_takes_part_in_no_crossfade() {
    // Nothing is heard from it, so nothing should be faded against it: the
    // clip beside it plays plain, as it would with the muted one deleted.
    let mut r = rig();
    let a = r.place(0, BAR * 2);
    let b = r.place(BAR, BAR * 2);
    r.project.clips[a].muted = true;
    assert_eq!(r.fades(b), (0, 0));
}

#[test]
fn a_note_clip_on_the_same_row_is_not_something_to_fade_against() {
    let mut r = rig();
    let a = r.place(0, BAR * 2);
    let channel = r.project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
    });
    r.project.clips.insert(Clip {
        lane: r.lane,
        start: BAR,
        length: BAR * 2,
        source: ClipSource::Notes(fontelle_model::NoteData {
            channel,
            notes: fontelle_model::Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    assert_eq!(r.fades(a), (0, 0));
}

#[test]
fn a_clip_wholly_inside_another_fades_in_over_all_of_itself_and_the_outer_one_fades_out_over_it() {
    // A one-bar clip dropped in the middle of a four-bar one. The inner
    // clip is the later one, so it comes in; the outer one goes out over
    // the same bar — and comes back, because the fade is measured from the
    // overlap, not from its own end. What comes back is the compiler's
    // business to keep honest: an out-fade at the *end* of the outer clip
    // would be a bar early and a bar long. So the outer clip's out-fade is
    // clamped to what it can express: the overlap reaching its end.
    let mut r = rig();
    let outer = r.place(0, BAR * 4);
    let inner = r.place(BAR, BAR);
    assert_eq!(r.fades(inner).0, BAR * SAMPLES_PER_TICK);
    // The outer clip does not end inside the overlap, so it has no tail to
    // fade: the compiler does not invent one.
    assert_eq!(r.fades(outer), (0, 0));
}
