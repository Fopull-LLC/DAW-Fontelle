//! The piano roll's view-model (item 8 of `docs/first-usable-plan.md`).
//!
//! §16.4 makes virtualisation mandatory and §2.5 makes everything that can be a
//! pure function one. Between them, almost the whole roll lives here: what is on
//! screen, where a tick lands, which note is under the pointer, what a drag
//! means. None of it needs a window, and the file below is the reason the
//! drawing code is short enough to read.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DEFAULT_LANE_HEIGHT, NotePart, RollEdit, RollHit, RollView, SnapDivision, Tool, hit_test,
    key_to_y, roll_layout, snap_tick, snap_unit, tick_to_x, visible_keys, visible_ticks, x_to_tick,
    y_to_key,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BEATS_PER_BAR: u32 = 4;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 84,
        key_offset: 0.0,
        pixels_per_tick: 0.125, // one beat (960 ticks) = 120 px
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(100.0, 40.0, 960.0, 480.0)
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
    for &(start, length, key) in items {
        arena.insert(note(start, length, key));
    }
    arena
}

// ---------------------------------------------------------------- layout ---

#[test]
fn the_roll_reserves_a_keyboard_and_a_ruler_and_gives_the_rest_to_the_grid() {
    // With the velocity lane hidden, the grid runs to the bottom of the panel;
    // the toolbar is above the ruler either way. `roll_interaction.rs` covers
    // what the lane does to this when it is showing.
    let l = roll_layout(
        Rect::new(0.0, 0.0, 1000.0, 600.0),
        &Theme::dark_default().metrics,
        0.0,
    );

    assert_eq!(l.keys.x, l.frame.x);
    assert_eq!(l.toolbar.y, l.frame.y);
    assert_eq!(l.ruler.y, l.toolbar.bottom());
    // The grid starts where the ruler and the keyboard end — they meet at its
    // top-left corner and the little square above the keyboard belongs to
    // neither.
    assert_eq!(l.grid.x, l.keys.right());
    assert_eq!(l.grid.y, l.ruler.bottom());
    assert_eq!(l.grid.right(), l.frame.right());
    assert_eq!(l.grid.bottom(), l.frame.bottom());
    assert!(!l.grid.intersects(&l.keys));
    assert!(!l.grid.intersects(&l.ruler));
    assert!(!l.grid.intersects(&l.toolbar));
    // The keyboard runs beside the grid, not beside the ruler.
    assert_eq!(l.keys.y, l.grid.y);
    assert_eq!(l.keys.height, l.grid.height);
}

#[test]
fn a_roll_too_small_for_its_chrome_has_an_empty_grid_and_no_negative_rects() {
    let m = Theme::dark_default().metrics;
    for (w, h) in [(0.0, 0.0), (10.0, 10.0), (30.0, 400.0), (400.0, 12.0)] {
        let l = roll_layout(Rect::new(0.0, 0.0, w, h), &m, DEFAULT_LANE_HEIGHT);
        for r in [l.frame, l.keys, l.ruler, l.grid] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} gave {r:?}");
        }
    }
}

// -------------------------------------------------------------- geometry ---

#[test]
fn the_left_edge_of_the_grid_is_the_scroll_position() {
    let (v, g) = (view(), grid());
    assert_eq!(tick_to_x(&v, g, 0), g.x);
    assert_eq!(x_to_tick(&v, g, g.x), 0);

    let scrolled = RollView {
        scroll_tick: PPQN * 4,
        ..v
    };
    assert_eq!(tick_to_x(&scrolled, g, PPQN * 4), g.x);
    assert_eq!(x_to_tick(&scrolled, g, g.x), PPQN * 4);
}

#[test]
fn a_beat_is_as_wide_as_the_zoom_says() {
    let (v, g) = (view(), grid());
    // 0.125 px per tick, 960 ticks to the beat.
    assert_eq!(tick_to_x(&v, g, PPQN) - g.x, 120.0);
    let zoomed = RollView {
        pixels_per_tick: 0.25,
        ..v
    };
    assert_eq!(tick_to_x(&zoomed, g, PPQN) - g.x, 240.0);
}

