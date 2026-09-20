//! The multiband distortion (`docs/flopsynth-next.md` §4.5): three bands
//! on two crossovers, each driven on its own, summed back to the wire
//! when nothing is driven.

mod common;

use common::*;
use fontelle_fx::Multiband;
use fontelle_types::MultibandConfig;

fn through(
    config: &MultibandConfig,
    mut left: Vec<f32>,
    mut right: Vec<f32>,
) -> (Vec<f32>, Vec<f32>) {
    let mut multiband = Multiband::new();
    multiband.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    multiband.process(&mut channels, config);
    (left, right)
}

/// Distortion at `hz`: the harmonics' energy over the fundamental's.
fn thd(config: &MultibandConfig, hz: f32) -> f32 {
    let (out, _) = through(config, sine(hz, 24_000), sine(hz, 24_000));
    let tail = &out[12_000..];
    let fundamental = energy_at(tail, hz);
    let harmonics: f32 = (2..=6).map(|h| energy_at(tail, hz * h as f32)).sum();
    harmonics / fundamental.max(1e-9)
}

/// The bands sum back to the signal: with nothing driven, a wire to
/// within the crossovers' own tolerance.
#[test]
fn undriven_the_bands_sum_back_to_the_input() {
    let config = MultibandConfig::new();
    for hz in [50.0, 200.0, 700.0, 3_000.0, 10_000.0] {
        let level = level_at(hz, 24_000, |l, r| through(&config, l, r));
        assert!((level - 1.0).abs() < 0.05, "{hz} Hz: {level}");
    }
}

/// Driving the top leaves the bottom clean, and the other way about.
#[test]
fn each_band_is_driven_on_its_own() {
    let mut top = MultibandConfig::new();
    top.high_drive_db = 30.0;
    assert!(
        thd(&top, 100.0) < 0.02,
        "the bass through a driven top: {}",
        thd(&top, 100.0)
    );
    assert!(
        thd(&top, 8_000.0) > 0.1,
        "the top itself: {}",
        thd(&top, 8_000.0)
    );
    let mut bottom = MultibandConfig::new();
    bottom.low_drive_db = 30.0;
    assert!(
        thd(&bottom, 100.0) > 0.1,
        "the bass: {}",
        thd(&bottom, 100.0)
    );
    assert!(
        thd(&bottom, 2_000.0) < 0.02,
        "the mids through a driven bottom: {}",
        thd(&bottom, 2_000.0)
    );
}

/// The crossovers decide which band a tone is in: with the mid driven, a
/// tone is distorted while it sits between the two and clean when the low
/// crossover is moved above it.
#[test]
fn the_crossovers_move_the_bands() {
    let mut config = MultibandConfig::new();
    config.mid_drive_db = 30.0;
    config.low_hz = 200.0;
    config.high_hz = 3_000.0;
    assert!(
        thd(&config, 500.0) > 0.1,
        "500 Hz in the mid band: {}",
        thd(&config, 500.0)
    );
    // Two octaves under the moved crossover: a 24 dB an octave crossover
    // leaks a twentieth an octave away, and a twentieth through a 30 dB
    // drive is still a distortion.
    config.low_hz = 2_000.0;
    assert!(
        thd(&config, 500.0) < 0.02,
        "500 Hz under the low crossover: {}",
        thd(&config, 500.0)
    );
}

#[test]
fn output_is_a_trim() {
    let mut config = MultibandConfig::new();
    config.output_db = -6.0;
    let level = level_at(1_000.0, 24_000, |l, r| through(&config, l, r));
    assert!((db(level) + 6.0).abs() < 0.4, "{}", db(level));
}

#[test]
fn every_knob_at_the_top_is_finite() {
    let mut config = MultibandConfig::new();
    config.low_drive_db = 40.0;
    config.mid_drive_db = 40.0;
    config.high_drive_db = 40.0;
    config.output_db = 24.0;
    let (out, _) = through(&config, sine(440.0, 24_000), sine(440.0, 24_000));
    assert!(out.iter().all(|s| s.is_finite()));
}
