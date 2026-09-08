//! What a prefab instance compiles to.
//!
//! The model half is `fontelle-model/tests/prefabs.rs`: a clip that follows a
//! prefab holds no notes of its own, and `Project::clip_source` is the only
//! correct way to ask one what it plays. This is the half that makes it
//! *sound* — and it is the one place the whole design could quietly fail,
//! because a compiler that reads `clip.source` directly still passes every
//! test written before prefabs existed and emits silence for every instance.
//!
//! Note the shape of these tests: they assert on **where the notes land in the
//! song**, not on the resolution pass. A prefab's content is in the prefab's
//! own ticks, and a place for it has a start of its own — getting that offset
//! wrong is the second way this feature fails, and it fails by playing the
//! right notes at the wrong time.

use std::collections::HashMap;

use fontelle_model::{
    AddNotes, AddPrefab, AddPrefabInstance, Arena, Clip, ClipSource, Command, Note, NoteData,
    NoteHome, Project, TempoMap,
};
use fontelle_types::{ChannelId, EventPayload, LaneId, NodeId, PPQN, PrefabId, Tick};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;

/// 120 BPM at 48 kHz is exactly 25 samples per tick — the same arithmetic
/// `tests/looping.rs` leans on, and for the same reason: no rounding to hide
/// behind.
const SAMPLES_PER_TICK: i64 = 25;

fn a_note(start: Tick, length: Tick, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

struct Rig {
    project: Project,
    channel: ChannelId,
    lane: LaneId,
    other: LaneId,
}

fn a_rig() -> Rig {
    let mut project = Project::new("prefabs");
    project.tempo_map = TempoMap::new(BPM, SR);
    let channel = project.channels.insert(fontelle_model::Channel {
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
        gain_db: 0.0,
    });
    let lane = project.lanes.insert(a_lane(0));
    let other = project.lanes.insert(a_lane(1));
    Rig {
        project,
        channel,
        lane,
        other,
    }
}

fn a_lane(order: u32) -> fontelle_model::Lane {
    fontelle_model::Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order,
    }
}

impl Rig {
    /// A prefab holding `notes`, in its own ticks.
    fn prefab(&mut self, notes: Vec<Note>) -> PrefabId {
        let mut add = AddPrefab::new(
            "Riff",
            ClipSource::Notes(NoteData {
                channel: self.channel,
                notes: Arena::default(),
            }),
        );
        add.apply(&mut self.project).unwrap();
        let id = add.prefab().unwrap();
        AddNotes::new(NoteHome::Prefab(id), notes)
            .apply(&mut self.project)
            .unwrap();
        id
    }

    fn place(&mut self, prefab: PrefabId, lane: LaneId, start: Tick, length: Tick) {
        AddPrefabInstance::new(prefab, lane, start, length)
            .apply(&mut self.project)
            .unwrap();
    }

    fn compile(&self) -> fontelle_types::CompiledTimeline {
        let node = NodeId::from(KeyData::from_ffi(1));
        let map: HashMap<ChannelId, NodeId> = [(self.channel, node)].into_iter().collect();
        fontelle_sequencer::compile(&self.project, &map, &Default::default())
    }
}

/// Every note-on as `(tick, key)`, in order.
fn note_ons(timeline: &fontelle_types::CompiledTimeline) -> Vec<(i64, u8)> {
    let mut out: Vec<(i64, u8)> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => Some((e.sample / SAMPLES_PER_TICK, key)),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out
}

/// **A prefab instance sounds.**
///
/// The failure this exists to catch: a compiler that reads `clip.source` gets
/// an empty arena and emits nothing, and every other test in this crate still
/// passes.
#[test]
fn a_clip_that_follows_a_prefab_plays_the_prefabs_notes() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60), a_note(PPQN, PPQN, 64)]);
    rig.place(prefab, rig.lane, 0, PPQN * 4);

    assert_eq!(note_ons(&rig.compile()), vec![(0, 60), (PPQN, 64)]);
}

/// And it sounds **where it was put**, not where the prefab's ticks start.
#[test]
fn an_instance_plays_at_its_own_start() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60), a_note(PPQN, PPQN, 64)]);
    rig.place(prefab, rig.lane, PPQN * 8, PPQN * 4);

    assert_eq!(
        note_ons(&rig.compile()),
        vec![(PPQN * 8, 60), (PPQN * 9, 64)]
    );
}

