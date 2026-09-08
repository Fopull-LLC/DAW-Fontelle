//! Automation clips are edited **in the arrangement**, inside their own
//! blocks (TDD §12, §16.4).
//!
//! Reported from using the window: *"right now when i right click a knob and
//! click create automation clip it opens the automation clip as a new
//! separate window... i want the automation graph to be a literal graph drawn
//! inside the clip for that automation working like how fl studio automation
//! clips work in the arrangement."*
//!
//! So an automation block has an anatomy: a caption band across the top that
//! moves and selects the clip like any other block, and under it the curve,
//! where a click makes a point, a drag moves one and a right-click on one
//! offers its shape. The geometry and the gestures are here, pure; the
//! window turns the edits into commands.

use fontelle_model::{Arena, CurveShape};
use fontelle_types::{ClipId, PPQN, PointId, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, AutomationBlock, ClipPart, Modifiers, MouseButton, SnapDivision, Timeline,
    TimelineHit, TimelineView, automation_block, automation_polyline, clip_rect, timeline_hit,
    timeline_layout,
};
use fontelle_ui::document::{ClipInfo, ClipKind, CurvePoint};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn body() -> Rect {
    Rect::new(0.0, 0.0, 900.0, 300.0)
}

fn view() -> TimelineView {
    TimelineView {
        // A bar is 192 pixels: room to aim at a point.
        pixels_per_tick: 0.05,
        lane_height: 40.0,
        snap: SnapDivision::Beat,
        ..TimelineView::default()
    }
}

fn clip_id() -> ClipId {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    arena.insert(())
}

fn point_ids(n: usize) -> Vec<PointId> {
    let mut arena: Arena<PointId, ()> = Arena::default();
    (0..n).map(|_| arena.insert(())).collect()
}

