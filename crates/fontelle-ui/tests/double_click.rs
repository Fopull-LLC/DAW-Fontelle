//! The clock a double-click is measured against.
//!
//! > *"after working in a project for a while double clicking just doesnt make
//! > new clips anymore like it just stops letting me do that."*
//!
//! Reported of the arrangement, where a double-click on empty grid is the only
//! way to draw a blank clip (`Timeline::double_press`) — and the same detector
//! opens an audio clip's editor, so both gestures had it.
//!
//! **What was wrong.** [`DoubleClick`] was stamped with `Instant::now()` at the
//! moment the window *got to* the press, and a press waits its turn: winit
//! hands the window one batch of events, the window draws, and only then does
//! it look at the queue again. So the pair was being measured against the
//! window's own responsiveness rather than against the hand. Any stretch longer
//! than [`DOUBLE_CLICK_WINDOW`] — a big project's repaint, an autosave, a
//! plugin's scan — silently turned one double-click into two single clicks, and
//! single clicks on empty grid make nothing by design.
//!
//! Measured in the real window, driving it with XTEST: two presses sent 80ms
//! apart were handled **871ms** apart, and one sent as a pair arrived 63µs
//! apart because the window had been blocked and drained both at once. Neither
//! reading is the hand's, and the first one loses the gesture.
//!
//! **The rule.** Time the window spent not listening is not time the hand spent
//! waiting, so it comes off the clock — with one frame's worth of each busy
//! stretch charged, because drawing is what a window is *for* and a frame is
//! too short for anybody to have clicked inside. [`InputClock`] is that
//! subtraction, kept here as a value so the arithmetic is a test rather than
//! something to notice by using a slow machine.

use fontelle_ui::pointer::{DOUBLE_CLICK_WINDOW, DoubleClick, InputClock};
use fontelle_ui::widget::FRAME_INTERVAL;
use std::time::{Duration, Instant};

/// The sequence the event loop actually runs: it wakes with a press, works,
/// waits, wakes with the next press.
fn press_at(
    clock: &mut InputClock,
    clicks: &mut DoubleClick,
    now: Instant,
    at: (f32, f32),
) -> bool {
    clock.busy(now);
    clicks.press(at.0, at.1, clock.stamp(now))
}

#[test]
fn a_double_click_survives_a_hitch_between_its_two_presses() {
    let mut clock = InputClock::default();
    let mut clicks = DoubleClick::default();
    let start = Instant::now();

    // The first press wakes the window, and the frame it asks for takes most
    // of a second — the case the report is about.
    assert!(!press_at(&mut clock, &mut clicks, start, (100.0, 100.0)));
    clock.listening(start + Duration::from_millis(900));

    // The hand clicked again 80ms after the first, and the window reaches that
    // press when it is free. It is still one double-click.
    assert!(
        press_at(
            &mut clock,
            &mut clicks,
            start + Duration::from_millis(980),
            (100.0, 100.0)
        ),
        "a frame longer than the double-click window ate the gesture"
    );
}

#[test]
fn two_presses_the_window_drained_together_are_a_double_click() {
    // The other half of the same fault: blocked long enough, both presses are
    // in one batch and arrive microseconds apart. That *is* a double-click —
    // nothing here should make it one only by accident.
    let mut clock = InputClock::default();
    let mut clicks = DoubleClick::default();
    let start = Instant::now();
    clock.busy(start);
    assert!(!clicks.press(10.0, 10.0, clock.stamp(start)));
    assert!(clicks.press(10.0, 10.0, clock.stamp(start + Duration::from_micros(63))));
}

#[test]
fn an_ordinary_frame_does_not_stretch_the_double_click_window() {
    // The correction is for hitches, not for drawing. A window keeping up must
    // still tell two deliberate clicks from a double, or clicking twice on the
    // same empty bar would leave a clip nobody asked for.
    let mut clock = InputClock::default();
    let mut clicks = DoubleClick::default();
    let start = Instant::now();

    assert!(!press_at(&mut clock, &mut clicks, start, (10.0, 10.0)));
    clock.listening(start + Duration::from_millis(5));

    let later = start + DOUBLE_CLICK_WINDOW + Duration::from_millis(100);
    assert!(
        !press_at(&mut clock, &mut clicks, later, (10.0, 10.0)),
        "two clicks half a second apart became a double-click"
    );
}

#[test]
fn a_busy_stretch_is_charged_one_frame() {
    // The whole of the arithmetic: what the clock reads is wall time less the
    // part of each busy stretch that is longer than a frame.
    let mut clock = InputClock::default();
    let start = Instant::now();
    assert_eq!(clock.stamp(start), start, "an idle clock is the wall clock");

    clock.busy(start);
    clock.listening(start + Duration::from_millis(500));
    let skipped = Duration::from_millis(500) - FRAME_INTERVAL;
    assert_eq!(
        clock.stamp(start + Duration::from_millis(500)),
        start + Duration::from_millis(500) - skipped,
        "the hitch was not taken off the clock"
    );
}

#[test]
fn waiting_for_input_is_time_the_hand_spent_waiting() {
    // A window asleep with nothing to do is a window whose clock runs at the
    // same speed as everybody else's: §16.3 lets it sleep for as long as
    // nothing is happening, and none of that is a hitch.
    let mut clock = InputClock::default();
    let start = Instant::now();
    clock.busy(start);
    clock.listening(start + Duration::from_millis(4));
    let woken = start + Duration::from_secs(30);
    clock.busy(woken);
    assert_eq!(
        clock.stamp(woken),
        woken,
        "half a minute of waiting came off the clock"
    );
}

#[test]
fn a_second_wake_before_the_window_waits_does_not_restart_the_stretch() {
    // Every event in one batch goes through `busy`, and the batch is one
    // stretch: charging each event separately would let a long batch of them
    // stand in for the frame it is really one of.
    let mut clock = InputClock::default();
    let start = Instant::now();
    clock.busy(start);
    clock.busy(start + Duration::from_millis(200));
    clock.listening(start + Duration::from_millis(400));
    let skipped = Duration::from_millis(400) - FRAME_INTERVAL;
    let now = start + Duration::from_millis(400);
    assert_eq!(clock.stamp(now), now - skipped);
}
