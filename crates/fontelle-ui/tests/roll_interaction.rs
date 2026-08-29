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
    Modifiers, MouseButton, PianoRoll, RollControl, RollEdit, RollView, SnapDivision, Tool,
    key_to_y, note_at_tick, roll_layout, tick_to_x, toolbar_hit, toolbar_layout, velocity_of_y,
    x_to_tick, zoom_x, zoom_y,
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

#[test]
fn drawing_a_note_and_dragging_right_sizes_it_in_one_gesture() {
    let mut roll = roll();
    let mut arena = notes(&[]);

    let step = PPQN / 4;
    let (x, y) = at(&roll.view, 0, 60);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    assert_eq!(
        edits,
        vec![RollEdit::Add {
            tick: 0,
            key: 60,
            length: step,
            velocity: roll.default_velocity,
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
    let edits = roll.paste(PPQN * 8);
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

    let edits = roll.paste(PPQN * 2);
    let [RollEdit::Insert(pasted)] = &edits[..] else {
        panic!("expected an insert")
    };
    assert_eq!(pasted.len(), 1);
    assert_eq!(pasted[0].start, PPQN * 2);
}

#[test]
fn pasting_an_empty_clipboard_asks_for_nothing() {
    let mut roll = roll();
    assert!(roll.paste(PPQN).is_empty());
    // And an empty selection copies nothing rather than emptying what is held.
    let arena = notes(&[(0, PPQN, 60)]);
    roll.select_all(&arena);
    roll.copy(&arena);
    roll.clear_selection();
    assert_eq!(roll.copy(&arena), 0);
    assert_eq!(
        roll.paste(0).len(),
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
    let edits = roll.press_velocity(x, lane.y + 8.0, lane, grid(), &arena);
    let [RollEdit::SetVelocity { ids: hit, velocity }] = &edits[..] else {
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
    let edits = roll.drag_velocity(x, lane.bottom() - 8.0, lane, grid(), &arena);
    let [RollEdit::SetVelocity { ids: hit, velocity }] = &edits[..] else {
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
    let edits = roll.press_velocity(x, lane.y + 8.0, lane, grid(), &arena);
    let [RollEdit::SetVelocity { ids, .. }] = &edits[..] else {
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
        roll.press_velocity(x, lane.y + 8.0, lane, grid(), &arena)
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
    let l = roll_layout(frame, &m, true);

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
    let without = roll_layout(frame, &m, false);
    assert!(without.velocity.is_empty());
    assert!(without.grid.height > l.grid.height);
    assert_eq!(without.grid.bottom(), frame.bottom());
}

#[test]
fn every_toolbar_control_is_inside_the_toolbar_and_clickable() {
    let m = metrics();
    let l = roll_layout(Rect::new(0.0, 0.0, 900.0, 520.0), &m, true);
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
