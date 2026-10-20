//! Somebody else's cursor and time marker, smoothed between the moments they
//! are heard.
//!
//! Ty, 2026-10-20: *"add smoothing to collaboration syncing on cursor and
//! time bar."* Whereabouts arrive fifteen times a second; drawn where each
//! message put them, a cursor stepped and a playing marker ticked along in
//! jumps. Both now glide, and a playing marker runs on at the pace it was
//! heard moving.

use fontelle_types::PPQN;
use fontelle_ui::canvas::{PlayheadGlide, Spot, Toward, glide_spot};

const FRAME: f64 = 1.0 / 60.0;
/// What fifteen messages a second at 120 bpm move a playing marker by.
const STEP: f64 = PPQN as f64 * 2.0 / 15.0;

#[test]
fn a_playing_marker_runs_on_between_messages_without_stepping() {
    let mut g = PlayheadGlide::default();
    let mut now = 0.0;
    let mut heard = 0.0;
    let mut shown = Vec::new();
    for frame in 0..240 {
        if frame % 4 == 0 {
            g.hear(heard, true, now);
            heard += STEP;
        }
        shown.push(g.advance(now, FRAME as f32));
        now += FRAME;
    }
    // After it has had a second to learn the pace: every frame moves on,
    // never back, and by about the same amount — no stair.
    let steps: Vec<f64> = shown[60..].windows(2).map(|w| w[1] - w[0]).collect();
    let per_frame = STEP / 4.0;
    for (i, step) in steps.iter().enumerate() {
        assert!(*step >= 0.0, "frame {i} went back by {step}");
        assert!(
            (*step - per_frame).abs() < per_frame * 0.6,
            "frame {i} moved {step} ticks, the pace is {per_frame}: {:?}",
            &steps[i.saturating_sub(3)..(i + 3).min(steps.len())]
        );
    }
}

#[test]
fn a_stopped_marker_glides_to_where_it_was_put_and_settles() {
    let mut g = PlayheadGlide::default();
    g.hear(0.0, false, 0.0);
    assert_eq!(
        g.advance(0.0, FRAME as f32),
        0.0,
        "the first is where it is"
    );
    g.hear(PPQN as f64, false, 0.1);
    let first = g.advance(0.1, FRAME as f32);
    assert!(first > 0.0 && first < PPQN as f64, "{first}");
    let mut now = 0.1;
    for _ in 0..60 {
        now += FRAME;
        g.advance(now, FRAME as f32);
    }
    assert!((g.advance(now, FRAME as f32) - PPQN as f64).abs() < 1.0);
    assert!(g.settled(), "still moving after a second");
}

/// A jump — somebody clicked the ruler bars away — is a jump, not a slide
/// across the song.
#[test]
fn a_seek_snaps() {
    let mut g = PlayheadGlide::default();
    g.hear(0.0, false, 0.0);
    g.advance(0.0, FRAME as f32);
    g.hear(PPQN as f64 * 64.0, false, 0.1);
    assert_eq!(g.advance(0.1, FRAME as f32), PPQN as f64 * 64.0);
}

/// Stopped after playing: it comes to rest where it was heard to stop,
/// rather than running on past it.
#[test]
fn a_marker_that_stops_does_not_run_on() {
    let mut g = PlayheadGlide::default();
    let mut now = 0.0;
    for i in 0..30 {
        g.hear(i as f64 * STEP, true, now);
        for _ in 0..4 {
            g.advance(now, FRAME as f32);
            now += FRAME;
        }
    }
    let stop = 30.0 * STEP;
    g.hear(stop, false, now);
    for _ in 0..120 {
        now += FRAME;
        g.advance(now, FRAME as f32);
    }
    assert!((g.advance(now, FRAME as f32) - stop).abs() < 1.0);
}

#[test]
fn a_cursor_eases_toward_where_it_was_heard() {
    let from = Some(Spot::Here { x: 100.0, y: 100.0 });
    let to = Some(Spot::Here { x: 200.0, y: 140.0 });
    let Some(Spot::Here { x, y }) = glide_spot(from, to, FRAME as f32) else {
        panic!("not a point");
    };
    assert!(x > 100.0 && x < 200.0, "{x}");
    assert!(y > 100.0 && y < 140.0, "{y}");
    let mut at = from;
    for _ in 0..30 {
        at = glide_spot(at, to, FRAME as f32);
    }
    let Some(Spot::Here { x, y }) = at else {
        panic!()
    };
    assert!(
        (x - 200.0).abs() < 0.5 && (y - 140.0).abs() < 0.5,
        "{x} {y}"
    );
}

/// Onto the screen from an arrow at its edge, off it, or gone: no slide.
#[test]
fn a_cursor_changing_kind_snaps() {
    let here = Some(Spot::Here { x: 100.0, y: 100.0 });
    let off = Some(Spot::Off {
        x: 10.0,
        y: 100.0,
        toward: Toward::Left,
    });
    assert_eq!(glide_spot(here, off, FRAME as f32), off);
    assert_eq!(glide_spot(off, here, FRAME as f32), here);
    assert_eq!(glide_spot(here, None, FRAME as f32), None);
    assert_eq!(glide_spot(None, here, FRAME as f32), here);
}
