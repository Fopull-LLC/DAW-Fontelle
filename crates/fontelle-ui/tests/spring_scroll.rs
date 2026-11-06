//! Springy scrolling (Ty, 2026-11-06: *"encorperate springy scrolling like
//! apple devices have just to make everything have more polished
//! feeling"*).
//!
//! What "like Apple" means here, as four rules this file holds the model
//! to:
//!
//! - **A notch of the wheel glides** to where it is going rather than
//!   jumping there, and **at an edge it gives a little and comes back**.
//! - **A touchpad follows the fingers exactly** — no easing between the
//!   finger and the page — and **past an edge the page resists**, further
//!   and further, like a rubber band, never more than a set stretch.
//! - **Let go mid-swipe and the page coasts**, slowing to a stop; coast
//!   into an edge and it bounces back to it.
//! - **It always comes to rest exactly** inside the content, and stops
//!   asking for frames.

use fontelle_ui::canvas::{MAX_OVERSCROLL, SpringScroll};

const FRAME: f32 = 1.0 / 60.0;

/// Runs frames until it settles (or ten seconds pass), from `now`; the time
/// it ended at, and every position it showed.
fn settle(s: &mut SpringScroll, mut now: f64) -> (f64, Vec<f32>) {
    let mut seen = Vec::new();
    for _ in 0..600 {
        now += f64::from(FRAME);
        let moving = s.step(FRAME, now);
        seen.push(s.position());
        if !moving {
            return (now, seen);
        }
    }
    panic!("never settled: {:?}", &seen[seen.len() - 5..]);
}

fn page(max: f32) -> SpringScroll {
    let mut s = SpringScroll::default();
    s.set_max(max);
    s
}

#[test]
fn a_notch_glides_and_lands_exactly() {
    let mut s = page(1000.0);
    s.wheel(120.0);
    s.step(FRAME, 0.0);
    let first = s.position();
    assert!(
        first > 0.0 && first < 120.0,
        "moving, not there yet: {first}"
    );
    let (_, seen) = settle(&mut s, 0.0);
    assert_eq!(s.position(), 120.0, "exactly where it was sent");
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "never backwards: {seen:?}"
    );
}

#[test]
fn a_notch_against_the_top_gives_a_little_and_comes_back() {
    let mut s = page(1000.0);
    s.wheel(-120.0);
    let (_, seen) = settle(&mut s, 0.0);
    let deepest = seen.iter().copied().fold(0.0_f32, f32::min);
    assert!(deepest < -1.0, "it gave: {deepest}");
    assert!(deepest > -MAX_OVERSCROLL, "a little: {deepest}");
    assert_eq!(s.position(), 0.0, "and is back at the top");
}

#[test]
fn a_notch_against_the_bottom_comes_back_to_the_bottom() {
    let mut s = page(300.0);
    s.jump_to(280.0);
    s.wheel(200.0);
    let (_, seen) = settle(&mut s, 0.0);
    assert!(seen.iter().any(|&p| p > 300.5), "it went past: {seen:?}");
    assert_eq!(s.position(), 300.0);
}

#[test]
fn a_touchpad_follows_the_fingers_exactly() {
    let mut s = page(1000.0);
    s.touch(30.0, 0.000);
    assert_eq!(s.position(), 30.0, "no easing between finger and page");
    s.touch(12.5, 0.008);
    assert_eq!(s.position(), 42.5);
    s.touch(-20.0, 0.016);
    assert_eq!(s.position(), 22.5);
}

