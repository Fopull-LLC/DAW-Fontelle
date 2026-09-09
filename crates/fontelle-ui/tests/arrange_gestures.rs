//! The flicker, a third time — on the arrangement.
//!
//! `tests/roll_gestures.rs` has the full story: a drag emits **deltas relative
//! to the previous step of the same drag**, so any clamp it applies has to be
//! measured *once, when the gesture starts*. Measured from the live document —
//! which the drag is itself changing — the clamp chases the thing it is
//! clamping and whatever is being dragged oscillates at mouse-move rate.
//!
//! It was fixed in the piano roll and left standing here, because the two
//! canvases were written a fortnight apart with the same shape and the report
//! only mentioned notes. `Timeline::drag` recomputed `earliest`, `highest` and
//! the resize floor from the `clips` slice on every step, and the window
//! refreshes that slice from the document between events — so it is the same
//! bug with the same trigger, waiting for somebody to drag a clip to bar one.
//!
//! Nudging from the keyboard is here too, for the same reason it is in
//! `roll_keys.rs`: it is the same edit and the same clamps.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, MouseButton, SnapDivision, Timeline, TimelineView, timeline_layout,
    timeline_tick_to_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn frame() -> Rect {
    Rect::new(0.0, 0.0, 900.0, 300.0)
}

fn layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(frame(), &Theme::dark_default().metrics)
}

fn clips(items: &[(Tick, Tick, usize)]) -> Vec<ClipInfo> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    items
        .iter()
        .map(|(start, length, lane)| ClipInfo {
            id: arena.insert(()),
            lane: *lane,
            start: *start,
            length: *length,
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
        })
        .collect()
}

/// What the host does with what the canvas asks for — including handing the
/// changed clips back, which is the half that makes the bug visible.
fn apply(clips: &mut [ClipInfo], edits: &[ArrangeEdit]) {
    for edit in edits {
        match edit {
            ArrangeEdit::Move {
                ids,
                tick_delta,
                lane_delta,
            } => {
                for clip in clips.iter_mut().filter(|c| ids.contains(&c.id)) {
                    clip.start = (clip.start + tick_delta).max(0);
                    clip.lane = (clip.lane as i32 + lane_delta).max(0) as usize;
                }
            }
            ArrangeEdit::Resize { ids, tick_delta } => {
                for clip in clips.iter_mut().filter(|c| ids.contains(&c.id)) {
                    clip.length = (clip.length + tick_delta).max(1);
                }
            }
            _ => {}
        }
    }
}

fn at(
    view: &TimelineView,
    l: &fontelle_ui::canvas::TimelineLayout,
    tick: Tick,
    lane: usize,
) -> (f32, f32) {
    (
        timeline_tick_to_x(view, l.grid, tick) + 1.0,
        fontelle_ui::canvas::lane_to_y(view, l.grid, lane) + 1.0,
    )
}

// ------------------------------------------------------------- the drag ---

#[test]
fn a_clip_dragged_to_the_start_of_the_song_stays_there() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(PPQN * 16, PPQN * 16, 0)]);
    let id = clips[0].id;

    let (x, y) = at(&timeline.view, &l, PPQN * 16, 0);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);

    // Hard left, past bar one, and then held there.
    let far_left = l.grid.x - 300.0;
    let first = timeline.drag(far_left, y, &l, &clips, 4);
    apply(&mut clips, &first);
    assert_eq!(clips[0].start, 0);

    for step in 0..5 {
        let again = timeline.drag(far_left, y, &l, &clips, 4);
        assert!(
            again.is_empty(),
            "step {step}: a stationary pointer asked for {again:?}"
        );
        apply(&mut clips, &again);
    }
    assert_eq!(clips[0].start, 0);
    assert_eq!(clips[0].id, id);
}

