//! Why note editing flickered.
//!
//! Reported from using the window: *"when placing notes while playing it
//! jitters and glitches out and changes the size of my notes. Sometimes it just
//! does that in general and it's weird."*
//!
//! One root cause, in two places. A drag emits **deltas relative to the
//! previous step of the same drag**, because that is what `MoveNotes` and
//! `ResizeNotes` take and what lets a drag coalesce into one undo entry. But
//! both clamps — "a note cannot go before the start of the clip" and "a note
//! cannot be shortened past nothing" — were recomputed **from the live
//! document**, which the drag is itself changing. So:
//!
//! 1. Drag a note at bar 3 hard left. `earliest` is bar 3, the clamp allows
//!    -3 bars, the note lands on bar 1.
//! 2. Next step, `earliest` is now bar 1, so the clamp allows **0**, and
//!    `wanted` becomes 0 against an `applied` of -3 bars — a delta of **+3
//!    bars**. The note jumps back.
//! 3. Next step it jumps forward again. For ever, at mouse-move rate.
//!
//! The resize had the identical shape, which is the "changes the size of my
//! notes" half. The fix is that a gesture's limits are captured **once, when it
//! starts**, so they cannot move underneath it.
//!
//! These tests apply the edits to the arena the way the host does, because a
//! test that reads the roll's output without ever applying it cannot see this
//! bug at all — which is exactly why the first version of the roll shipped
//! with it.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    MouseButton, PianoRoll, RollEdit, RollView, SnapDivision, key_to_y, tick_to_x,
};
use fontelle_ui::layout::Rect;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
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
    }
}

fn notes(items: &[(Tick, Tick, u8)]) -> Arena<NoteId, Note> {
    let mut arena = Arena::default();
    for (start, length, key) in items {
        arena.insert(note(*start, *length, *key));
    }
    arena
}

fn roll() -> PianoRoll {
    PianoRoll::new(view())
}

fn at(view: &RollView, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(view, grid(), tick) + 1.0,
        key_to_y(view, grid(), key) + 1.0,
    )
}

/// What the host does with what the roll asks for.
///
/// The same arithmetic `MoveNotes` and `ResizeNotes` perform, minus the undo
/// bookkeeping — enough that the roll sees its own edits land, which is the
/// whole point of these tests.
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
            _ => {}
        }
    }
}

/// A drag held at one place must have nothing left to say after the first step.
///
/// This is the invariant the oscillation broke, and it is the one worth
/// asserting: a pointer that is not moving cannot be asking for anything new.
fn assert_settles(
    roll: &mut PianoRoll,
    arena: &mut Arena<NoteId, Note>,
    x: f32,
    y: f32,
    what: &str,
) {
    let first = roll.drag(x, y, grid(), arena, 4);
    apply(arena, &first);
    for step in 0..5 {
        let again = roll.drag(x, y, grid(), arena, 4);
        assert!(
            again.is_empty(),
            "{what}: step {step} with the pointer stationary asked for {again:?}"
        );
        apply(arena, &again);
    }
}

// ------------------------------------------------------------- moving ---

#[test]
fn a_note_dragged_to_the_start_of_the_clip_stays_there() {
    let mut roll = roll();
    let mut arena = notes(&[(PPQN * 2, PPQN, 60)]);
    let id = arena.keys().next().expect("a note");

    let (x, y) = at(&roll.view, PPQN * 2, 60);
    roll.press(MouseButton::Left, x + 20.0, y, grid(), &arena, 4);

    // Hard left, and held there.
    assert_settles(&mut roll, &mut arena, grid().x - 400.0, y, "moving");
    assert_eq!(
        arena[id].start, 0,
        "the note is at bar 1 and stayed at bar 1"
    );
}

#[test]
fn a_note_dragged_off_the_bottom_of_the_keyboard_stays_there() {
    let mut roll = roll();
    let mut arena = notes(&[(PPQN, PPQN, 40)]);
    let id = arena.keys().next().expect("a note");
    let mut view = view();
    view.top_key = 48;
    roll.view = view;

    let (x, y) = at(&roll.view, PPQN, 40);
    roll.press(MouseButton::Left, x + 10.0, y, grid(), &arena, 4);
    assert_settles(
        &mut roll,
        &mut arena,
        x + 10.0,
        grid().bottom() + 300.0,
        "transposing down",
    );
    assert!(arena[id].key <= 40);
}

