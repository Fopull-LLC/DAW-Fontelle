//! Transpose, grown: scale steps and a range to stay inside.

mod tool_kit;

use fontelle_model::transposed;
use tool_kit::{note, scope};

const C_MAJOR: u16 = 0b1010_1011_0101;

fn keys(k: &[u8], by: i32, key: Option<u16>, range: Option<(u8, u8)>) -> Vec<u8> {
    let s = scope(
        k.iter()
            .enumerate()
            .map(|(i, k)| note(i as i64 * 240, 240, *k))
            .collect(),
    );
    transposed(&s, by, key, range)
        .after(&s)
        .iter()
        .map(|n| n.key)
        .collect()
}

#[test]
fn by_semitones() {
    assert_eq!(keys(&[60, 64], 12, None, None), vec![72, 76]);
    assert_eq!(keys(&[60, 64], -1, None, None), vec![59, 63]);
}

#[test]
fn by_scale_steps_in_a_key() {
    assert_eq!(
        keys(&[60, 64, 71], 2, Some(C_MAJOR), None),
        vec![64, 67, 74]
    );
    assert_eq!(keys(&[60, 64], -1, Some(C_MAJOR), None), vec![59, 62]);
}

#[test]
fn a_note_outside_the_key_steps_to_the_nearest_inside() {
    assert_eq!(keys(&[61], 1, Some(C_MAJOR), None), vec![62]);
    assert_eq!(keys(&[61], -1, Some(C_MAJOR), None), vec![60]);
}

#[test]
fn a_range_folds_by_octaves() {
    assert_eq!(keys(&[60, 50], 24, None, Some((48, 72))), vec![72, 62]);
    assert_eq!(keys(&[60], -24, None, Some((48, 72))), vec![48]);
}

#[test]
fn nothing_leaves_the_keyboard_and_nothing_is_no_change() {
    assert_eq!(keys(&[120], 24, None, None), vec![127]);
    let s = scope(vec![note(0, 240, 60)]);
    assert!(transposed(&s, 0, None, None).is_empty());
}
