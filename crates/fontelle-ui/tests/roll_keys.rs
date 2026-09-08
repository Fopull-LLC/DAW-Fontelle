//! Editing the selection from the keyboard.
//!
//! Reported from using the window: *"we need to ensure our keybinds are
//! expansive so like in the piano roll, I should be able to do ctrl and up or
//! down arrow to move a selection in the piano roll up or down an octave."*
//!
//! The roll had no arrow keys at all — the only way to move a note was to drag
//! it, which is fine for a phrase you are placing and hopeless for one you are
//! correcting. Everything here is the same edit a drag produces, so the same
//! clamps hold and the same undo entry results; what the *window* binds each
//! one to is [`crate::app`]'s business and is listed there.
//!
//! The clamps are the interesting half. A transpose is all-or-nothing in the
//! document — `MoveNotes` moves every named note or none — so a chord with one
//! note near the top of the keyboard has to be clamped by the roll, or pressing
//! up does nothing at all and looks broken.

use fontelle_model::{Arena, Note, NoteProperty};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{LaneProperty, PianoRoll, RollEdit, RollView, SnapDivision};

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
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
        channel: None,
    }
}

fn notes(items: &[(Tick, Tick, u8)]) -> (Arena<NoteId, Note>, Vec<NoteId>) {
    let mut arena = Arena::default();
    let ids = items
        .iter()
        .map(|(start, length, key)| arena.insert(note(*start, *length, *key)))
        .collect();
    (arena, ids)
}