#[test]
fn ticks_and_pixels_round_trip_within_a_pixel() {
    let (v, g) = (view(), grid());
    for tick in [0, 1, 480, 960, 12_345, PPQN * 64] {
        let back = x_to_tick(&v, g, tick_to_x(&v, g, tick));
        let tolerance = (1.0 / v.pixels_per_tick) as Tick;
        assert!(
            (back - tick).abs() <= tolerance,
            "{tick} came back as {back}"
        );
    }
}

#[test]
fn pitch_increases_upwards() {
    let (v, g) = (view(), grid());
    // The single thing everyone gets backwards once. C5 must be above C4.
    assert!(key_to_y(&v, g, 72) < key_to_y(&v, g, 60));
    // The top key's row starts at the top of the grid.
    assert_eq!(key_to_y(&v, g, v.top_key), g.y);
    assert_eq!(key_to_y(&v, g, v.top_key - 1), g.y + v.key_height);
}

#[test]
fn a_point_in_a_rows_band_is_that_row() {
    let (v, g) = (view(), grid());
    for key in [40, 60, 72, 84] {
        let y = key_to_y(&v, g, key);
        assert_eq!(y_to_key(&v, g, y + 0.1), key);
        assert_eq!(y_to_key(&v, g, y + v.key_height - 0.1), key);
    }
}

#[test]
fn keys_and_ticks_are_clamped_to_what_midi_and_the_song_allow() {
    let (v, g) = (view(), grid());
    // Dragging off the top of the roll must not produce key 300.
    assert!(y_to_key(&v, g, g.y - 10_000.0) <= 127);
    assert_eq!(y_to_key(&v, g, g.y + 100_000.0), 0);
    // And scrubbing left of bar 1 is tick 0, not a negative tick.
    assert_eq!(x_to_tick(&v, g, g.x - 500.0), 0);
}

// -------------------------------------------------- virtualisation (§16.4) --

#[test]
fn only_the_visible_window_is_asked_for() {
    let (v, g) = (view(), grid());
    // 960 px at 0.125 px/tick is 7680 ticks — eight beats, two bars.
    let ticks = visible_ticks(&v, g);
    assert_eq!(ticks.start, 0);
    assert_eq!(ticks.end, 7680 + 1);

    // 480 px of 12 px rows is 40 keys, counting down from the top one.
    let keys = visible_keys(&v, g);
    assert_eq!(keys.end, i32::from(v.top_key) + 1);
    assert_eq!(keys.start, i32::from(v.top_key) - 40 + 1);
}

#[test]
fn a_project_with_a_hundred_thousand_notes_still_only_asks_for_a_screenful() {
    // The §16.4 promise, as arithmetic: what is visible depends on the
    // viewport and the zoom, and on nothing else at all.
    let (v, g) = (view(), grid());
    let small = visible_ticks(&v, g);
    let far = visible_ticks(
        &RollView {
            scroll_tick: PPQN * 4 * 1000,
            ..v
        },
        g,
    );
    assert_eq!(far.end - far.start, small.end - small.start);
}

#[test]
fn scrolling_past_the_top_of_the_keyboard_does_not_ask_for_keys_that_do_not_exist() {
    let (v, g) = (view(), grid());
    let keys = visible_keys(&RollView { top_key: 3, ..v }, g);
    assert!(keys.start >= 0, "asked for a negative key: {keys:?}");
    assert!(keys.end <= 128);
}

// ------------------------------------------------------------------ snap ---

#[test]
fn the_snap_divisions_are_the_ones_the_keymap_names() {
    assert_eq!(snap_unit(SnapDivision::Bar, BEATS_PER_BAR), PPQN * 4);
    assert_eq!(snap_unit(SnapDivision::Beat, BEATS_PER_BAR), PPQN);
    // A step is a sixteenth — the step sequencer's grid, hence the name.
    assert_eq!(snap_unit(SnapDivision::Step, BEATS_PER_BAR), PPQN / 4);
    assert_eq!(
        snap_unit(SnapDivision::Division(3), BEATS_PER_BAR),
        PPQN / 3
    );
    assert_eq!(snap_unit(SnapDivision::Triplet, BEATS_PER_BAR), PPQN / 3);
    assert_eq!(snap_unit(SnapDivision::None, BEATS_PER_BAR), 0);
}

