//! Taking hold of a clip's edge, and placing it exactly with the snap off.
//!
//! > *"im finding it kind of hard to grab the edges of clips to size them.
//! > please make sure the interactions work seamlessly without frustration.
//! > also right now when i have snapping set to none, its really hard to snap
//! > it exactly to an edge ... in fl studio, this is solved by just zooming in
//! > super far and you can see the tiny individual snaps of moving with none
//! > snapping but in our daw no matter how far you zoom in it looks extremely
//! > smooth when moving with snap set to none."*
//!
//! Three causes, one per section below. The grip was only the seven pixels
//! *inside* the block (a third of a narrow one), so a pointer a hair past the
//! edge got empty grid. The deepest zoom was half a pixel per tick, so a step
//! of the snap-off drag was never as wide as a pixel, and the grid drew bar
//! lines only, so there was nothing to read the travel against. And nothing
//! helped an edge land *on* another edge except a steady hand.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, ClipPart, EDGE_REACH_PX, MAGNET_PX, MAX_TIMELINE_PPT, Modifiers, MouseButton,
    SnapDivision, Timeline, TimelineHit, TimelineLine, TimelineTool, TimelineView, clip_rect,
    lane_to_y, timeline_beat_labels, timeline_grab, timeline_grid_units, timeline_hit,
    timeline_layout, timeline_tick_to_x, timeline_zoom_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 900.0, 300.0),
        &Theme::dark_default().metrics,
    )
}

fn clips(items: &[(Tick, Tick, usize, ClipKind)]) -> Vec<ClipInfo> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    items
        .iter()
        .map(|(start, length, lane, kind)| ClipInfo {
            id: arena.insert(()),
            lane: *lane,
            start: *start,
            length: *length,
            name: "Part".to_string(),
            muted: false,
            open: false,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            loop_length: None,
            kind: *kind,
            curve: Vec::new(),
            notes: Vec::new(),
            audio: Default::default(),
            prefab: None,
        })
        .collect()
}

fn free() -> TimelineView {
    TimelineView {
        snap: SnapDivision::None,
        ..TimelineView::default()
    }
}

/// The middle of a lane's row, well clear of any caption or fade handle.
fn row_y(view: &TimelineView, l: &fontelle_ui::canvas::TimelineLayout, lane: usize) -> f32 {
    lane_to_y(view, l.grid, lane) + view.lane_height * 0.7
}

fn moved(edits: &[ArrangeEdit]) -> Tick {
    edits
        .iter()
        .map(|edit| match edit {
            ArrangeEdit::Move { tick_delta, .. } => *tick_delta,
            _ => 0,
        })
        .sum()
}

fn resized(edits: &[ArrangeEdit]) -> Tick {
    edits
        .iter()
        .map(|edit| match edit {
            ArrangeEdit::Resize { tick_delta, .. } => *tick_delta,
            _ => 0,
        })
        .sum()
}

fn trimmed(edits: &[ArrangeEdit]) -> Tick {
    edits
        .iter()
        .map(|edit| match edit {
            ArrangeEdit::TrimStart { tick_delta, .. } => *tick_delta,
            _ => 0,
        })
        .sum()
}

// --------------------------------------------------------- the grip ---

#[test]
fn a_pointer_just_past_the_right_edge_takes_hold_of_it() {
    let l = layout();
    let view = TimelineView::default();
    let clips = clips(&[(BAR, BAR, 0, ClipKind::Notes)]);
    let block = clip_rect(&view, l.grid, &clips[0]);
    let y = row_y(&view, &l, 0);

    assert_eq!(
        timeline_grab(&view, &l, &clips, block.right() + EDGE_REACH_PX - 1.0, y),
        TimelineHit::Clip(clips[0].id, ClipPart::RightEdge),
        "outside the block, within reach of its edge"
    );
    assert!(
        matches!(
            timeline_grab(&view, &l, &clips, block.right() + EDGE_REACH_PX + 3.0, y),
            TimelineHit::Empty { .. }
        ),
        "out of reach is empty grid"
    );
    // The eraser and the menus ask the plain question: a right-click a hair
    // past a clip must not rub it out.
    assert!(matches!(
        timeline_hit(&view, &l, &clips, block.right() + 2.0, y),
        TimelineHit::Empty { .. }
    ));
}

