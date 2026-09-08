//! Making a clip in the arrangement.
//!
//! Reported from using the window:
//!
//! > *"It's also way too difficult to just make a new clip in the arrangement
//! > right now — I can't even figure out how. It's like I'm given one clip
//! > when I make a new channel and I have to roll with that."*
//!
//! That was exactly true. The arrangement could move, resize, duplicate,
//! delete, copy, cut, paste, mute and loop clips — everything except **make**
//! one. A press on empty grid started a marquee, always, so the one gesture
//! anybody would try first did nothing visible.
//!
//! # A tool, not a double-click
//!
//! The fix is FL Studio's: the arrangement gets a **draw** tool and a
//! **select** tool, two chips on the toolbar it already has, and draw is the
//! default. Two reasons for a tool rather than a double-click: a double-click
//! is invisible — there is nothing on screen that says it exists, which is the
//! whole complaint — and the roll next to it already works this way, so it is
//! one idea rather than two.
//!
//! The marquee is still there: pick Select, or hold Ctrl.

use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, ClipPart, Modifiers, MouseButton, Timeline, TimelineControl, TimelineHit,
    TimelineTool, TimelineView, clip_rect, timeline_hit, timeline_layout, timeline_toolbar_hit,
    timeline_toolbar_layout,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn body() -> Rect {
    Rect::new(0.0, 0.0, 900.0, 300.0)
}

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        ..TimelineView::default()
    }
}

fn a_clip(id: ClipId, lane: usize, start: Tick, length: Tick) -> ClipInfo {
    ClipInfo {
        id,
        lane,
        start,
        length,
        name: "Part".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: ClipKind::Notes,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

fn an_id() -> ClipId {
    let mut arena: fontelle_model::Arena<ClipId, ()> = fontelle_model::Arena::default();
    arena.insert(())
}

/// A press on empty grid at `(tick, lane)`, and what it asked for.
fn press_empty(
    timeline: &mut Timeline,
    clips: &[ClipInfo],
    tick: Tick,
    lane: usize,
) -> Vec<ArrangeEdit> {
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let x = fontelle_ui::canvas::timeline_tick_to_x(&timeline.view, l.grid, tick);
    let y = fontelle_ui::canvas::lane_to_y(&timeline.view, l.grid, lane)
        + timeline.view.lane_height / 2.0;
    timeline.press(MouseButton::Left, x, y, &l, clips, 4)
}

// ----------------------------------------------------------- the default ---

#[test]
fn the_arrangement_starts_in_draw_because_the_first_thing_you_want_is_a_clip() {
    let timeline = Timeline::new(view());
    assert_eq!(timeline.tool(), TimelineTool::Draw);
}

#[test]
fn drawing_on_empty_grid_asks_for_a_clip() {
    let mut timeline = Timeline::new(view());
    let edits = press_empty(&mut timeline, &[], PPQN * 8, 0);

    assert_eq!(
        edits,
        vec![ArrangeEdit::Add {
            lane: 0,
            start: PPQN * 8,
        }]
    );
}

#[test]
fn a_drawn_clip_lands_on_the_grid_rather_than_where_the_pointer_was() {
    // A clip half a beat off the bar is a clip somebody has to nudge before
    // they can use it, and the snap is right there in the toolbar saying what
    // it should have been.
    let mut timeline = Timeline::new(view());
    let edits = press_empty(&mut timeline, &[], PPQN * 8 + 37, 0);

    match edits.as_slice() {
        [ArrangeEdit::Add { start, .. }] => assert_eq!(
            *start,
            PPQN * 8,
            "snapped to the bar, not left at {}",
            PPQN * 8 + 37
        ),
        other => panic!("expected one Add, got {other:?}"),
    }
}

#[test]
fn a_clip_is_never_drawn_before_the_start_of_the_song() {
    let mut timeline = Timeline::new(view());
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    // Left of the grid entirely.
    let edits = timeline.press(
        MouseButton::Left,
        l.grid.x - 200.0,
        l.grid.y + 5.0,
        &l,
        &[],
        4,
    );
    match edits.as_slice() {
        [ArrangeEdit::Add { start, .. }] => assert!(*start >= 0, "start {start}"),
        [] => {}
        other => panic!("expected one Add or none, got {other:?}"),
    }
}

#[test]
fn drawing_on_a_clip_still_moves_it() {
    // The tool decides what an *empty* press means. A press on a clip is a
    // press on a clip in either tool, or the draw tool would make the
    // arrangement unusable the moment it had anything in it.
    let id = an_id();
    let clips = vec![a_clip(id, 0, 0, PPQN * 4)];
    let mut timeline = Timeline::new(view());
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);

    let edits = timeline.press(
        MouseButton::Left,
        block.x + block.width / 2.0,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    assert!(
        edits.is_empty(),
        "a move emits on drag, not on press: {edits:?}"
    );
    assert_eq!(timeline.selection(), &[id]);
}

// ------------------------------------------------------------- the marquee ---

#[test]
fn the_select_tool_marquees_the_way_the_arrangement_always_did() {
    let mut timeline = Timeline::new(view());
    timeline.set_tool(TimelineTool::Select);
    let edits = press_empty(&mut timeline, &[], PPQN * 8, 0);
    assert!(edits.is_empty(), "a marquee asks for nothing: {edits:?}");
    assert!(timeline.marquee().is_some(), "and it is being drawn");
}

#[test]
fn ctrl_marquees_without_leaving_the_draw_tool() {
    // Holding a modifier to get the other reading of a drag is what the roll
    // already does, and it means selecting a few clips does not cost two trips
    // to the toolbar.
    let mut timeline = Timeline::new(view());
    timeline.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::default()
    });
    let edits = press_empty(&mut timeline, &[], PPQN * 8, 0);
    assert!(edits.is_empty(), "{edits:?}");
    assert!(timeline.marquee().is_some());
    assert_eq!(
        timeline.tool(),
        TimelineTool::Draw,
        "and the tool is unchanged"
    );
}