/// Two places, one set of notes, twice in the song.
#[test]
fn every_place_a_prefab_is_drawn_plays_it() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60)]);
    rig.place(prefab, rig.lane, 0, PPQN * 4);
    rig.place(prefab, rig.other, PPQN * 16, PPQN * 4);

    assert_eq!(note_ons(&rig.compile()), vec![(0, 60), (PPQN * 16, 60)]);
}

/// One edit, every place — through the compiler.
#[test]
fn editing_the_prefab_changes_what_every_place_plays() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60)]);
    rig.place(prefab, rig.lane, 0, PPQN * 4);
    rig.place(prefab, rig.other, PPQN * 16, PPQN * 4);
    assert_eq!(note_ons(&rig.compile()), vec![(0, 60), (PPQN * 16, 60)]);

    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(PPQN * 2, PPQN, 67)])
        .apply(&mut rig.project)
        .unwrap();

    assert_eq!(
        note_ons(&rig.compile()),
        vec![(0, 60), (PPQN * 2, 67), (PPQN * 16, 60), (PPQN * 18, 67)],
        "one note drawn once, heard in both places"
    );
}

/// A place shorter than its content **cuts** it, exactly as an ordinary clip
/// does.
///
/// The prefab is the notes; the place is the window on them. Growing the
/// window to fit is the thing `clip-ends-and-growth` says a clip must never
/// do, and an instance is not an exception.
#[test]
fn a_place_shorter_than_the_prefab_cuts_it() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![
        a_note(0, PPQN, 60),
        a_note(PPQN * 2, PPQN, 64),
        a_note(PPQN * 6, PPQN, 67),
    ]);
    // Four beats of window over eight beats of content.
    rig.place(prefab, rig.lane, 0, PPQN * 4);

    assert_eq!(
        note_ons(&rig.compile()),
        vec![(0, 60), (PPQN * 2, 64)],
        "the note past the end of the place does not sound"
    );
}

/// A place can loop, and what repeats is the prefab's content.
#[test]
fn a_place_that_loops_repeats_the_prefabs_content() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60)]);
    rig.place(prefab, rig.lane, 0, PPQN * 16);
    let clip = rig.project.clips.keys().next().unwrap();
    rig.project.clips.get_mut(clip).unwrap().loop_length = Some(PPQN * 4);

    assert_eq!(
        note_ons(&rig.compile()),
        vec![(0, 60), (PPQN * 4, 60), (PPQN * 8, 60), (PPQN * 12, 60)]
    );
}

/// A muted place is silent, and the prefab's other places are not.
#[test]
fn muting_one_place_leaves_the_others_sounding() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60)]);
    rig.place(prefab, rig.lane, 0, PPQN * 4);
    rig.place(prefab, rig.other, PPQN * 16, PPQN * 4);
    let first = rig.project.clips.keys().next().unwrap();
    rig.project.clips.get_mut(first).unwrap().muted = true;

    assert_eq!(note_ons(&rig.compile()), vec![(PPQN * 16, 60)]);
}

/// A clip pointing at a prefab that is gone is **silent, not a panic**.
#[test]
fn a_place_whose_prefab_has_gone_compiles_to_nothing() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60)]);
    rig.place(prefab, rig.lane, 0, PPQN * 4);
    rig.project.prefabs.remove(prefab);

    assert_eq!(note_ons(&rig.compile()), Vec::new());
}

/// And an ordinary clip beside an instance compiles exactly as it always did.
#[test]
fn an_ordinary_clip_is_untouched_by_any_of_this() {
    let mut rig = a_rig();
    let prefab = rig.prefab(vec![a_note(0, PPQN, 60)]);
    rig.place(prefab, rig.lane, 0, PPQN * 4);

    let mut notes = Arena::default();
    notes.insert(a_note(0, PPQN, 72));
    rig.project.clips.insert(Clip {
        lane: rig.other,
        start: PPQN * 8,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel: rig.channel,
            notes,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    assert_eq!(note_ons(&rig.compile()), vec![(0, 60), (PPQN * 8, 72)]);
}
