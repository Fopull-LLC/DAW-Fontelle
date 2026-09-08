//! The piano roll's second pass: everything §16.5 asks for that the MVP left
//! out, and the two things that made the MVP feel wrong to use.
//!
//! The two:
//!
//! - **You could not draw a note to length.** In FL Studio, and in every roll
//!   built after it, a click on empty grid makes a note and the same drag sizes
//!   it. Here the click made a note and the drag did nothing, because the roll
//!   never learned the id of the note it had just asked for.
//! - **Zoom was one axis, about the left edge.** §16.4 says "continuous and
//!   independently controllable per axis", and zooming about the edge means
//!   whatever you were looking at is not where you left it.
//!
//! Everything below is a pure function or a state machine over one, per §2.5.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DEFAULT_LANE_HEIGHT, Modifiers, MouseButton, PianoRoll, RollControl, RollEdit, RollView,
    SnapDivision, Tool, key_to_y, note_at_tick, roll_layout, tick_to_x, toolbar_hit,
    toolbar_layout, velocity_of_y, x_to_tick, zoom_x, zoom_y,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

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
        channel: None,
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

/// The pixel at the start of a cell, a hair inside it.
fn at(view: &RollView, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(view, grid(), tick) + 1.0,
        key_to_y(view, grid(), key) + 1.0,
    )
}

// ------------------------------------------------- drawing a note to length ---

/// **This used to be the default and is now opt-in.** Reported from using the
/// window: *"if I click to place a note, then my cursor moves to drag it, it
/// shouldn't change the note length it should move the note."* The gesture
/// itself is unchanged and still tested here; what changed is which of the two
/// a bare press gets. See `tests/roll_draw.rs` for the move, and `DrawDrag`
/// for why the default went the other way.
#[test]
fn drawing_a_note_and_dragging_right_sizes_it_in_one_gesture() {
    let mut roll = roll();
    roll.draw_drag = fontelle_ui::canvas::DrawDrag::Resize;
    let mut arena = notes(&[]);

    let step = PPQN / 4;
    let (x, y) = at(&roll.view, 0, 60);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    assert_eq!(
        edits,
        vec![RollEdit::Add {
            note: Note {
                start: 0,
                length: step,
                key: 60,
                ..roll.template()
            },
        }]
    );

    // The host applies it and hands back the id — the piece that was missing.
    let id = arena.insert(note(0, step, 60));
    roll.note_added(id);
    assert_eq!(
        roll.selection(),
        &[id],
        "the note you just drew is selected"
    );

    // Now the same drag lengthens it, without letting go of the button.
    let (x, y) = at(&roll.view, step * 4, 60);
    let edits = roll.drag(x, y, grid(), &arena, 4);
    assert_eq!(
        edits,
        vec![RollEdit::Resize {
            ids: vec![id],
            tick_delta: step * 3,
        }],
        "drawing and sizing is one gesture, as it is in every roll built after \
         FL Studio's"
    );
}

#[test]
fn drawing_and_letting_go_without_moving_leaves_a_note_of_one_snap_unit() {
    let mut roll = roll();
    let mut arena = notes(&[]);
    let step = PPQN / 4;

    let (x, y) = at(&roll.view, step * 2, 64);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    let id = arena.insert(note(step * 2, step, 64));
    roll.note_added(id);

    // The pointer has not moved. Nothing further is asked for — a note that
    // shrank because the mouse jittered is the bug this is guarding.
    assert!(roll.drag(x, y, grid(), &arena, 4).is_empty());
    roll.release();
    assert_eq!(arena[id].length, step);
}

#[test]
fn a_press_that_makes_no_note_starts_no_sizing_gesture() {
    let mut roll = roll();
    roll.tool = Tool::Select;
    let arena = notes(&[]);

    let (x, y) = at(&roll.view, 0, 60);
    assert!(
        roll.press(MouseButton::Left, x, y, grid(), &arena, 4)
            .is_empty()
    );
    // Nothing was added, so nothing can be sized: a stale pending-add would
    // capture the *next* note drawn anywhere in the roll.
    let (x, y) = at(&roll.view, PPQN * 4, 60);
    assert!(roll.drag(x, y, grid(), &arena, 4).is_empty());
}

// ------------------------------------------------------------------- zoom ---

