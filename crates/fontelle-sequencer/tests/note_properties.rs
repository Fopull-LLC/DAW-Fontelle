//! The seam the four dead properties cross: score to wire.
//!
//! `fontelle-core/tests/note_properties.rs` is the other half — what a voice
//! does once it has them. This half is narrower and, historically, the place
//! they were lost: the compiler read `key`, `velocity` and `pan` off a `Note`
//! and dropped fine pitch, release and the two modulation values on the floor,
//! so a property lane you could draw in produced a timeline that had never
//! heard of it.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

fn a_note(start: Tick, key: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

fn compile(notes: Vec<Note>) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("properties");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel = project.channels.insert(fontelle_model::Channel {
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
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
        length: PPQN * 16,
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
    let map: HashMap<ChannelId, NodeId> = [(channel, node)].into_iter().collect();
    fontelle_sequencer::compile(&project, &map, &Default::default())
}

/// The one note-on in the timeline, unpacked.
fn only_note_on(timeline: &fontelle_types::CompiledTimeline) -> (i16, u8, u8, u8) {
    let mut found = None;
    for event in &timeline.events {
        if let EventPayload::NoteOn {
            fine_pitch,
            release,
            mod_x,
            mod_y,
            ..
        } = event.payload
        {
            assert!(found.is_none(), "expected exactly one note-on");
            found = Some((fine_pitch, release, mod_x, mod_y));
        }
    }
    found.expect("no note-on compiled")
}

#[test]
fn every_property_a_note_carries_reaches_the_note_on() {
    // All four at once, each a different value, so a compiler that wired one
    // field to another's source fails here rather than looking right.
    let mut note = a_note(0, 60);
    note.fine_pitch = -350;
    note.release = 90;
    note.mod_x = 12;
    note.mod_y = 101;

    assert_eq!(only_note_on(&compile(vec![note])), (-350, 90, 12, 101));
}

#[test]
fn a_plain_note_compiles_to_the_defaults_it_always_did() {
    assert_eq!(only_note_on(&compile(vec![a_note(0, 60)])), (0, 0, 0, 0));
}

#[test]
fn each_note_carries_its_own_properties() {
    // Per note, not per clip: the reason they ride on the note-on at all.
    let mut first = a_note(0, 60);
    first.fine_pitch = 100;
    first.mod_x = 20;
    let mut second = a_note(PPQN * 2, 64);
    second.fine_pitch = -100;
    second.mod_x = 90;

    let timeline = compile(vec![first, second]);
    let mut seen: Vec<(u8, i16, u8)> = timeline
        .events
        .iter()
        .filter_map(|event| match event.payload {
            EventPayload::NoteOn {
                key,
                fine_pitch,
                mod_x,
                ..
            } => Some((key, fine_pitch, mod_x)),
            _ => None,
        })
        .collect();
    seen.sort_unstable();
    assert_eq!(seen, vec![(60, 100, 20), (64, -100, 90)]);
}

#[test]
fn a_looped_clip_repeats_the_properties_with_the_notes() {
    // A loop replays the *same* note, so every repeat carries what the first
    // one carried — a repeat that lost the properties would be a different
    // note wearing the same key.
    let mut note = a_note(0, 60);
    note.fine_pitch = 250;
    note.release = 40;

    let mut project = Project::new("looped");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel = project.channels.insert(fontelle_model::Channel {
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
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
    arena.insert(note);
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
        loop_length: Some(PPQN * 2),
    });
    let node = NodeId::from(KeyData::from_ffi(1));
    let map: HashMap<ChannelId, NodeId> = [(channel, node)].into_iter().collect();
    let timeline = fontelle_sequencer::compile(&project, &map, &Default::default());

    let carried: Vec<(i16, u8)> = timeline
        .events
        .iter()
        .filter_map(|event| match event.payload {
            EventPayload::NoteOn {
                fine_pitch,
                release,
                ..
            } => Some((fine_pitch, release)),
            _ => None,
        })
        .collect();
    assert_eq!(
        carried,
        vec![(250, 40); 4],
        "four passes, all the same note"
    );
}