#[test]
fn a_clip_dragged_to_the_first_lane_stays_on_it() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(0, PPQN * 4, 3)]);

    let (x, y) = at(&timeline.view, &l, 0, 3);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);

    let above = l.grid.y - 200.0;
    let first = timeline.drag(x, above, &l, &clips, 4);
    apply(&mut clips, &first);
    assert_eq!(clips[0].lane, 0);

    for step in 0..5 {
        let again = timeline.drag(x, above, &l, &clips, 4);
        assert!(again.is_empty(), "step {step} asked for {again:?}");
        apply(&mut clips, &again);
    }
}

#[test]
fn a_clip_shortened_to_its_minimum_stays_there() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(0, PPQN * 8, 0)]);

    let end = clips[0].start + clips[0].length;
    let (x, y) = (
        timeline_tick_to_x(&timeline.view, l.grid, end) - 2.0,
        fontelle_ui::canvas::lane_to_y(&timeline.view, l.grid, 0) + 1.0,
    );
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);

    let far_left = l.grid.x - 300.0;
    let first = timeline.drag(far_left, y, &l, &clips, 4);
    apply(&mut clips, &first);
    let settled = clips[0].length;

    for step in 0..5 {
        let again = timeline.drag(far_left, y, &l, &clips, 4);
        assert!(again.is_empty(), "step {step} asked for {again:?}");
        apply(&mut clips, &again);
    }
    assert_eq!(clips[0].length, settled);
    assert!(settled >= 1);
}

#[test]
fn a_clip_dragged_to_the_start_can_be_dragged_away_from_it_again() {
    // The clamp must not become a trap: this is what broke when the first fix
    // for the roll simply refused to move a note at bar one.
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(PPQN * 16, PPQN * 16, 0)]);

    let (x, y) = at(&timeline.view, &l, PPQN * 16, 0);
    timeline.press(MouseButton::Left, x, y, &l, &clips, 4);

    let edits = timeline.drag(l.grid.x - 300.0, y, &l, &clips, 4);
    apply(&mut clips, &edits);
    assert_eq!(clips[0].start, 0);

    let (bx, _) = at(&timeline.view, &l, PPQN * 32, 0);
    let edits = timeline.drag(bx, y, &l, &clips, 4);
    apply(&mut clips, &edits);
    assert!(
        clips[0].start > 0,
        "a clip parked at bar one has to be draggable away from it"
    );
}

// --------------------------------------------------------- the keyboard ---

#[test]
fn the_arrangement_takes_the_arrow_keys_too() {
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(PPQN * 4, PPQN * 4, 1)]);
    timeline.select(vec![clips[0].id]);

    let bar = PPQN * 4;
    let edits = timeline.nudge(&clips, bar, 0);
    apply(&mut clips, &edits);
    assert_eq!(clips[0].start, PPQN * 8);

    let edits = timeline.nudge(&clips, 0, -1);
    apply(&mut clips, &edits);
    assert_eq!(clips[0].lane, 0);
}

#[test]
fn the_arrangements_arrow_keys_are_clamped_like_its_drag() {
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(PPQN, PPQN * 4, 0), (PPQN * 16, PPQN * 4, 2)]);
    timeline.select(clips.iter().map(|c| c.id).collect());

    let edits = timeline.nudge(&clips, -PPQN * 4, -4);
    apply(&mut clips, &edits);
    assert_eq!(
        (clips[0].start, clips[1].start),
        (0, PPQN * 15),
        "the shape survives the clamp"
    );
    assert_eq!((clips[0].lane, clips[1].lane), (0, 2));

    assert!(timeline.nudge(&clips, -PPQN * 4, -4).is_empty());
}

#[test]
fn the_arrangements_length_keys_never_shorten_past_nothing() {
    let mut timeline = Timeline::new(TimelineView::default());
    let mut clips = clips(&[(0, PPQN, 0)]);
    timeline.select(vec![clips[0].id]);

    for _ in 0..20 {
        let edits = timeline.resize_selection(&clips, -PPQN);
        apply(&mut clips, &edits);
    }
    assert!(clips[0].length >= 1, "got {}", clips[0].length);
}