#[test]
fn snapping_goes_to_the_nearest_line_not_the_one_before() {
    let step = PPQN / 4; // 240
    assert_eq!(snap_tick(0, SnapDivision::Step, BEATS_PER_BAR), 0);
    assert_eq!(snap_tick(100, SnapDivision::Step, BEATS_PER_BAR), 0);
    assert_eq!(snap_tick(140, SnapDivision::Step, BEATS_PER_BAR), step);
    assert_eq!(snap_tick(239, SnapDivision::Step, BEATS_PER_BAR), step);
    assert_eq!(snap_tick(step, SnapDivision::Step, BEATS_PER_BAR), step);
}

#[test]
fn no_snap_leaves_the_tick_exactly_where_it_was() {
    for tick in [0, 1, 137, 99_999] {
        assert_eq!(snap_tick(tick, SnapDivision::None, BEATS_PER_BAR), tick);
    }
}

#[test]
fn snapping_never_produces_a_negative_tick() {
    assert_eq!(snap_tick(-100, SnapDivision::Bar, BEATS_PER_BAR), 0);
}

// ----------------------------------------------------------- hit-testing ---

#[test]
fn clicking_a_note_finds_it() {
    let (v, g) = (view(), grid());
    let arena = notes(&[(0, PPQN, 60)]);
    let id = arena.keys().next().expect("one note");

    let x = tick_to_x(&v, g, PPQN / 2);
    let y = key_to_y(&v, g, 60) + v.key_height / 2.0;
    assert_eq!(
        hit_test(&v, g, &arena, x, y),
        RollHit::Note(id, NotePart::Body)
    );
}

#[test]
fn the_ends_of_a_note_are_its_resize_handles() {
    let (v, g) = (view(), grid());
    let arena = notes(&[(0, PPQN, 60)]);
    let id = arena.keys().next().expect("one note");
    let y = key_to_y(&v, g, 60) + v.key_height / 2.0;

    let right = hit_test(&v, g, &arena, tick_to_x(&v, g, PPQN) - 2.0, y);
    assert_eq!(right, RollHit::Note(id, NotePart::RightEdge));
    let left = hit_test(&v, g, &arena, tick_to_x(&v, g, 0) + 1.0, y);
    assert_eq!(left, RollHit::Note(id, NotePart::LeftEdge));
}

#[test]
fn a_note_too_short_to_have_handles_is_all_body() {
    let (v, g) = (view(), grid());
    // Six pixels wide at this zoom. Two eight-pixel handles would leave no
    // way to move it, and dragging a note is more common than resizing one.
    let arena = notes(&[(0, 48, 60)]);
    let id = arena.keys().next().expect("one note");
    let y = key_to_y(&v, g, 60) + v.key_height / 2.0;
    let x = tick_to_x(&v, g, 24);
    assert_eq!(
        hit_test(&v, g, &arena, x, y),
        RollHit::Note(id, NotePart::Body)
    );
}

#[test]
fn clicking_between_notes_reports_the_cell_you_clicked() {
    let (v, g) = (view(), grid());
    let arena = notes(&[(0, PPQN, 60)]);

    let x = tick_to_x(&v, g, PPQN * 2 + 30);
    let y = key_to_y(&v, g, 64) + 2.0;
    match hit_test(&v, g, &arena, x, y) {
        RollHit::Empty { tick, key } => {
            assert_eq!(key, 64);
            // Unsnapped — snapping is the caller's decision, because
            // Alt-drag bypasses it (§16.5) and a hit-test that had already
            // snapped could not offer that.
            assert!((tick - (PPQN * 2 + 30)).abs() < 10);
        }
        other => panic!("expected an empty cell, got {other:?}"),
    }
}