/// A four-bar automation clip at bar 1 with a curve through `values`, spaced
/// evenly across it.
fn automation_clip(values: &[f64]) -> ClipInfo {
    let ids = point_ids(values.len());
    let last = values.len().saturating_sub(1).max(1) as Tick;
    ClipInfo {
        id: clip_id(),
        lane: 0,
        start: 0,
        length: BAR * 4,
        name: "Master \u{2014} gain".to_string(),
        muted: false,
        open: false,
        color: [0xb4, 0xa2, 0xe8, 0xff],
        loop_length: None,
        kind: ClipKind::Automation,
        curve: values
            .iter()
            .enumerate()
            .map(|(i, value)| CurvePoint {
                id: ids[i],
                tick: BAR * 4 * i as Tick / last,
                value: *value,
                curve: CurveShape::Linear,
            })
            .collect(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

fn note_clip() -> ClipInfo {
    ClipInfo {
        id: clip_id(),
        lane: 0,
        start: 0,
        length: BAR * 4,
        name: "Piano".to_string(),
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

// ----------------------------------------------------------- the anatomy ---

#[test]
fn an_automation_block_is_a_caption_band_over_a_curve_area() {
    let clip = automation_clip(&[0.5, 0.5]);
    let block = Rect::new(100.0, 40.0, 400.0, 40.0);
    let AutomationBlock { header, area, handles } = automation_block(block, &clip);

    assert!(!header.is_empty() && !area.is_empty());
    assert!(header.bottom() <= area.y + 0.001, "the band is above the curve");
    assert!(!header.intersects(&area));
    assert_eq!(header.intersection(&block), header, "inside the block");
    assert_eq!(area.intersection(&block), area);
    assert!(
        area.height > header.height,
        "the curve gets most of the room: {area:?} under {header:?}"
    );
    assert_eq!(handles.len(), 2, "one handle per point");
}

#[test]
fn a_handle_sits_on_its_point_and_one_is_up() {
    let clip = automation_clip(&[0.0, 1.0]);
    let block = Rect::new(100.0, 40.0, 400.0, 40.0);
    let AutomationBlock { area, handles, .. } = automation_block(block, &clip);
    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (x0, y0) = centre(handles[0].1);
    let (x1, y1) = centre(handles[1].1);
    assert!((x0 - area.x).abs() < 0.5, "the first point is at the start");
    assert!((x1 - area.right()).abs() < 0.5, "the last at the end");
    assert!(y1 < y0, "one is up, zero is down");
    assert!(y0 <= area.bottom() + 0.5 && y1 >= area.y - 0.5);
    assert_eq!(handles[0].0, clip.curve[0].id);
}

#[test]
fn a_block_too_short_for_a_band_still_has_a_curve() {
    // Zoomed out to a fourteen-pixel lane the band collapses before the
    // curve does: the shape is the content, the caption is a courtesy.
    let clip = automation_clip(&[0.5, 0.5]);
    let block = Rect::new(0.0, 0.0, 200.0, 14.0);
    let AutomationBlock { header, area, .. } = automation_block(block, &clip);
    assert!(!area.is_empty());
    assert!(header.height < area.height);
    for r in [header, area] {
        assert!(r.width >= 0.0 && r.height >= 0.0, "{r:?}");
    }
}

// -------------------------------------------------------------- the line ---

#[test]
fn the_curve_is_drawn_through_its_points() {
    let clip = automation_clip(&[0.0, 1.0, 0.5]);
    let block = Rect::new(100.0, 40.0, 400.0, 40.0);
    let line = automation_polyline(block, clip.length, &clip.curve);
    let AutomationBlock { area, handles, .. } = automation_block(block, &clip);
    assert!(line.len() >= 3);
    for (_, rect) in &handles {
        let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let nearest = line
            .iter()
            .map(|(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt())
            .fold(f32::MAX, f32::min);
        assert!(nearest < 1.5, "the line misses the handle at ({cx}, {cy}) by {nearest}");
    }
    for (x, y) in &line {
        assert!(area.x - 0.5 <= *x && *x <= area.right() + 0.5, "x {x} left the area");
        assert!(area.y - 0.5 <= *y && *y <= area.bottom() + 0.5, "y {y} left the area");
    }
}

#[test]
fn a_bent_segment_is_drawn_bent() {
    // An S-curve between two points is not a straight line: the picture in
    // the block has to be the curve the audio thread hears, or the shape you
    // chose is a shape you cannot see.
    let mut clip = automation_clip(&[0.0, 1.0]);
    clip.curve[0].curve = CurveShape::SCurve;
    let block = Rect::new(0.0, 0.0, 400.0, 40.0);
    let bent = automation_polyline(block, clip.length, &clip.curve);
    clip.curve[0].curve = CurveShape::Linear;
    let straight = automation_polyline(block, clip.length, &clip.curve);
    let quarter = |line: &[(f32, f32)]| line[line.len() / 4].1;
    assert!(
        (quarter(&bent) - quarter(&straight)).abs() > 1.0,
        "the S-curve draws the same as a line"
    );
}

#[test]
fn a_polyline_never_divides_by_a_missing_length() {
    let mut clip = automation_clip(&[0.5]);
    clip.length = 0;
    for line in [
        automation_polyline(Rect::new(0.0, 0.0, 100.0, 20.0), 0, &clip.curve),
        automation_polyline(Rect::ZERO, PPQN, &clip.curve),
        automation_polyline(Rect::new(0.0, 0.0, 100.0, 20.0), PPQN, &[]),
    ] {
        for (x, y) in &line {
            assert!(x.is_finite() && y.is_finite());
        }
    }
}

// ---------------------------------------------------------- hit-testing ---

/// A fresh arrangement canvas and its geometry. The clips are passed to each
/// call rather than held here, the way the canvas itself takes them.
fn rig() -> (Timeline, fontelle_ui::canvas::TimelineLayout) {
    let m = Theme::dark_default().metrics;
    (Timeline::new(view()), timeline_layout(body(), &m))
}

#[test]
fn the_band_is_the_body_and_the_curve_is_the_curve() {
    let clip = automation_clip(&[0.5, 0.5]);
    let (t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { header, area, .. } = automation_block(block, &clip);

    let on_band = timeline_hit(&t.view, &l, std::slice::from_ref(&clip), header.x + 40.0, header.y + 2.0);
    assert_eq!(on_band, TimelineHit::Clip(clip.id, ClipPart::Body));

    // Half way across, somewhere the flat curve is not.
    let hit = timeline_hit(&t.view, &l, std::slice::from_ref(&clip), area.x + area.width / 2.0, area.y + 2.0);
    match hit {
        TimelineHit::Clip(id, ClipPart::Curve { tick, value }) => {
            assert_eq!(id, clip.id);
            assert!((tick - BAR * 2).abs() < BAR / 8, "half way across is bar 3: {tick}");
            assert!(value > 0.8, "near the top of the area: {value}");
        }
        other => panic!("expected the curve, got {other:?}"),
    }
}

#[test]
fn a_point_is_found_before_the_curve_under_it() {
    let clip = automation_clip(&[0.5, 0.5]);
    let (t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { handles, .. } = automation_block(block, &clip);
    let (id, rect) = handles[0];
    let hit = timeline_hit(&t.view, &l, std::slice::from_ref(&clip), rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    assert_eq!(hit, TimelineHit::Clip(clip.id, ClipPart::Point(id)));
}

#[test]
fn the_right_edge_grip_still_wins_on_an_automation_block() {
    let clip = automation_clip(&[0.5, 0.5]);
    let (t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let hit = timeline_hit(&t.view, &l, std::slice::from_ref(&clip), block.right() - 2.0, block.y + block.height / 2.0);
    assert_eq!(hit, TimelineHit::Clip(clip.id, ClipPart::RightEdge));
}

#[test]
fn a_note_block_has_no_curve_to_hit() {
    let clip = note_clip();
    let (t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let hit = timeline_hit(&t.view, &l, std::slice::from_ref(&clip), block.x + 100.0, block.bottom() - 3.0);
    assert_eq!(hit, TimelineHit::Clip(clip.id, ClipPart::Body));
}

// ------------------------------------------------------------ gestures ---

#[test]
fn clicking_the_curve_makes_a_point_there_on_the_grid() {
    let clip = automation_clip(&[0.5, 0.5]);
    let (mut t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { area, .. } = automation_block(block, &clip);

    // A little past bar 2, near the top.
    let x = area.x + (BAR as f32 + 40.0) * t.view.pixels_per_tick;
    let edits = t.press(MouseButton::Left, x, area.y + 1.0, &l, std::slice::from_ref(&clip), 4);
    assert_eq!(edits.len(), 1);
    match &edits[0] {
        ArrangeEdit::AddPoint { clip: id, tick, value } => {
            assert_eq!(*id, clip.id);
            assert_eq!(*tick, BAR, "snapped to the beat grid: {tick}");
            assert!(*value > 0.9, "near the top: {value}");
        }
        other => panic!("expected a point, got {other:?}"),
    }
}

#[test]
fn a_new_point_is_picked_up_by_the_press_that_made_it() {
    // The same handshake drawing a note has: the id comes back from the host,
    // and the drag that follows moves it.
    let clip = automation_clip(&[0.5, 0.5]);
    let (mut t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { area, .. } = automation_block(block, &clip);
    let x = area.x + BAR as f32 * t.view.pixels_per_tick;
    let y = area.y + area.height / 2.0;
    t.press(MouseButton::Left, x, y, &l, std::slice::from_ref(&clip), 4);
    let made = point_ids(1)[0];
    t.points_inserted(clip.id, vec![made]);
    assert_eq!(t.point_selection(), &[made]);

    // Down by half the area, one beat right.
    let edits = t.drag(x + PPQN as f32 * t.view.pixels_per_tick, y + area.height / 2.0, &l, std::slice::from_ref(&clip), 4);
    assert_eq!(edits.len(), 1);
    match &edits[0] {
        ArrangeEdit::MovePoints { clip: id, ids, tick_delta, value_delta } => {
            assert_eq!(*id, clip.id);
            assert_eq!(ids, &[made]);
            assert_eq!(*tick_delta, PPQN);
            assert!((*value_delta + 0.5).abs() < 0.05, "half way down: {value_delta}");
        }
        other => panic!("expected a move, got {other:?}"),
    }
}

#[test]
fn dragging_a_point_emits_deltas_relative_to_the_last_step() {
    let clip = automation_clip(&[0.5, 0.5]);
    let (mut t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { area, handles, .. } = automation_block(block, &clip);
    let (id, rect) = handles[0];
    let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let edits = t.press(MouseButton::Left, x, y, &l, std::slice::from_ref(&clip), 4);
    assert!(edits.is_empty(), "grabbing a point changes nothing yet");
    assert_eq!(t.point_selection(), &[id]);

    let beat = PPQN as f32 * t.view.pixels_per_tick;
    let first = t.drag(x + beat, y, &l, std::slice::from_ref(&clip), 4);
    let second = t.drag(x + beat * 2.0, y, &l, std::slice::from_ref(&clip), 4);
    let held = t.drag(x + beat * 2.0 + 1.0, y, &l, std::slice::from_ref(&clip), 4);
    for edits in [&first, &second] {
        assert!(matches!(
            edits.as_slice(),
            [ArrangeEdit::MovePoints { tick_delta, .. }] if *tick_delta == PPQN
        ), "{edits:?}");
    }
    assert!(held.is_empty(), "a sub-grid move is not an edit: {held:?}");
    let _ = area;
}

#[test]
fn a_stationary_pointer_asks_for_nothing() {
    // The trap this codebase keeps falling into — see `MoveLimits`.
    let clip = automation_clip(&[0.5, 0.5]);
    let (mut t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { handles, .. } = automation_block(block, &clip);
    let (_, rect) = handles[1];
    let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    t.press(MouseButton::Left, x, y, &l, std::slice::from_ref(&clip), 4);
    for _ in 0..5 {
        assert!(t.drag(x, y, &l, std::slice::from_ref(&clip), 4).is_empty());
    }
}

#[test]
fn a_right_click_on_a_point_asks_for_its_menu_rather_than_erasing_the_clip() {
    let clip = automation_clip(&[0.5, 0.5]);
    let (mut t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { handles, area, .. } = automation_block(block, &clip);
    let (id, rect) = handles[1];
    let edits = t.press(MouseButton::Right, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0, &l, std::slice::from_ref(&clip), 4);
    assert!(edits.is_empty(), "a right-click on a point is a question, not a deletion: {edits:?}");
    assert_eq!(t.take_point_menu(), Some((clip.id, id)));
    assert_eq!(t.take_point_menu(), None, "taken once");

    // And on the bare curve, nothing at all: the eraser must not take the
    // whole clip because the pointer was two pixels off a point.
    let edits = t.press(MouseButton::Right, area.x + area.width / 2.0, area.y + 2.0, &l, std::slice::from_ref(&clip), 4);
    assert!(edits.is_empty(), "{edits:?}");
    assert_eq!(t.take_point_menu(), None);

    // The band is still the block, and the block still erases.
    let edits = t.press(MouseButton::Right, block.x + 40.0, block.y + 2.0, &l, std::slice::from_ref(&clip), 4);
    assert_eq!(edits, vec![ArrangeEdit::Remove(vec![clip.id])]);
}

#[test]
fn deleting_and_reshaping_the_selected_points_are_edits_on_their_clip() {
    let clip = automation_clip(&[0.5, 0.5, 0.5]);
    let (mut t, l) = rig();
    let block = clip_rect(&t.view, l.grid, &clip);
    let AutomationBlock { handles, .. } = automation_block(block, &clip);
    let (id, rect) = handles[1];
    t.press(MouseButton::Left, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0, &l, std::slice::from_ref(&clip), 4);
    t.release();

    assert_eq!(
        t.set_point_curve(CurveShape::SCurve),
        vec![ArrangeEdit::SetPointCurve { clip: clip.id, ids: vec![id], curve: CurveShape::SCurve }]
    );
    assert_eq!(
        t.delete_points(),
        vec![ArrangeEdit::RemovePoints { clip: clip.id, ids: vec![id] }]
    );
    assert!(t.point_selection().is_empty(), "deleted points are not selected");
    assert!(t.delete_points().is_empty(), "nothing selected, nothing to remove");
}

#[test]
fn selecting_a_clip_drops_the_point_selection_from_another() {
    let a = automation_clip(&[0.5, 0.5]);
    let mut b = automation_clip(&[0.2, 0.2]);
    b.lane = 1;
    let clips = vec![a.clone(), b.clone()];
    let (mut t, l) = rig();
    let block_a = clip_rect(&t.view, l.grid, &a);
    let AutomationBlock { handles, .. } = automation_block(block_a, &a);
    let (_, rect) = handles[0];
    t.press(MouseButton::Left, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0, &l, &clips, 4);
    t.release();
    assert_eq!(t.point_selection().len(), 1);

    let block_b = clip_rect(&t.view, l.grid, &b);
    t.press(MouseButton::Left, block_b.x + 30.0, block_b.y + 2.0, &l, &clips, 4);
    t.release();
    assert!(t.point_selection().is_empty());
    assert_eq!(t.selection(), &[b.id]);
    let _ = Modifiers::default();
}
