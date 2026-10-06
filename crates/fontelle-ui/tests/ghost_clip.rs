//! A faint clip on an empty arrangement, saying how to make a real one.
//!
//! Ty: *"whenever there are no clips in the arrangement, theres a small ghost
//! of a clip in the arrangement that is kind of pulsating / flickering and it
//! has a tip text saying double click with the pencil to create a new clip or
//! something along those lines. ... doesnt really get in the way of returning
//! users"*.
//!
//! It is a picture and nothing else: no part of the arrangement's input reads
//! it, so a press where it is drawn does what a press on empty grid does —
//! and a double-click there, with the draw tool, is the very gesture it
//! describes.

use fontelle_types::{ClipId, PPQN};
use fontelle_ui::canvas::{
    ArrangeEdit, MouseButton, Timeline, TimelineHit, TimelineTool, TimelineView, ghost_clip,
    ghost_clip_hint, ghost_pulse, timeline_hit, timeline_layout, timeline_tick_to_x,
};
use fontelle_ui::document::ClipInfo;
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn body() -> Rect {
    Rect::new(0.0, 0.0, 900.0, 300.0)
}

fn a_clip() -> ClipInfo {
    let mut arena: fontelle_model::Arena<ClipId, ()> = fontelle_model::Arena::default();
    ClipInfo {
        id: arena.insert(()),
        lane: 2,
        start: PPQN * 64,
        length: PPQN * 4,
        name: "Part".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: fontelle_ui::document::ClipKind::Notes,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

#[test]
fn an_empty_arrangement_shows_a_ghost_at_the_start_of_the_first_lane() {
    let l = timeline_layout(body(), &Theme::dark_default().metrics);
    let view = TimelineView::default();
    let ghost = ghost_clip(&view, l.grid, &[], 4, 4).expect("nothing there, so a ghost");
    assert!(
        (ghost.x - timeline_tick_to_x(&view, l.grid, 0)).abs() <= 3.0,
        "at the song's start: {ghost:?}"
    );
    assert!(ghost.y >= l.grid.y && ghost.bottom() <= l.grid.y + view.lane_height + 0.5);
    assert!(ghost.width > 40.0, "big enough to see: {ghost:?}");
    assert!(l.grid.contains(ghost.x + 1.0, ghost.y + 1.0));
}

#[test]
fn it_goes_the_moment_there_is_a_clip_and_never_comes_with_no_lane() {
    let l = timeline_layout(body(), &Theme::dark_default().metrics);
    let view = TimelineView::default();
    assert_eq!(ghost_clip(&view, l.grid, &[a_clip()], 4, 4), None);
    assert_eq!(ghost_clip(&view, l.grid, &[], 0, 4), None);
}

#[test]
fn its_tip_names_the_gesture_that_really_makes_a_clip() {
    // `Timeline::double_press` on empty grid, with the draw tool — and a
    // plain press makes nothing, so the tip must not say "click".
    let draw = ghost_clip_hint(TimelineTool::Draw);
    assert!(draw.to_lowercase().contains("double-click"), "{draw}");
    // With another tool on, the double-click does not draw: the tip says how
    // to get the pencil back first.
    let select = ghost_clip_hint(TimelineTool::Select);
    assert!(
        select.contains('P') && select.to_lowercase().contains("double-click"),
        "{select}"
    );
}

#[test]
fn it_pulses_gently_and_holds_still_when_motion_is_off() {
    let samples: Vec<f32> = (0..40).map(|i| ghost_pulse(i as f32 * 0.1, true)).collect();
    let (low, high) = samples
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    assert!(
        low > 0.0 && high < 1.0,
        "faint, never gone and never solid: {low}..{high}"
    );
    assert!(high - low > 0.15, "it visibly breathes: {low}..{high}");
    // No jump between neighbouring frames: a pulse, not a flicker.
    for pair in samples.windows(2) {
        assert!((pair[0] - pair[1]).abs() < 0.08, "{pair:?}");
    }
    let still: Vec<f32> = (0..40)
        .map(|i| ghost_pulse(i as f32 * 0.1, false))
        .collect();
    assert!(
        still.windows(2).all(|p| p[0] == p[1]),
        "Still and Off hold it"
    );
}

#[test]
fn a_double_click_where_it_is_drawn_makes_the_clip_it_promised() {
    let l = timeline_layout(body(), &Theme::dark_default().metrics);
    let mut timeline = Timeline::new(TimelineView::default());
    let ghost = ghost_clip(&timeline.view, l.grid, &[], 4, 4).unwrap();
    let (x, y) = (ghost.x + ghost.width / 2.0, ghost.y + ghost.height / 2.0);
    // Nothing is there to hit: it is grid.
    assert!(matches!(
        timeline_hit(&timeline.view, &l, &[], x, y),
        TimelineHit::Empty { lane: 0, .. }
    ));
    let first = timeline.press(MouseButton::Left, x, y, &l, &[], 4);
    assert!(first.is_empty(), "a single press makes nothing: {first:?}");
    timeline.release();
    let second = timeline.double_press(MouseButton::Left, x, y, &l, &[], 4);
    assert!(
        matches!(second.as_slice(), [ArrangeEdit::Add { lane: 0, .. }]),
        "{second:?}"
    );
}
