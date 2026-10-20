//! Flam and grace notes (FL's Flam, Live 12's Ornament).

mod tool_kit;

use fontelle_model::{FlamPitch, FlamSpec, flammed};
use fontelle_types::PPQN;
use tool_kit::{loud, note, scope, shape};

const C_MAJOR: u16 = 0b1010_1011_0101;

fn spec() -> FlamSpec {
    FlamSpec {
        time: 60,
        before: true,
        velocity: 60,
        pitch: FlamPitch::Same,
        count: 1,
    }
}

#[test]
fn a_flam_is_a_softer_short_note_just_before() {
    let s = scope(vec![loud(PPQN, 240, 60, 100)]);
    let r = flammed(&s, spec(), None);
    assert!(
        r.rewrite.is_empty() && r.remove.is_empty(),
        "the note stays"
    );
    assert_eq!(r.insert.len(), 1);
    let grace = &r.insert[0];
    assert_eq!(
        (grace.start, grace.length, grace.key, grace.velocity),
        (PPQN - 60, 60, 60, 60)
    );
}

#[test]
fn on_the_beat_the_grace_takes_the_start_and_the_note_moves_over() {
    let s = scope(vec![note(PPQN, 240, 60)]);
    let mut on = spec();
    on.before = false;
    assert_eq!(
        shape(&s, &flammed(&s, on, None)),
        vec![(PPQN, 60, 60), (PPQN + 60, 180, 60)]
    );
}

#[test]
fn a_note_with_no_room_before_it_makes_room() {
    let s = scope(vec![note(0, 240, 60)]);
    assert_eq!(
        shape(&s, &flammed(&s, spec(), None)),
        vec![(0, 60, 60), (60, 180, 60)]
    );
}

#[test]
fn above_and_below_are_a_scale_step_in_a_key_and_two_semitones_without() {
    let s = scope(vec![note(PPQN, 240, 64)]);
    let key = |pitch, k| {
        let mut f = spec();
        f.pitch = pitch;
        flammed(&s, f, k).insert[0].key
    };
    assert_eq!(key(FlamPitch::Above, Some(C_MAJOR)), 65);
    assert_eq!(key(FlamPitch::Below, Some(C_MAJOR)), 62);
    assert_eq!(key(FlamPitch::Above, None), 66);
    assert_eq!(key(FlamPitch::Below, None), 62);
}

#[test]
fn several_grace_notes_lead_in_one_after_another() {
    let s = scope(vec![note(PPQN, 240, 60)]);
    let mut drag = spec();
    drag.count = 3;
    let starts: Vec<i64> = flammed(&s, drag, None)
        .insert
        .iter()
        .map(|n| n.start)
        .collect();
    assert_eq!(starts, vec![PPQN - 180, PPQN - 120, PPQN - 60]);
}
