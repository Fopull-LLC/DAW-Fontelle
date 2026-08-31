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
    ArrangeEdit, MouseButton, Timeline, TimelineView, timeline_layout, timeline_tick_to_x,
};
use fontelle_ui::document::ClipInfo;
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