#[test]
fn zooming_horizontally_keeps_the_tick_under_the_pointer_under_the_pointer() {
    let mut view = view();
    let grid = grid();
    let anchor = grid.x + 500.0;

    // Stated in **pixels**, which is the claim that actually matters and the
    // one that does not soften as you zoom out: whatever was under the pointer
    // is still under the pointer, to within less than a pixel. Stating it in
    // ticks would be a weaker promise at low zoom and an impossible one at
    // high, because `scroll_tick` is a whole tick.
    // Scrolled into the song, because the interesting case is the one where
    // there is material on both sides of the pointer. At tick zero the view
    // runs out of song to reveal and the anchor has to give — which is the
    // next test.
    view.scroll_tick = PPQN * 8;
    let tracked = x_to_tick(&view, grid, anchor);
    for factor in [1.25, 1.25, 1.25, 0.8, 0.8, 0.8, 0.8] {
        zoom_x(&mut view, grid, anchor, factor);
        let landed = tick_to_x(&view, grid, tracked);
        assert!(
            (landed - anchor).abs() <= 1.0,
            "zooming about the pointer moved the song under it by {} px",
            landed - anchor
        );
    }
    assert!(view.pixels_per_tick > 0.0);
}

#[test]
fn zooming_vertically_keeps_the_key_under_the_pointer_within_a_row() {
    let mut view = view();
    let grid = grid();
    let anchor = grid.y + 200.0;

    for factor in [1.3, 1.3, 0.7, 0.7, 1.3] {
        // A row is the finest the roll can hold a key at — `top_key` is a key,
        // not a fraction of one — so one row per step is the honest bound.
        let before = fontelle_ui::canvas::y_to_key(&view, grid, anchor);
        zoom_y(&mut view, grid, anchor, factor);
        let after = fontelle_ui::canvas::y_to_key(&view, grid, anchor);
        assert!(
            (i32::from(after) - i32::from(before)).abs() <= 1,
            "zooming about the pointer moved the keyboard under it: {before} -> {after}"
        );
    }
}

#[test]
fn zooming_out_at_the_top_of_the_song_stops_at_the_top_of_the_song() {
    let mut view = view();
    let grid = grid();
    // Nothing before bar 1, so the anchor cannot be held: the alternative is a
    // negative scroll position, which every conversion here would then have to
    // defend against.
    for _ in 0..8 {
        zoom_x(&mut view, grid, grid.x + 500.0, 0.8);
        assert!(view.scroll_tick >= 0);
    }
    assert_eq!(view.scroll_tick, 0);
}

#[test]
fn zoom_is_bounded_at_both_ends_on_both_axes() {
    let mut view = view();
    let grid = grid();
    for _ in 0..200 {
        zoom_x(&mut view, grid, grid.x, 2.0);
        zoom_y(&mut view, grid, grid.y, 2.0);
    }
    assert!(view.pixels_per_tick.is_finite() && view.pixels_per_tick > 0.0);
    assert!(
        view.key_height <= 64.0,
        "a key row taller than a panel is not a zoom"
    );

    for _ in 0..200 {
        zoom_x(&mut view, grid, grid.x, 0.5);
        zoom_y(&mut view, grid, grid.y, 0.5);
    }
    assert!(
        view.pixels_per_tick > 0.0,
        "a zoom that reaches zero pixels per tick divides by it everywhere"
    );
    assert!(view.key_height >= 1.0);
}

#[test]
fn the_two_axes_zoom_independently() {
    let mut view = view();
    let grid = grid();
    let key_height = view.key_height;
    zoom_x(&mut view, grid, grid.x, 2.0);
    assert_eq!(
        view.key_height, key_height,
        "zooming time changed the keyboard"
    );

    let ppt = view.pixels_per_tick;
    zoom_y(&mut view, grid, grid.y, 2.0);
    assert_eq!(
        view.pixels_per_tick, ppt,
        "zooming pitch changed the timeline"
    );
}

// -------------------------------------------------------- marquee selection ---

#[test]
fn the_select_tool_drags_a_marquee_and_selects_what_it_covers() {
    let mut roll = roll();
    roll.tool = Tool::Select;
    let arena = notes(&[
        (0, PPQN, 60),
        (PPQN * 2, PPQN, 62),
        (PPQN * 8, PPQN, 64), // well outside the box
    ]);
    let ids: Vec<NoteId> = arena.keys().collect();

    let (x0, y0) = at(&roll.view, 0, 63);
    roll.press(MouseButton::Left, x0, y0, grid(), &arena, 4);
    let (x1, y1) = at(&roll.view, PPQN * 3, 59);
    assert!(
        roll.drag(x1, y1, grid(), &arena, 4).is_empty(),
        "a marquee edits nothing"
    );
    assert!(
        roll.marquee().is_some(),
        "and it is visible while it is happening"
    );

    roll.release_over(x1, y1, grid(), &arena);
    let mut selected = roll.selection().to_vec();
    selected.sort();
    let mut wanted = vec![ids[0], ids[1]];
    wanted.sort();
    assert_eq!(selected, wanted);
    assert!(
        roll.marquee().is_none(),
        "and it is gone once the button is up"
    );
}

