//! Motion tells you what changed (`docs/flopsynth-next.md` §3.1 principle
//! 13): an arc eases to a value set by anything but the pointer, a page
//! fades in, a bubble rises. Pure arithmetic on a clock the window hands
//! in, so every one is a test rather than a thing to notice at 60 Hz — and
//! the registry says whether anything is still moving, which is what the
//! animator count is told.

use fontelle_ui::motion::{ARC, BUBBLE, Ease, Motions, PAGE};
use std::time::{Duration, Instant};

#[test]
fn an_ease_runs_from_its_start_to_its_end_smoothly_and_is_then_done() {
    let t0 = Instant::now();
    let ease = Ease::new(0.2, 0.8, t0, Duration::from_millis(100));
    assert!((ease.at(t0) - 0.2).abs() < 1e-6);
    let mid = ease.at(t0 + Duration::from_millis(50));
    assert!((mid - 0.5).abs() < 1e-3, "smoothstep is symmetric: {mid}");
    let early = ease.at(t0 + Duration::from_millis(10));
    assert!(early > 0.2 && early < 0.2 + 0.06, "slow to leave: {early}");
    assert!((ease.at(t0 + Duration::from_millis(100)) - 0.8).abs() < 1e-6);
    assert!(
        (ease.at(t0 + Duration::from_secs(9)) - 0.8).abs() < 1e-6,
        "and stays"
    );
    assert!(!ease.done(t0 + Duration::from_millis(99)));
    assert!(ease.done(t0 + Duration::from_millis(100)));
    // The three durations the window uses.
    assert_eq!(ARC, Duration::from_millis(80));
    assert_eq!(PAGE, Duration::from_millis(120));
    assert_eq!(BUBBLE, Duration::from_millis(100));
}

#[test]
fn the_registry_tracks_one_ease_per_key_and_knows_when_nothing_moves() {
    let t0 = Instant::now();
    let mut motions: Motions<&str> = Motions::new();
    assert!(!motions.is_moving(t0));
    motions.begin("cutoff", 0.0, 1.0, t0, ARC);
    assert!(motions.is_moving(t0));
    assert_eq!(motions.value("cutoff", t0), Some(0.0));
    assert_eq!(motions.value("nothing", t0), None);
    // Begun again mid-way, the new ease starts from where the old one was
    // — a value that jumped back to its start would be the flicker this is
    // meant to remove.
    let half = t0 + Duration::from_millis(40);
    let was = motions.value("cutoff", half).unwrap();
    motions.begin("cutoff", 0.0, 0.2, half, ARC);
    assert!((motions.value("cutoff", half).unwrap() - was).abs() < 1e-6);
    // Finished eases are pruned, and their value is their end.
    let late = t0 + Duration::from_secs(1);
    assert!((motions.value("cutoff", late).unwrap() - 0.2).abs() < 1e-6);
    motions.prune(late);
    assert!(!motions.is_moving(late));
    assert_eq!(motions.value("cutoff", late), None, "gone once done");
}
