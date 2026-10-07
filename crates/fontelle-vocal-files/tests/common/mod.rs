//! Shared by the format tests: reading a lane at a tick.
#![allow(dead_code)]

use fontelle_sing::Lane;
use fontelle_types::Tick;
use fontelle_vocal_files::SongPart;

/// The standard lane `lane` of `part` at song tick `tick` (0 when undrawn).
pub fn lane(part: &SongPart, lane: Lane, tick: Tick) -> f32 {
    part.vocal.lanes.get(&lane).map_or(0.0, |c| c.at(tick, 0.0))
}

pub fn near(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

#[track_caller]
pub fn assert_near(what: &str, got: f32, want: f32, tolerance: f32) {
    assert!(
        near(got, want, tolerance),
        "{what}: got {got}, want {want} (±{tolerance})"
    );
}