#[test]
fn a_marquee_selects_the_same_notes_dragged_in_any_direction() {
    let arena = notes(&[(0, PPQN, 60), (PPQN * 2, PPQN, 62)]);
    let corners = [
        (0, 63, PPQN * 3, 59),
        (PPQN * 3, 59, 0, 63),
        (0, 59, PPQN * 3, 63),
    ];
    let mut expected: Option<Vec<NoteId>> = None;

    for (t0, k0, t1, k1) in corners {
        let mut roll = roll();
        roll.tool = Tool::Select;
        let (x0, y0) = at(&roll.view, t0, k0);
        let (x1, y1) = at(&roll.view, t1, k1);
        roll.press(MouseButton::Left, x0, y0, grid(), &arena, 4);
        roll.drag(x1, y1, grid(), &arena, 4);
        roll.release_over(x1, y1, grid(), &arena);

        let mut got = roll.selection().to_vec();
        got.sort();
        match &expected {
            None => expected = Some(got),
            Some(first) => assert_eq!(
                &got, first,
                "dragging the box the other way selected differently"
            ),
        }
    }
    assert_eq!(expected.expect("ran at least once").len(), 2);
}

// ------------------------------------------------------------- the modifiers ---

#[test]
fn holding_alt_bypasses_the_snap() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let id = arena.keys().next().unwrap();

    roll.set_modifiers(Modifiers {
        alt: true,
        ..Default::default()
    });
    let (x, y) = at(&roll.view, PPQN / 2, 60);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);

    // Somewhere that is not on a grid line at all.
    let off_grid = PPQN / 2 + 37;
    let (x, y) = at(&roll.view, off_grid, 60);
    let edits = roll.drag(x, y, grid(), &arena, 4);
    let RollEdit::Move { tick_delta, .. } = &edits[0] else {
        panic!("expected a move, got {edits:?}")
    };
    assert!(
        tick_delta % (PPQN / 4) != 0,
        "Alt is free positioning (§16.5); {tick_delta} landed on the grid anyway"
    );
    assert_eq!(
        edits[0],
        RollEdit::Move {
            ids: vec![id],
            tick_delta: *tick_delta,
            key_delta: 0
        }
    );
}

#[test]
fn holding_shift_constrains_a_drag_to_one_axis() {
    let arena = notes(&[(PPQN, PPQN, 60)]);

    // Mostly sideways: the pitch must not move.
    let mut first = roll();
    first.set_modifiers(Modifiers {
        shift: true,
        ..Default::default()
    });
    let (x, y) = at(&first.view, PPQN, 60);
    first.press(MouseButton::Left, x, y, grid(), &arena, 4);
    let (x, y) = at(&first.view, PPQN * 3, 61);
    let edits = first.drag(x, y, grid(), &arena, 4);
    let RollEdit::Move {
        tick_delta,
        key_delta,
        ..
    } = &edits[0]
    else {
        panic!("expected a move")
    };
    assert!(
        *tick_delta != 0 && *key_delta == 0,
        "sideways drag moved the pitch"
    );

    // Mostly upwards: the time must not move.
    let mut second = roll();
    second.set_modifiers(Modifiers {
        shift: true,
        ..Default::default()
    });
    let (x, y) = at(&second.view, PPQN, 60);
    second.press(MouseButton::Left, x, y, grid(), &arena, 4);
    let (x, y) = at(&second.view, PPQN + PPQN / 8, 67);
    let edits = second.drag(x, y, grid(), &arena, 4);
    let RollEdit::Move {
        tick_delta,
        key_delta,
        ..
    } = &edits[0]
    else {
        panic!("expected a move")
    };
    assert!(
        *key_delta != 0 && *tick_delta == 0,
        "upward drag moved the time"
    );
}

// ----------------------------------------------------------- the clipboard ---

#[test]
fn copy_and_paste_puts_the_phrase_down_where_it_was_asked_for() {
    let mut roll = roll();
    let arena = notes(&[(PPQN * 4, PPQN, 60), (PPQN * 5, PPQN / 2, 64)]);
    roll.select_all(&arena);

    assert_eq!(roll.copy(&arena), 2);
    let edits = roll.paste(PPQN * 8, 4);
    let [RollEdit::Insert(pasted)] = &edits[..] else {
        panic!("expected one insert, got {edits:?}")
    };
    assert_eq!(pasted.len(), 2);
    // The phrase keeps its own shape: the earliest note lands on the paste
    // point, and everything else keeps its offset from it.
    assert_eq!(pasted[0].start, PPQN * 8);
    assert_eq!(pasted[1].start, PPQN * 9);
    assert_eq!(pasted[0].key, 60);
    assert_eq!(pasted[1].key, 64);
    assert_eq!(pasted[1].length, PPQN / 2);
}