#[test]
fn nudging_nothing_is_not_an_undo_entry() {
    let clips = clips(&[(0, PPQN, 0)]);
    let mut timeline = Timeline::new(TimelineView::default());
    assert!(timeline.nudge(&clips, PPQN, 0).is_empty());
    assert!(timeline.resize_selection(&clips, PPQN).is_empty());
}

// -------------------------------------------------- the blade shows the cut

/// The view every blade test uses: a default one over the standard layout.
fn blade_view() -> TimelineView {
    TimelineView::default()
}

/// **The stroke you draw is not where the cut lands**, and the preview has to
/// show the second, not the first.
///
/// > *"it is displaying the actual visuals of the tool from the exact pixel of
/// > where I'm clicking and dragging my mouse instead of actually displaying
/// > it rounded to the grid that it's going to cut the actual clip at."*
///
/// `clip_cuts` snaps each crossing to the arrangement's grid, so a stroke
/// drawn a third of a beat late cuts on the beat — and the line drawn under
/// the pointer was showing the third of a beat. The marks are worked out with
/// the same call that will make the cut, so the two cannot disagree.
#[test]
fn the_blades_marks_are_where_the_cuts_will_land_not_where_the_mouse_is() {
    let items = clips(&[(0, PPQN * 8, 0)]);
    let l = layout();
    let view = blade_view();
    // A stroke straight down through the row, deliberately off the grid.
    let off_grid = timeline_tick_to_x(&view, l.grid, PPQN * 2 + PPQN / 3);
    let from = (off_grid, l.grid.y + 2.0);
    let to = (off_grid, fontelle_ui::canvas::lane_to_y(&view, l.grid, 1));

    let cuts =
        fontelle_ui::canvas::clip_cuts(&view, l.grid, &items, from, to, SnapDivision::Beat, 4);
    assert_eq!(cuts.len(), 1, "the stroke crosses one clip");
    let marks =
        fontelle_ui::canvas::slice_marks(&view, l.grid, &items, from, to, SnapDivision::Beat, 4);
    assert_eq!(marks.len(), 1, "one cut, one mark");
    let want = timeline_tick_to_x(&view, l.grid, cuts[0].1);
    let centre = marks[0].x + marks[0].width / 2.0;
    assert!(
        (centre - want).abs() < 1.5,
        "the mark is at {centre} and the cut lands at {want}"
    );
    assert!(
        (centre - off_grid).abs() > 2.0,
        "the mark is still following the pointer"
    );
}

/// A stroke that will cut nothing draws nothing: the marks and the cuts are
/// the same answer, so a stroke past the end of a clip has neither.
#[test]
fn a_stroke_that_cuts_nothing_marks_nothing() {
    let items = clips(&[(0, PPQN * 4, 0)]);
    let l = layout();
    let view = blade_view();
    let past = timeline_tick_to_x(&view, l.grid, PPQN * 12);
    let marks = fontelle_ui::canvas::slice_marks(
        &view,
        l.grid,
        &items,
        (past, l.grid.y + 2.0),
        (past, fontelle_ui::canvas::lane_to_y(&view, l.grid, 1)),
        SnapDivision::Beat,
        4,
    );
    assert!(marks.is_empty(), "a stroke past the clip marked something");
}

/// One mark per clip the stroke crosses, each on **its own** row — a diagonal
/// stroke across three lanes cuts three clips at three places, and one line
/// through all of them would be showing a cut nobody is making.
#[test]
fn a_diagonal_stroke_marks_every_row_it_crosses() {
    let items = clips(&[(0, PPQN * 8, 0), (0, PPQN * 8, 1), (0, PPQN * 8, 2)]);
    let l = layout();
    let view = blade_view();
    let from = (timeline_tick_to_x(&view, l.grid, PPQN), l.grid.y + 2.0);
    let to = (
        timeline_tick_to_x(&view, l.grid, PPQN * 6),
        fontelle_ui::canvas::lane_to_y(&view, l.grid, 3),
    );
    let marks =
        fontelle_ui::canvas::slice_marks(&view, l.grid, &items, from, to, SnapDivision::Beat, 4);
    assert_eq!(marks.len(), 3, "three rows crossed, three marks");
    for pair in marks.windows(2) {
        assert!(pair[1].y > pair[0].y, "two marks share a row");
        assert!(
            pair[1].x >= pair[0].x,
            "the stroke leans right; the marks do not"
        );
    }
}

