//! The arrangement canvas — clips as blocks on lanes (TDD §16.4, and the half
//! of item 9 of `docs/first-usable-plan.md` that was still outstanding).
//!
//! Reported from using the window: *"why is there no timeline right now, only
//! piano roll?"* There was not one because nothing had been built; the piano
//! roll shows one clip and there was no view of the piece.
//!
//! This is the roll's shape applied to a coarser grid: virtualised the same way
//! (§16.4 — a project with two hundred lanes builds a screenful of rectangles),
//! edits by value rather than by mutation (INVARIANT 2), and every decision a
//! pure function so it is testable without a window (§2.5).

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, ClipPart, MouseButton, SnapDivision, Timeline, TimelineHit, TimelineView,
    clip_rect, lane_to_y, timeline_hit, timeline_layout, visible_lanes, y_to_lane,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn view() -> TimelineView {
    TimelineView {
        scroll_tick: 0,
        top_lane: 0,
        // A bar (4 beats = 3840 ticks) is 96 pixels: about what an arrangement
        // is read at.
        pixels_per_tick: 0.025,
        lane_height: 34.0,
        snap: SnapDivision::Bar,
    }
}

fn grid() -> Rect {
    Rect::new(120.0, 24.0, 900.0, 200.0)
}

/// Clips with real ids, minted the way the document mints them — there is no
/// other way to make a `ClipId`, and inventing one would be inventing a key
/// the arena never issued.
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
        })
        .collect()
}

/// One clip, when that is all a test needs.
fn clip(_id: u32, lane: usize, start: Tick, length: Tick) -> ClipInfo {
    clips(&[(lane, start, length)]).remove(0)
}

const BAR: Tick = PPQN * 4;

// ------------------------------------------------------------- geometry ---

#[test]
fn the_arrangement_reserves_a_ruler_and_a_lane_header_column() {
    let m = metrics();
    let frame = Rect::new(0.0, 0.0, 1000.0, 220.0);
    let l = timeline_layout(frame, &m);

    // The toolbar is the top of the panel now — the arrangement had no
    // controls at all until snap, repeat and the clipboard needed somewhere to
    // live, and a control nobody can see is a control nobody has.
    assert_eq!(l.toolbar.y, frame.y, "the toolbar is the top of the panel");
    assert_eq!(l.ruler.y, l.toolbar.bottom(), "the ruler sits under it");
    assert_eq!(l.grid.y, l.ruler.bottom());
    assert!(!l.toolbar.intersects(&l.ruler));
    assert!(!l.toolbar.intersects(&l.grid));
    assert_eq!(l.headers.y, l.grid.y, "the headers run beside the grid");
    assert_eq!(l.headers.height, l.grid.height);
    assert_eq!(l.grid.x, l.headers.right());
    assert_eq!(l.grid.right(), frame.right());
    assert_eq!(l.grid.bottom(), frame.bottom());
    assert!(!l.grid.intersects(&l.headers));
    assert!(!l.grid.intersects(&l.ruler));
}

#[test]
fn a_panel_too_small_for_its_chrome_yields_no_negative_rectangles() {
    let m = metrics();
    for (w, h) in [(0.0, 0.0), (8.0, 8.0), (40.0, 300.0), (600.0, 6.0)] {
        let l = timeline_layout(Rect::new(0.0, 0.0, w, h), &m);
        for r in [l.frame, l.toolbar, l.ruler, l.headers, l.grid] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} gave {r:?}");
        }
    }
}

#[test]
fn only_the_visible_lanes_are_built_however_many_there_are() {
    let v = view();
    let g = grid();
    // 200 pixels of grid at 34 per lane is six rows; a project with two hundred
    // lanes must cost six rectangles, not two hundred (§16.4).
    let lanes = visible_lanes(&v, g, 200);
    assert!(lanes.len() <= 8, "built {} lanes", lanes.len());
    assert_eq!(lanes.start, 0);

    // Scrolled down, the window moves rather than growing.
    let scrolled = visible_lanes(&TimelineView { top_lane: 40, ..v }, g, 200);
    assert_eq!(scrolled.start, 40);
    assert_eq!(scrolled.len(), lanes.len());

    // And it never runs past the end of the project.
    let short = visible_lanes(&v, g, 2);
    assert_eq!(short, 0..2);
}

