//! A selection box held off the edge of the view, while the view scrolls
//! under it.
//!
//! Reported: *"selection box moves with the auto scroll when cursor goes off
//! screen instead of the box staying where it started and going offscreen so
//! you can actually just be expanding your selection offscreen instead of
//! capping your selection box as your screen size and just moving the box"*.
//!
//! The box's first corner was a screen pixel. Edge scrolling moves the music
//! under the screen, so the corner slid along with the view and a box could
//! never hold more than one screen of notes. The corner belongs to the music:
//! where the press landed in ticks and keys (or ticks and lanes), wherever the
//! view has gone since.
//!
//! The app's edge scroll changes `view` between two drags; these tests do the
//! same, which is all the canvas ever sees of it.

use fontelle_model::{Arena, Note};
use fontelle_types::{ClipId, NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    MouseButton, PianoRoll, RollView, SnapDivision, Timeline, TimelineTool, TimelineView, Tool,
    key_to_y, lane_to_y, tick_to_x, timeline_layout, timeline_tick_to_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

// ------------------------------------------------------------- the roll ---

fn roll_grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
}

fn roll() -> PianoRoll {
    PianoRoll::new(RollView {
        scroll_tick: 0,
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    })
}

fn notes(items: &[(Tick, Tick, u8)]) -> Arena<NoteId, Note> {
    let mut arena = Arena::default();
    for (start, length, key) in items {
        arena.insert(Note {
            start: *start,
            length: *length,
            key: *key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
        });
    }
    arena
}

#[test]
fn a_roll_marquee_keeps_its_first_corner_on_the_music_while_the_view_scrolls() {
    let grid = roll_grid();
    let mut roll = roll();
    roll.tool = Tool::Select;
    // The grid shows ticks 0..3200 and keys 72 down to 39.
    let arena = notes(&[
        (0, PPQN, 60),         // under the press: on screen at the start
        (6000, PPQN, 60),      // right of the screen the press saw
        (6000, PPQN, 20),      // right of it and below it
        (0, PPQN, 70),         // above the corner: never in the box
        (PPQN * 12, PPQN, 30), // past where the pointer reaches
    ]);
    let ids: Vec<NoteId> = arena.keys().collect();

    let x0 = tick_to_x(&roll.view, grid, 0) + 1.0;
    let y0 = key_to_y(&roll.view, grid, 63) + 1.0;
    roll.press(MouseButton::Left, x0, y0, grid, &arena, 4);

    // Off the bottom right corner, and held there while the view travels.
    let (px, py) = (grid.right() + 40.0, grid.bottom() + 40.0);
    roll.drag(px, py, grid, &arena, 4);
    roll.view.scroll_tick += 4000;
    roll.view.top_key -= 40; // 72 -> 32: keys 32 down to -1 now on screen
    roll.drag(px, py, grid, &arena, 4);

    let shown = roll.marquee().expect("the box is still being drawn");
    let corner_x = tick_to_x(&roll.view, grid, 0) + 1.0;
    let corner_y = key_to_y(&roll.view, grid, 63) + 1.0;
    assert!(
        (shown.x - corner_x).abs() < 0.5 && (shown.y - corner_y).abs() < 0.5,
        "the box starts where the press landed in the music, now off screen at \
         ({corner_x}, {corner_y}), not at ({}, {})",
        shown.x,
        shown.y
    );
    assert!(
        shown.right() <= grid.right() && shown.bottom() <= grid.bottom(),
        "and ends at the edge the pointer is held past: {shown:?}"
    );

    roll.release_over(px, py, grid, &arena);
    let mut got = roll.selection().to_vec();
    got.sort();
    let mut wanted = vec![ids[0], ids[1], ids[2]];
    wanted.sort();
    assert_eq!(
        got, wanted,
        "everything from the press to the pointer, on screen or not"
    );
}

