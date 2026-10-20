//! Glue: touching notes on a key become one (FL's Glue, Live's Join).

mod tool_kit;

use fontelle_model::glued;
use tool_kit::{as_left, note, scope, shape};

#[test]
fn two_touching_notes_become_the_first_one_longer() {
    let s = scope(vec![note(0, 240, 60), note(240, 240, 60)]);
    let r = glued(&s);
    assert_eq!(shape(&s, &r), vec![(0, 480, 60)]);
    assert_eq!(r.remove, vec![s[1].0]);
    assert_eq!(
        as_left(&s, &r, s[0].0).unwrap().length,
        480,
        "the first keeps its id"
    );
}

#[test]
fn a_run_of_three_is_one_note_and_an_overlap_reaches_the_furthest_end() {
    let s = scope(vec![
        note(0, 240, 60),
        note(240, 400, 60),
        note(480, 100, 60),
    ]);
    assert_eq!(shape(&s, &glued(&s)), vec![(0, 640, 60)]);
}

#[test]
fn a_gap_or_another_key_is_not_glued() {
    let s = scope(vec![
        note(0, 240, 60),
        note(300, 240, 60),
        note(540, 240, 62),
    ]);
    assert!(glued(&s).is_empty());
}