#[test]
fn cutting_removes_what_it_copied_and_paste_still_works_afterwards() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let id = arena.keys().next().unwrap();
    roll.select_all(&arena);

    let edits = roll.cut(&arena);
    assert_eq!(edits, vec![RollEdit::Remove(vec![id])]);
    assert!(
        roll.selection().is_empty(),
        "what was cut is not still selected"
    );

    let edits = roll.paste(PPQN * 2, 4);
    let [RollEdit::Insert(pasted)] = &edits[..] else {
        panic!("expected an insert")
    };
    assert_eq!(pasted.len(), 1);
    assert_eq!(pasted[0].start, PPQN * 2);
}

#[test]
fn pasting_an_empty_clipboard_asks_for_nothing() {
    let mut roll = roll();
    assert!(roll.paste(PPQN, 4).is_empty());
    // And an empty selection copies nothing rather than emptying what is held.
    let arena = notes(&[(0, PPQN, 60)]);
    roll.select_all(&arena);
    roll.copy(&arena);
    roll.clear_selection();
    assert_eq!(roll.copy(&arena), 0);
    assert_eq!(
        roll.paste(0, 4).len(),
        1,
        "the clipboard is not cleared by a bad copy"
    );
}

#[test]
fn duplicating_puts_the_copy_immediately_after_the_original() {
    let mut roll = roll();
    // Two bars of 4/4 worth of phrase, so the offset is obvious.
    let arena = notes(&[(0, PPQN, 60), (PPQN * 2, PPQN, 62)]);
    roll.select_all(&arena);

    let edits = roll.duplicate(&arena, 4);
    let [RollEdit::Insert(copies)] = &edits[..] else {
        panic!("expected one insert, got {edits:?}")
    };
    // Ctrl+B in FL: the copy starts where the selection ends, rounded up to the
    // bar, so duplicating a phrase makes a phrase twice as long rather than an
    // overlap nobody asked for.
    assert_eq!(copies[0].start, PPQN * 4);
    assert_eq!(copies[1].start, PPQN * 6);
}

// -------------------------------------------------------- the velocity lane ---

#[test]
fn the_velocity_lane_reads_top_for_loud_and_bottom_for_quiet() {
    let lane = Rect::new(0.0, 100.0, 400.0, 80.0);
    assert_eq!(velocity_of_y(lane, lane.y), 127);
    assert_eq!(velocity_of_y(lane, lane.bottom() - 0.01), 1);
    // Never zero: a note with velocity 0 is a note-off in MIDI, so a lane that
    // can produce one can silently delete a note.
    for y in [lane.bottom(), lane.bottom() + 50.0, lane.y - 50.0] {
        let v = velocity_of_y(lane, y);
        assert!((1..=127).contains(&v), "y={y} gave velocity {v}");
    }
    let middle = velocity_of_y(lane, lane.y + lane.height / 2.0);
    assert!(
        (60..=70).contains(&middle),
        "the middle of the lane gave {middle}"
    );
}

#[test]
fn dragging_in_the_velocity_lane_sets_the_note_under_the_pointer() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (PPQN * 4, PPQN, 62)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let lane = Rect::new(grid().x, 500.0, grid().width, 80.0);

    let x = tick_to_x(&roll.view, grid(), PPQN / 2);
    let edits = roll.press_lane(x, lane.y + 8.0, lane, grid(), &arena);
    let [
        RollEdit::SetProperty {
            ids: hit,
            value: velocity,
            ..
        },
    ] = &edits[..]
    else {
        panic!("expected a velocity edit, got {edits:?}")
    };
    assert_eq!(
        hit,
        &vec![ids[0]],
        "the note the pointer is over, and only it"
    );
    assert!(
        *velocity > 110,
        "near the top of the lane is loud, got {velocity}"
    );

    // Dragging down the lane keeps editing the same note.
    let edits = roll.drag_lane(x, lane.bottom() - 8.0, lane, grid(), &arena);
    let [
        RollEdit::SetProperty {
            ids: hit,
            value: velocity,
            ..
        },
    ] = &edits[..]
    else {
        panic!("expected a velocity edit, got {edits:?}")
    };
    assert_eq!(hit, &vec![ids[0]]);
    assert!(*velocity < 20, "near the bottom is quiet, got {velocity}");
}

#[test]
fn the_velocity_lane_edits_the_whole_selection_when_you_grab_one_of_it() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (PPQN * 4, PPQN, 62)]);
    roll.select_all(&arena);
    let lane = Rect::new(grid().x, 500.0, grid().width, 80.0);

    let x = tick_to_x(&roll.view, grid(), PPQN / 2);
    let edits = roll.press_lane(x, lane.y + 8.0, lane, grid(), &arena);
    let [RollEdit::SetProperty { ids, .. }] = &edits[..] else {
        panic!("expected a velocity edit")
    };
    assert_eq!(
        ids.len(),
        2,
        "grabbing one of a selection in the lane sets the lot — that is what \
         makes flattening a chord one gesture"
    );
}

