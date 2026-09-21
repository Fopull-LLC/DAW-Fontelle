//! What order events at the **same sample** come out in.
//!
//! The report: *"sometimes notes will not play if I have a note extending
//! before it all the way until where the new note starts, it just has a chance
//! not to play."*
//!
//! "A chance" is the tell. Two notes on one key, the first ending exactly where
//! the second begins, compile to a note-off and a note-on at the same sample —
//! and the compiler emitted them in whatever order the clip's arena happened to
//! hold the notes. When the off came second, it found the voice the *on* had
//! just started (the pool hands out the lowest free slot, and the first note's
//! voice is free the moment its sample runs out) and released it. The note was
//! there, and silent.
//!
//! So the order is not an accident of arena iteration any more. At one sample:
//! every parameter value first, then every note-off, then slides, then the
//! note-ons. See `compile`'s `rank`.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;

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

/// One clip holding `notes`, compiled. The notes go into the arena in the
/// order given, which is exactly the thing this file is about.
fn compile_notes(notes: Vec<Note>) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("order");
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
        ab: Default::default(),
        gain_db: 0.0,
    });
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut arena = Arena::default();
    for note in notes {
        arena.insert(note);
    }
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 8,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    let node = NodeId::from(KeyData::from_ffi(1));
    let mut nodes: HashMap<ChannelId, NodeId> = HashMap::new();
    nodes.insert(channel, node);
    fontelle_sequencer::compile(&project, &nodes, &HashMap::new())
}

/// Every event at `sample`, in the order the RT side will walk them.
fn at(timeline: &fontelle_types::CompiledTimeline, sample: i64) -> Vec<&EventPayload> {
    timeline
        .events
        .iter()
        .filter(|e| e.sample == sample)
        .map(|e| &e.payload)
        .collect()
}

fn is_off(payload: &EventPayload) -> bool {
    matches!(payload, EventPayload::NoteOff { .. })
}

fn is_on(payload: &EventPayload) -> bool {
    matches!(payload, EventPayload::NoteOn { .. })
}

/// The report, as a test. A whole note held to the beat where the next one
/// starts — written **second**, so the arena hands it back after the note that
/// follows it, which is the order that used to drop the second note.
#[test]
fn a_note_ending_where_the_next_begins_lets_go_before_the_next_starts() {
    let timeline = compile_notes(vec![
        // The one that starts at the seam, inserted first on purpose.
        a_note(PPQN * 4, PPQN * 4, 60),
        // The held one that ends there.
        a_note(0, PPQN * 4, 60),
    ]);
    let seam = timeline
        .events
        .iter()
        .map(|e| e.sample)
        .filter(|s| *s > 0)
        .min()
        .expect("the seam is an event");
    let payloads = at(&timeline, seam);
    let off = payloads
        .iter()
        .position(|p| is_off(p))
        .expect("the held note lets go here");
    let on = payloads
        .iter()
        .position(|p| is_on(p))
        .expect("the next note starts here");
    assert!(
        off < on,
        "the off has to be delivered first, or it releases the voice the on just took: {payloads:?}"
    );
}

/// The same seam, with the notes written the other way round. Insertion order
/// must make no difference at all — that it did is what made the failure
/// intermittent.
#[test]
fn the_order_does_not_depend_on_the_order_the_notes_were_written_in() {
    let one = compile_notes(vec![
        a_note(0, PPQN * 4, 60),
        a_note(PPQN * 4, PPQN * 4, 60),
    ]);
    let other = compile_notes(vec![
        a_note(PPQN * 4, PPQN * 4, 60),
        a_note(0, PPQN * 4, 60),
    ]);
    let kinds = |t: &fontelle_types::CompiledTimeline| -> Vec<String> {
        t.events
            .iter()
            .map(|e| format!("{}:{:?}", e.sample, std::mem::discriminant(&e.payload)))
            .collect()
    };
    assert_eq!(kinds(&one), kinds(&other), "same song, same event stream");
}

/// A parameter value at the same sample as a note-on is the value that note
/// should sound **at** — so it is applied first.
#[test]
fn a_parameter_lands_before_the_note_it_belongs_to() {
    // Built by hand rather than through a project: what is being checked is
    // the sort, and an automation clip would only make the fixture longer.
    let mut events = vec![
        fontelle_types::TimedEvent {
            sample: 100,
            target: NodeId::from(KeyData::from_ffi(1)),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            },
        },
        fontelle_types::TimedEvent {
            sample: 100,
            target: NodeId::from(KeyData::from_ffi(1)),
            payload: EventPayload::ParamValue {
                target: fontelle_types::ParamAddress::new("mixer:1/gain"),
                value: 0.5,
            },
        },
    ];
    fontelle_sequencer::sort_events(&mut events);
    assert!(
        matches!(events[0].payload, EventPayload::ParamValue { .. }),
        "the value the note is played at is set before the note"
    );
}

/// Nothing about the ordering may move an event to a different sample.
#[test]
fn every_event_keeps_the_sample_it_was_written_for() {
    let timeline = compile_notes(vec![a_note(0, PPQN, 60), a_note(PPQN, PPQN, 62)]);
    let samples: Vec<i64> = timeline.events.iter().map(|e| e.sample).collect();
    let mut sorted = samples.clone();
    sorted.sort_unstable();
    assert_eq!(samples, sorted, "still in time order");
}