#[test]
fn a_roll_marquee_dragged_back_past_its_corner_after_scrolling_still_turns_round() {
    let grid = roll_grid();
    let mut roll = roll();
    roll.tool = Tool::Select;
    let arena = notes(&[(4000, PPQN, 60), (6000, PPQN, 60)]);
    let ids: Vec<NoteId> = arena.keys().collect();

    // Press at tick 6000 after scrolling there, then scroll back left past the
    // press and select leftwards.
    roll.view.scroll_tick = 5000;
    let x0 = tick_to_x(&roll.view, grid, 6000 + PPQN) + 1.0;
    let y0 = key_to_y(&roll.view, grid, 62) + 1.0;
    roll.press(MouseButton::Left, x0, y0, grid, &arena, 4);
    roll.view.scroll_tick = 3000;
    let x1 = tick_to_x(&roll.view, grid, 3500);
    let y1 = key_to_y(&roll.view, grid, 59);
    roll.drag(x1, y1, grid, &arena, 4);
    roll.release_over(x1, y1, grid, &arena);

    let mut got = roll.selection().to_vec();
    got.sort();
    let mut wanted = ids.clone();
    wanted.sort();
    assert_eq!(got, wanted);
}

// ------------------------------------------------------- the arrangement ---

const BAR: Tick = PPQN * 4;

fn clips(specs: &[(usize, Tick, Tick)]) -> Vec<ClipInfo> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    specs
        .iter()
        .enumerate()
        .map(|(n, (lane, start, length))| ClipInfo {
            id: arena.insert(()),
            lane: *lane,
            start: *start,
            length: *length,
            name: format!("Clip {}", n + 1),
            muted: false,
            open: false,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            loop_length: None,
            kind: ClipKind::Notes,
            curve: Vec::new(),
            notes: Vec::new(),
            audio: Default::default(),
            prefab: None,
        })
        .collect()
}

#[test]
fn an_arrangement_marquee_keeps_its_first_corner_on_the_song_while_the_view_scrolls() {
    let mut t = Timeline::new(TimelineView {
        scroll_tick: 0,
        top_lane: 0,
        lane_offset: 0.0,
        pixels_per_tick: 0.025,
        lane_height: 34.0,
        snap: SnapDivision::Bar,
    });
    t.set_tool(TimelineTool::Select);
    let l = timeline_layout(
        Rect::new(0.0, 0.0, 1020.0, 224.0),
        &Theme::dark_default().metrics,
    );
    let grid = l.grid;
    let clips = clips(&[
        (1, BAR * 2, BAR),  // on screen at the press
        (7, BAR * 14, BAR), // right of and below the screen the press saw
        (1, BAR * 40, BAR), // past where the pointer reaches
        (1, 0, BAR / 2),    // left of the corner
    ]);

    let x0 = timeline_tick_to_x(&t.view, grid, BAR) + 1.0;
    let y0 = lane_to_y(&t.view, grid, 0) + 1.0;
    t.press(MouseButton::Left, x0, y0, &l, &clips, 4);

    let (px, py) = (grid.right() + 40.0, grid.bottom() + 40.0);
    t.drag(px, py, &l, &clips, 4);
    t.view.scroll_tick += BAR * 8;
    t.view.top_lane += 4;
    t.drag(px, py, &l, &clips, 4);

    let shown = t.marquee().expect("the box is still being drawn");
    let corner_x = timeline_tick_to_x(&t.view, grid, BAR) + 1.0;
    let corner_y = lane_to_y(&t.view, grid, 0) + 1.0;
    assert!(
        (shown.x - corner_x).abs() < 0.5 && (shown.y - corner_y).abs() < 0.5,
        "the box starts where the press landed in the song, now off screen at \
         ({corner_x}, {corner_y}), not at ({}, {})",
        shown.x,
        shown.y
    );
    assert!(
        shown.right() <= grid.right() && shown.bottom() <= grid.bottom(),
        "and ends at the edge the pointer is held past: {shown:?}"
    );

    t.release_over(px, py, &l, &clips, 4);
    let got = t.selection();
    assert_eq!(got.len(), 2, "got {got:?}");
    assert!(got.contains(&clips[0].id));
    assert!(got.contains(&clips[1].id));
}
