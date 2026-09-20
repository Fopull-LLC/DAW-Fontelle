//! The frequency shifter (`docs/flopsynth-next.md` §4.5): every partial
//! moved by the same number of hertz, which is not a pitch shift — the
//! harmonics stop being harmonics — and is the one thing an EQ cannot do.
//! A Hilbert pair and a quadrature oscillator; feedback makes the
//! barber-pole.

mod common;

use common::*;
use fontelle_fx::Shifter;
use fontelle_types::{ShiftDirection, ShifterConfig};

fn through(
    config: &ShifterConfig,
    mut left: Vec<f32>,
    mut right: Vec<f32>,
) -> (Vec<f32>, Vec<f32>) {
    let mut shifter = Shifter::new();
    shifter.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    shifter.process(&mut channels, config);
    (left, right)
}

fn lines(config: &ShifterConfig, hz: f32, at: &[f32]) -> Vec<f32> {
    let (out, _) = through(config, sine(hz, 48_000), sine(hz, 48_000));
    let tail = &out[24_000..];
    at.iter().map(|f| energy_at(tail, *f)).collect()
}

#[test]
fn a_tone_moves_up_by_the_shift_and_nowhere_else() {
    let mut config = ShifterConfig::new();
    config.shift_hz = 200.0;
    config.direction = ShiftDirection::Up;
    config.feedback = 0.0;
    let [below, same, above] = lines(&config, 1_000.0, &[800.0, 1_000.0, 1_200.0])[..] else {
        unreachable!()
    };
    assert!(above > 0.2, "the line at 1200 Hz: {above}");
    assert!(
        db(same / above) < -30.0,
        "nothing left at 1000: {}",
        db(same / above)
    );
    assert!(
        db(below / above) < -30.0,
        "nothing at 800: {}",
        db(below / above)
    );
}

#[test]
fn down_moves_it_down_and_both_moves_it_both_ways() {
    let mut config = ShifterConfig::new();
    config.shift_hz = 200.0;
    config.direction = ShiftDirection::Down;
    let [below, same, above] = lines(&config, 1_000.0, &[800.0, 1_000.0, 1_200.0])[..] else {
        unreachable!()
    };
    assert!(below > 0.2 && db(above / below) < -30.0 && db(same / below) < -30.0);
    config.direction = ShiftDirection::Both;
    let [below, same, above] = lines(&config, 1_000.0, &[800.0, 1_000.0, 1_200.0])[..] else {
        unreachable!()
    };
    assert!(
        (db(above / below)).abs() < 2.0,
        "both sides alike: {above} {below}"
    );
    assert!(db(same / above) < -25.0);
}

/// The fine knob adds to the coarse one.
#[test]
fn fine_adds_to_the_shift() {
    let mut config = ShifterConfig::new();
    config.shift_hz = 100.0;
    config.fine_hz = 7.0;
    let [at_100, at_107] = lines(&config, 1_000.0, &[1_100.0, 1_107.0])[..] else {
        unreachable!()
    };
    assert!(at_107 > at_100 * 3.0, "{at_107} against {at_100}");
}

/// A shift of nothing is a wire in level: the Hilbert pair passes
/// everything.
#[test]
fn no_shift_is_the_input_in_level() {
    let mut config = ShifterConfig::new();
    config.shift_hz = 0.0;
    config.fine_hz = 0.0;
    for hz in [100.0, 1_000.0, 8_000.0] {
        let level = level_at(hz, 24_000, |l, r| through(&config, l, r));
        assert!((level - 1.0).abs() < 0.05, "{hz} Hz: {level}");
    }
}

/// Feedback is the barber-pole: the shifted signal shifted again, so a
/// second line appears two shifts up.
#[test]
fn feedback_makes_the_barber_pole() {
    let mut config = ShifterConfig::new();
    config.shift_hz = 200.0;
    config.feedback = 0.7;
    let [once, twice] = lines(&config, 1_000.0, &[1_200.0, 1_400.0])[..] else {
        unreachable!()
    };
    assert!(
        twice > once * 0.3,
        "a second line at 1400: {twice} against {once}"
    );
    config.feedback = 0.0;
    let [_, none] = lines(&config, 1_000.0, &[1_200.0, 1_400.0])[..] else {
        unreachable!()
    };
    assert!(none < twice * 0.1, "and none without feedback: {none}");
}

#[test]
fn every_knob_at_the_top_is_finite() {
    let mut config = ShifterConfig::new();
    config.shift_hz = 5_000.0;
    config.fine_hz = 20.0;
    config.feedback = 0.9;
    config.direction = ShiftDirection::Both;
    let (out, _) = through(&config, sine(440.0, 48_000), sine(440.0, 48_000));
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(peak(&out) < 12.0);
}