#[test]
fn clicking_outside_the_grid_is_not_a_cell() {
    let (v, g) = (view(), grid());
    let arena = notes(&[]);
    assert_eq!(
        hit_test(&v, g, &arena, g.x - 5.0, g.y + 5.0),
        RollHit::Outside
    );
    assert_eq!(
        hit_test(&v, g, &arena, g.x + 5.0, g.y - 5.0),
        RollHit::Outside
    );
}

#[test]
fn the_topmost_note_wins_when_two_overlap() {
    let (v, g) = (view(), grid());
    // Two notes on the same key and the same tick: the later one in the
    // arena is the one drawn on top, so it is the one you clicked.
    let mut arena = Arena::default();
    let _under = arena.insert(note(0, PPQN, 60));
    let over = arena.insert(note(0, PPQN, 60));
    let x = tick_to_x(&v, g, PPQN / 2);
    let y = key_to_y(&v, g, 60) + v.key_height / 2.0;
    assert_eq!(
        hit_test(&v, g, &arena, x, y),
        RollHit::Note(over, NotePart::Body)
    );
}

// -------------------------------------------------------------- gestures ---

use fontelle_ui::canvas::{MouseButton, PianoRoll};

fn roll() -> PianoRoll {
    PianoRoll::new(view())
}

#[test]
fn drawing_on_an_empty_cell_asks_for_a_note_of_one_snap_unit() {
    let mut roll = roll();
    let arena = notes(&[]);
    let (v, g) = (roll.view, grid());

    let edits = roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 500),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert_eq!(
        edits,
        vec![RollEdit::Add {
            note: Note {
                start: snap_tick(500, SnapDivision::Step, BEATS_PER_BAR),
                length: snap_unit(SnapDivision::Step, BEATS_PER_BAR),
                key: 60,
                ..roll.template()
            },
        }]
    );
}

#[test]
fn right_clicking_a_note_deletes_it_and_nothing_else() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (PPQN, PPQN, 62)]);
    let id = arena.keys().next().expect("a note");
    let (v, g) = (roll.view, grid());

    let edits = roll.press(
        MouseButton::Right,
        tick_to_x(&v, g, PPQN / 2),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert_eq!(edits, vec![RollEdit::Remove(vec![id])]);
}

#[test]
fn right_clicking_empty_space_draws_nothing() {
    let mut roll = roll();
    let arena = notes(&[]);
    let (v, g) = (roll.view, grid());
    let edits = roll.press(
        MouseButton::Right,
        tick_to_x(&v, g, 500),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    // Right-click is delete in every tool (§16.5). Deleting nothing is not
    // drawing something.
    assert!(edits.is_empty());
}

#[test]
fn dragging_a_note_moves_it_by_whole_snap_steps() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (v, g) = (roll.view, grid());
    let y = key_to_y(&v, g, 60) + 2.0;

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        y,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    // Not far enough to reach the next step: no movement, and crucially no
    // zero-delta edit, which would be a history entry for nothing.
    let none = roll.drag(tick_to_x(&v, g, 150), y, g, &arena, BEATS_PER_BAR);
    assert!(none.is_empty(), "a sub-step drag moved the note: {none:?}");

    let step = snap_unit(SnapDivision::Step, BEATS_PER_BAR);
    let moved = roll.drag(tick_to_x(&v, g, 100 + step), y, g, &arena, BEATS_PER_BAR);
    match moved.as_slice() {
        [
            RollEdit::Move {
                tick_delta,
                key_delta,
                ..
            },
        ] => {
            assert_eq!(*tick_delta, step);
            assert_eq!(*key_delta, 0);
        }
        other => panic!("expected one move, got {other:?}"),
    }
}

#[test]
fn a_drag_emits_the_step_since_the_last_one_not_the_total() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (v, g) = (roll.view, grid());
    let y = key_to_y(&v, g, 60) + 2.0;
    let step = snap_unit(SnapDivision::Step, BEATS_PER_BAR);

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 0),
        y,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    roll.drag(tick_to_x(&v, g, step), y, g, &arena, BEATS_PER_BAR);
    let second = roll.drag(tick_to_x(&v, g, step * 2), y, g, &arena, BEATS_PER_BAR);
    // `MoveNotes` is a delta command that coalesces, so each step of the drag
    // must be relative to the last. Sending the total each time would move the
    // note by 1 + 2 + 3 + ... steps.
    match second.as_slice() {
        [RollEdit::Move { tick_delta, .. }] => assert_eq!(*tick_delta, step),
        other => panic!("expected one move of one step, got {other:?}"),
    }
}