#[test]
fn a_clip_too_narrow_for_a_grip_can_still_be_sized_from_outside() {
    let l = layout();
    let view = TimelineView::default();
    // Six pixels wide at the default zoom.
    let clips = clips(&[(BAR, 240, 0, ClipKind::Notes)]);
    let block = clip_rect(&view, l.grid, &clips[0]);
    assert!(block.width < 8.0, "{block:?}");
    let y = row_y(&view, &l, 0);

    assert_eq!(
        timeline_grab(&view, &l, &clips, block.x + block.width / 2.0, y),
        TimelineHit::Clip(clips[0].id, ClipPart::Body),
        "its middle still moves it"
    );
    assert_eq!(
        timeline_grab(&view, &l, &clips, block.right() + 3.0, y),
        TimelineHit::Clip(clips[0].id, ClipPart::RightEdge)
    );
}

#[test]
fn an_audio_blocks_left_edge_reaches_out_too_and_the_nearer_edge_wins() {
    let l = layout();
    let view = TimelineView::default();
    // A note block, a gap of eight pixels (320 ticks), an audio block.
    let clips = clips(&[
        (BAR, BAR, 0, ClipKind::Notes),
        (BAR * 2 + 320, BAR, 0, ClipKind::Audio),
    ]);
    let a = clip_rect(&view, l.grid, &clips[0]);
    let b = clip_rect(&view, l.grid, &clips[1]);
    let y = row_y(&view, &l, 0);

    assert_eq!(
        timeline_grab(&view, &l, &clips, a.right() + 2.0, y),
        TimelineHit::Clip(clips[0].id, ClipPart::RightEdge)
    );
    assert_eq!(
        timeline_grab(&view, &l, &clips, b.x - 2.0, y),
        TimelineHit::Clip(clips[1].id, ClipPart::LeftEdge)
    );
}

#[test]
fn the_reach_is_the_blocks_own_row_only() {
    let l = layout();
    let view = TimelineView::default();
    let clips = clips(&[(BAR, BAR, 1, ClipKind::Notes)]);
    let block = clip_rect(&view, l.grid, &clips[0]);
    assert!(matches!(
        timeline_grab(&view, &l, &clips, block.right() + 2.0, row_y(&view, &l, 0)),
        TimelineHit::Empty { .. }
    ));
}

#[test]
fn pressing_just_past_the_edge_resizes_rather_than_drawing_a_clip() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    timeline.set_tool(TimelineTool::Draw);
    let clips = clips(&[(BAR, BAR, 0, ClipKind::Notes)]);
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    let (x, y) = (block.right() + 3.0, row_y(&timeline.view, &l, 0));

    let pressed = timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
    assert!(
        !pressed
            .iter()
            .any(|edit| matches!(edit, ArrangeEdit::Add { .. })),
        "{pressed:?}"
    );
    assert!(timeline.sizing_edge(), "the gesture in hand is the edge");
    let dragged = timeline.drag(
        x + BAR as f32 * timeline.view.pixels_per_tick,
        y,
        &l,
        &clips,
        4,
    );
    assert_eq!(resized(&dragged), BAR);

    // And the double-click that often follows is not a new clip either.
    timeline.release();
    let again = timeline.double_press(MouseButton::Left, x, y, &l, &clips, 4);
    assert!(
        !again
            .iter()
            .any(|edit| matches!(edit, ArrangeEdit::Add { .. })),
        "{again:?}"
    );
}

#[test]
fn a_right_click_just_past_the_edge_erases_nothing() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let clips = clips(&[(BAR, BAR, 0, ClipKind::Notes)]);
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    let edits = timeline.press(
        MouseButton::Right,
        block.right() + 3.0,
        row_y(&timeline.view, &l, 0),
        &l,
        &clips,
        4,
    );
    assert!(edits.is_empty(), "{edits:?}");
}

#[test]
fn the_body_is_a_move_and_says_so() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let clips = clips(&[(BAR, BAR, 0, ClipKind::Notes)]);
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    timeline.press(
        MouseButton::Left,
        block.x + block.width / 2.0,
        row_y(&timeline.view, &l, 0),
        &l,
        &clips,
        4,
    );
    assert!(!timeline.sizing_edge());
}

// ---------------------------------------------------- the deep zoom ---

#[test]
fn the_arrangement_zooms_in_until_a_tick_is_wider_than_a_pixel() {
    const { assert!(MAX_TIMELINE_PPT >= 8.0) };
    let l = layout();
    let mut view = TimelineView::default();
    for _ in 0..40 {
        timeline_zoom_x(&mut view, l.grid, l.grid.x + 100.0, 2.0);
    }
    assert_eq!(view.pixels_per_tick, MAX_TIMELINE_PPT);
}

