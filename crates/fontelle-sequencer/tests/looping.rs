//! What a looped clip compiles to.
//!
//! The model half is `fontelle-model/tests/looping.rs`: `Clip::loop_length` is
//! the period the content repeats at. This is the half that makes the repeats
//! *sound* — and it is where the difference between looping and copying stops
//! being a matter of opinion. A copy is several clips with notes of their own;
//! a loop is one clip whose one set of notes reaches the timeline again every
//! period.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;

/// 120 BPM at 48 kHz is exactly 25 samples per tick — no rounding to hide
/// behind, which is why the whole file uses it.
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

/// A project with one clip `length` long holding `notes`, looping at `period`.
fn compile_clip(
    length: Tick,
    period: Option<Tick>,
    notes: Vec<Note>,
) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("looping");
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
        length,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: period,
    });

    let node = NodeId::from(KeyData::from_ffi(1));
    let map: HashMap<ChannelId, NodeId> = [(channel, node)].into_iter().collect();
    fontelle_sequencer::compile(&project, &map, &Default::default())
}

/// Every note-on in the timeline, back in ticks.
fn note_on_ticks(timeline: &fontelle_types::CompiledTimeline) -> Vec<i64> {
    let mut ticks: Vec<i64> = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::NoteOn { .. }))
        .map(|e| e.sample / SAMPLES_PER_TICK)
        .collect();
    ticks.sort_unstable();
    ticks
}

#[test]
fn a_clip_that_does_not_loop_compiles_exactly_as_it_always_did() {
    let timeline = compile_clip(PPQN * 4, None, vec![a_note(0, PPQN, 60)]);
    assert_eq!(note_on_ticks(&timeline), vec![0]);
}

#[test]
fn a_looped_clip_plays_its_content_again_every_period() {
    // One bar of content in a four-bar clip, looping every bar: four passes.
    let timeline = compile_clip(PPQN * 16, Some(PPQN * 4), vec![a_note(0, PPQN, 60)]);
    assert_eq!(
        note_on_ticks(&timeline),
        vec![0, PPQN * 4, PPQN * 8, PPQN * 12]
    );
}

#[test]
fn every_note_in_the_period_comes_round_again_not_just_the_first() {
    let timeline = compile_clip(
        PPQN * 4,
        Some(PPQN * 2),
        vec![a_note(0, PPQN / 2, 60), a_note(PPQN, PPQN / 2, 64)],
    );
    assert_eq!(note_on_ticks(&timeline), vec![0, PPQN, PPQN * 2, PPQN * 3]);
}

#[test]
fn a_note_written_past_the_period_is_not_part_of_the_loop() {
    // It is content the loop does not contain — what the clip would play if it
    // were not looping. Playing it on every pass would be a second, invisible
    // loop inside the first.
    let timeline = compile_clip(
        PPQN * 4,
        Some(PPQN),
        vec![a_note(0, PPQN / 4, 60), a_note(PPQN * 2, PPQN / 4, 72)],
    );
    assert_eq!(
        note_on_ticks(&timeline),
        vec![0, PPQN, PPQN * 2, PPQN * 3],
        "four passes of the one note inside the period, and nothing else"
    );
}

#[test]
fn a_partial_repeat_at_the_end_plays_what_fits_and_no_more() {
    // Three half-bars of a two-beat loop: the last pass is cut short.
    let timeline = compile_clip(
        PPQN * 6,
        Some(PPQN * 2),
        vec![a_note(0, PPQN / 4, 60), a_note(PPQN, PPQN / 4, 64)],
    );
    assert_eq!(
        note_on_ticks(&timeline),
        vec![0, PPQN, PPQN * 2, PPQN * 3, PPQN * 4, PPQN * 5],
        "the notes that start before the clip ends, and only those"
    );
}

#[test]
fn a_repeat_that_starts_past_the_clips_end_does_not_sound() {
    // A five-beat clip looping every four: the second pass's note at beat 6 is
    // off the end.
    let timeline = compile_clip(
        PPQN * 5,
        Some(PPQN * 4),
        vec![a_note(PPQN * 2, PPQN / 4, 60)],
    );
    assert_eq!(note_on_ticks(&timeline), vec![PPQN * 2]);
}

#[test]
fn a_loop_does_not_ring_past_its_own_end() {
    // The last pass has to sound like the others. A note left running past the
    // clip is a loop whose final repeat is longer than every one before it.
    let timeline = compile_clip(PPQN * 2, Some(PPQN * 2), vec![a_note(0, PPQN * 8, 60)]);
    let offs: Vec<i64> = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::NoteOff { .. }))
        .map(|e| e.sample / SAMPLES_PER_TICK)
        .collect();
    assert_eq!(offs, vec![PPQN * 2], "clamped to the clip's own end");
}

// ----------------------------------------------------------- slide notes ---

/// A slide note carries no note-on and no note-off; it carries a bend.
#[test]
fn a_slide_note_compiles_to_a_bend_and_not_to_a_note() {
    let mut notes = vec![a_note(0, PPQN * 2, 60)];
    let mut slide = a_note(PPQN, PPQN, 67);
    slide.slide = true;
    notes.push(slide);

    let timeline = compile_clip(PPQN * 4, None, notes);
    let kinds: Vec<&str> = timeline
        .events
        .iter()
        .map(|e| match e.payload {
            EventPayload::NoteOn { .. } => "on",
            EventPayload::NoteOff { .. } => "off",
            EventPayload::NoteSlide { .. } => "slide",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["on", "slide", "off"],
        "one note, one bend, one end: {kinds:?}"
    );
}

#[test]
fn a_slides_length_is_how_long_the_bend_takes() {
    // The note's length *is* the glide time, which is what makes the shape of
    // a slide something you can see on the grid.
    let mut slide = a_note(0, PPQN, 67);
    slide.slide = true;
    let timeline = compile_clip(PPQN * 4, None, vec![slide]);

    let EventPayload::NoteSlide {
        key, glide_samples, ..
    } = timeline.events[0].payload
    else {
        panic!("expected a slide, got {:?}", timeline.events[0].payload);
    };
    assert_eq!(key, 67, "it bends to its own pitch");
    assert_eq!(
        glide_samples as i64,
        PPQN * SAMPLES_PER_TICK,
        "a quarter note at 120 BPM is half a second"
    );
}

#[test]
fn a_slide_inside_a_loop_comes_round_with_everything_else() {
    let mut slide = a_note(PPQN, PPQN, 67);
    slide.slide = true;
    let timeline = compile_clip(PPQN * 8, Some(PPQN * 2), vec![a_note(0, PPQN, 60), slide]);

    let slides = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::NoteSlide { .. }))
        .count();
    assert_eq!(slides, 4, "four passes, four bends");
}
