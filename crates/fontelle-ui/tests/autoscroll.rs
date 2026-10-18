//! Middle-click autoscroll.
//!
//! Reported by a user (2026-10-18, through Ty): *"we're lacking a way for
//! people to scroll without their scroll wheel needing to turn. traditionally
//! people do this by clicking with the middle mouse and then that puts you in
//! a scroll mode inside the window you middle clicked in and then from that
//! point if you move your mouse up or down it will scroll in that direction
//! faster depending on how far you move your mouse cursor away from the
//! scroll point you middle clicked at."*
//!
//! The model is pure: an anchor, a pointer, a clock. The window asks it how
//! far to move each frame and what a release means.

use fontelle_ui::canvas::{AUTOSCROLL_DEAD_ZONE, AUTOSCROLL_TAP_S, Autoscroll};

const ANCHOR: (f32, f32) = (400.0, 300.0);

#[test]
fn inside_the_dead_zone_nothing_moves() {
    let mut a = Autoscroll::new(ANCHOR);
    let near = AUTOSCROLL_DEAD_ZONE * 0.8;
    assert_eq!(a.velocity((ANCHOR.0 + near, ANCHOR.1)), (0.0, 0.0));
    assert_eq!(a.velocity((ANCHOR.0, ANCHOR.1 - near)), (0.0, 0.0));
    assert_eq!(a.step((ANCHOR.0, ANCHOR.1 + near), 0.016), (0.0, 0.0));
}

#[test]
fn the_view_moves_the_way_the_pointer_went() {
    let a = Autoscroll::new(ANCHOR);
    // Down the screen is down the content, as a wheel turned towards you.
    let (vx, vy) = a.velocity((ANCHOR.0, ANCHOR.1 + 100.0));
    assert_eq!(vx, 0.0);
    assert!(vy > 0.0, "below the anchor scrolls down, got {vy}");
    let (_, vy) = a.velocity((ANCHOR.0, ANCHOR.1 - 100.0));
    assert!(vy < 0.0, "above the anchor scrolls up, got {vy}");
    let (vx, vy) = a.velocity((ANCHOR.0 + 100.0, ANCHOR.1));
    assert!(vx > 0.0 && vy == 0.0);
    let (vx, _) = a.velocity((ANCHOR.0 - 100.0, ANCHOR.1));
    assert!(vx < 0.0);
}

#[test]
fn farther_from_the_anchor_is_faster() {
    let a = Autoscroll::new(ANCHOR);
    let mut last = 0.0;
    for d in [20.0, 50.0, 100.0, 200.0, 400.0] {
        let (_, vy) = a.velocity((ANCHOR.0, ANCHOR.1 + d));
        assert!(
            vy > last,
            "{d} px away scrolled {vy} px/s, no faster than {last}"
        );
        last = vy;
    }
    // Usable at both ends: a slow crawl near the anchor, a fast pass far out.
    let (_, slow) = a.velocity((ANCHOR.0, ANCHOR.1 + AUTOSCROLL_DEAD_ZONE + 10.0));
    let (_, fast) = a.velocity((ANCHOR.0, ANCHOR.1 + 400.0));
    assert!(
        slow < 120.0,
        "just outside the dead zone is a crawl, got {slow}"
    );
    assert!(fast > 2000.0, "far out is fast, got {fast}");
}

#[test]
fn a_step_is_the_velocity_times_the_time_and_a_long_stall_is_capped() {
    let mut a = Autoscroll::new(ANCHOR);
    let pointer = (ANCHOR.0, ANCHOR.1 + 150.0);
    let (_, v) = a.velocity(pointer);
    let (_, d) = a.step(pointer, 0.02);
    assert!((d - v * 0.02).abs() < 1e-3, "{d} vs {}", v * 0.02);
    // A frame that arrives a second late is not a second's worth of jump.
    let (_, d) = a.step(pointer, 1.0);
    assert!(d <= v * 0.1 + 1e-3, "capped, got {d}");
    // Nor is a broken clock anything at all.
    assert_eq!(a.step(pointer, f32::NAN), (0.0, 0.0));
}

#[test]
fn whole_rows_carry_their_remainder() {
    // A list scrolls in rows; a frame's travel is a fraction of one. The
    // fraction is kept, so a slow autoscroll still gets there.
    let mut a = Autoscroll::new(ANCHOR);
    let mut rows = 0;
    for _ in 0..10 {
        rows += a.whole((0.0, 5.0), (20.0, 20.0)).1;
    }
    assert_eq!(rows, 2, "fifty pixels at twenty a row is two rows");
    let mut up = 0;
    for _ in 0..10 {
        up += a.whole((0.0, -5.0), (20.0, 20.0)).1;
    }
    assert_eq!(rows + up, 0, "and back again");
    let (x, _) = a.whole((45.0, 0.0), (20.0, 20.0));
    assert_eq!(x, 2);
}

#[test]
fn a_tap_latches_and_a_hold_does_not() {
    // Click the middle button and let go: scroll mode stays on until the
    // next click, as in a browser.
    let mut tap = Autoscroll::new(ANCHOR);
    assert!(tap.release((ANCHOR.0 + 2.0, ANCHOR.1 + 1.0), 0.1));

    // Hold it and move away: scrolling while held, and letting go stops it.
    let mut held = Autoscroll::new(ANCHOR);
    held.step((ANCHOR.0, ANCHOR.1 + 80.0), 0.016);
    assert!(!held.release((ANCHOR.0, ANCHOR.1 + 80.0), 0.2));

    // Strayed and came back before letting go: still a hold, not a tap.
    let mut back = Autoscroll::new(ANCHOR);
    back.step((ANCHOR.0, ANCHOR.1 + 80.0), 0.016);
    assert!(!back.release(ANCHOR, 0.2));

    // Held still for a while: a hold.
    let mut long = Autoscroll::new(ANCHOR);
    assert!(!long.release(ANCHOR, AUTOSCROLL_TAP_S + 0.1));
}