#[test]
fn a_lane_and_its_row_convert_both_ways() {
    let v = view();
    let g = grid();
    for lane in 0..5 {
        let y = lane_to_y(&v, g, lane);
        assert_eq!(y_to_lane(&v, g, y + 1.0), lane);
    }
    assert_eq!(y_to_lane(&v, g, g.y - 100.0), 0, "clamped above the grid");
}

#[test]
fn a_clip_becomes_the_block_its_start_and_length_describe() {
    let v = view();
    let g = grid();
    let block = clip_rect(&v, g, &clip(1, 2, BAR * 4, BAR * 2));

    assert_eq!(block.y, lane_to_y(&v, g, 2));
    assert_eq!(block.height, v.lane_height);
    assert!((block.width - (BAR * 2) as f32 * v.pixels_per_tick).abs() < 0.01);
    // A clip too short to see is still a block you can click.
    let tiny = clip_rect(&v, g, &clip(2, 0, 0, 1));
    assert!(tiny.width >= 1.0);
}

// ---------------------------------------------------------- hit-testing ---

#[test]
fn hit_testing_says_which_clip_and_which_part_of_it() {
    let v = view();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, BAR, BAR * 4)]);
    let block = clip_rect(&v, l.grid, &clips[0]);

    let body = timeline_hit(&v, &l, &clips, block.x + block.width / 2.0, block.y + 4.0);
    assert_eq!(
        body,
        TimelineHit::Clip(clips[0].id, ClipPart::Body),
        "got {body:?}"
    );

    let edge = timeline_hit(&v, &l, &clips, block.right() - 2.0, block.y + 4.0);
    assert_eq!(edge, TimelineHit::Clip(clips[0].id, ClipPart::RightEdge));

    // Empty grid says where, so a double-click there could make a clip.
    match timeline_hit(&v, &l, &clips, block.right() + 200.0, block.y + 4.0) {
        TimelineHit::Empty { lane, .. } => assert_eq!(lane, 0),
        other => panic!("expected empty grid, got {other:?}"),
    }

    // The ruler is its own answer: that is where the time marker is set.
    let on_ruler = timeline_hit(&v, &l, &clips, l.ruler.x + 300.0, l.ruler.y + 2.0);
    assert!(
        matches!(on_ruler, TimelineHit::Ruler(_)),
        "got {on_ruler:?}"
    );
    // As is a lane header.
    let on_header = timeline_hit(&v, &l, &clips, l.headers.x + 4.0, l.headers.y + 4.0);
    assert_eq!(on_header, TimelineHit::Lane(0));

    assert_eq!(
        timeline_hit(&v, &l, &clips, -50.0, -50.0),
        TimelineHit::Outside
    );
}

// ------------------------------------------------------------- gestures ---

fn timeline() -> Timeline {
    Timeline::new(view())
}

#[test]
fn clicking_a_clip_selects_it_and_asks_for_it_to_be_opened() {
    let mut t = timeline();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, BAR, BAR * 4), (1, 0, BAR)]);
    let block = clip_rect(&t.view, l.grid, &clips[0]);

    let edits = t.press(
        MouseButton::Left,
        block.x + 20.0,
        block.y + 4.0,
        &l,
        &clips,
        4,
    );
    assert!(edits.is_empty(), "selecting is not an edit: {edits:?}");
    assert_eq!(t.selection(), &[clips[0].id]);
    assert_eq!(
        t.take_open(),
        Some(clips[0].id),
        "the roll follows the arrangement — clicking a clip opens it"
    );
    assert_eq!(t.take_open(), None, "and it is asked for exactly once");
}