// ----------------------------------------------- a trim is never lossy ----

/// An audio clip that is currently stretching.
fn a_stretched_clip(start: Tick, length: Tick) -> Vec<ClipInfo> {
    let mut items = clips(&[(start, length, 0)]);
    items[0].kind = ClipKind::Audio;
    items[0].audio.stretched = true;
    items[0].audio.natural_length = length;
    items[0].audio.peaks = vec![(-0.5, 0.5); 64];
    items
}

/// **A drag never turns stretching off behind your back.**
///
/// It used to: an edge drag sent `SetStretch { Off }` as its first step
/// whenever the toolbar switch was off, and turning a stretched clip off
/// *freezes* the rate it was being played at into the clip's own speed. A
/// clip stretched down to a quarter and then dragged became a clip genuinely
/// playing four times too fast, whose take really was a quarter as long — and
/// no drag could bring the rest back. That is what was behind both reports of
/// audio "going blank" after a shorten-and-grow.
///
/// Turning stretching off is a deliberate act with a switch for it, and doing
/// it deliberately still freezes the sound. A **drag** only ever turns
/// stretching *on*, which is what the switch being on asks for.
#[test]
fn an_edge_drag_never_turns_stretching_off_behind_your_back() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    timeline.set_stretch(false); // the ordinary state of the toolbar
    let items = a_stretched_clip(0, PPQN * 16);

    // Grab the right-hand edge and pull it out.
    let edge = timeline_tick_to_x(&timeline.view, l.grid, PPQN * 16) - 2.0;
    // Below the caption band, where the fade handles live — the edge grip is
    // in the content band under them.
    let y = fontelle_ui::canvas::lane_to_y(&timeline.view, l.grid, 0)
        + timeline.view.lane_height * 0.75;
    timeline.press(MouseButton::Left, edge, y, &l, &items, 4);
    let edits = timeline.drag(edge + 200.0, y, &l, &items, 4);

    assert!(
        !edits.iter().any(|edit| matches!(
            edit,
            ArrangeEdit::SetStretch {
                stretch: fontelle_types::ClipStretch::Off,
                ..
            }
        )),
        "the drag froze the clip's stretch without being asked: {edits:?}"
    );
    assert!(
        edits
            .iter()
            .any(|edit| matches!(edit, ArrangeEdit::Resize { .. })),
        "and it still resized the clip"
    );
}

/// With the switch **on**, a drag still turns stretching on — that is what
/// the switch is for, and it is the direction that loses nothing.
#[test]
fn an_edge_drag_with_the_switch_on_still_turns_stretching_on() {
    let l = layout();
    let mut timeline = Timeline::new(TimelineView::default());
    timeline.set_stretch(true);
    let mut items = a_stretched_clip(0, PPQN * 16);
    items[0].audio.stretched = false; // an ordinary, unstretched take

    let edge = timeline_tick_to_x(&timeline.view, l.grid, PPQN * 16) - 2.0;
    // Below the caption band, where the fade handles live — the edge grip is
    // in the content band under them.
    let y = fontelle_ui::canvas::lane_to_y(&timeline.view, l.grid, 0)
        + timeline.view.lane_height * 0.75;
    timeline.press(MouseButton::Left, edge, y, &l, &items, 4);
    let edits = timeline.drag(edge + 200.0, y, &l, &items, 4);

    assert!(
        edits.iter().any(|edit| matches!(
            edit,
            ArrangeEdit::SetStretch {
                stretch: fontelle_types::ClipStretch::Resample,
                ..
            }
        )),
        "the switch is on and the drag did not stretch: {edits:?}"
    );
}