#[test]
fn zoomed_all_the_way_in_the_snap_off_drag_moves_in_visible_steps() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView {
        pixels_per_tick: MAX_TIMELINE_PPT,
        scroll_tick: BAR - 20,
        ..free()
    });
    let clips = clips(&[(BAR, BAR, 0, ClipKind::Notes)]);
    let x = timeline_tick_to_x(&timeline.view, l.grid, BAR + 10) + 0.5;
    let y = row_y(&timeline.view, &l, 0);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);

    // Less than half a tick is no move at all...
    let step = MAX_TIMELINE_PPT;
    assert!(timeline.drag(x + step * 0.3, y, &l, &clips, 4).is_empty());
    // ...and one tick's width is exactly one tick.
    assert_eq!(moved(&timeline.drag(x + step, y, &l, &clips, 4)), 1);
}

#[test]
fn the_grid_shows_beats_normally_and_single_ticks_zoomed_all_the_way_in() {
    let normal = timeline_grid_units(&TimelineView::default(), 4);
    assert!(normal.contains(&(BAR, TimelineLine::Bar)), "{normal:?}");
    assert!(normal.contains(&(PPQN, TimelineLine::Beat)), "{normal:?}");
    assert!(
        !normal.iter().any(|(_, line)| *line == TimelineLine::Tick),
        "{normal:?}"
    );

    let deep = timeline_grid_units(
        &TimelineView {
            pixels_per_tick: MAX_TIMELINE_PPT,
            ..free()
        },
        4,
    );
    assert!(deep.contains(&(1, TimelineLine::Tick)), "{deep:?}");

    // Zoomed right out nothing finer than a bar is drawn: lines a pixel
    // apart are a grey smear, not a grid.
    let far = timeline_grid_units(
        &TimelineView {
            pixels_per_tick: 0.002,
            ..TimelineView::default()
        },
        4,
    );
    assert!(far.iter().all(|(unit, _)| *unit >= BAR), "{far:?}");
}

#[test]
fn the_ruler_counts_beats_once_a_bar_no_longer_fits() {
    let l = layout();
    assert!(
        timeline_beat_labels(&TimelineView::default(), l.grid, 4).is_empty(),
        "at the default zoom the bar numbers are enough"
    );

    let deep = TimelineView {
        pixels_per_tick: 0.2,
        scroll_tick: BAR * 2,
        ..TimelineView::default()
    };
    let labels = timeline_beat_labels(&deep, l.grid, 4);
    assert!(
        labels.contains(&(BAR * 2 + PPQN, "3.2".to_string())),
        "{labels:?}"
    );
    // A bar's own first beat is its bar number, drawn already.
    assert!(
        !labels.iter().any(|(tick, _)| tick % BAR == 0),
        "{labels:?}"
    );

    // All the way in, a beat is wider than the panel: the ruler still says
    // where it is, down to the tick within the beat.
    let deepest = TimelineView {
        pixels_per_tick: MAX_TIMELINE_PPT,
        scroll_tick: BAR * 4 + PPQN + 100,
        ..TimelineView::default()
    };
    let labels = timeline_beat_labels(&deepest, l.grid, 4);
    assert!(labels.len() >= 2, "{labels:?}");
    for (tick, name) in &labels {
        assert_eq!(
            *name,
            format!("5.2.{}", tick - BAR * 4 - PPQN),
            "{labels:?}"
        );
    }
}

// ------------------------------------------------------ the magnet ---

/// A block on the first lane and one to line it up against on the second.
fn lining_up() -> (Timeline, Vec<ClipInfo>, fontelle_ui::canvas::TimelineLayout) {
    let l = layout();
    let timeline = Timeline::new(free());
    let clips = clips(&[
        (0, BAR, 0, ClipKind::Notes),
        (BAR * 3, BAR, 1, ClipKind::Notes),
    ]);
    (timeline, clips, l)
}

