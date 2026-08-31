//! Looping a clip from the arrangement, and drawing one so it looks looped.
//!
//! Reported from using the window:
//!
//! > *"I want to make it easier to loop things vs just extend the clip. Let's
//! > make it so if you're dragging a clip while holding shift it makes it
//! > loop. Also let's make the loop visuals look more like it's actually
//! > looping rather than a copy paste."*
//!
//! Both halves are here. The **gesture** is the same drag on the same edge
//! with Shift held, which is what makes it easy: nothing new to find, and the
//! two readings of "make this longer" sit under one grip. The **picture** is
//! `loop_marks`, which is what stops a four-bar loop looking like a four-bar
//! block that happens to be long.
//!
//! The model half — what a loop *is*, and why it is not a copy — lives in
//! `fontelle-model/tests/looping.rs` and `fontelle-sequencer/tests/looping.rs`.

use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, ClipPart, Modifiers, MouseButton, Timeline, TimelineHit, TimelineView, clip_rect,
    loop_marks, timeline_hit, timeline_layout,
};
use fontelle_ui::document::ClipInfo;
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

fn ids(n: usize) -> Vec<ClipId> {
    let mut arena: fontelle_model::Arena<ClipId, ()> = fontelle_model::Arena::default();
    (0..n).map(|_| arena.insert(())).collect()
}

fn clip(id: ClipId, start: Tick, length: Tick, loop_length: Option<Tick>) -> ClipInfo {
    ClipInfo {
        id,
        lane: 0,
        start,
        length,
        name: "Part".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length,
        kind: fontelle_ui::document::ClipKind::Notes,
        curve: Vec::new(),
    }
}

// ------------------------------------------------------------ the gesture ---

