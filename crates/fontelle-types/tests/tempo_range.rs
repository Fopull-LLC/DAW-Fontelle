//! The tempo as an automatable parameter (TDD §12.3).
//!
//! An automation point is a normalised value, 0..1, and the tempo is beats
//! per minute — so there has to be one agreed mapping between the two, and it
//! has to live beside `ParamTarget::Tempo`, because the lane that draws it,
//! the compiler that reads it and the box that shows it all need the same
//! answer.

use fontelle_types::{TEMPO_MAX_BPM, TEMPO_MIN_BPM, normalised_tempo, tempo_from_normalised};

#[test]
fn the_ends_of_the_lane_are_the_ends_of_the_range() {
    assert_eq!(tempo_from_normalised(0.0), TEMPO_MIN_BPM);
    assert_eq!(tempo_from_normalised(1.0), TEMPO_MAX_BPM);
}

#[test]
fn the_range_covers_the_tempos_music_is_written_at() {
    // Wide enough for a ballad and a breakcore track, and no wider: a range
    // to a thousand puts every ordinary song in the bottom tenth of the lane,
    // where a curve is a flat line nobody can edit.
    const {
        assert!(TEMPO_MIN_BPM <= 20.0);
        assert!(TEMPO_MAX_BPM >= 280.0);
        assert!(TEMPO_MAX_BPM <= 400.0);
    }
}

#[test]
fn a_tempo_goes_onto_the_lane_and_comes_back_the_same() {
    for bpm in [20.0, 60.0, 92.5, 120.0, 174.0, 280.0] {
        let back = tempo_from_normalised(normalised_tempo(bpm));
        assert!((back - bpm).abs() < 1e-9, "{bpm} came back as {back}");
    }
}

#[test]
fn values_off_either_end_are_clamped_rather_than_extrapolated() {
    assert_eq!(tempo_from_normalised(-1.0), TEMPO_MIN_BPM);
    assert_eq!(tempo_from_normalised(2.0), TEMPO_MAX_BPM);
    assert_eq!(normalised_tempo(1.0), 0.0);
    assert_eq!(normalised_tempo(10_000.0), 1.0);
}
