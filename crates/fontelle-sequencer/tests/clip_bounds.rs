//! What a clip's **end** means for the notes inside it.
//!
//! > *"clip endings don't actually cut the clip short audibly right now it
//! > keeps playing"*
//!
//! Dragging a clip's right edge in makes the block shorter and, until this,
//! changed nothing about what was heard: a note written past the new end
//! still sounded, and a note crossing it still rang to its own length. Only
//! *looped* clips clamped, because a loop whose last pass is longer than the
//! others is obviously wrong — but the rule was never about looping. A
//! clip's length is the window on its content, the same way an audio clip's
//! is (`compile`'s placement runs `clip.start .. clip.start + clip.length`
//! and has since audio clips existed), and this file is the note half of
//! that one rule.
//!
//! What is *kept* is the release: a note cut at the clip's end is a note-off
//! at the end, so the instrument's own tail rings out. Cutting the sound
//! dead at the boundary would be a click, and no DAW does it.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;
/// 120 BPM at 48 kHz is exactly 25 samples per tick.
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

/// A project with one **plain** clip `length` long holding `notes`.
fn compile_clip(length: Tick, notes: Vec<Note>) -> fontelle_types::CompiledTimeline {
    compile_clip_looping(length, None, notes)
}

fn compile_clip_looping(
    length: Tick,
    period: Option<Tick>,
    notes: Vec<Note>,
) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("clip bounds");
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

fn ticks_of(
    timeline: &fontelle_types::CompiledTimeline,
    want: fn(&EventPayload) -> bool,
) -> Vec<i64> {
    let mut ticks: Vec<i64> = timeline
        .events
        .iter()
        .filter(|e| want(&e.payload))
        .map(|e| e.sample / SAMPLES_PER_TICK)
        .collect();
    ticks.sort_unstable();
    ticks
}

fn ons(timeline: &fontelle_types::CompiledTimeline) -> Vec<i64> {
    ticks_of(timeline, |p| matches!(p, EventPayload::NoteOn { .. }))
}

fn offs(timeline: &fontelle_types::CompiledTimeline) -> Vec<i64> {
    ticks_of(timeline, |p| matches!(p, EventPayload::NoteOff { .. }))
}

#[test]
fn a_note_running_past_a_clips_end_is_cut_at_the_end() {
    // Two beats of clip holding an eight-beat note: it stops with the clip.
    let timeline = compile_clip(PPQN * 2, vec![a_note(0, PPQN * 8, 60)]);
    assert_eq!(ons(&timeline), vec![0]);
    assert_eq!(offs(&timeline), vec![PPQN * 2], "cut at the clip's end");
}

#[test]
fn a_note_starting_past_a_clips_end_does_not_sound() {
    let timeline = compile_clip(
        PPQN * 2,
        vec![a_note(0, PPQN / 2, 60), a_note(PPQN * 3, PPQN / 2, 72)],
    );
    assert_eq!(ons(&timeline), vec![0], "only the note inside the clip");
}

/// A note that starts exactly **on** the end is outside it, the way the last
/// frame of an audio clip is: the range is half-open, so a clip ending at
/// beat two and one beginning there do not both play the same tick.
#[test]
fn a_note_starting_exactly_at_the_end_is_outside_the_clip() {
    let timeline = compile_clip(PPQN * 2, vec![a_note(PPQN * 2, PPQN, 60)]);
    assert!(ons(&timeline).is_empty(), "{:?}", ons(&timeline));
}

#[test]
fn a_note_that_fits_inside_the_clip_is_left_alone() {
    let timeline = compile_clip(PPQN * 4, vec![a_note(PPQN, PPQN, 60)]);
    assert_eq!(ons(&timeline), vec![PPQN]);
    assert_eq!(offs(&timeline), vec![PPQN * 2], "its own length, untouched");
}

/// A note ending exactly on the clip's end is not shortened by a tick.
#[test]
fn a_note_ending_exactly_at_the_end_keeps_its_length() {
    let timeline = compile_clip(PPQN * 2, vec![a_note(0, PPQN * 2, 60)]);
    assert_eq!(offs(&timeline), vec![PPQN * 2]);
}

/// The gesture the report came from: the same content, trimmed.
#[test]
fn trimming_a_clip_is_heard() {
    let notes = || {
        vec![
            a_note(0, PPQN, 60),
            a_note(PPQN * 2, PPQN, 64),
            a_note(PPQN * 4, PPQN * 4, 67),
        ]
    };
    let whole = compile_clip(PPQN * 8, notes());
    assert_eq!(ons(&whole), vec![0, PPQN * 2, PPQN * 4]);
    assert_eq!(offs(&whole), vec![PPQN, PPQN * 3, PPQN * 8]);

    let trimmed = compile_clip(PPQN * 5, notes());
    assert_eq!(
        ons(&trimmed),
        vec![0, PPQN * 2, PPQN * 4],
        "the third note still starts inside the clip"
    );
    assert_eq!(
        offs(&trimmed),
        vec![PPQN, PPQN * 3, PPQN * 5],
        "and is cut where the clip now ends"
    );

    let shorter = compile_clip(PPQN * 3, notes());
    assert_eq!(
        ons(&shorter),
        vec![0, PPQN * 2],
        "the third note is outside"
    );
}