#[test]
fn dragging_a_notes_right_edge_resizes_it() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (v, g) = (roll.view, grid());
    let y = key_to_y(&v, g, 60) + 2.0;
    let step = snap_unit(SnapDivision::Step, BEATS_PER_BAR);

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, PPQN) - 2.0,
        y,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    let edits = roll.drag(tick_to_x(&v, g, PPQN + step), y, g, &arena, BEATS_PER_BAR);
    match edits.as_slice() {
        [RollEdit::Resize { tick_delta, .. }] => assert_eq!(*tick_delta, step),
        other => panic!("expected one resize, got {other:?}"),
    }
}

#[test]
fn dragging_up_a_row_transposes_by_a_semitone() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (v, g) = (roll.view, grid());

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    let edits = roll.drag(
        tick_to_x(&v, g, 100),
        key_to_y(&v, g, 63) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    match edits.as_slice() {
        [
            RollEdit::Move {
                key_delta,
                tick_delta,
                ..
            },
        ] => {
            assert_eq!(*key_delta, 3);
            assert_eq!(*tick_delta, 0);
        }
        other => panic!("expected a transpose, got {other:?}"),
    }
}

#[test]
fn clicking_a_note_selects_it_and_clicking_another_replaces_the_selection() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (PPQN, PPQN, 62)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (v, g) = (roll.view, grid());

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert_eq!(roll.selection(), &[ids[0]]);

    roll.release();
    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, PPQN + 100),
        key_to_y(&v, g, 62) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert_eq!(roll.selection(), &[ids[1]]);
}

#[test]
fn dragging_moves_the_whole_selection() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (0, PPQN, 64)]);
    let ids: Vec<NoteId> = arena.keys().collect();
    let (v, g) = (roll.view, grid());
    let step = snap_unit(SnapDivision::Step, BEATS_PER_BAR);

    roll.select_all(&arena);
    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    let edits = roll.drag(
        tick_to_x(&v, g, 100 + step),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    match edits.as_slice() {
        [RollEdit::Move { ids: moved, .. }] => {
            assert_eq!(moved.len(), 2, "only part of the selection moved");
            for id in &ids {
                assert!(moved.contains(id));
            }
        }
        other => panic!("expected one move of both notes, got {other:?}"),
    }
}

#[test]
fn pressing_a_note_that_is_already_selected_keeps_the_selection() {
    // Otherwise dragging a chord would collapse it to whichever note you
    // happened to grab.
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (0, PPQN, 64)]);
    let (v, g) = (roll.view, grid());

    roll.select_all(&arena);
    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert_eq!(roll.selection().len(), 2);
}

#[test]
fn deleting_the_selection_removes_exactly_it() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60), (0, PPQN, 64)]);
    roll.select_all(&arena);
    match roll.delete_selection().as_slice() {
        [RollEdit::Remove(ids)] => assert_eq!(ids.len(), 2),
        other => panic!("expected one removal, got {other:?}"),
    }
    // And with nothing selected it asks for nothing, rather than for an
    // empty removal that would still be a history entry.
    assert!(roll.delete_selection().is_empty());
}

#[test]
fn the_delete_tool_removes_on_press_instead_of_selecting() {
    let mut roll = roll();
    roll.tool = Tool::Delete;
    let arena = notes(&[(0, PPQN, 60)]);
    let id = arena.keys().next().expect("a note");
    let (v, g) = (roll.view, grid());

    let edits = roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert_eq!(edits, vec![RollEdit::Remove(vec![id])]);
}

#[test]
fn the_select_tool_does_not_draw_on_empty_space() {
    let mut roll = roll();
    roll.tool = Tool::Select;
    let arena = notes(&[]);
    let (v, g) = (roll.view, grid());
    let edits = roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 500),
        key_to_y(&v, g, 60) + 2.0,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    assert!(edits.is_empty(), "the select tool drew a note: {edits:?}");
}