#[test]
fn dragging_a_clip_moves_it_by_whole_bars_and_never_before_the_start() {
    let mut t = timeline();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, BAR * 4, BAR * 4)]);
    let block = clip_rect(&t.view, l.grid, &clips[0]);
    let y = block.y + 4.0;

    t.press(MouseButton::Left, block.x + 20.0, y, &l, &clips, 4);

    // A nudge shorter than a bar is not a move, and not a history entry.
    let none = t.drag(block.x + 21.0, y, &l, &clips, 4);
    assert!(none.is_empty(), "a sub-bar drag moved it: {none:?}");

    let one_bar = BAR as f32 * t.view.pixels_per_tick;
    let moved = t.drag(block.x + 20.0 + one_bar, y, &l, &clips, 4);
    let [
        ArrangeEdit::Move {
            ids,
            tick_delta,
            lane_delta,
        },
    ] = &moved[..]
    else {
        panic!("expected one move, got {moved:?}");
    };
    assert_eq!(ids, &vec![clips[0].id]);
    assert_eq!(*tick_delta, BAR);
    assert_eq!(*lane_delta, 0);

    // And dragged hard left it stops at bar 1 rather than going negative.
    let back = t.drag(l.grid.x - 500.0, y, &l, &clips, 4);
    let total: Tick = back
        .iter()
        .map(|e| match e {
            ArrangeEdit::Move { tick_delta, .. } => *tick_delta,
            _ => 0,
        })
        .sum::<Tick>()
        + BAR;
    assert_eq!(
        clips[0].start + total,
        0,
        "the clip lands on bar 1, not before it"
    );
}

#[test]
fn dragging_a_clip_down_a_row_changes_its_lane() {
    let mut t = timeline();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, 0, BAR * 4)]);
    let block = clip_rect(&t.view, l.grid, &clips[0]);

    t.press(
        MouseButton::Left,
        block.x + 20.0,
        block.y + 4.0,
        &l,
        &clips,
        4,
    );
    let moved = t.drag(
        block.x + 20.0,
        block.y + t.view.lane_height * 2.0 + 4.0,
        &l,
        &clips,
        4,
    );
    let [ArrangeEdit::Move { lane_delta, .. }] = &moved[..] else {
        panic!("expected one move, got {moved:?}");
    };
    assert_eq!(*lane_delta, 2);
}

#[test]
fn dragging_a_clips_right_edge_resizes_it() {
    let mut t = timeline();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, 0, BAR * 4)]);
    let block = clip_rect(&t.view, l.grid, &clips[0]);
    let y = block.y + 4.0;

    t.press(MouseButton::Left, block.right() - 2.0, y, &l, &clips, 4);
    let one_bar = BAR as f32 * t.view.pixels_per_tick;
    let edits = t.drag(block.right() - 2.0 + one_bar, y, &l, &clips, 4);
    let [ArrangeEdit::Resize { ids, tick_delta }] = &edits[..] else {
        panic!("expected one resize, got {edits:?}");
    };
    assert_eq!(ids, &vec![clips[0].id]);
    assert_eq!(*tick_delta, BAR);
}

#[test]
fn right_clicking_a_clip_deletes_it() {
    let mut t = timeline();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, 0, BAR * 4)]);
    let block = clip_rect(&t.view, l.grid, &clips[0]);

    let edits = t.press(
        MouseButton::Right,
        block.x + 20.0,
        block.y + 4.0,
        &l,
        &clips,
        4,
    );
    assert_eq!(edits, vec![ArrangeEdit::Remove(vec![clips[0].id])]);
}