#[test]
fn with_the_snap_off_a_moved_edge_lands_exactly_on_one_nearby() {
    let (mut timeline, clips, l) = lining_up();
    let ppt = timeline.view.pixels_per_tick;
    let x = timeline_tick_to_x(&timeline.view, l.grid, BAR / 2);
    let y = row_y(&timeline.view, &l, 0);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);

    // The end headed for the other block's start, three pixels short of it.
    let short = 3.0 / ppt;
    let edits = timeline.drag(x + (BAR * 2) as f32 * ppt - 3.0, y, &l, &clips, 4);
    assert_eq!(moved(&edits), BAR * 2, "{short} ticks short is on it");
    assert_eq!(timeline.magnet_tick(), Some(BAR * 3));

    // Well away from any edge the drag is free again.
    let free_edits = timeline.drag(x + (BAR * 2) as f32 * ppt - 40.0, y, &l, &clips, 4);
    let at = BAR * 2 + moved(&free_edits);
    assert!((at - (BAR * 2 - (40.0 / ppt) as Tick)).abs() <= 2, "{at}");
    assert_eq!(timeline.magnet_tick(), None);

    timeline.release();
    assert_eq!(timeline.magnet_tick(), None);
}

#[test]
fn alt_lets_an_edge_sit_right_beside_another() {
    let (mut timeline, clips, l) = lining_up();
    let ppt = timeline.view.pixels_per_tick;
    let x = timeline_tick_to_x(&timeline.view, l.grid, BAR / 2);
    let y = row_y(&timeline.view, &l, 0);
    timeline.set_modifiers(Modifiers {
        alt: true,
        ..Modifiers::default()
    });
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
    let edits = timeline.drag(x + (BAR * 2) as f32 * ppt - 3.0, y, &l, &clips, 4);
    let want = BAR * 2 - (3.0 / ppt) as Tick;
    assert!((moved(&edits) - want).abs() <= 2, "{edits:?}");
    assert_eq!(timeline.magnet_tick(), None);
}

#[test]
fn a_right_edge_dragged_near_another_block_meets_it() {
    let (mut timeline, clips, l) = lining_up();
    let ppt = timeline.view.pixels_per_tick;
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    let (x, y) = (block.right() - 2.0, row_y(&timeline.view, &l, 0));
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
    assert!(timeline.sizing_edge());
    let edits = timeline.drag(
        x + (BAR * 2) as f32 * ppt + (MAGNET_PX - 2.0),
        y,
        &l,
        &clips,
        4,
    );
    assert_eq!(resized(&edits), BAR * 2);
    assert_eq!(timeline.magnet_tick(), Some(BAR * 3));
}

#[test]
fn a_left_edge_trimmed_near_another_blocks_end_meets_it() {
    let l = layout();
    let mut timeline = Timeline::new(free());
    let clips = clips(&[
        (0, BAR, 1, ClipKind::Notes),
        (0, BAR * 4, 0, ClipKind::Audio),
    ]);
    let ppt = timeline.view.pixels_per_tick;
    let block = clip_rect(&timeline.view, l.grid, &clips[1]);
    let (x, y) = (block.x + 2.0, row_y(&timeline.view, &l, 0));
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
    assert!(timeline.sizing_edge());
    let edits = timeline.drag(x + BAR as f32 * ppt - 4.0, y, &l, &clips, 4);
    assert_eq!(trimmed(&edits), BAR);
    assert_eq!(timeline.magnet_tick(), Some(BAR));
}

#[test]
fn a_block_is_never_pulled_onto_its_own_edges() {
    // Alone on the arrangement: nothing to meet, whatever it passes.
    let l = layout();
    let mut timeline = Timeline::new(free());
    let clips = clips(&[(BAR, BAR, 0, ClipKind::Notes)]);
    let x = timeline_tick_to_x(&timeline.view, l.grid, BAR + BAR / 2);
    let y = row_y(&timeline.view, &l, 0);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
    timeline.drag(x + 3.0, y, &l, &clips, 4);
    assert_eq!(timeline.magnet_tick(), None);
}

#[test]
fn a_grid_snap_is_left_as_it_was() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let clips = clips(&[
        (0, BAR, 0, ClipKind::Notes),
        (BAR * 3 - 200, BAR, 1, ClipKind::Notes),
    ]);
    let ppt = timeline.view.pixels_per_tick;
    let x = timeline_tick_to_x(&timeline.view, l.grid, BAR / 2);
    let y = row_y(&timeline.view, &l, 0);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
    let edits = timeline.drag(x + (BAR * 2) as f32 * ppt - 5.0, y, &l, &clips, 4);
    assert_eq!(moved(&edits), BAR * 2);
    assert_eq!(timeline.magnet_tick(), None);
}
