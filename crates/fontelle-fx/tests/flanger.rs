//! The flanger (`docs/flopsynth-next.md` §4.5): one short modulated delay
//! with signed feedback, and a through-zero mode. Wet only, like the
//! chorus it shares a line with.

mod common;

use common::*;
use fontelle_fx::Flanger;
use fontelle_types::FlangerConfig;

fn through(
    config: &FlangerConfig,
    mut left: Vec<f32>,
    mut right: Vec<f32>,
) -> (Vec<f32>, Vec<f32>) {
    let mut flanger = Flanger::new();
    flanger.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    flanger.process(&mut channels, config, BPM);
    (left, right)
}

fn still(delay_ms: f32) -> FlangerConfig {
    let mut config = FlangerConfig::new();
    config.delay_ms = delay_ms;
    config.depth = 0.0;
    config.feedback = 0.0;
    config.spread = 0.0;
    config.through_zero = false;
    config
}

/// At no depth and no feedback the wet is the input, `delay` later.
#[test]
fn at_rest_the_wet_is_the_input_a_delay_later() {
    let config = still(1.0);
    let (out, _) = through(&config, impulse(4_800), impulse(4_800));
    let at = out
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap();
    assert!(
        (at as i64 - 48).abs() <= 1,
        "the copy landed at {at}, not 48"
    );
    assert!(peak(&out) > 0.9);
}

/// Summed with the dry, a millisecond's comb notches at the odd multiples
/// of 500 Hz.
#[test]
fn summed_with_the_dry_it_is_a_comb() {
    let config = still(1.0);
    let summed = |hz: f32| {
        let dry = sine(hz, 12_000);
        let (wet, _) = through(&config, dry.clone(), dry.clone());
        let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(d, w)| (d + w) * 0.5).collect();
        db(peak(&mixed[6_000..]))
    };
    assert!(summed(500.0) < -20.0, "500 Hz: {}", summed(500.0));
    assert!(summed(1_500.0) < -20.0, "1500 Hz: {}", summed(1_500.0));
    assert!(summed(1_000.0).abs() < 1.0, "1000 Hz: {}", summed(1_000.0));
}

/// Feedback makes the wet itself a comb: positive peaks at the multiples
/// of the delay's frequency, negative at the halves — the hollow one.
#[test]
fn feedback_is_signed_and_the_sign_moves_the_teeth() {
    let mut positive = still(1.0);
    positive.feedback = 0.8;
    let mut negative = still(1.0);
    negative.feedback = -0.8;
    let wet_level =
        |config: &FlangerConfig, hz: f32| db(level_at(hz, 24_000, |l, r| through(config, l, r)));
    assert!(
        wet_level(&positive, 1_000.0) > wet_level(&positive, 500.0) + 8.0,
        "positive: {} at 1000 against {} at 500",
        wet_level(&positive, 1_000.0),
        wet_level(&positive, 500.0)
    );
    assert!(
        wet_level(&negative, 500.0) > wet_level(&negative, 1_000.0) + 8.0,
        "negative: {} at 500 against {} at 1000",
        wet_level(&negative, 500.0),
        wet_level(&negative, 1_000.0)
    );
}

/// The sweep moves the comb: at depth, a tone at a notch comes and goes.
#[test]
fn the_sweep_moves_the_comb() {
    let mut config = still(1.0);
    config.depth = 1.0;
    config.rate_hz = 0.5;
    let dry = sine(500.0, 96_000);
    let (wet, _) = through(&config, dry.clone(), dry.clone());
    let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(d, w)| (d + w) * 0.5).collect();
    // Short windows: the null is narrow and the sweep passes it quickly.
    let envelope: Vec<f32> = mixed.chunks(480).map(peak).collect();
    let low = envelope.iter().cloned().fold(f32::INFINITY, f32::min);
    let high = envelope.iter().cloned().fold(0.0f32, f32::max);
    assert!(low < 0.15 && high > 0.7, "{low} .. {high}");
}

/// Through-zero: a fixed copy at the centre beside the swept one, so the
/// sweep passes through no difference at all — where the two cancel
/// everything and reinforce everything, once a cycle each.
#[test]
fn through_zero_passes_through_nothing() {
    let mut config = still(2.0);
    config.through_zero = true;
    config.depth = 1.0;
    config.rate_hz = 0.5;
    let (wet, _) = through(&config, sine(4_000.0, 96_000), sine(4_000.0, 96_000));
    // Two-millisecond windows: at 4 kHz the null is an eighth of a
    // millisecond wide and the sweep crosses it fast.
    let envelope: Vec<f32> = wet.chunks(96).map(peak).collect();
    let low = envelope.iter().cloned().fold(f32::INFINITY, f32::min);
    let high = envelope.iter().cloned().fold(0.0f32, f32::max);
    assert!(high > 0.9, "the two copies line up once a cycle: {high}");
    assert!(low < 0.1, "and cancel once a cycle: {low}");
    // Without it the wet is one copy and never cancels itself.
    config.through_zero = false;
    let (wet, _) = through(&config, sine(4_000.0, 96_000), sine(4_000.0, 96_000));
    // Past the first window, which is the two milliseconds before the copy
    // arrives.
    let low = wet[4_800..]
        .chunks(96)
        .map(peak)
        .fold(f32::INFINITY, f32::min);
    assert!(low > 0.5, "one copy has nothing to cancel against: {low}");
}

#[test]
fn spread_puts_the_sides_apart() {
    let mut config = still(1.0);
    config.depth = 0.8;
    config.rate_hz = 1.0;
    let apart = |spread: f32| {
        let mut config = config;
        config.spread = spread;
        let (l, r) = through(&config, sine(3_000.0, 24_000), sine(3_000.0, 24_000));
        l.iter()
            .zip(&r)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max)
    };
    assert!(apart(0.0) < 1e-4);
    assert!(apart(1.0) > 0.3, "{}", apart(1.0));
}

#[test]
fn every_knob_at_the_top_is_finite() {
    let mut config = FlangerConfig::new();
    config.delay_ms = 10.0;
    config.depth = 1.0;
    config.feedback = 0.95;
    config.rate_hz = 20.0;
    config.through_zero = true;
    let (out, _) = through(&config, sine(440.0, 48_000), sine(440.0, 48_000));
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(peak(&out) < 12.0, "{}", peak(&out));
}