#[test]
fn the_selection_can_be_duplicated_muted_and_deleted() {
    let mut t = timeline();
    let clips = clips(&[(0, 0, BAR * 4), (1, BAR, BAR)]);
    t.select(vec![clips[0].id, clips[1].id]);

    // Duplicate lands after the end of what was selected, rounded to a bar, so
    // a phrase copied twice is twice as long rather than an overlap.
    let [ArrangeEdit::Duplicate { ids, tick_offset }] = &t.duplicate(&clips, 4)[..] else {
        panic!("expected a duplicate");
    };
    assert_eq!(ids.len(), 2);
    assert_eq!(*tick_offset, BAR * 4);

    let [ArrangeEdit::SetMuted { muted, .. }] = &t.toggle_mute(&clips)[..] else {
        panic!("expected a mute");
    };
    assert!(
        *muted,
        "none of them was muted, so all of them become muted"
    );

    assert_eq!(
        t.delete_selection(),
        vec![ArrangeEdit::Remove(vec![clips[0].id, clips[1].id])]
    );
    assert!(
        t.delete_selection().is_empty(),
        "deleting nothing is not an edit"
    );
}

#[test]
fn a_marquee_over_the_arrangement_selects_what_it_covers() {
    let mut t = timeline();
    // The arrangement starts in **draw** now — a press on empty grid makes a
    // clip, which is the thing it could not do before (see
    // `tests/arrange_draw.rs`). The marquee is the select tool's, and Ctrl's.
    t.set_tool(fontelle_ui::canvas::TimelineTool::Select);
    let l = timeline_layout(Rect::new(0.0, 0.0, 1020.0, 224.0), &metrics());
    let clips = clips(&[(0, 0, BAR), (1, 0, BAR), (2, BAR * 20, BAR)]);

    // Press on empty grid past the end of everything, then drag back over the
    // first two lanes.
    let from = clip_rect(&t.view, l.grid, &clips[0]);
    t.press(
        MouseButton::Left,
        l.grid.right() - 4.0,
        l.grid.bottom() - 4.0,
        &l,
        &clips,
        4,
    );
    t.drag(from.x + 1.0, from.y + 1.0, &l, &clips, 4);
    assert!(t.marquee().is_some(), "there is a box to draw");
    t.release_over(from.x + 1.0, from.y + 1.0, &l, &clips, 4);

    let selected = t.selection();
    assert_eq!(selected.len(), 2, "got {selected:?}");
    assert!(selected.contains(&clips[0].id));
    assert!(selected.contains(&clips[1].id));
}

#[test]
fn zooming_the_arrangement_holds_the_bar_under_the_pointer() {
    let mut t = timeline();
    let g = grid();
    let anchor = g.x + 400.0;
    let before = fontelle_ui::canvas::timeline_x_to_tick(&t.view, g, anchor);
    fontelle_ui::canvas::timeline_zoom_x(&mut t.view, g, anchor, 2.0);
    let after = fontelle_ui::canvas::timeline_x_to_tick(&t.view, g, anchor);
    assert!(
        (before - after).abs() < BAR / 8,
        "the bar under the pointer moved from {before} to {after}"
    );
}

// ------------------------------------ automation blocks show their curves ---
//
// Reported from using the window: *"automation clips aren't drawn on the
// arrangement — they play and open, but a lane of them looks like a lane of
// empty clips."*
//
// The block's anatomy, its handles and its gestures are `automation_blocks.rs`
// now that the block is where the clip is edited; what stays here is the one
// property the arrangement's drawing has to keep whatever else changes.

fn curve_points(specs: &[(Tick, f64)]) -> Vec<fontelle_ui::document::CurvePoint> {
    let mut arena: Arena<fontelle_types::PointId, ()> = Arena::default();
    specs
        .iter()
        .map(|(tick, value)| fontelle_ui::document::CurvePoint {
            id: arena.insert(()),
            tick: *tick,
            value: *value,
            curve: fontelle_model::CurveShape::Linear,
        })
        .collect()
}