#[test]
fn pressing_the_velocity_lane_where_there_is_no_note_does_nothing() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let lane = Rect::new(grid().x, 500.0, grid().width, 80.0);
    let x = tick_to_x(&roll.view, grid(), PPQN * 8);
    assert!(
        roll.press_lane(x, lane.y + 8.0, lane, grid(), &arena)
            .is_empty()
    );
}

#[test]
fn the_note_under_a_column_is_the_one_that_covers_it() {
    let view = view();
    let arena = notes(&[(0, PPQN, 60), (PPQN * 4, PPQN, 62)]);
    let ids: Vec<NoteId> = arena.keys().collect();

    let inside = tick_to_x(&view, grid(), PPQN / 2);
    assert_eq!(note_at_tick(&view, grid(), &arena, inside), Some(ids[0]));
    let between = tick_to_x(&view, grid(), PPQN * 2);
    assert_eq!(note_at_tick(&view, grid(), &arena, between), None);
    let second = tick_to_x(&view, grid(), PPQN * 4 + 10);
    assert_eq!(note_at_tick(&view, grid(), &arena, second), Some(ids[1]));
}

// ---------------------------------------------------------------- the tools ---

#[test]
fn the_roll_reserves_a_toolbar_and_a_velocity_lane() {
    let m = metrics();
    let frame = Rect::new(0.0, 0.0, 900.0, 520.0);
    let l = roll_layout(frame, &m, DEFAULT_LANE_HEIGHT);

    assert_eq!(l.toolbar.y, frame.y, "the toolbar is the top of the panel");
    assert!(l.ruler.y >= l.toolbar.bottom());
    assert!(l.grid.y >= l.ruler.bottom());
    assert!(!l.velocity.is_empty());
    assert!(
        l.velocity.y >= l.grid.bottom(),
        "the lane is under the grid"
    );
    assert_eq!(
        l.velocity.x, l.grid.x,
        "and lines up with it, column for column"
    );
    assert_eq!(l.velocity.width, l.grid.width);
    assert!(l.velocity.bottom() <= frame.bottom() + 0.001);

    // Hidden, the grid takes the room back.
    let without = roll_layout(frame, &m, 0.0);
    assert!(without.velocity.is_empty());
    assert!(without.grid.height > l.grid.height);
    assert_eq!(without.grid.bottom(), frame.bottom());
}

