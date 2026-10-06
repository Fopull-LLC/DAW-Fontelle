//! A note's **path** reaching the wire (`docs/note-paths-plan.md`).
//!
//! One note-on, one note-off on the key it started on, and a glide at the
//! start of each slide — addressed to *that* note, which is the whole
//! difference from the FL slide note (`NoteSlide`), which bends every note in
//! its context and so cannot slide a chord apart.
//!
//! 120 BPM at 48 kHz: a beat is 24 000 samples.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, PathPoint, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

const BEAT: i64 = 24_000;

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
        path: Vec::new(),
        channel: None,
    }
}

fn with_path(mut note: Note, points: &[(Tick, i8)]) -> Note {
    note.path = points
        .iter()
        .map(|&(at, offset)| PathPoint { at, offset })
        .collect();
    note
}

fn compile_in(clip_length: Tick, notes: Vec<Note>) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("paths");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
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
        soloed: false,
        order: 0,
    });
    let mut arena = Arena::default();
    for note in notes {
        arena.insert(note);
    }
    project.clips.insert(Clip {
        name: None,
        lane,
        start: 0,
        length: clip_length,
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

fn compile(notes: Vec<Note>) -> fontelle_types::CompiledTimeline {
    compile_in(PPQN * 16, notes)
}

/// What the timeline says, in a form a test can compare whole.
#[derive(Debug, PartialEq)]
enum Seen {
    On(i64, u8),
    Off(i64, u8),
    Glide(i64, u8, f32, u32),
}

fn seen(timeline: &fontelle_types::CompiledTimeline) -> Vec<Seen> {
    timeline
        .events
        .iter()
        .filter_map(|event| match event.payload {
            EventPayload::NoteOn { key, .. } => Some(Seen::On(event.sample, key)),
            EventPayload::NoteOff { key, .. } => Some(Seen::Off(event.sample, key)),
            EventPayload::NoteGlide {
                key,
                semitones,
                glide_samples,
                ..
            } => Some(Seen::Glide(event.sample, key, semitones, glide_samples)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plain_note_compiles_with_no_glide() {
    assert_eq!(
        seen(&compile(vec![a_note(0, PPQN, 60)])),
        vec![Seen::On(0, 60), Seen::Off(BEAT, 60)]
    );
}

#[test]
fn a_hold_then_a_slide_is_one_note_and_one_glide() {
    // Flat for a beat, a fifth up over the next, held to the end.
    let note = with_path(a_note(0, PPQN * 4, 60), &[(PPQN, 0), (PPQN * 2, 7)]);
    assert_eq!(
        seen(&compile(vec![note])),
        vec![
            Seen::On(0, 60),
            Seen::Glide(BEAT, 60, 7.0, BEAT as u32),
            Seen::Off(BEAT * 4, 60),
        ],
        "the note-off names the key the note started on, which is the voice"
    );
}

#[test]
fn a_chord_slides_apart() {
    // The thing the FL slide note cannot do: two notes, two destinations.
    let low = with_path(a_note(0, PPQN * 2, 60), &[(PPQN, 5)]);
    let high = with_path(a_note(0, PPQN * 2, 64), &[(PPQN, -2)]);
    let glides: Vec<Seen> = seen(&compile(vec![low, high]))
        .into_iter()
        .filter(|s| matches!(s, Seen::Glide(..)))
        .collect();
    assert_eq!(glides.len(), 2);
    assert!(glides.contains(&Seen::Glide(0, 60, 5.0, BEAT as u32)));
    assert!(glides.contains(&Seen::Glide(0, 64, -2.0, BEAT as u32)));
}

#[test]
fn a_melody_from_one_note_is_a_glide_per_slide_and_nothing_per_hold() {
    let note = with_path(
        a_note(0, PPQN * 6, 60),
        &[
            (PPQN, 2),
            (PPQN * 2, 2),
            (PPQN * 3, -1),
            (PPQN * 4, -1),
            (PPQN * 5, 4),
        ],
    );
    assert_eq!(
        seen(&compile(vec![note])),
        vec![
            Seen::On(0, 60),
            Seen::Glide(0, 60, 2.0, BEAT as u32),
            Seen::Glide(BEAT * 2, 60, -1.0, BEAT as u32),
            Seen::Glide(BEAT * 4, 60, 4.0, BEAT as u32),
            Seen::Off(BEAT * 6, 60),
        ]
    );
}

#[test]
fn a_step_at_the_very_start_lands_after_the_note_on() {
    // Two points at one tick are a jump. One at the note's start has to
    // reach the voice the note-on just made, so it must sort after it.
    let note = with_path(a_note(0, PPQN, 60), &[(0, 0), (0, 5)]);
    assert_eq!(
        seen(&compile(vec![note])),
        vec![
            Seen::On(0, 60),
            Seen::Glide(0, 60, 5.0, 0),
            Seen::Off(BEAT, 60)
        ]
    );
}

#[test]
fn a_clip_end_cuts_a_slide_where_it_has_got_to() {
    // The clip stops a beat and a half in, halfway up the fifth: the glide
    // goes as far as the note gets to sound and no further, so a release
    // tail does not go on climbing after the cut.
    let note = with_path(a_note(0, PPQN * 4, 60), &[(PPQN, 0), (PPQN * 2, 7)]);
    assert_eq!(
        seen(&compile_in(PPQN + PPQN / 2, vec![note])),
        vec![
            Seen::On(0, 60),
            Seen::Glide(BEAT, 60, 3.5, (BEAT / 2) as u32),
            Seen::Off(BEAT + BEAT / 2, 60),
        ]
    );
}

#[test]
fn a_path_past_the_notes_end_is_not_played() {
    // A note shortened after its slide was drawn keeps the points (so it can
    // be lengthened again) and plays none past its end.
    let note = with_path(a_note(0, PPQN, 60), &[(PPQN * 2, 0), (PPQN * 3, 7)]);
    assert_eq!(
        seen(&compile(vec![note])),
        vec![Seen::On(0, 60), Seen::Off(BEAT, 60)]
    );
}