#[test]
fn a_curve_fills_the_block_it_belongs_to() {
    use fontelle_ui::canvas::automation_polyline;

    let block = Rect::new(100.0, 40.0, 200.0, 30.0);
    let points = automation_polyline(
        block,
        PPQN * 4,
        &curve_points(&[(0, 0.0), (PPQN * 2, 1.0), (PPQN * 4, 0.5)]),
    );

    assert!(points.len() >= 3);
    for (x, y) in &points {
        assert!(
            (block.x..=block.right()).contains(x) && (block.y..=block.bottom()).contains(y),
            "({x}, {y}) is outside the block {block:?}"
        );
    }
    // Time runs left to right.
    assert!(points.windows(2).all(|w| w[0].0 <= w[1].0));
    // And **one is up**: an automation curve drawn upside down is a lie about
    // the value, and the mistake is invisible until you compare it with the
    // editor. The peak is in the middle and the ends are lower.
    let (first, last) = (points[0], points[points.len() - 1]);
    let top = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    assert!(top < first.1, "value 1.0 must be above value 0.0: {points:?}");
    assert!(last.1 < first.1 && last.1 > top, "0.5 is between");
}

#[test]
fn the_ends_of_a_curve_are_inside_the_block_rather_than_on_its_edge() {
    // A point at 0.0 or 1.0 is drawn as a dot, and a dot centred on the block's
    // own edge is half of a dot.
    use fontelle_ui::canvas::automation_polyline;

    let block = Rect::new(0.0, 0.0, 120.0, 24.0);
    let points = automation_polyline(block, PPQN, &curve_points(&[(0, 0.0), (PPQN, 1.0)]));
    let (first, last) = (points[0], points[points.len() - 1]);
    assert!(first.1 < block.bottom(), "the bottom point touches the edge");
    assert!(last.1 > block.y, "the top point touches the edge");
}

#[test]
fn a_value_outside_the_normal_range_is_clamped_into_the_block() {
    // Nothing should produce one, and a curve that escaped its own block would
    // be drawn over the lane above it.
    use fontelle_ui::canvas::automation_polyline;

    let block = Rect::new(10.0, 10.0, 100.0, 20.0);
    let points =
        automation_polyline(block, PPQN, &curve_points(&[(-PPQN, -3.0), (PPQN * 9, 4.0)]));
    for (x, y) in &points {
        assert!((block.x..=block.right()).contains(x), "x {x} escaped");
        assert!((block.y..=block.bottom()).contains(y), "y {y} escaped");
    }
}

// -------------------------------------------------------- the cut tool ---
//
// *"theres no tool for cutting up clips in the arrangement right now (should be
// c key) should work like the same tool in fl studio."* The model half of it is
// `fontelle-model/tests/arranging.rs` — what a cut *does* to a clip, looped
// clips included. This is where the line somebody draws turns into a list of
// cuts, and it follows the roll's rule exactly: a clip is cut where the stroke
// crosses the middle of its own row.

/// Where the middle of lane `lane` is on screen.
fn row_middle(v: &TimelineView, g: Rect, lane: usize) -> f32 {
    lane_to_y(v, g, lane) + v.lane_height / 2.0
}

#[test]
fn a_stroke_across_a_clip_cuts_it_where_it_crossed() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 4)]);
    // Down through the middle of row 0, two bars in.
    let x = fontelle_ui::canvas::timeline_tick_to_x(&v, g, BAR * 2);
    let cuts = fontelle_ui::canvas::clip_cuts(
        &v,
        g,
        &clips,
        (x, row_middle(&v, g, 0) - 20.0),
        (x, row_middle(&v, g, 0) + 20.0),
        SnapDivision::Bar,
        4,
    );
    assert_eq!(cuts.len(), 1, "one clip crossed");
    assert_eq!(cuts[0], (clips[0].id, BAR * 2), "cut where the line crossed");
}

/// A diagonal stroke cuts each row where it crosses *that* row — which is the
/// gesture, not an artefact.
#[test]
fn a_diagonal_stroke_cuts_each_row_at_its_own_bar() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 8), (1, 0, BAR * 8)]);
    let from = (
        fontelle_ui::canvas::timeline_tick_to_x(&v, g, BAR),
        row_middle(&v, g, 0),
    );
    let to = (
        fontelle_ui::canvas::timeline_tick_to_x(&v, g, BAR * 5),
        row_middle(&v, g, 1),
    );
    let cuts = fontelle_ui::canvas::clip_cuts(&v, g, &clips, from, to, SnapDivision::Bar, 4);
    assert_eq!(cuts.len(), 2);
    assert_eq!(cuts[0].1, BAR, "the top row where the stroke started");
    assert_eq!(cuts[1].1, BAR * 5, "and the one below where it ended");
}

