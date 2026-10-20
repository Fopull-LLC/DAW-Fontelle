//! Where two notes on one key sound over each other.
//!
//! Ty, 2026-10-20: *"add subtle cross pattern showing when notes overlap in
//! the piano roll."* Two notes on the same key that overlap are drawn as one
//! long block, and the second strike is invisible; the roll cross-hatches
//! the stretch they share.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN};
use fontelle_ui::canvas::note_overlaps;

fn note(start: i64, length: i64, key: u8) -> Note {
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
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

fn arena(notes: &[Note]) -> Arena<NoteId, Note> {
    let mut a = Arena::default();
    for n in notes {
        a.insert(n.clone());
    }
    a
}

#[test]
fn two_notes_on_one_key_share_the_stretch_they_overlap() {
    let a = arena(&[note(0, PPQN, 60), note(PPQN / 2, PPQN, 60)]);
    assert_eq!(note_overlaps(&a), vec![(60, PPQN / 2, PPQN)]);
}

#[test]
fn touching_notes_and_other_keys_do_not_overlap() {
    let a = arena(&[
        note(0, PPQN, 60),
        note(PPQN, PPQN, 60),
        note(0, PPQN * 2, 64),
        note(0, PPQN * 2, 67),
    ]);
    assert!(note_overlaps(&a).is_empty());
}

/// Three on one key: each stretch more than one note sounds, merged, so the
/// hatch is not drawn twice over itself.
#[test]
fn three_deep_is_one_stretch() {
    let a = arena(&[
        note(0, PPQN * 2, 60),
        note(PPQN / 2, PPQN / 2, 60),
        note(PPQN / 2 + 10, PPQN, 60),
    ]);
    assert_eq!(note_overlaps(&a), vec![(60, PPQN / 2, PPQN * 3 / 2 + 10)]);
}

/// A note inside another, and the same note written twice.
#[test]
fn a_note_inside_another_and_a_doubled_note() {
    let a = arena(&[note(0, PPQN * 4, 48), note(PPQN, PPQN, 48)]);
    assert_eq!(note_overlaps(&a), vec![(48, PPQN, PPQN * 2)]);
    let b = arena(&[note(PPQN, PPQN, 50), note(PPQN, PPQN, 50)]);
    assert_eq!(note_overlaps(&b), vec![(50, PPQN, PPQN * 2)]);
}