#[test]
fn every_toolbar_control_is_inside_the_toolbar_and_clickable() {
    let m = metrics();
    let l = roll_layout(Rect::new(0.0, 0.0, 900.0, 520.0), &m, DEFAULT_LANE_HEIGHT);
    let bar = toolbar_layout(l.toolbar, &m);

    assert!(!bar.items.is_empty());
    for (control, rect) in &bar.items {
        assert!(
            l.toolbar.intersects(rect),
            "{control:?} at {rect:?} is outside the toolbar {:?}",
            l.toolbar
        );
        let hit = toolbar_hit(&bar, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(hit, Some(*control));
    }
    // The tools §16.5 names first, and both zooms on both axes.
    let controls: Vec<RollControl> = bar.items.iter().map(|(c, _)| *c).collect();
    for wanted in [
        RollControl::Tool(Tool::Draw),
        RollControl::Tool(Tool::Select),
        RollControl::Tool(Tool::Delete),
        RollControl::Snap,
        RollControl::ZoomInX,
        RollControl::ZoomOutX,
        RollControl::ZoomInY,
        RollControl::ZoomOutY,
        RollControl::Velocity,
    ] {
        assert!(
            controls.contains(&wanted),
            "{wanted:?} is not on the toolbar"
        );
    }
    assert_eq!(toolbar_hit(&bar, -50.0, -50.0), None);
}

#[test]
fn a_toolbar_with_no_room_produces_nothing_rather_than_overlapping_controls() {
    let m = metrics();
    let bar = toolbar_layout(Rect::new(0.0, 0.0, 10.0, 4.0), &m);
    for (_, rect) in &bar.items {
        assert!(rect.width >= 0.0 && rect.height >= 0.0);
    }
    assert_eq!(
        toolbar_hit(&bar, 5.0, 2.0),
        bar.items.first().map(|(c, _)| *c)
    );
}

/// Reported from using the window: *"when I paste notes in the piano roll
/// they're off time instead of snapped."*
///
/// The window worked out *where* to paste from the playhead, or — when the
/// playhead is somewhere else in the song — from the raw pointer x, and handed
/// that tick over untouched. A pointer is never on the grid, so a pasted
/// phrase never was either.
///
/// It is fixed **here**, in the roll, rather than at the call site: the roll is
/// what owns the snap division, and a rule enforced in the caller is a rule
/// the next caller forgets.
#[test]
fn pasting_lands_on_the_grid_rather_than_where_the_pointer_happened_to_be() {
    let mut roll = PianoRoll::new(view());
    let mut notes = Arena::default();
    let id = notes.insert(note(0, PPQN / 2, 60));
    roll.select(vec![id]);
    assert_eq!(roll.copy(&notes), 1);

    // Seventeen ticks past bar 3 — the sort of number a pointer produces.
    let edits = roll.paste(PPQN * 8 + 17, 4);
    let [RollEdit::Insert(pasted)] = &edits[..] else {
        panic!("expected one insert, got {edits:?}");
    };
    assert_eq!(
        pasted[0].start,
        PPQN * 8,
        "a paste lands on the nearest line of the grid that is switched on"
    );
}

/// The snap the roll is *actually* on, not a hard-coded one: a phrase pasted
/// with the grid set to bars belongs on a bar.
#[test]
fn a_paste_uses_the_snap_division_that_is_switched_on() {
    let mut roll = PianoRoll::new(RollView {
        snap: SnapDivision::Bar,
        ..view()
    });
    let mut notes = Arena::default();
    let id = notes.insert(note(0, PPQN, 60));
    roll.select(vec![id]);
    roll.copy(&notes);

    let edits = roll.paste(PPQN * 5, 4);
    let [RollEdit::Insert(pasted)] = &edits[..] else {
        panic!("expected one insert, got {edits:?}");
    };
    assert_eq!(
        pasted[0].start,
        PPQN * 4,
        "bar snap puts it on the bar, not on the beat it was asked for"
    );
}

/// And §16.5's free positioning still wins, as it does for every other
/// gesture: Alt turns the grid off for as long as it is held.
#[test]
fn alt_pastes_free_of_the_grid() {
    let mut roll = PianoRoll::new(view());
    let mut notes = Arena::default();
    let id = notes.insert(note(0, PPQN / 2, 60));
    roll.select(vec![id]);
    roll.copy(&notes);
    roll.set_modifiers(Modifiers {
        alt: true,
        ..Default::default()
    });

    let edits = roll.paste(PPQN * 8 + 17, 4);
    let [RollEdit::Insert(pasted)] = &edits[..] else {
        panic!("expected one insert, got {edits:?}");
    };
    assert_eq!(pasted[0].start, PPQN * 8 + 17, "Alt means exactly there");
}

// ------------------------------------------------- painting the lane (FL) ---
//
// Reported from using the window:
//
// > *"make the piano rolls velocity controls less like a slider you drag up
// > and down and more like fls where youre kind of drawing it and the notes at
// > the same part your mouse is at horizontally just match where youre
// > clicking while youre clicking ... currently its hard to actually edit
// > multiple notes velocities at once or in a long string or if notes are
// > overlapping eachother or start at the same time."*
//
// Two changes, and they are one idea: the lane is a **canvas you draw on**
// rather than one slider per note. A drag takes whatever bar it crosses,
// including the ones it skipped over between two mouse reports; and a column
// means every bar standing in it, not the topmost note covering that tick.

/// The velocity written by an edit, and the notes it names.
fn lane_edit(edits: &[RollEdit]) -> (Vec<NoteId>, i32) {
    let [RollEdit::SetProperty { ids, value, .. }] = edits else {
        panic!("expected one property edit, got {edits:?}")
    };
    (ids.clone(), *value)
}

fn lane() -> Rect {
    Rect::new(grid().x, 500.0, grid().width, 80.0)
}

/// Every note an edit names, and what it was set to.
fn lane_values(edits: &[RollEdit]) -> Vec<(NoteId, i32)> {
    let mut out = Vec::new();
    for edit in edits {
        let RollEdit::SetProperty { ids, value, .. } = edit else {
            panic!("expected property edits, got {edits:?}")
        };
        out.extend(ids.iter().map(|id| (*id, *value)));
    }
    out
}

fn value_of(edits: &[RollEdit], id: NoteId) -> i32 {
    lane_values(edits)
        .into_iter()
        .find(|(hit, _)| *hit == id)
        .unwrap_or_else(|| panic!("{id:?} was not written by {edits:?}"))
        .1
}

#[test]
fn dragging_across_the_lane_paints_every_bar_it_passes() {
    let mut roll = roll();
    let arena = notes(&[
        (0, PPQN, 60),
        (PPQN, PPQN, 62),
        (PPQN * 2, PPQN, 64),
        (PPQN * 3, PPQN, 65),
    ]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());

    // Down on the first bar, near the bottom: quiet.
    let quiet = lane.bottom() - 6.0;
    let edits = roll.press_lane(tick_to_x(&roll.view, grid, 0), quiet, lane, grid, &arena);
    let (hit, first) = lane_edit(&edits);
    assert_eq!(hit, vec![ids[0]]);
    assert!(first < 20, "the bottom of the lane is quiet, got {first}");

    // Then straight across, still low. Each bar it reaches is written as it
    // is reached — the stroke is a stroke, not four separate grabs.
    for (n, id) in ids.iter().enumerate().skip(1) {
        let x = tick_to_x(&roll.view, grid, PPQN * n as Tick);
        let edits = roll.drag_lane(x, quiet, lane, grid, &arena);
        assert!(
            value_of(&edits, *id) < 20,
            "step {n} of the stroke: {edits:?}"
        );
    }
}

#[test]
fn a_fast_drag_paints_the_bars_it_jumped_over() {
    // A mouse reports a hundred times a second and a hand crosses four bars
    // in less than that, so a lane that only wrote the column it landed in
    // would leave holes in a stroke that looked continuous.
    //
    // And each bar takes the height the pointer was **over it**, not the
    // height it finished at: one report spanning a whole ramp has to come out
    // as the ramp, or drawing quickly and drawing slowly are different tools.
    let mut roll = roll();
    let arena = notes(&[
        (0, PPQN, 60),
        (PPQN, PPQN, 62),
        (PPQN * 2, PPQN, 64),
        (PPQN * 3, PPQN, 65),
    ]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());

    roll.press_lane(
        tick_to_x(&roll.view, grid, 0),
        lane.bottom() - 4.0,
        lane,
        grid,
        &arena,
    );
    let far = tick_to_x(&roll.view, grid, PPQN * 3);
    let edits = roll.drag_lane(far, lane.y + 4.0, lane, grid, &arena);
    let written = lane_values(&edits);
    for id in &ids[1..] {
        assert!(
            written.iter().any(|(hit, _)| hit == id),
            "a bar was jumped over: {written:?}"
        );
    }
    let ramp: Vec<i32> = ids.iter().map(|id| value_of(&edits, *id)).collect();
    for pair in ramp.windows(2) {
        assert!(
            pair[1] > pair[0],
            "the stroke rose, so the bars should: {ramp:?}"
        );
    }
    assert!(ramp[0] < 20 && *ramp.last().unwrap() > 110, "{ramp:?}");
}

#[test]
fn a_column_means_every_note_that_starts_in_it() {
    // *"or if notes are overlapping eachother or start at the same time."* A
    // chord's bars stand on top of one another in the lane, and picking the
    // topmost meant three of the four notes could not be reached at all.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (0, PPQN, 64), (0, PPQN, 67)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());

    let edits = roll.press_lane(
        tick_to_x(&roll.view, grid, 0),
        lane.y + 6.0,
        lane,
        grid,
        &arena,
    );
    let (hit, value) = lane_edit(&edits);
    assert_eq!(hit.len(), 3, "the whole chord, not the top note: {hit:?}");
    for id in &ids {
        assert!(hit.contains(id));
    }
    assert!(value > 110, "got {value}");
}

#[test]
fn a_long_note_underneath_a_chord_does_not_come_with_it() {
    // The pad is still reachable — its own bar is at its own start — but a
    // stroke over the chord on top of it is about the chord. Picking by the
    // note's *span* is what used to make a held pad answer for every column
    // it covered.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN * 8, 48), (PPQN * 4, PPQN, 72)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());

    let edits = roll.press_lane(
        tick_to_x(&roll.view, grid, PPQN * 4),
        lane.y + 6.0,
        lane,
        grid,
        &arena,
    );
    let (hit, _) = lane_edit(&edits);
    assert_eq!(
        hit,
        vec![ids[1]],
        "the short note's bar is the one in that column"
    );

    // And the pad, from its own bar.
    let edits = roll.press_lane(
        tick_to_x(&roll.view, grid, 0),
        lane.y + 6.0,
        lane,
        grid,
        &arena,
    );
    assert_eq!(lane_edit(&edits).0, vec![ids[0]]);
}

#[test]
fn a_lone_held_note_is_still_grabbable_anywhere_along_it() {
    // The forgiving half, and the reason the column rule has a fallback: with
    // nothing else in the column, a bar you can only hit at the note's exact
    // start is a bar nobody can hit.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN * 4, 60)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());
    let edits = roll.press_lane(
        tick_to_x(&roll.view, grid, PPQN * 2),
        lane.y + 6.0,
        lane,
        grid,
        &arena,
    );
    assert_eq!(lane_edit(&edits).0, vec![ids[0]]);
}

#[test]
fn grabbing_a_selected_bar_still_flattens_the_whole_selection() {
    // The one gesture that does *not* follow the pointer: with a selection in
    // hand, the lane sets the lot and keeps setting the lot however far the
    // drag travels. That is what makes flattening a chord one gesture, and it
    // is also the only way to aim a stroke at chosen notes rather than at
    // everything under the brush.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (PPQN * 4, PPQN, 62), (PPQN * 8, PPQN, 64)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());
    roll.select_all(&arena);

    let edits = roll.press_lane(
        tick_to_x(&roll.view, grid, 0),
        lane.y + 6.0,
        lane,
        grid,
        &arena,
    );
    assert_eq!(lane_edit(&edits).0.len(), 3);

    // Dragged sideways over the second bar, and it is still all three.
    let edits = roll.drag_lane(
        tick_to_x(&roll.view, grid, PPQN * 4),
        lane.bottom() - 6.0,
        lane,
        grid,
        &arena,
    );
    let (hit, value) = lane_edit(&edits);
    assert_eq!(hit.len(), 3, "the selection, not what the pointer crossed");
    for id in &ids {
        assert!(hit.contains(id));
    }
    assert!(value < 20);
}

#[test]
fn a_stationary_pointer_asks_for_nothing_new_in_the_lane() {
    // The same rule the fade drag keeps: a held mouse is not a hundred
    // history entries a second.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (lane, grid) = (lane(), grid());
    let x = tick_to_x(&roll.view, grid, 0);
    assert!(
        !roll
            .press_lane(x, lane.y + 6.0, lane, grid, &arena)
            .is_empty()
    );
    assert!(
        roll.drag_lane(x, lane.y + 6.0, lane, grid, &arena)
            .is_empty(),
        "the pointer has not moved and the value has not changed"
    );
}

#[test]
fn a_stroke_over_empty_lane_writes_nothing_and_keeps_going() {
    // A gap in a phrase is a gap in the stroke, not the end of it: the drag
    // stays live so the bars after the rest are still painted.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (PPQN * 8, PPQN, 62)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (lane, grid) = (lane(), grid());

    roll.press_lane(
        tick_to_x(&roll.view, grid, 0),
        lane.y + 6.0,
        lane,
        grid,
        &arena,
    );
    let gap = tick_to_x(&roll.view, grid, PPQN * 4);
    assert!(
        roll.drag_lane(gap, lane.y + 6.0, lane, grid, &arena)
            .is_empty(),
        "nothing is under the brush there"
    );
    let edits = roll.drag_lane(
        tick_to_x(&roll.view, grid, PPQN * 8),
        lane.bottom() - 6.0,
        lane,
        grid,
        &arena,
    );
    assert_eq!(lane_edit(&edits).0, vec![ids[1]]);
}

// ------------------------------------------------------------ the snap chip ---

#[test]
fn the_snap_chip_lists_every_division_in_the_order_it_steps_them() {
    // *"a lot of options that could be knobs or sliders or dropdowns for some
    // reason are instead shown as buttons you click to toggle through a list
    // of options in order iteratively."* The chip drops this list now; `S`
    // still walks it, and the two have to be the same walk or the menu and
    // the key disagree about what comes next.
    use fontelle_ui::canvas::SNAP_DIVISIONS;
    assert!(SNAP_DIVISIONS.len() >= 6);
    for pair in SNAP_DIVISIONS.windows(2) {
        assert_eq!(
            pair[0].next(),
            pair[1],
            "{:?} does not step to {:?}",
            pair[0],
            pair[1]
        );
    }
    assert_eq!(
        SNAP_DIVISIONS.last().unwrap().next(),
        SNAP_DIVISIONS[0],
        "the cycle comes round"
    );
}

#[test]
fn the_snap_chip_wears_a_caret_because_it_drops_a_list() {
    // The lane chip's rule, applied to the chip that got the same treatment:
    // a control that opens something has to look like one.
    let caption = fontelle_ui::canvas::snap_caption(SnapDivision::Step);
    assert!(caption.starts_with(SnapDivision::Step.label()));
    assert!(caption.ends_with('\u{25be}'), "no caret on {caption:?}");
}

#[test]
fn the_snap_chip_still_has_room_for_what_it_says() {
    // A caret costs width, and a chip whose caption is clipped is a chip
    // nobody can read.
    let bar = toolbar_layout(Rect::new(0.0, 0.0, 900.0, 26.0), &metrics());
    let (_, chip) = bar
        .items
        .iter()
        .find(|(control, _)| *control == RollControl::Snap)
        .expect("the snap chip");
    assert!(chip.width >= 60.0, "the snap chip is {} wide", chip.width);
}
