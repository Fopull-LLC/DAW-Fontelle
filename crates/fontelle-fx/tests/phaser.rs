//! The phaser (`docs/flopsynth-next.md` §4.5, `docs/effects-catalogue.md`
//! §2.4): a cascade of first-order all-passes swept by an LFO. What it
//! writes is the all-passed signal alone; the notches are what happens
//! when `EffectNode` puts the dry signal back under it, and are measured
//! here by doing that sum by hand.

mod common;

use common::*;
use fontelle_fx::Phaser;
use fontelle_types::PhaserConfig;

const FRAMES: usize = 96_000;

fn through(config: &PhaserConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut phaser = Phaser::new();
    phaser.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    phaser.process(&mut channels, config, BPM);
    (left, right)
}

fn still(stages: u32, centre_hz: f32) -> PhaserConfig {
    let mut config = PhaserConfig::new();
    config.stages = stages;
    config.centre_hz = centre_hz;
    config.depth = 0.0;
    config.feedback = 0.0;
    config.spread = 0.0;
    config
}

/// An all-pass passes everything: the wet is the input's level at every
/// frequency, whatever the stage count.
#[test]
fn the_wet_alone_has_unit_gain_everywhere() {
    for stages in [2u32, 6, 12] {
        let config = still(stages, 1_000.0);
        for hz in [100.0, 414.0, 1_000.0, 4_000.0, 12_000.0] {
            let level = level_at(hz, 24_000, |l, r| through(&config, l, r));
            assert!(
                (level - 1.0).abs() < 0.03,
                "{stages} stages at {hz} Hz: {level}"
            );
        }
    }
}

/// Summed with the dry signal, four stages at 1 kHz notch at `tan(π/8)`
/// and `tan(3π/8)` of the centre — the same places the filter model's
/// phaser puts them, because it is the same chain.
#[test]
fn summed_with_the_dry_it_notches_where_the_stages_reach_half_a_turn() {
    let config = still(4, 1_000.0);
    let summed_level = |hz: f32| {
        let dry = sine(hz, 24_000);
        let (wet, _) = through(&config, dry.clone(), dry.clone());
        let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(d, w)| (d + w) * 0.5).collect();
        peak(&mixed[12_000..])
    };
    let first = 1_000.0 * (std::f32::consts::PI / 8.0).tan();
    let second = 1_000.0 * (3.0 * std::f32::consts::PI / 8.0).tan();
    assert!(
        db(summed_level(first)) < -20.0,
        "notch at {first:.0} Hz: {} dB",
        db(summed_level(first))
    );
    assert!(db(summed_level(second)) < -20.0, "notch at {second:.0} Hz");
    assert!(
        db(summed_level(1_000.0)).abs() < 1.0,
        "the centre is in phase"
    );
}

/// The LFO moves the notch: at depth, the level at the still notch's
/// frequency comes and goes over a cycle.
#[test]
fn the_sweep_moves_the_notch() {
    let mut config = still(4, 1_000.0);
    config.depth = 1.0;
    config.rate_hz = 0.5;
    let first = 1_000.0 * (std::f32::consts::PI / 8.0).tan();
    let dry = sine(first, FRAMES);
    let (wet, _) = through(&config, dry.clone(), dry.clone());
    let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(d, w)| (d + w) * 0.5).collect();
    // The envelope over 10 ms windows across the two seconds: short,
    // because the notch is narrow and passes the tone quickly.
    let window = 480;
    let envelope: Vec<f32> = mixed.chunks(window).map(peak).collect();
    let (low, high) = (
        envelope.iter().cloned().fold(f32::INFINITY, f32::min),
        envelope.iter().cloned().fold(0.0f32, f32::max),
    );
    assert!(low < 0.15, "the notch passes over the tone: {low}");
    assert!(high > 0.7, "and leaves it: {high}");
}

/// Twelve stages have three notches under the centre where four have one.
#[test]
fn more_stages_are_more_notches() {
    let count_notches = |stages: u32| {
        let config = still(stages, 2_000.0);
        let mut notches = 0;
        let mut hz = 60.0f32;
        let mut was_deep = false;
        while hz < 2_000.0 {
            let dry = sine(hz, 12_000);
            let (wet, _) = through(&config, dry.clone(), dry.clone());
            let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(d, w)| (d + w) * 0.5).collect();
            let deep = db(peak(&mixed[6_000..])) < -12.0;
            if deep && !was_deep {
                notches += 1;
            }
            was_deep = deep;
            hz *= 1.03;
        }
        notches
    };
    assert_eq!(count_notches(4), 1);
    assert_eq!(count_notches(12), 3);
}

/// Feedback lifts the peaks between the notches.
#[test]
fn feedback_makes_the_peaks_resonant() {
    let plain = still(4, 1_000.0);
    let mut fed = plain;
    fed.feedback = 0.8;
    let level = |config: &PhaserConfig| {
        let dry = sine(1_000.0, 24_000);
        let (wet, _) = through(config, dry.clone(), dry.clone());
        let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(d, w)| (d + w) * 0.5).collect();
        peak(&mixed[12_000..])
    };
    assert!(
        db(level(&fed)) > db(level(&plain)) + 3.0,
        "{} against {}",
        db(level(&fed)),
        db(level(&plain))
    );
}

/// Spread puts the two channels at different points of the sweep: a mono
/// signal comes out with two different sides, and at no spread the same.
#[test]
fn spread_puts_the_sides_apart_in_the_sweep() {
    let mut config = still(6, 800.0);
    config.depth = 0.8;
    config.rate_hz = 1.0;
    let apart = |spread: f32| {
        let mut config = config;
        config.spread = spread;
        let (l, r) = through(&config, sine(600.0, 24_000), sine(600.0, 24_000));
        l.iter()
            .zip(&r)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max)
    };
    assert!(apart(0.0) < 1e-4, "no spread: {}", apart(0.0));
    assert!(apart(1.0) > 0.3, "full spread: {}", apart(1.0));
}

/// Nothing runs away at the top of every knob.
#[test]
fn every_knob_at_the_top_is_finite() {
    let mut config = PhaserConfig::new();
    config.stages = 12;
    config.depth = 1.0;
    config.feedback = 0.9;
    config.rate_hz = 20.0;
    config.centre_hz = 8_000.0;
    let (out, _) = through(&config, sine(440.0, 48_000), sine(440.0, 48_000));
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(peak(&out) < 12.0, "{}", peak(&out));
}
