//! One clipboard of notes for the whole window (`docs/analyze-musically-plan.md`
//! §3.6): what the piano roll copies, and what Analyze Musically puts there
//! for the roll to paste.
//!
//! The roll's own copy and paste are pinned in `roll_interaction.rs` and must
//! not change; these are what the shared clipboard adds — notes put there by
//! somebody else, and *Paste at original position*.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{NoteClipboard, PianoRoll, RollEdit, RollView, SnapDivision};

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn note(start: Tick, length: Tick, key: u8) -> Note {
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
    }
}

fn arena(items: &[(Tick, Tick, u8)]) -> Arena<NoteId, Note> {
    let mut arena = Arena::default();
    for (start, length, key) in items {
        arena.insert(note(*start, *length, *key));
    }
    arena
}

fn inserted(edits: &[RollEdit]) -> &[Note] {
    match edits {
        [RollEdit::Insert(notes)] => notes,
        other => panic!("expected one insert, got {other:?}"),
    }
}

#[test]
fn the_clipboard_holds_a_phrase_from_zero_and_where_it_came_from() {
    let mut clipboard = NoteClipboard::new();
    assert!(clipboard.is_empty());
    assert_eq!(clipboard.origin(), None);
    // Notes as Analyze hands them over: in song ticks, out of order.
    clipboard.put(
        vec![note(PPQN * 9 + 5, PPQN, 64), note(PPQN * 8 + 17, PPQN, 60)],
        Some(PPQN * 8 + 17),
    );
    assert_eq!(clipboard.len(), 2);
    let starts: Vec<Tick> = clipboard.notes().iter().map(|n| n.start).collect();
    assert_eq!(starts, vec![0, PPQN - 12], "sorted, earliest at zero");
    assert_eq!(clipboard.origin(), Some(PPQN * 8 + 17));
}

#[test]
fn the_rolls_copy_fills_the_shared_clipboard_with_no_origin() {
    let mut roll = PianoRoll::new(view());
    roll.clipboard_mut()
        .put(vec![note(0, PPQN, 50)], Some(PPQN * 3));
    let notes = arena(&[(PPQN * 4, PPQN, 60), (PPQN * 5, PPQN / 2, 64)]);
    roll.select_all(&notes);
    assert_eq!(roll.copy(&notes), 2);
    assert_eq!(roll.clipboard().len(), 2);
    assert_eq!(
        roll.clipboard().origin(),
        None,
        "a phrase copied in the roll has no place in the song to go back to"
    );
    assert_eq!(roll.clipboard().notes()[0].start, 0);
}

#[test]
fn notes_from_analyze_paste_snapped_like_any_others() {
    let mut roll = PianoRoll::new(view());
    roll.clipboard_mut().put(
        vec![note(PPQN * 2 + 31, PPQN, 57), note(PPQN * 3 + 2, PPQN, 60)],
        Some(PPQN * 2 + 31),
    );
    let edits = roll.paste(PPQN * 8 + 17, 4);
    let pasted = inserted(&edits);
    assert_eq!(pasted.len(), 2);
    // The first on the grid (a step is a sixteenth), the rest as played.
    assert_eq!(pasted[0].start % (PPQN / 4), 0, "{pasted:?}");
    assert_eq!(pasted[1].start - pasted[0].start, PPQN - 29);
}

#[test]
fn paste_at_original_position_lands_under_the_audio() {
    let mut roll = PianoRoll::new(view());
    // Analyze copied two notes heard at song ticks 2 bars + 17 and later.
    let origin = PPQN * 8 + 17;
    roll.clipboard_mut().put(
        vec![note(origin, PPQN, 57), note(origin + PPQN + 3, PPQN, 60)],
        Some(origin),
    );
    // The open clip starts a bar in: the notes land at their song time,
    // counted from the clip's start, and are not snapped.
    let clip_start = PPQN * 4;
    let edits = roll.paste_at_origin(clip_start);
    let pasted = inserted(&edits);
    assert_eq!(pasted[0].start, origin - clip_start);
    assert_eq!(pasted[1].start, origin - clip_start + PPQN + 3);

    // A phrase copied in the roll has no origin, so there is nothing to do.
    let notes = arena(&[(0, PPQN, 60)]);
    roll.select_all(&notes);
    roll.copy(&notes);
    assert!(roll.paste_at_origin(clip_start).is_empty());
}

#[test]
fn a_note_before_the_clip_starts_is_not_pasted_before_zero() {
    let mut roll = PianoRoll::new(view());
    roll.clipboard_mut().put(
        vec![note(PPQN, PPQN, 57), note(PPQN * 6, PPQN, 60)],
        Some(PPQN),
    );
    let edits = roll.paste_at_origin(PPQN * 4);
    let pasted = inserted(&edits);
    assert_eq!(
        pasted.len(),
        1,
        "the note heard before the clip begins has nowhere to go"
    );
    assert_eq!(pasted[0].start, PPQN * 2);
}