#[test]
fn a_release_ends_the_gesture_so_the_next_drag_is_a_new_one() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (v, g) = (roll.view, grid());
    let y = key_to_y(&v, g, 60) + 2.0;

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 0),
        y,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    roll.release();
    // Moving the mouse with the button up must not move anything. This is
    // also what makes one drag one history entry: the app breaks the gesture
    // on the same release.
    let edits = roll.drag(tick_to_x(&v, g, 5000), y, g, &arena, BEATS_PER_BAR);
    assert!(
        edits.is_empty(),
        "a mouse-up did not end the drag: {edits:?}"
    );
}

#[test]
fn a_note_cannot_be_dragged_off_the_start_of_the_clip() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let (v, g) = (roll.view, grid());
    let y = key_to_y(&v, g, 60) + 2.0;

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, 100),
        y,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    let edits = roll.drag(tick_to_x(&v, g, 0) - 400.0, y, g, &arena, BEATS_PER_BAR);
    for edit in &edits {
        if let RollEdit::Move { tick_delta, .. } = edit {
            assert!(
                *tick_delta >= -0,
                "dragged a note at tick 0 to a negative tick"
            );
        }
    }
}

#[test]
fn resizing_never_makes_a_note_shorter_than_nothing() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN / 4, 60)]);
    let (v, g) = (roll.view, grid());
    let y = key_to_y(&v, g, 60) + 2.0;

    roll.press(
        MouseButton::Left,
        tick_to_x(&v, g, PPQN / 4) - 2.0,
        y,
        g,
        &arena,
        BEATS_PER_BAR,
    );
    let edits = roll.drag(tick_to_x(&v, g, 0) - 500.0, y, g, &arena, BEATS_PER_BAR);
    match edits.as_slice() {
        [] => {}
        [RollEdit::Resize { tick_delta, .. }] => assert!(
            PPQN / 4 + tick_delta > 0,
            "resized a note down to {} ticks",
            PPQN / 4 + tick_delta
        ),
        other => panic!("expected at most one resize, got {other:?}"),
    }
}

// --------------------------------------------------------- the grid's levels ---

/// Reported from using the window: *"it's kind of hard to tell the time right
/// now so ensure between bars or measures there's more dividers so I can see
/// the on and offbeats and stuff in the piano roll — just makes it easier to
/// lay stuff down."*
///
/// Two things were wrong and only one of them was the ink. The other is this:
/// the roll drew its finest lines at **the snap division**, so setting the
/// snap to bars or turning it off removed every line between the bars and left
/// a bar-wide empty box to place notes in by eye.
///
/// The grid is not the snap. It is the ruler you read the time off, and it
/// keeps showing at least the eighths whatever the snap is doing.
#[test]
fn the_grid_shows_the_offbeats_however_coarse_the_snap_is() {
    use fontelle_ui::canvas::subdivision_unit;

    assert_eq!(
        subdivision_unit(SnapDivision::Bar, 4),
        PPQN / 2,
        "snapping to bars must not empty the bar"
    );
    assert_eq!(subdivision_unit(SnapDivision::Beat, 4), PPQN / 2);
    assert_eq!(
        subdivision_unit(SnapDivision::None, 4),
        PPQN / 2,
        "free positioning is about where a note may go, not about what you can see"
    );
}

/// And a snap finer than an eighth shows *itself*, because then the grid and
/// the snap agree and the lines you are aiming at are the lines you can see.
#[test]
fn a_snap_finer_than_an_eighth_is_the_grid_that_is_drawn() {
    use fontelle_ui::canvas::subdivision_unit;

    assert_eq!(subdivision_unit(SnapDivision::Step, 4), PPQN / 4);
    assert_eq!(subdivision_unit(SnapDivision::Division(8), 4), PPQN / 8);
    assert_eq!(
        subdivision_unit(SnapDivision::Triplet, 4),
        PPQN / 3,
        "a triplet grid is the one thing an eighth grid cannot show"
    );
}
