//! What the drag after drawing a note is *for*.
//!
//! Reported from using the window: *"when placing a note, it thinks I'm
//! resizing it right after I created it, instead of still placing it. If I
//! click to place a note, then my cursor moves to drag it, it shouldn't change
//! the note length it should move the note."*
//!
//! The roll shipped with FL Studio's draw-and-size gesture: press on empty
//! grid, and the same drag that follows sets the length. It is a real habit and
//! it is a defensible default, but it is not the one asked for here, and it has
//! a sharp edge that this report is the sound of — a press is never perfectly
//! still, so *every* drawn note was a resize in progress, and the smallest
//! wobble of the hand changed a length nobody meant to change.
//!
//! So placing a note now leaves you holding it: the drag moves it, in both
//! axes, and the right edge is what resizes it — the same handle that resizes
//! every other note. [`DrawDrag`] keeps the old gesture for anyone who wants
//! it back.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DrawDrag, MouseButton, PianoRoll, RollEdit, RollView, SnapDivision, key_to_y, tick_to_x,
};
use fontelle_ui::layout::Rect;

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

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
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

/// The host's half of the handshake: apply the edits, and hand back the id of
/// anything added — which is what turns a press into a live gesture.
fn host(roll: &mut PianoRoll, arena: &mut Arena<NoteId, Note>, edits: &[RollEdit]) {
    for edit in edits {
        match edit {
            RollEdit::Add { note } => {
                let id = arena.insert(*note);
                roll.note_added(id);
            }
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

/// Press on empty grid, and hand the id back the way the window does.
fn draw(roll: &mut PianoRoll, arena: &mut Arena<NoteId, Note>, tick: Tick, key: u8) -> NoteId {
    let (x, y) = at(&roll.view, tick, key);
    let edits = roll.press(MouseButton::Left, x, y, grid(), arena, 4);
    host(roll, arena, &edits);
    *roll
        .selection()
        .first()
        .expect("the drawn note is selected")
}

// --------------------------------------------------------------- moving ---

#[test]
fn the_drag_after_drawing_a_note_moves_it() {
    let mut roll = roll();
    let mut arena = Arena::default();
    let id = draw(&mut roll, &mut arena, PPQN, 60);
    let length = arena[id].length;

    let (x, y) = at(&roll.view, PPQN * 3, 60);
    let edits = roll.drag(x, y, grid(), &arena, 4);
    host(&mut roll, &mut arena, &edits);

    assert_eq!(
        arena[id].start,
        PPQN * 3,
        "it should have followed the mouse"
    );
    assert_eq!(
        arena[id].length, length,
        "and its length is not the drag's business"
    );
}

#[test]
fn the_drag_after_drawing_a_note_transposes_it_too() {
    // A resize has no vertical axis at all, so this was simply impossible
    // before: you drew a note on the wrong row and had to let go and drag it.
    let mut roll = roll();
    let mut arena = Arena::default();
    let id = draw(&mut roll, &mut arena, PPQN, 60);

    let (x, y) = at(&roll.view, PPQN, 67);
    let edits = roll.drag(x, y, grid(), &arena, 4);
    host(&mut roll, &mut arena, &edits);
    assert_eq!(arena[id].key, 67);
}

#[test]
fn a_note_drawn_at_bar_one_is_not_dragged_before_it() {
    let mut roll = roll();
    let mut arena = Arena::default();
    let id = draw(&mut roll, &mut arena, 0, 60);

    let edits = roll.drag(grid().x - 400.0, grid().y + 100.0, grid(), &arena, 4);
    host(&mut roll, &mut arena, &edits);
    assert_eq!(arena[id].start, 0);

    // And the clamp does not then oscillate, which is the bug `roll_gestures`
    // is named after — the limits of *this* gesture are the drawn note's own.
    for _ in 0..4 {
        let again = roll.drag(grid().x - 400.0, grid().y + 100.0, grid(), &arena, 4);
        assert!(again.is_empty(), "a stationary pointer asked for {again:?}");
    }
}

#[test]
fn drawing_a_note_and_letting_go_without_moving_leaves_one_note_as_drawn() {
    let mut roll = roll();
    let mut arena = Arena::default();
    let id = draw(&mut roll, &mut arena, PPQN, 60);
    roll.release();

    assert_eq!(arena.len(), 1);
    assert_eq!(arena[id].start, PPQN);
    assert_eq!(arena[id].key, 60);
    assert_eq!(
        arena[id].length,
        PPQN / 4,
        "the template's length, untouched by a press that never moved"
    );
}

// -------------------------------------------------------------- resizing ---

#[test]
fn the_right_edge_still_resizes_a_note_that_was_just_drawn() {
    // The gesture did not go away; it moved to the handle every other note
    // already uses, which is the point.
    let mut roll = roll();
    let mut arena = Arena::default();
    let id = draw(&mut roll, &mut arena, PPQN, 60);
    roll.release();

    let end = arena[id].start + arena[id].length;
    let (x, y) = (
        tick_to_x(&roll.view, grid(), end) - 2.0,
        key_to_y(&roll.view, grid(), 60) + 1.0,
    );
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    host(&mut roll, &mut arena, &edits);
    let edits = roll.drag(
        tick_to_x(&roll.view, grid(), PPQN * 2),
        y,
        grid(),
        &arena,
        4,
    );
    host(&mut roll, &mut arena, &edits);
    assert!(
        arena[id].length > PPQN / 4,
        "dragging the right edge must still lengthen it, got {}",
        arena[id].length
    );
}

#[test]
fn the_old_draw_and_size_gesture_is_still_there_for_anyone_who_wants_it() {
    let mut roll = roll();
    roll.draw_drag = DrawDrag::Resize;
    let mut arena = Arena::default();
    let id = draw(&mut roll, &mut arena, 0, 60);

    let (x, y) = at(&roll.view, PPQN * 2, 60);
    let edits = roll.drag(x, y, grid(), &arena, 4);
    host(&mut roll, &mut arena, &edits);
    assert_eq!(arena[id].start, 0, "sizing does not move the note");
    assert_eq!(arena[id].length, PPQN * 2);
}

#[test]
fn moving_is_what_a_roll_does_out_of_the_box() {
    assert_eq!(roll().draw_drag, DrawDrag::Move);
}

// ----------------------------------------------------------- and paint ---

#[test]
fn the_paint_tool_is_unaffected_and_still_lays_a_note_per_cell() {
    // Paint has never had a pending add — it draws on every cell it crosses —
    // and changing what a *draw* drag means must not have reached it.
    let mut roll = roll();
    roll.tool = fontelle_ui::canvas::Tool::Paint;
    let mut arena = Arena::default();
    let (x, y) = at(&roll.view, 0, 60);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    host(&mut roll, &mut arena, &edits);

    for step in 1..4 {
        let (x, y) = at(&roll.view, PPQN * step, 60);
        let edits = roll.drag(x, y, grid(), &arena, 4);
        host(&mut roll, &mut arena, &edits);
    }
    assert_eq!(arena.len(), 4);
}