/// A **slide** is bounded the same way: it bends over its own length, and a
/// clip that ends first ends the bend with it. One that starts past the end
/// does not happen at all.
#[test]
fn a_slide_is_bounded_by_the_clips_end_too() {
    let mut long = a_note(0, PPQN * 8, 72);
    long.slide = true;
    let timeline = compile_clip(PPQN * 2, vec![a_note(0, PPQN, 60), long]);
    let glide: Vec<u32> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteSlide { glide_samples, .. } => Some(glide_samples),
            _ => None,
        })
        .collect();
    assert_eq!(
        glide,
        vec![(PPQN * 2 * SAMPLES_PER_TICK) as u32],
        "the glide runs to the clip's end, not the note's"
    );

    let mut late = a_note(PPQN * 4, PPQN, 72);
    late.slide = true;
    let outside = compile_clip(PPQN * 2, vec![a_note(0, PPQN, 60), late]);
    assert!(
        !outside
            .events
            .iter()
            .any(|e| matches!(e.payload, EventPayload::NoteSlide { .. })),
        "a slide past the clip's end does not sound"
    );
}

/// Looping already did this, and goes on doing it: one rule for every clip.
#[test]
fn a_looped_clip_still_stops_at_its_own_end() {
    let timeline = compile_clip_looping(PPQN * 2, Some(PPQN * 2), vec![a_note(0, PPQN * 8, 60)]);
    assert_eq!(offs(&timeline), vec![PPQN * 2]);
}

// --- and it loops cleanly from there ---------------------------------------

/// > *"it should just cut off wherever you put the ending to be and then
/// > cleanly loop from that point"*
///
/// Every pass of a loop is cut where the loop ends, not where the *clip*
/// ends. A note written longer than the period used to ring on through the
/// passes that followed it, so the second pass played over the first one's
/// tail and the third over both — the loop got thicker as it went, which is
/// not a loop. Each pass now sounds exactly like the one before it.
#[test]
fn every_pass_of_a_loop_is_cut_where_the_loop_ends() {
    let timeline = compile_clip_looping(PPQN * 4, Some(PPQN), vec![a_note(0, PPQN * 8, 60)]);
    assert_eq!(ons(&timeline), vec![0, PPQN, PPQN * 2, PPQN * 3]);
    assert_eq!(
        offs(&timeline),
        vec![PPQN, PPQN * 2, PPQN * 3, PPQN * 4],
        "each pass ends where the next begins"
    );
}

/// A note that fits inside the period keeps its own length — the cut is a
/// boundary, not a rule that every note ends with the bar.
#[test]
fn a_note_inside_the_period_keeps_its_length_in_every_pass() {
    let timeline = compile_clip_looping(PPQN * 4, Some(PPQN * 2), vec![a_note(0, PPQN, 60)]);
    assert_eq!(ons(&timeline), vec![0, PPQN * 2]);
    assert_eq!(offs(&timeline), vec![PPQN, PPQN * 3]);
}

/// The **last** pass is cut by whichever comes first, its own end or the
/// clip's: a loop cut short by the block it sits in stops there.
#[test]
fn the_last_pass_is_cut_by_the_clips_end_when_that_comes_first() {
    // Three beats of a two-beat loop: the second pass has one beat to run.
    let timeline = compile_clip_looping(PPQN * 3, Some(PPQN * 2), vec![a_note(0, PPQN * 2, 60)]);
    assert_eq!(ons(&timeline), vec![0, PPQN * 2]);
    assert_eq!(offs(&timeline), vec![PPQN * 2, PPQN * 3]);
}

/// A **slide** inside a loop is bounded the same way: it bends over its own
/// length, and the pass it is in ends the bend with it.
#[test]
fn a_slide_in_a_loop_is_cut_at_the_loop_point() {
    let mut long = a_note(0, PPQN * 8, 72);
    long.slide = true;
    let timeline = compile_clip_looping(PPQN * 2, Some(PPQN), vec![a_note(0, PPQN / 4, 60), long]);
    let glide: Vec<u32> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteSlide { glide_samples, .. } => Some(glide_samples),
            _ => None,
        })
        .collect();
    assert_eq!(
        glide,
        vec![(PPQN * SAMPLES_PER_TICK) as u32; 2],
        "one period each, in both passes"
    );
}