#[test]
fn a_right_press_still_erases_in_either_tool() {
    let id = an_id();
    let clips = vec![a_clip(id, 0, 0, PPQN * 4)];
    let mut timeline = Timeline::new(view());
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);

    let edits = timeline.press(
        MouseButton::Right,
        block.x + block.width / 2.0,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    assert_eq!(edits, vec![ArrangeEdit::Remove(vec![id])]);
}

// ------------------------------------------------------------- the toolbar ---

#[test]
fn the_toolbar_carries_both_tools_and_says_which_is_on() {
    let m = Theme::dark_default().metrics;
    let bar = timeline_toolbar_layout(Rect::new(0.0, 0.0, 900.0, 26.0), &m);

    for tool in [TimelineControl::Draw, TimelineControl::Select] {
        let chip = bar
            .items
            .iter()
            .find(|(c, _)| *c == tool)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{tool:?} is not on the toolbar"));
        assert_eq!(
            timeline_toolbar_hit(&bar, chip.x + chip.width / 2.0, chip.y + chip.height / 2.0),
            Some(tool)
        );
        assert!(tool.icon().is_some(), "{tool:?} draws a glyph");
    }
}

#[test]
fn the_tools_come_first_because_they_decide_what_every_other_press_means() {
    let m = Theme::dark_default().metrics;
    let bar = timeline_toolbar_layout(Rect::new(0.0, 0.0, 900.0, 26.0), &m);
    let x_of = |want: TimelineControl| {
        bar.items
            .iter()
            .find(|(c, _)| *c == want)
            .map(|(_, r)| r.x)
            .expect("on the toolbar")
    };
    assert!(x_of(TimelineControl::Draw) < x_of(TimelineControl::Snap));
    assert!(x_of(TimelineControl::Select) < x_of(TimelineControl::Snap));
}

#[test]
fn the_edge_grip_still_resizes_whichever_tool_is_on() {
    let id = an_id();
    let clips = vec![a_clip(id, 0, 0, PPQN * 4)];
    let v = view();
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let block = clip_rect(&v, l.grid, &clips[0]);
    assert_eq!(
        timeline_hit(&v, &l, &clips, block.right() - 2.0, block.y + 5.0),
        TimelineHit::Clip(id, ClipPart::RightEdge)
    );
}
