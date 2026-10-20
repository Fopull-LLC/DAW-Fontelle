//! Chop: notes cut into repeats. Ty, 2026-10-20: *"chop (lke repeated chops
//! basically)"*. FL Studio's Chop and Quick chop, Live 12's Chop.

mod tool_kit;

use fontelle_model::{ChopSpec, ChopVelocity, chopped, glued};
use fontelle_types::PPQN;
use tool_kit::{loud, note, scope, shape};

const SIXTEENTH: i64 = PPQN / 4;

fn by(step: i64) -> ChopSpec {
    ChopSpec {
        step,
        gate: 1.0,
        pattern: u16::MAX,
        pattern_len: 4,
        velocity: ChopVelocity::Flat,
        pitch_step: 0,
        on_grid: true,
    }
}

#[test]
fn a_beat_chopped_by_sixteenths_is_four_sixteenths() {
    let s = scope(vec![note(0, PPQN, 60)]);
    let r = chopped(&s, by(SIXTEENTH));
    assert_eq!(
        shape(&s, &r),
        vec![(0, 240, 60), (240, 240, 60), (480, 240, 60), (720, 240, 60)]
    );
    assert_eq!(r.remove, vec![s[0].0], "the note it was is gone");
}

#[test]
fn the_pattern_leaves_its_off_steps_silent() {
    let s = scope(vec![note(0, PPQN, 60)]);
    let mut spec = by(SIXTEENTH);
    spec.pattern = 0b0101;
    let r = chopped(&s, spec);
    assert_eq!(shape(&s, &r), vec![(0, 240, 60), (480, 240, 60)]);
}

#[test]
fn the_pattern_repeats_after_its_length() {
    let s = scope(vec![note(0, PPQN * 2, 60)]);
    let mut spec = by(SIXTEENTH);
    spec.pattern = 0b01;
    spec.pattern_len = 2;
    let r = chopped(&s, spec);
    let starts: Vec<i64> = r.after(&s).iter().map(|n| n.start).collect();
    assert_eq!(starts, vec![0, 480, 960, 1440]);
}

#[test]
fn the_gate_shortens_each_piece() {
    let s = scope(vec![note(0, PPQN, 60)]);
    let mut spec = by(SIXTEENTH);
    spec.gate = 0.5;
    let r = chopped(&s, spec);
    assert!(r.after(&s).iter().all(|n| n.length == 120));
}

#[test]
fn on_the_grid_the_cuts_fall_on_its_lines() {
    let s = scope(vec![note(100, 480, 60)]);
    let r = chopped(&s, by(SIXTEENTH));
    assert_eq!(
        shape(&s, &r),
        vec![(100, 140, 60), (240, 240, 60), (480, 100, 60)]
    );
}

#[test]
fn off_the_grid_the_cuts_follow_the_note() {
    let s = scope(vec![note(100, 480, 60)]);
    let mut spec = by(SIXTEENTH);
    spec.on_grid = false;
    let r = chopped(&s, spec);
    assert_eq!(shape(&s, &r), vec![(100, 240, 60), (340, 240, 60)]);
}

#[test]
fn a_pitch_step_makes_a_stair() {
    let s = scope(vec![note(0, PPQN, 60)]);
    let mut spec = by(SIXTEENTH);
    spec.pitch_step = 2;
    let keys: Vec<u8> = chopped(&s, spec).after(&s).iter().map(|n| n.key).collect();
    assert_eq!(keys, vec![60, 62, 64, 66]);
}

#[test]
fn velocity_falls_rises_or_accents_and_stays_a_note() {
    let s = scope(vec![loud(0, PPQN, 60, 100)]);
    let v = |shape: ChopVelocity| -> Vec<u8> {
        let mut spec = by(SIXTEENTH);
        spec.velocity = shape;
        chopped(&s, spec)
            .after(&s)
            .iter()
            .map(|n| n.velocity)
            .collect()
    };
    let fall = v(ChopVelocity::Fall);
    assert!(fall.windows(2).all(|w| w[0] > w[1]), "{fall:?}");
    let rise = v(ChopVelocity::Rise);
    assert!(rise.windows(2).all(|w| w[0] < w[1]), "{rise:?}");
    let accent = v(ChopVelocity::Accent);
    assert_eq!(accent[0], 100);
    assert!(accent[1..].iter().all(|x| *x < 100), "{accent:?}");
    for set in [fall, rise, accent] {
        assert!(set.iter().all(|x| (1..=127).contains(x)));
    }
}

#[test]
fn a_note_too_short_to_cut_is_left_alone() {
    let s = scope(vec![note(0, 200, 60)]);
    assert!(chopped(&s, by(SIXTEENTH)).is_empty());
}

#[test]
fn chop_then_glue_gives_the_note_back() {
    let s = scope(vec![note(0, PPQN, 60), note(PPQN * 2, PPQN * 2, 64)]);
    let pieces = chopped(&s, by(SIXTEENTH)).after(&s);
    let again = scope(pieces);
    let back = glued(&again);
    assert_eq!(
        shape(&again, &back),
        vec![(0, PPQN, 60), (PPQN * 2, PPQN * 2, 64)]
    );
}
