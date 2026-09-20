//! Hyper (`docs/flopsynth-next.md` §4.5): unison as an effect — up to four
//! copies of the signal each detuned by a fixed number of cents, spread
//! across the image. A copy is a delay line read at a steady rate other
//! than one, with two heads crossfaded so the wrap is not heard. Wet
//! only: the copies, and none of the signal that made them.

mod common;

use common::*;
use fontelle_fx::Hyper;
use fontelle_types::HyperConfig;

fn through(config: &HyperConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut hyper = Hyper::new();
    hyper.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    hyper.process(&mut channels, config);
    (left, right)
}

/// Two copies at ±20 cents of 440 Hz sit at 434.9 and 445.1 Hz, and the
/// original is not among them.
#[test]
fn two_voices_are_a_pair_of_detuned_copies() {
    let mut config = HyperConfig::new();
    config.voices = 2;
    config.detune_cents = 20.0;
    config.spread = 0.0;
    let (out, _) = through(&config, sine(440.0, 96_000), sine(440.0, 96_000));
    let tail = &out[48_000..];
    let (low, mid, high) = (
        energy_at(tail, 440.0 * 2f32.powf(-20.0 / 1_200.0)),
        energy_at(tail, 440.0),
        energy_at(tail, 440.0 * 2f32.powf(20.0 / 1_200.0)),
    );
    // Each copy is a quarter of a unit sine in amplitude, less what the
    // crossfade's two heads take off each other where their phases
    // differ — the granular shifter's own comb, which is why the floor
    // is where it is.
    assert!(low > 0.05 && high > 0.05, "the two copies: {low} {high}");
    assert!(
        db(mid / high) < -12.0,
        "and not the original: {mid} against {high}"
    );
}

/// No detune is the input again, at its level: the copies are copies.
#[test]
fn no_detune_is_the_input_in_level() {
    let mut config = HyperConfig::new();
    config.detune_cents = 0.0;
    config.spread = 0.0;
    for hz in [200.0, 2_000.0] {
        let level = level_at(hz, 48_000, |l, r| through(&config, l, r));
        assert!((level - 1.0).abs() < 0.1, "{hz} Hz: {level}");
    }
}

/// Spread pans the copies apart; at none, a mono signal stays mono.
#[test]
fn spread_puts_the_copies_across_the_image() {
    let mut config = HyperConfig::new();
    config.voices = 4;
    config.detune_cents = 15.0;
    let apart = |spread: f32| {
        let mut config = config;
        config.spread = spread;
        let (l, r) = through(&config, sine(440.0, 24_000), sine(440.0, 24_000));
        l.iter()
            .zip(&r)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max)
    };
    assert!(apart(0.0) < 1e-4, "{}", apart(0.0));
    assert!(apart(1.0) > 0.3, "{}", apart(1.0));
}

/// The window is the crossfade's length; a shorter one keeps a transient
/// closer to where it was.
#[test]
fn the_copies_arrive_within_the_window() {
    let mut config = HyperConfig::new();
    config.voices = 2;
    config.detune_cents = 10.0;
    config.window_ms = 10.0;
    let (out, _) = through(&config, impulse(9_600), impulse(9_600));
    let last = out.iter().rposition(|s| s.abs() > 0.01).unwrap_or(0);
    assert!(
        last < (0.011 * SR) as usize,
        "the copies had gone by {last}"
    );
}

#[test]
fn every_knob_at_the_top_is_finite() {
    let mut config = HyperConfig::new();
    config.voices = 4;
    config.detune_cents = 100.0;
    config.spread = 1.0;
    config.window_ms = 50.0;
    let (out, _) = through(&config, sine(440.0, 48_000), sine(440.0, 48_000));
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(peak(&out) < 4.0);
}