#[test]
fn a_chord_dragged_off_the_top_of_the_keyboard_clamps_rather_than_asking_for_key_128() {
    let mut roll = roll();
    // Two notes a fifth apart near the ceiling: the clamp has to be measured
    // from the *highest* of them, or the interval is destroyed.
    let mut arena = notes(&[(0, PPQN, 118), (0, PPQN, 125)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let mut view = view();
    view.top_key = 127;
    roll.view = view;

    roll.select_all(&arena);
    let (x, y) = at(&roll.view, 0, 118);
    roll.press(MouseButton::Left, x + 10.0, y, grid(), &arena, 4);

    let edits = roll.drag(x + 10.0, grid().y, grid(), &arena, 4);
    for edit in &edits {
        if let RollEdit::Move { key_delta, .. } = edit {
            assert!(
                125 + i32::from(*key_delta) <= 127,
                "asked for key {} — the document would refuse the whole move, so \
                 the chord would not move at all",
                125 + i32::from(*key_delta)
            );
        }
    }
    apply(&mut arena, &edits);
    assert_eq!(
        arena[ids[1]].key - arena[ids[0]].key,
        7,
        "the interval survived the clamp"
    );
    assert_settles(&mut roll, &mut arena, x + 10.0, grid().y, "transposing up");
}

// ------------------------------------------------------------ resizing ---

#[test]
fn a_note_resized_down_to_its_shortest_stays_there() {
    let mut roll = roll();
    let mut arena = notes(&[(0, PPQN * 2, 60)]);
    let id = arena.keys().next().expect("a note");

    // Grab the right-hand edge.
    let x = tick_to_x(&roll.view, grid(), PPQN * 2) - 2.0;
    let y = key_to_y(&roll.view, grid(), 60) + 1.0;
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);

    assert_settles(&mut roll, &mut arena, grid().x, y, "resizing");
    assert!(
        arena[id].length > 0,
        "a note cannot be resized out of existence"
    );
}

#[test]
fn drawing_a_note_and_dragging_back_over_its_own_start_does_not_flicker() {
    // The one people hit constantly: draw a note, and the same gesture that
    // sizes it wanders back to the left. The pending-add handshake leaves a
    // resize whose floor is the note that was *just* created, so this is where
    // the oscillation was loudest.
    let mut roll = roll();
    let mut arena = notes(&[]);

    let (x, y) = at(&roll.view, PPQN * 2, 60);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    let [RollEdit::Add { note }] = &edits[..] else {
        panic!("expected one add, got {edits:?}");
    };
    let id = arena.insert(*note);
    roll.note_added(id);

    assert_settles(&mut roll, &mut arena, grid().x, y, "sizing a new note");
    assert!(arena[id].length > 0);
}

#[test]
fn a_resize_still_reaches_every_length_between_the_ends() {
    // The fix must not turn the clamp into a ratchet: dragging back out after
    // dragging in has to lengthen the note again.
    let mut roll = roll();
    let mut arena = notes(&[(0, PPQN * 2, 60)]);
    let id = arena.keys().next().expect("a note");
    let y = key_to_y(&roll.view, grid(), 60) + 1.0;

    let x = tick_to_x(&roll.view, grid(), PPQN * 2) - 2.0;
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);

    let shorter = roll.drag(tick_to_x(&roll.view, grid(), PPQN), y, grid(), &arena, 4);
    apply(&mut arena, &shorter);
    assert_eq!(arena[id].length, PPQN, "dragged in to one beat");

    let longer = roll.drag(
        tick_to_x(&roll.view, grid(), PPQN * 3),
        y,
        grid(),
        &arena,
        4,
    );
    apply(&mut arena, &longer);
    assert_eq!(arena[id].length, PPQN * 3, "and back out to three");
}

#[test]
fn a_move_still_reaches_every_position_between_the_ends() {
    let mut roll = roll();
    let mut arena = notes(&[(PPQN * 2, PPQN, 60)]);
    let id = arena.keys().next().expect("a note");
    let (x, y) = at(&roll.view, PPQN * 2, 60);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);

    let left = roll.drag(grid().x, y, grid(), &arena, 4);
    apply(&mut arena, &left);
    assert_eq!(arena[id].start, 0);

    let back = roll.drag(
        tick_to_x(&roll.view, grid(), PPQN * 3),
        y,
        grid(),
        &arena,
        4,
    );
    apply(&mut arena, &back);
    assert_eq!(
        arena[id].start,
        PPQN * 3,
        "a note dragged to the start must still be draggable away from it"
    );
}

// ------------------------------------------------- hearing what you touch ---

#[test]
fn pressing_a_note_asks_for_it_to_be_sounded() {
    // Reported: *"I can't click on notes in the piano roll to hear them."* The
    // roll had no way to say so — only a *drawn* note was auditioned, and
    // clicking an existing one was silent.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 64)]);
    assert_eq!(
        roll.take_audition().map(|a| a.key),
        None,
        "nothing has been touched yet"
    );

    let (x, y) = at(&roll.view, PPQN / 2, 64);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    assert_eq!(roll.take_audition().map(|a| a.key), Some(64));
    assert_eq!(
        roll.take_audition().map(|a| a.key),
        None,
        "and it is asked for once"
    );
}

#[test]
fn dragging_a_note_to_a_new_pitch_sounds_the_pitch_it_lands_on() {
    let mut roll = roll();
    let mut arena = notes(&[(0, PPQN, 60)]);
    let (x, y) = at(&roll.view, PPQN / 2, 60);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    roll.take_audition();

    let edits = roll.drag(x, key_to_y(&roll.view, grid(), 67) + 1.0, grid(), &arena, 4);
    apply(&mut arena, &edits);
    assert_eq!(
        roll.take_audition().map(|a| a.key),
        Some(67),
        "transposing a note has to sound where it landed, or you are dragging blind"
    );
}

#[test]
fn a_note_that_is_only_moved_in_time_is_not_re_sounded() {
    // Otherwise a horizontal drag machine-guns the same pitch.
    let mut roll = roll();
    let mut arena = notes(&[(0, PPQN, 60)]);
    let (x, y) = at(&roll.view, PPQN / 2, 60);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    roll.take_audition();

    let edits = roll.drag(
        tick_to_x(&roll.view, grid(), PPQN * 2),
        y,
        grid(),
        &arena,
        4,
    );
    apply(&mut arena, &edits);
    assert_eq!(roll.take_audition().map(|a| a.key), None);
}

#[test]
fn deleting_a_note_does_not_sound_it() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (x, y) = at(&roll.view, PPQN / 2, 60);
    roll.press(MouseButton::Right, x, y, grid(), &arena, 4);
    assert_eq!(roll.take_audition().map(|a| a.key), None);
}
