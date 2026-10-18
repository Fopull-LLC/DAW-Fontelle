//! Middle-click autoscroll, through the window.
//!
//! > *"we're lacking a way for people to scroll without their scroll wheel
//! > needing to turn ... clicking with the middle mouse and then that puts you
//! > in a scroll mode inside the window you middle clicked in"* (a user,
//! > 2026-10-18)
//!
//! The rate arithmetic is `fontelle-ui/tests/autoscroll.rs`; this is the
//! gesture as the window delivers it — presses, moves, frames — on a real
//! studio, with no window opened (`fontelle_ui::Windowless`).

mod common;

use winit::event::MouseButton;

fn window() -> fontelle_ui::Windowless {
    let session = common::a_session_for(common::a_clip_project(8));
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
}

fn middle_of(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn holding_the_middle_button_and_moving_scrolls_the_arrangement() {
    let mut w = window();
    let (x, y) = middle_of(w.timeline_frame());
    let clips = w.clip_count();
    assert_eq!(w.timeline_scroll_tick(), 0);

    w.press_studio(MouseButton::Middle, x, y);
    w.move_studio(x + 150.0, y);
    for _ in 0..20 {
        w.frame(0.016);
    }
    let moved = w.timeline_scroll_tick();
    assert!(
        moved > 0,
        "the pointer to the right scrolls later in the song"
    );

    // Farther out is faster.
    w.move_studio(x + 350.0, y);
    for _ in 0..20 {
        w.frame(0.016);
    }
    let further = w.timeline_scroll_tick() - moved;
    assert!(
        further > moved,
        "{further} after moving farther, {moved} before"
    );

    // Let go after moving: it stops.
    w.release_studio(MouseButton::Middle);
    let stopped = w.timeline_scroll_tick();
    for _ in 0..20 {
        w.frame(0.016);
    }
    assert_eq!(
        w.timeline_scroll_tick(),
        stopped,
        "letting go stops it dead — no easing runs on after the button"
    );
    assert_eq!(w.clip_count(), clips, "nothing was edited");
}

#[test]
fn a_middle_click_latches_and_the_next_click_ends_it_without_editing() {
    let mut w = window();
    let (x, y) = middle_of(w.timeline_frame());
    let clips = w.clip_count();
    w.press_studio(MouseButton::Middle, x, y);
    w.release_studio(MouseButton::Middle);
    w.move_studio(x + 200.0, y);
    for _ in 0..20 {
        w.frame(0.016);
    }
    let moved = w.timeline_scroll_tick();
    assert!(moved > 0, "a tap leaves scroll mode on");

    // A left click anywhere ends it — and is only that.
    w.press_studio(MouseButton::Left, x + 200.0, y);
    w.release_studio(MouseButton::Left);
    for _ in 0..30 {
        w.frame(0.016);
    }
    let settled = w.timeline_scroll_tick();
    for _ in 0..20 {
        w.frame(0.016);
    }
    assert_eq!(w.timeline_scroll_tick(), settled, "scroll mode is off");
    assert_eq!(w.clip_count(), clips, "the click drew nothing");
}

#[test]
fn a_middle_release_does_not_end_a_left_drag() {
    // The button that came up is the one that is asked about: a middle
    // release used to end whatever the left button was doing.
    let mut w = window();
    let (x, y) = middle_of(w.timeline_frame());
    w.press_studio(MouseButton::Left, x, y);
    w.release_studio(MouseButton::Middle);
    w.release_studio(MouseButton::Left);
    // Nothing to assert beyond not panicking and nothing scrolling.
    for _ in 0..10 {
        w.frame(0.016);
    }
    assert_eq!(w.timeline_scroll_tick(), 0);
}