fn apply(arena: &mut Arena<NoteId, Note>, edits: &[RollEdit]) {
    for edit in edits {
        match edit {
            RollEdit::Move {
                ids,
                tick_delta,
                key_delta,
            } => {
                for id in ids {
                    if let Some(n) = arena.get_mut(*id) {
                        n.start = (n.start + tick_delta).max(0);
                        n.key = (i16::from(n.key) + key_delta).clamp(0, 127) as u8;
                    }
                }
            }
            RollEdit::Resize { ids, tick_delta } => {
                for id in ids {
                    if let Some(n) = arena.get_mut(*id) {
                        n.length = (n.length + tick_delta).max(1);
                    }
                }
            }
            RollEdit::SetProperty {
                ids,
                property,
                value,
            } => {
                for id in ids {
                    if let Some(n) = arena.get_mut(*id) {
                        property.set(n, *value);
                    }
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------ transpose ---

#[test]
fn the_selection_moves_a_semitone_at_a_time() {
    let (mut arena, ids) = notes(&[(0, PPQN, 60), (0, PPQN, 64)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge(&arena, 0, 1);
    apply(&mut arena, &edits);
    assert_eq!((arena[ids[0]].key, arena[ids[1]].key), (61, 65));

    let edits = roll.nudge(&arena, 0, -1);
    apply(&mut arena, &edits);
    assert_eq!((arena[ids[0]].key, arena[ids[1]].key), (60, 64));
}

#[test]
fn an_octave_is_the_same_edit_twelve_times_over() {
    // What Ctrl+Up is bound to. The roll does not know about Ctrl; it knows
    // about twelve.
    let (mut arena, ids) = notes(&[(0, PPQN, 60)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge(&arena, 0, 12);
    apply(&mut arena, &edits);
    assert_eq!(arena[ids[0]].key, 72);
}

#[test]
fn a_chord_that_would_run_off_the_top_moves_as_far_as_it_can() {
    // `MoveNotes` is all-or-nothing: unclamped, one note at 120 stops the
    // whole chord dead and pressing Ctrl+Up simply does nothing.
    let (mut arena, ids) = notes(&[(0, PPQN, 60), (0, PPQN, 120)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge(&arena, 0, 12);
    apply(&mut arena, &edits);
    assert_eq!(
        (arena[ids[0]].key, arena[ids[1]].key),
        (67, 127),
        "the shape has to survive the clamp"
    );

    assert!(
        roll.nudge(&arena, 0, 12).is_empty(),
        "and once it is against the ceiling there is nothing left to ask for"
    );
}

#[test]
fn a_chord_at_the_bottom_of_the_keyboard_is_clamped_the_same_way() {
    let (mut arena, ids) = notes(&[(0, PPQN, 3), (0, PPQN, 40)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge(&arena, 0, -12);
    apply(&mut arena, &edits);
    assert_eq!((arena[ids[0]].key, arena[ids[1]].key), (0, 37));
}

// ----------------------------------------------------------------- time ---

#[test]
fn the_selection_slides_by_one_snap_unit() {
    let (mut arena, ids) = notes(&[(PPQN, PPQN, 60)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let step = roll.step(4);
    assert_eq!(step, PPQN / 4, "a sixteenth, which is what the snap says");

    let edits = roll.nudge(&arena, step, 0);
    apply(&mut arena, &edits);
    assert_eq!(arena[ids[0]].start, PPQN + PPQN / 4);
}

#[test]
fn the_step_follows_the_snap_and_is_never_zero() {
    let mut roll = PianoRoll::new(view());
    roll.view.snap = SnapDivision::Bar;
    assert_eq!(roll.step(4), PPQN * 4);
    roll.view.snap = SnapDivision::Beat;
    assert_eq!(roll.step(4), PPQN);
    // With the snap off an arrow key still has to mean *something*, or the
    // keyboard stops working the moment you turn free positioning on.
    roll.view.snap = SnapDivision::None;
    assert!(roll.step(4) > 0);
}

#[test]
fn nothing_is_ever_nudged_before_the_start_of_the_clip() {
    let (mut arena, ids) = notes(&[(PPQN / 4, PPQN, 60), (PPQN * 4, PPQN, 62)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge(&arena, -PPQN, 0);
    apply(&mut arena, &edits);
    assert_eq!(
        (arena[ids[0]].start, arena[ids[1]].start),
        (0, PPQN * 4 - PPQN / 4),
        "the phrase keeps its shape and stops at bar one"
    );
    assert!(roll.nudge(&arena, -PPQN, 0).is_empty());
}

// --------------------------------------------------------------- length ---

#[test]
fn shift_arrows_lengthen_and_shorten() {
    let (mut arena, ids) = notes(&[(0, PPQN, 60)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let step = roll.step(4);
    let edits = roll.resize_selection(&arena, step);
    apply(&mut arena, &edits);
    assert_eq!(arena[ids[0]].length, PPQN + PPQN / 4);

    let edits = roll.resize_selection(&arena, -step);
    apply(&mut arena, &edits);
    assert_eq!(arena[ids[0]].length, PPQN);
}

#[test]
fn a_note_is_never_shortened_past_nothing() {
    let (mut arena, ids) = notes(&[(0, PPQN / 4, 60), (0, PPQN * 4, 62)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    for _ in 0..20 {
        let edits = roll.resize_selection(&arena, -(PPQN / 4));
        apply(&mut arena, &edits);
    }
    assert!(
        arena[ids[0]].length >= 1 && arena[ids[1]].length >= 1,
        "got {} and {}",
        arena[ids[0]].length,
        arena[ids[1]].length
    );
}

// ------------------------------------------------------------- property ---

#[test]
fn the_lane_property_can_be_bumped_from_the_keyboard() {
    // Velocity by ear rather than by dragging a three-pixel bar.
    let (mut arena, ids) = notes(&[(0, PPQN, 60), (0, PPQN, 64)]);
    arena[ids[1]].velocity = 40;
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge_property(&arena, 10);
    apply(&mut arena, &edits);
    assert_eq!(
        (arena[ids[0]].velocity, arena[ids[1]].velocity),
        (110, 50),
        "each note keeps its own value — a bump is not a flatten"
    );
}

#[test]
fn a_bumped_property_is_clamped_to_what_the_model_allows() {
    let (mut arena, ids) = notes(&[(0, PPQN, 60)]);
    arena[ids[0]].velocity = 125;
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    let edits = roll.nudge_property(&arena, 10);
    apply(&mut arena, &edits);
    let (_, max) = NoteProperty::Velocity.range();
    assert_eq!(i32::from(arena[ids[0]].velocity), max);
    assert!(
        roll.nudge_property(&arena, 10).is_empty(),
        "already at the ceiling: nothing to ask for"
    );
}

#[test]
fn it_bumps_whichever_property_the_lane_is_showing() {
    let (mut arena, ids) = notes(&[(0, PPQN, 60)]);
    let mut roll = PianoRoll::new(view());
    roll.lane_property = LaneProperty::Pan;
    roll.select(ids.clone());

    let edits = roll.nudge_property(&arena, -20);
    apply(&mut arena, &edits);
    assert_eq!(arena[ids[0]].pan, -20);
    assert_eq!(arena[ids[0]].velocity, 100, "and nothing else moved");
}

// -------------------------------------------------------------- nothing ---

#[test]
fn a_keystroke_with_nothing_selected_is_not_an_undo_entry() {
    let (arena, _) = notes(&[(0, PPQN, 60)]);
    let mut roll = PianoRoll::new(view());
    assert!(roll.nudge(&arena, PPQN, 0).is_empty());
    assert!(roll.resize_selection(&arena, PPQN).is_empty());
    assert!(roll.nudge_property(&arena, 1).is_empty());
}

#[test]
fn a_nudge_is_silent_because_it_is_editing() {
    // It used to sound the pitch it landed on, on the principle that you hear
    // what you touch. Reported from using the window, and it applies to an
    // arrow key exactly as much as to a drag: *"i should only be played a
    // preview if i bare clicked on the note not if im just editing at all...
    // ill constantly be hearing wrong notes just because i moved a note
    // around."* An arrow key held down over a rolling transport machine-guns
    // for the same reason a drag did.
    let (arena, ids) = notes(&[(0, PPQN, 60)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());

    roll.nudge(&arena, 0, 4);
    assert_eq!(roll.take_audition().map(|a| a.key), None);

    roll.nudge(&arena, PPQN, 0);
    assert_eq!(roll.take_audition().map(|a| a.key), None);
}

// ------------------------------------------------------------- legato ---
//
// > *"if i press ctrl l with a note selection in the piano roll it makes all
// > the notes lengths not have gaps like how it does in fl studio with that
// > same keybind. just makes all the notes cleanly connect to eachother
// > basically in length."*
//
// FL's Quick Legato. The arithmetic is `fontelle_model::legato_lengths` and is
// tested there; this is the roll's half — that it acts on the **selection**,
// that it is one edit, and that it asks for nothing when there is nothing to
// close up.

fn lengths(edits: &[RollEdit], ids: &[NoteId]) -> Vec<Tick> {
    let [
        RollEdit::SetLengths {
            ids: named,
            lengths,
        },
    ] = edits
    else {
        panic!("expected one length edit, got {edits:?}")
    };
    ids.iter()
        .map(|id| {
            let at = named
                .iter()
                .position(|hit| hit == id)
                .unwrap_or_else(|| panic!("{id:?} was not named by {edits:?}"));
            lengths[at]
        })
        .collect()
}

#[test]
fn legato_closes_the_gaps_between_the_selected_notes() {
    let (arena, ids) = notes(&[
        (0, PPQN / 4, 60),
        (PPQN, PPQN / 4, 62),
        (PPQN * 2, PPQN / 4, 64),
    ]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());
    let edits = roll.legato(&arena);
    assert_eq!(lengths(&edits, &ids), vec![PPQN, PPQN, PPQN / 4]);
}

#[test]
fn legato_only_touches_what_is_selected() {
    // The note left out is neither moved nor treated as the thing the one
    // before it should reach: a selection is a statement about which notes
    // this is *about*.
    let (arena, ids) = notes(&[
        (0, PPQN / 4, 60),
        (PPQN, PPQN / 4, 62),
        (PPQN * 2, PPQN / 4, 64),
    ]);
    let mut roll = PianoRoll::new(view());
    roll.select(vec![ids[0], ids[2]]);
    let edits = roll.legato(&arena);
    let [
        RollEdit::SetLengths {
            ids: named,
            lengths,
        },
    ] = &edits[..]
    else {
        panic!("expected one length edit, got {edits:?}")
    };
    assert!(!named.contains(&ids[1]), "an unselected note was resized");
    let at = named.iter().position(|hit| *hit == ids[0]).unwrap();
    assert_eq!(
        lengths[at],
        PPQN * 2,
        "the first note reaches the next *selected* one"
    );
}

#[test]
fn legato_with_nothing_selected_asks_for_nothing() {
    let (arena, _) = notes(&[(0, PPQN / 4, 60), (PPQN, PPQN / 4, 62)]);
    let mut roll = PianoRoll::new(view());
    assert!(roll.legato(&arena).is_empty());
}

#[test]
fn legato_on_a_phrase_that_is_already_joined_up_asks_for_nothing() {
    // An edit that changes nothing is not an undo entry — the rule every
    // other tool in this window keeps, and the reason pressing Ctrl+L twice
    // does not cost two presses of Ctrl+Z.
    let (arena, ids) = notes(&[(0, PPQN, 60), (PPQN, PPQN, 62), (PPQN * 2, PPQN, 64)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids);
    assert!(roll.legato(&arena).is_empty());
}

#[test]
fn legato_on_one_note_asks_for_nothing() {
    // There is nothing after it to touch, so there is nothing to do.
    let (arena, ids) = notes(&[(PPQN, PPQN / 4, 60)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids);
    assert!(roll.legato(&arena).is_empty());
}

#[test]
fn legato_is_one_edit_however_many_lengths_it_writes() {
    // One press of the tool is one thing somebody did, and one press of
    // Ctrl+Z has to take all of it back.
    let (arena, ids) = notes(&[
        (0, 10, 60),
        (PPQN, 10, 62),
        (PPQN * 3, 10, 64),
        (PPQN * 7, 10, 65),
    ]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids.clone());
    let edits = roll.legato(&arena);
    assert_eq!(edits.len(), 1, "got {edits:?}");
    assert_eq!(
        lengths(&edits, &ids),
        vec![PPQN, PPQN * 2, PPQN * 4, 10],
        "each reaches the next, and the last keeps what it had"
    );
}

#[test]
fn legato_is_silent_because_it_is_editing() {
    // The same rule an arrow key follows: *"i should only be played a preview
    // if i bare clicked on the note not if im just editing at all."*
    let (arena, ids) = notes(&[(0, PPQN / 4, 60), (PPQN, PPQN / 4, 62)]);
    let mut roll = PianoRoll::new(view());
    roll.select(ids);
    roll.legato(&arena);
    assert_eq!(roll.take_audition().map(|a| a.key), None);
}
