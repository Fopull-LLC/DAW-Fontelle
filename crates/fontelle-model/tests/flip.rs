//! Flip: mirrored in time or in pitch (FL's Flip, Live's Reverse/Invert).

mod tool_kit;

use fontelle_model::{flipped_in_pitch, flipped_in_time};
use fontelle_types::NoteId;
use tool_kit::{note, scope, shape};

const C_MAJOR: u16 = 0b1010_1011_0101;

fn reapplied(
    s: &[(NoteId, fontelle_model::Note)],
    f: impl Fn(&[(NoteId, fontelle_model::Note)]) -> fontelle_model::ToolResult,
) -> Vec<(i64, i64, u8)> {
    let once = scope(f(s).after(s));
    let twice = f(&once);
    shape(&once, &twice)
}

#[test]
fn in_time_the_last_note_comes_first() {
    let s = scope(vec![note(0, 240, 60), note(480, 240, 62)]);
    let r = flipped_in_time(&s, false);
    assert_eq!(shape(&s, &r), vec![(0, 240, 62), (480, 240, 60)]);
}

#[test]
fn a_mirror_moves_starts_and_keeping_the_grid_does_not() {
    let s = scope(vec![note(0, 100, 60), note(480, 240, 62)]);
    assert_eq!(
        shape(&s, &flipped_in_time(&s, false)),
        vec![(0, 240, 62), (620, 100, 60)]
    );
    assert_eq!(
        shape(&s, &flipped_in_time(&s, true)),
        vec![(0, 240, 62), (480, 100, 60)]
    );
}

#[test]
fn flipping_twice_in_time_is_where_it_started() {
    let s = scope(vec![
        note(120, 100, 60),
        note(480, 240, 62),
        note(480, 240, 65),
    ]);
    let want = shape(&s, &Default::default());
    assert_eq!(reapplied(&s, |s| flipped_in_time(s, false)), want);
    assert_eq!(reapplied(&s, |s| flipped_in_time(s, true)), want);
}

#[test]
fn in_pitch_the_top_becomes_the_bottom() {
    let s = scope(vec![
        note(0, 240, 60),
        note(240, 240, 64),
        note(480, 240, 67),
    ]);
    let keys: Vec<u8> = flipped_in_pitch(&s, None)
        .after(&s)
        .iter()
        .map(|n| n.key)
        .collect();
    assert_eq!(keys, vec![67, 63, 60]);
}

#[test]
fn in_a_key_it_mirrors_by_scale_steps_and_stays_in_the_key() {
    let s = scope(vec![
        note(0, 240, 60),
        note(240, 240, 64),
        note(480, 240, 67),
    ]);
    let keys: Vec<u8> = flipped_in_pitch(&s, Some(C_MAJOR))
        .after(&s)
        .iter()
        .map(|n| n.key)
        .collect();
    assert_eq!(keys, vec![67, 64, 60]);
}

#[test]
fn flipping_twice_in_pitch_is_where_it_started() {
    let s = scope(vec![
        note(0, 240, 60),
        note(240, 240, 65),
        note(480, 240, 71),
    ]);
    let want = shape(&s, &Default::default());
    assert_eq!(reapplied(&s, |s| flipped_in_pitch(s, None)), want);
    assert_eq!(reapplied(&s, |s| flipped_in_pitch(s, Some(C_MAJOR))), want);
}

#[test]
fn a_slide_turns_upside_down_with_its_note() {
    let mut sliding = note(0, 480, 60);
    sliding.path = vec![fontelle_model::PathPoint { at: 240, offset: 3 }];
    let s = scope(vec![sliding, note(0, 240, 72)]);
    let after = flipped_in_pitch(&s, None).after(&s);
    let flipped = after.iter().find(|n| !n.path.is_empty()).unwrap();
    assert_eq!(flipped.key, 72);
    assert_eq!(flipped.path[0].offset, -3);
}
