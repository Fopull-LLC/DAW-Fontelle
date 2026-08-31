//! Turning a captured take into notes (TDD §14.7).
//!
//! The conversion is a pure function on purpose: everything that decides what
//! a recording *is* — where a note starts, how long it lasts, what happens to
//! one still held when the tape stops — is decided here, where it can be
//! tested, rather than inside a callback that only a sound card can run.

use fontelle_model::{ClipSource, TempoMap, notes_from_capture};
use fontelle_types::{EventPayload, NodeId, PPQN, TimedEvent};

const SR: f64 = 48_000.0;

fn tempo() -> TempoMap {
    // 120 bpm at 48 kHz: a quarter note is exactly 24 000 samples, so every
    // number below is exact rather than approximately right.
    TempoMap::new(120.0, SR)
}

fn on(sample: i64, key: u8, velocity: u8) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key,
            velocity,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: u32::MAX,
        },
    }
}

fn off(sample: i64, key: u8) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: EventPayload::NoteOff {
            key,
            voice_context: u32::MAX,
        },
    }
}

/// Every note, sorted, as (start, length, key, velocity) in ticks.
fn notes(source: &ClipSource) -> Vec<(i64, i64, u8, u8)> {
    let ClipSource::Notes(data) = source else {
        unreachable!("this is a note clip")
    };
    let mut out: Vec<_> = data
        .notes
        .values()
        .map(|n| (n.start, n.length, n.key, n.velocity))
        .collect();
    out.sort();
    out
}

#[test]
fn a_note_on_and_its_note_off_become_one_note() {
    let clip = notes_from_capture(&[on(0, 60, 100), off(24_000, 60)], &tempo(), 0, 48_000);
    assert_eq!(notes(&clip), vec![(0, PPQN, 60, 100)]);
}

#[test]
fn positions_go_through_the_tempo_map_rather_than_arithmetic_on_the_bpm() {
    // INVARIANT 5, and not only on principle: a take over a piece that changes
    // tempo has no single bpm to divide by.
    let map = TempoMap::from_segments(
        vec![
            fontelle_model::TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            },
            fontelle_model::TempoSegment {
                start_tick: PPQN * 4,
                bpm: 60.0,
            },
        ],
        SR,
    );
    // Four quarter notes at 120 bpm is 96 000 samples; one more at 60 bpm is
    // another 48 000.
    let clip = notes_from_capture(&[on(96_000, 72, 90), off(144_000, 72)], &map, 0, 192_000);
    assert_eq!(notes(&clip), vec![(PPQN * 4, PPQN, 72, 90)]);
}

#[test]
fn notes_are_placed_relative_to_the_clip_they_land_in() {
    // Recording from bar 5 puts the take in a clip at bar 5, and a note's
    // start is relative to its clip (TDD §10.4).
    let clip = notes_from_capture(
        &[on(96_000, 60, 100), off(120_000, 60)],
        &tempo(),
        PPQN * 4,
        192_000,
    );
    assert_eq!(notes(&clip), vec![(0, PPQN, 60, 100)]);
}

#[test]
fn a_note_still_held_when_recording_stops_ends_there() {
    // Letting it hang forever, or dropping it, are both worse: a held chord at
    // the end of a take is a normal way to finish playing.
    let clip = notes_from_capture(&[on(0, 60, 100)], &tempo(), 0, 36_000);
    assert_eq!(notes(&clip), vec![(0, PPQN / 2 * 3, 60, 100)]);
}

#[test]
fn the_same_key_played_twice_becomes_two_notes() {
    let clip = notes_from_capture(
        &[
            on(0, 60, 100),
            off(12_000, 60),
            on(24_000, 60, 40),
            off(36_000, 60),
        ],
        &tempo(),
        0,
        48_000,
    );
    assert_eq!(
        notes(&clip),
        vec![(0, PPQN / 2, 60, 100), (PPQN, PPQN / 2, 60, 40)]
    );
}

#[test]
fn a_key_retriggered_before_its_note_off_ends_the_first_note() {
    // A keyboard that repeats, or two events reordered by the block stamping.
    // Two overlapping notes on one key is a shape the piano roll cannot draw
    // and the sampler cannot voice sensibly.
    let clip = notes_from_capture(
        &[on(0, 60, 100), on(24_000, 60, 60), off(48_000, 60)],
        &tempo(),
        0,
        96_000,
    );
    assert_eq!(notes(&clip), vec![(0, PPQN, 60, 100), (PPQN, PPQN, 60, 60)]);
}

#[test]
fn a_zero_velocity_note_on_ends_a_note_rather_than_starting_one() {
    // The MIDI decoder already applies this rule, so a take through a real
    // keyboard never carries one. A capture from anywhere else might, and
    // reading it literally starts a silent note that never ends.
    let clip = notes_from_capture(&[on(0, 60, 100), on(24_000, 60, 0)], &tempo(), 0, 48_000);
    assert_eq!(notes(&clip), vec![(0, PPQN, 60, 100)]);
}

#[test]
fn a_note_off_for_a_key_that_was_never_played_is_ignored() {
    // Recording started with a key already held: its note-off arrives with no
    // note-on in front of it.
    let clip = notes_from_capture(
        &[off(1_000, 60), on(24_000, 64, 90), off(48_000, 64)],
        &tempo(),
        0,
        96_000,
    );
    assert_eq!(notes(&clip), vec![(PPQN, PPQN, 64, 90)]);
}

#[test]
fn a_note_shorter_than_a_tick_still_has_a_length() {
    // A note-on and its note-off inside one block round to the same tick. A
    // zero-length note is a note-on and note-off on the same sample, which the
    // sequencer emits and the sampler cannot sound — the fastest possible stab
    // would silently vanish from the take.
    let clip = notes_from_capture(&[on(1_000, 60, 100), off(1_010, 60)], &tempo(), 0, 48_000);
    let notes = notes(&clip);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].1 >= 1, "got a note of length {}", notes[0].1);
}

#[test]
fn an_empty_take_is_an_empty_clip_rather_than_nothing() {
    // The caller decides whether to keep it; a `None` here would make "did
    // anything get recorded" the conversion's judgement instead.
    let clip = notes_from_capture(&[], &tempo(), 0, 48_000);
    assert!(notes(&clip).is_empty());
}

#[test]
fn a_note_played_before_the_clip_starts_is_dropped_rather_than_wrapped() {
    // Negative starts are a real possibility with a count-in, and a note at a
    // negative offset inside a clip is not something the piano roll can show.
    let clip = notes_from_capture(
        &[
            on(0, 60, 100),
            off(24_000, 60),
            on(96_000, 64, 90),
            off(120_000, 64),
        ],
        &tempo(),
        PPQN * 4,
        192_000,
    );
    assert_eq!(notes(&clip), vec![(0, PPQN, 64, 90)]);
}