#[test]
fn past_an_edge_the_page_resists_more_and_more_and_never_too_far() {
    let mut s = page(1000.0);
    let mut stretch = Vec::new();
    for i in 0..200 {
        s.touch(-10.0, f64::from(i) * 0.008);
        stretch.push(-s.position());
    }
    // 2000 points of finger past the top...
    let total = *stretch.last().unwrap();
    assert!(total > 20.0, "it does stretch: {total}");
    assert!(
        total < MAX_OVERSCROLL,
        "...and never more than the band: {total}"
    );
    // ...and each step gives less than the last.
    let steps: Vec<f32> = stretch.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        steps[0] > steps[100] && steps[100] > steps[198],
        "{steps:?}"
    );
    assert!(steps.iter().all(|&d| d >= 0.0));
    // Fingers away: back to the top, exactly.
    s.release(1.6);
    settle(&mut s, 1.6);
    assert_eq!(s.position(), 0.0);
}

#[test]
fn let_go_mid_swipe_and_it_coasts_to_a_stop() {
    let mut s = page(10_000.0);
    // A swipe: 25 points a frame at 120 Hz — 3000 points a second.
    for i in 0..12 {
        s.touch(25.0, f64::from(i) / 120.0);
    }
    let lifted = s.position();
    s.release(12.0 / 120.0);
    let (_, seen) = settle(&mut s, 12.0 / 120.0);
    let coasted = s.position() - lifted;
    assert!(coasted > 200.0, "it ran on: {coasted}");
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "in the swipe's direction"
    );
    // Slowing down: each frame's step is no more than the last.
    let steps: Vec<f32> = seen.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.windows(2).all(|w| w[1] <= w[0] + 0.01), "{steps:?}");
}

#[test]
fn coasting_into_the_bottom_bounces_back_to_it() {
    let mut s = page(500.0);
    s.jump_to(400.0);
    for i in 0..8 {
        s.touch(30.0, f64::from(i) / 120.0);
    }
    s.release(8.0 / 120.0);
    let (_, seen) = settle(&mut s, 8.0 / 120.0);
    let furthest = seen.iter().copied().fold(0.0_f32, f32::max);
    assert!(furthest > 500.5, "it went past: {furthest}");
    assert!(furthest < 500.0 + MAX_OVERSCROLL);
    assert_eq!(s.position(), 500.0);
}

#[test]
fn fingers_that_stop_before_they_lift_do_not_fling() {
    let mut s = page(10_000.0);
    for i in 0..10 {
        s.touch(25.0, f64::from(i) / 120.0);
    }
    // Held still for a fifth of a second, then lifted.
    let held = s.position();
    s.release(10.0 / 120.0 + 0.2);
    settle(&mut s, 10.0 / 120.0 + 0.2);
    assert!(
        (s.position() - held).abs() < 1.0,
        "{} from {held}",
        s.position()
    );
}

#[test]
fn a_touchpad_that_never_says_it_let_go_still_coasts() {
    // Not every desktop reports the lift: the events just stop.
    let mut s = page(10_000.0);
    for i in 0..12 {
        s.touch(25.0, f64::from(i) / 120.0);
    }
    let lifted = s.position();
    settle(&mut s, 12.0 / 120.0);
    assert!(s.position() - lifted > 200.0);
}

#[test]
fn content_that_shrinks_under_the_page_springs_to_its_new_end() {
    let mut s = page(800.0);
    s.jump_to(700.0);
    s.set_max(200.0);
    let (_, seen) = settle(&mut s, 0.0);
    assert!(seen[0] > 200.0, "springs, not a jump: {seen:?}");
    assert_eq!(s.position(), 200.0);
}

#[test]
fn nothing_to_scroll_still_gives_and_comes_back() {
    let mut s = page(0.0);
    s.wheel(120.0);
    let (_, seen) = settle(&mut s, 0.0);
    assert!(seen.iter().any(|&p| p > 0.5));
    assert_eq!(s.position(), 0.0);
}

#[test]
fn at_rest_it_asks_for_no_frames() {
    let mut s = page(1000.0);
    assert!(!s.step(FRAME, 0.0));
    s.wheel(40.0);
    let (now, _) = settle(&mut s, 0.0);
    assert!(!s.step(FRAME, now + 0.1));
    assert_eq!(s.position(), 40.0);
}
