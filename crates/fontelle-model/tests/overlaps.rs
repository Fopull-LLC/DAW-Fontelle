//! Cleaning up overlaps — the hatched notes of 0.30.0, dealt with.

mod tool_kit;

use fontelle_model::{made_mono, trimmed_overlaps, without_doubles};
use tool_kit::{note, scope, shape};

#[test]
fn doubles_come_down_to_the_longest() {
    let s = scope(vec![note(0, 240, 60), note(0, 480, 60), note(0, 240, 62)]);
    let r = without_doubles(&s);
    assert_eq!(shape(&s, &r), vec![(0, 480, 60), (0, 240, 62)]);
    assert_eq!(r.remove, vec![s[0].0]);
}

#[test]
fn an_overlap_on_a_key_is_cut_back_to_meet_the_next() {
    let s = scope(vec![note(0, 480, 60), note(240, 240, 60), note(0, 960, 64)]);
    assert_eq!(
        shape(&s, &trimmed_overlaps(&s)),
        vec![(0, 240, 60), (0, 960, 64), (240, 240, 60)]
    );
}

#[test]
fn trimming_also_takes_doubles() {
    let s = scope(vec![note(0, 240, 60), note(0, 240, 60)]);
    assert_eq!(shape(&s, &trimmed_overlaps(&s)), vec![(0, 240, 60)]);
}

#[test]
fn mono_keeps_the_top_of_each_chord_and_one_note_at_a_time() {
    let s = scope(vec![
        note(0, 960, 60),
        note(0, 960, 64),
        note(0, 960, 67),
        note(480, 240, 62),
    ]);
    assert_eq!(
        shape(&s, &made_mono(&s)),
        vec![(0, 480, 67), (480, 240, 62)]
    );
}

#[test]
fn clean_notes_are_left_alone() {
    let s = scope(vec![note(0, 240, 60), note(240, 240, 62)]);
    assert!(without_doubles(&s).is_empty());
    assert!(trimmed_overlaps(&s).is_empty());
    assert!(made_mono(&s).is_empty());
}