#[test]
fn dragging_the_right_edge_makes_the_clip_longer() {
    // The plain reading, unchanged: the clip's *window* grows and its content
    // does not repeat.
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let id = ids(1)[0];
    let clips = vec![clip(id, 0, PPQN * 4, None)];

    let mut timeline = Timeline::new(view());
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    timeline.press(
        MouseButton::Left,
        block.right() - 2.0,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    let edits = timeline.drag(
        block.right() + PPQN as f32 * 4.0 * timeline.view.pixels_per_tick,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    assert!(
        edits
            .iter()
            .any(|e| matches!(e, ArrangeEdit::Resize { .. })),
        "a plain edge drag resizes: {edits:?}"
    );
    assert!(
        !edits
            .iter()
            .any(|e| matches!(e, ArrangeEdit::SetLoop { .. })),
        "and does not loop"
    );
}

#[test]
fn holding_shift_on_the_same_edge_loops_it_instead() {
    // The whole request, in one gesture: nothing new to find, the same grip,
    // and the difference is a modifier — which is where every other "same
    // drag, other reading" in this window already lives.
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let id = ids(1)[0];
    let clips = vec![clip(id, 0, PPQN * 4, None)];

    let mut timeline = Timeline::new(view());
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    timeline.press(
        MouseButton::Left,
        block.right() - 2.0,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    let edits = timeline.drag(
        block.right() + PPQN as f32 * 4.0 * timeline.view.pixels_per_tick,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );

    // The clip gets longer *and* starts repeating: "make this play twice" is
    // one gesture, not "stretch it" then "and also loop it".
    assert!(
        edits.iter().any(|e| matches!(
            e,
            ArrangeEdit::SetLoop {
                loop_length: Some(length),
                ..
            } if *length == PPQN * 4
        )),
        "the period is the length the clip had when the drag started: {edits:?}"
    );
    assert!(
        edits
            .iter()
            .any(|e| matches!(e, ArrangeEdit::Resize { .. })),
        "and it still grows: {edits:?}"
    );
}

#[test]
fn a_clip_that_already_loops_keeps_its_period_when_it_is_stretched_further() {
    // Dragging a one-bar loop out to eight bars must not make it an eight-bar
    // loop. The period is the content, and the content did not change.
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let id = ids(1)[0];
    let clips = vec![clip(id, 0, PPQN * 8, Some(PPQN * 4))];

    let mut timeline = Timeline::new(view());
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    timeline.press(
        MouseButton::Left,
        block.right() - 2.0,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    let edits = timeline.drag(
        block.right() + PPQN as f32 * 8.0 * timeline.view.pixels_per_tick,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    assert!(
        !edits.iter().any(|e| matches!(
            e,
            ArrangeEdit::SetLoop {
                loop_length: Some(length),
                ..
            } if *length != PPQN * 4
        )),
        "the period must not follow the length: {edits:?}"
    );
}

#[test]
fn shift_dragging_a_clips_body_still_just_moves_it() {
    // Shift is a *reading of the edge grip*, not a mode. A clip dragged by its
    // middle goes where it is dragged, held or not.
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let id = ids(1)[0];
    let clips = vec![clip(id, PPQN * 4, PPQN * 4, None)];

    let mut timeline = Timeline::new(view());
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    timeline.press(
        MouseButton::Left,
        block.x + block.width / 2.0,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    let edits = timeline.drag(
        block.x + block.width / 2.0 + PPQN as f32 * 4.0 * timeline.view.pixels_per_tick,
        block.y + 5.0,
        &l,
        &clips,
        4,
    );
    assert!(edits.iter().all(|e| matches!(e, ArrangeEdit::Move { .. })));
}

#[test]
fn the_edge_grip_is_still_the_edge() {
    let m = Theme::dark_default().metrics;
    let l = timeline_layout(body(), &m);
    let id = ids(1)[0];
    let clips = vec![clip(id, 0, PPQN * 4, Some(PPQN))];
    let v = view();
    let block = clip_rect(&v, l.grid, &clips[0]);

    assert_eq!(
        timeline_hit(&v, &l, &clips, block.right() - 2.0, block.y + 5.0),
        TimelineHit::Clip(id, ClipPart::RightEdge),
        "a looped clip is gripped the same way as any other"
    );
}

// ------------------------------------------------------------ the picture ---

#[test]
fn a_clip_that_does_not_loop_is_marked_nowhere() {
    let v = view();
    let grid = Rect::new(0.0, 0.0, 900.0, 300.0);
    let clips = [clip(ids(1)[0], 0, PPQN * 8, None)];
    assert!(loop_marks(&v, grid, &clips[0]).is_empty());
}

#[test]
fn a_looped_clip_is_marked_at_every_seam_and_not_at_its_ends() {
    // The seams are what says "this repeats"; a line at the start and the end
    // is just a border, and a block with a border is the copy-paste picture
    // this is meant to stop looking like.
    let v = view();
    let grid = Rect::new(0.0, 0.0, 900.0, 300.0);
    let clip = clip(ids(1)[0], 0, PPQN * 16, Some(PPQN * 4));
    let block = clip_rect(&v, grid, &clip);
    let marks = loop_marks(&v, grid, &clip);

    assert_eq!(marks.len(), 3, "four passes have three seams: {marks:?}");
    for x in &marks {
        assert!(*x > block.x + 0.5, "a mark at the very start is a border");
        assert!(*x < block.right() - 0.5, "and one at the end is too");
    }
    // Evenly spaced, because the period is.
    let step = PPQN as f32 * 4.0 * v.pixels_per_tick;
    for (index, x) in marks.iter().enumerate() {
        let expected = block.x + step * (index + 1) as f32;
        assert!(
            (x - expected).abs() < 0.5,
            "seam {index} at {x}, expected {expected}"
        );
    }
}

#[test]
fn a_partial_last_pass_is_still_marked_where_it_begins() {
    let v = view();
    let grid = Rect::new(0.0, 0.0, 900.0, 300.0);
    let clip = clip(ids(1)[0], 0, PPQN * 6, Some(PPQN * 4));
    assert_eq!(
        loop_marks(&v, grid, &clip).len(),
        1,
        "one and a half passes: one seam"
    );
}

#[test]
fn a_loop_zoomed_out_past_legibility_is_marked_sparsely_rather_than_solid() {
    // At one pixel per repeat a mark per seam is a filled rectangle, which
    // reads as a solid block — the opposite of what the marks are for.
    let v = TimelineView {
        pixels_per_tick: 0.0005,
        ..TimelineView::default()
    };
    let grid = Rect::new(0.0, 0.0, 900.0, 300.0);
    let clip = clip(ids(1)[0], 0, PPQN * 4 * 200, Some(PPQN * 4));
    let marks = loop_marks(&v, grid, &clip);
    assert!(
        marks.len() < 40,
        "200 repeats drew {} marks in {} pixels",
        marks.len(),
        clip_rect(&v, grid, &clip).width
    );
}