/// A line drawn *along* a row never crosses its middle, so it cuts nothing —
/// rather than cutting somewhere nobody aimed at.
#[test]
fn a_horizontal_stroke_cuts_nothing() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 4)]);
    let y = row_middle(&v, g, 0);
    let cuts = fontelle_ui::canvas::clip_cuts(
        &v,
        g,
        &clips,
        (g.x + 10.0, y),
        (g.x + 300.0, y),
        SnapDivision::Bar,
        4,
    );
    assert!(cuts.is_empty());
}

/// A press with no drag is somebody putting the pointer down.
#[test]
fn a_press_without_a_drag_cuts_nothing() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 4)]);
    let at = (
        fontelle_ui::canvas::timeline_tick_to_x(&v, g, BAR),
        row_middle(&v, g, 0),
    );
    assert!(
        fontelle_ui::canvas::clip_cuts(&v, g, &clips, at, at, SnapDivision::Bar, 4).is_empty()
    );
}

/// A cut aimed at a bar the clip does not cover is refused rather than
/// clamped: a clip of nothing is one you can neither see nor grab.
#[test]
fn a_stroke_past_a_clips_end_cuts_nothing() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 2)]);
    let x = fontelle_ui::canvas::timeline_tick_to_x(&v, g, BAR * 6);
    let cuts = fontelle_ui::canvas::clip_cuts(
        &v,
        g,
        &clips,
        (x, row_middle(&v, g, 0) - 20.0),
        (x, row_middle(&v, g, 0) + 20.0),
        SnapDivision::Bar,
        4,
    );
    assert!(cuts.is_empty());
}

/// The cut snaps, unlike the roll's: a clip boundary half a beat off the bar is
/// a boundary somebody has to nudge before they can use it.
#[test]
fn a_cut_lands_on_the_grid() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 4)]);
    // A few pixels past the second bar line.
    let x = fontelle_ui::canvas::timeline_tick_to_x(&v, g, BAR * 2) + 5.0;
    let cuts = fontelle_ui::canvas::clip_cuts(
        &v,
        g,
        &clips,
        (x, row_middle(&v, g, 0) - 20.0),
        (x, row_middle(&v, g, 0) + 20.0),
        SnapDivision::Bar,
        4,
    );
    assert_eq!(cuts[0].1, BAR * 2, "snapped back to the bar it was aimed at");
}

/// The tool takes the press whatever is under it, and the cut lands on
/// **release** — a line half-drawn is not a cut.
#[test]
fn the_cut_tool_emits_its_edit_when_the_button_comes_up() {
    let m = metrics();
    let l = timeline_layout(Rect::new(0.0, 0.0, 1000.0, 260.0), &m);
    let mut t = Timeline::new(view());
    t.set_tool(fontelle_ui::canvas::TimelineTool::Slice);
    let clips = clips(&[(0, 0, BAR * 4)]);
    let x = fontelle_ui::canvas::timeline_tick_to_x(&t.view, l.grid, BAR * 2);
    let y = row_middle(&t.view, l.grid, 0);

    let edits = t.press(MouseButton::Left, x, y - 20.0, &l, &clips, 4);
    assert!(edits.is_empty(), "nothing happens on the way down");
    t.drag(x, y + 20.0, &l, &clips, 4);
    assert!(t.slice_line().is_some(), "the stroke is visible while drawn");

    let edits = t.release_over(x, y + 20.0, &l, &clips, 4);
    match edits.as_slice() {
        [ArrangeEdit::Split { cuts }] => {
            assert_eq!(cuts, &vec![(clips[0].id, BAR * 2)]);
        }
        other => panic!("expected one cut, got {other:?}"),
    }
    assert!(t.slice_line().is_none(), "and the stroke is gone after");
}
