//! The paint brush stays on the row it started on.
//!
//! Ty, using the window:
//!
//! > *"when using the brush tool right now youre able to move your mouse up
//! > and down for it to go to other note lanes. when dragging out the brush
//! > left and right it should be locked to the lane youre clicking in this
//! > makes it easier to draw things like hi hat patterns. maybe a key should
//! > be held to make the lane lock not apply in case anyone actually wants to
//! > do it like this"*
//!
//! So a stroke paints on the key it was pressed on, however the hand drifts
//! up or down, and **Shift** held lets it follow the pointer across rows the
//! way it used to. Shift because it is already this window's "constrain or
//! free one axis" modifier (a note dragged with Shift keeps to one axis), Alt
//! is the grid and the scale, and Ctrl is the marquee. Read live, so it can
//! be pressed and let go of in the middle of a stroke.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    Modifiers, MouseButton, PianoRoll, RollEdit, RollView, SnapDivision, Tool, key_to_y, tick_to_x,
};
use fontelle_ui::layout::Rect;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Beat,
    }
}

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
}

fn at(view: &RollView, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(view, grid(), tick) + 1.0,
        key_to_y(view, grid(), key) + 1.0,
    )
}

fn host(roll: &mut PianoRoll, arena: &mut Arena<NoteId, Note>, edits: &[RollEdit]) {
    for edit in edits {
        if let RollEdit::Add { note } = edit {
            let id = arena.insert(note.clone());
            roll.note_added(id);
        }
    }
}

fn painter() -> PianoRoll {
    let mut roll = PianoRoll::new(view());
    roll.tool = Tool::Paint;
    roll
}

/// A stroke from `(0, start_key)` through each `(beat, key)` in turn.
fn stroke(roll: &mut PianoRoll, start_key: u8, path: &[(i64, u8)]) -> Arena<NoteId, Note> {
    let mut arena = Arena::default();
    let (x, y) = at(&roll.view, 0, start_key);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena);
    host(roll, &mut arena, &edits);
    for (beat, key) in path {
        let (x, y) = at(&roll.view, PPQN * beat, *key);
        let edits = roll.drag(x, y, grid(), &arena);
        host(roll, &mut arena, &edits);
    }
    roll.release();
    arena
}

fn keys(arena: &Arena<NoteId, Note>) -> Vec<(Tick, u8)> {
    let mut keys: Vec<_> = arena.values().map(|n| (n.start, n.key)).collect();
    keys.sort();
    keys
}

#[test]
fn a_stroke_that_wanders_up_and_down_paints_the_row_it_started_on() {
    // A hi-hat run drawn by a hand that is not a ruler.
    let mut roll = painter();
    let arena = stroke(&mut roll, 60, &[(1, 62), (2, 57), (3, 66)]);
    assert_eq!(
        keys(&arena),
        vec![(0, 60), (PPQN, 60), (PPQN * 2, 60), (PPQN * 3, 60)],
        "every note on the key the press was on"
    );
}

#[test]
fn moving_only_up_or_down_within_a_cell_paints_nothing_more() {
    // Off the row and back over the same beat: one cell, one note.
    let mut roll = painter();
    let arena = stroke(&mut roll, 60, &[(0, 64), (0, 55)]);
    assert_eq!(keys(&arena), vec![(0, 60)]);
}

#[test]
fn with_shift_held_the_stroke_follows_the_pointer_across_rows() {
    let mut roll = painter();
    roll.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let arena = stroke(&mut roll, 60, &[(1, 62), (2, 57)]);
    assert_eq!(keys(&arena), vec![(0, 60), (PPQN, 62), (PPQN * 2, 57)]);
}

#[test]
fn shift_can_be_taken_up_and_let_go_of_mid_stroke() {
    // Read live: let go, and the brush is back on the row it started on.
    let mut roll = painter();
    let mut arena = Arena::default();
    let (x, y) = at(&roll.view, 0, 60);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena);
    host(&mut roll, &mut arena, &edits);
    for (beat, key, shift) in [(1, 64, true), (2, 64, false), (3, 50, false)] {
        roll.set_modifiers(Modifiers {
            shift,
            ..Modifiers::default()
        });
        let (x, y) = at(&roll.view, PPQN * beat, key);
        let edits = roll.drag(x, y, grid(), &arena);
        host(&mut roll, &mut arena, &edits);
    }
    assert_eq!(
        keys(&arena),
        vec![(0, 60), (PPQN, 64), (PPQN * 2, 60), (PPQN * 3, 60)]
    );
}

#[test]
fn the_draw_tools_drag_is_untouched_by_the_lock() {
    // The lock is the brush's. A note drawn with the pencil is still moved
    // by the drag that follows, in both axes.
    let mut roll = PianoRoll::new(view());
    roll.tool = Tool::Draw;
    let mut arena = Arena::default();
    let (x, y) = at(&roll.view, 0, 60);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena);
    host(&mut roll, &mut arena, &edits);
    let (x, y) = at(&roll.view, PPQN, 64);
    let edits = roll.drag(x, y, grid(), &arena);
    for edit in &edits {
        if let RollEdit::Move {
            ids,
            tick_delta,
            key_delta,
        } = edit
        {
            for id in ids {
                let note = &mut arena[*id];
                note.start += tick_delta;
                note.key = (i16::from(note.key) + key_delta) as u8;
            }
        }
    }
    assert_eq!(keys(&arena), vec![(PPQN, 64)]);
}

#[test]
fn the_brushs_tip_says_how_to_cross_rows() {
    let tip = Tool::Paint.tip();
    assert!(tip.contains("Shift"), "{tip:?} does not mention the key");
    assert!(tip.len() <= 64, "{tip:?} is a paragraph");
}
