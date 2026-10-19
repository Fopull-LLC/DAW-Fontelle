//! "What scale am I in?" — the roll's *Find my scale…*.
//!
//! Ty, 2026-10-18: *"if you dont know the scale you are writing stuff in,
//! there should be a button inside of the scales section that says like
//! what scale am i writing ... a list of possible scales with the notes you
//! have ... the closeness percentage, and see why ... selecting it should
//! just set the piano roll to that scale."*
//!
//! The notes are weighed by how long they sound; each root and scale scores
//! the share of that weight inside it. The same seven notes are C major and
//! A minor, so which note sounds like home breaks the tie, and scales of the
//! same notes are one row.

use fontelle_types::{KeyScale, PPQN, ScaleNote, find_scales};

fn note(key: u8, beat: i64, beats: i64) -> ScaleNote {
    ScaleNote {
        key,
        start: beat * PPQN,
        length: beats * PPQN,
    }
}

/// A tune on the given keys, one beat each.
fn tune(keys: &[u8]) -> Vec<ScaleNote> {
    keys.iter()
        .enumerate()
        .map(|(i, k)| note(*k, i as i64, 1))
        .collect()
}

#[test]
fn a_c_major_tune_is_c_major_and_says_a_minor_is_the_same_notes() {
    let notes = tune(&[60, 62, 64, 65, 67, 69, 71, 72, 67, 64, 60]);
    let found = find_scales(&notes);
    let top = &found.rows[0];
    assert_eq!(top.key, KeyScale::new(0, "major"));
    assert!((top.fit - 1.0).abs() < 1e-6);
    assert!(top.out.is_empty());
    assert!(
        top.also.contains(&KeyScale::new(9, "natural-minor")),
        "{:?}",
        top.also
    );
    assert!(!found.few_notes);
}

/// The same notes, but A is home — first, last, longest, lowest.
#[test]
fn the_note_that_sounds_like_home_breaks_the_tie() {
    let mut notes = vec![note(57, 0, 4)];
    notes.extend(
        [60, 62, 64, 65, 67, 71, 72]
            .iter()
            .enumerate()
            .map(|(i, k)| note(*k, 4 + i as i64, 1)),
    );
    notes.push(note(69, 12, 4));
    let found = find_scales(&notes);
    assert_eq!(found.rows[0].key, KeyScale::new(9, "natural-minor"));
    assert!(found.rows[0].also.contains(&KeyScale::new(0, "major")));
}

/// One note outside the key: under 100 %, and the row says which.
#[test]
fn a_note_out_of_the_scale_is_named() {
    let mut notes = tune(&[60, 62, 64, 65, 67, 69, 71, 72, 64, 60]);
    notes.push(note(66, 20, 1)); // an F#
    let found = find_scales(&notes);
    let c = found
        .rows
        .iter()
        .find(|r| r.key == KeyScale::new(0, "major"))
        .expect("C major is still offered");
    assert!(c.fit < 1.0 && c.fit > 0.85, "{}", c.fit);
    assert_eq!(c.out, vec![6]);
}

/// Long F#s and no F: G major.
#[test]
fn weight_decides_between_neighbouring_keys() {
    let mut notes = tune(&[67, 69, 71, 72, 74, 76, 79]);
    notes.push(note(66, 10, 4));
    notes.push(note(67, 14, 4));
    let found = find_scales(&notes);
    assert_eq!(found.rows[0].key, KeyScale::new(7, "major"));
}

/// Four notes fit dozens of scales perfectly: said, not pretended.
#[test]
fn a_handful_of_notes_says_many_scales_fit() {
    let found = find_scales(&tune(&[60, 64, 67, 60]));
    assert!(found.few_notes);
    assert!(!found.rows.is_empty());
}

#[test]
fn no_notes_no_rows() {
    let found = find_scales(&[]);
    assert!(found.rows.is_empty());
    assert!(found.few_notes);
}

/// Simple scales before exotic ones of the same fit.
#[test]
fn the_familiar_scales_come_first() {
    let found = find_scales(&tune(&[60, 62, 64, 65, 67, 69, 71, 72, 60]));
    let names: Vec<&str> = found.rows.iter().map(|r| r.key.scale.as_str()).collect();
    assert_eq!(names[0], "major", "{names:?}");
    assert!(found.rows.len() <= fontelle_types::MOST_SCALE_ROWS);
}
