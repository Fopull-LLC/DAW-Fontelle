//! The wavefolder (`docs/flopsynth-next.md` §4.5): a signal driven past
//! full scale is folded back rather than clipped, so the harmonics keep
//! coming as the drive goes up instead of flattening into a square.

mod common;

use common::*;
use fontelle_fx::Fold;
use fontelle_types::FoldConfig;

fn through(config: &FoldConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut fold = Fold::new();
    fold.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    fold.process(&mut channels, config);
    (left, right)
}

fn harmonics(config: &FoldConfig, level: f32) -> Vec<f32> {
    let input: Vec<f32> = sine(500.0, 24_000).iter().map(|s| s * level).collect();
    let (out, _) = through(config, input.clone(), input);
    let tail = &out[12_000..];
    (1..=8).map(|h| energy_at(tail, 500.0 * h as f32)).collect()
}

#[test]
fn a_fresh_fold_is_a_wire() {
    let config = FoldConfig::new();
    let input = sine(440.0, 4_800);
    let (out, _) = through(&config, input.clone(), input.clone());
    let apart = input
        .iter()
        .zip(&out)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(apart < 1e-5, "{apart}");
}

/// Driven, a sine folds: it stays inside full scale and gains odd
/// harmonics — and past the first fold the third keeps growing where a
/// clipper's would have flattened.
#[test]
fn drive_folds_the_wave_inside_full_scale() {
    let mut config = FoldConfig::new();
    config.drive_db = 18.0;
    let input = sine(500.0, 24_000);
    let (out, _) = through(&config, input.clone(), input);
    assert!(
        peak(&out[12_000..]) <= 1.01,
        "folded inside full scale: {}",
        peak(&out)
    );
    let h = harmonics(&config, 1.0);
    assert!(h[2] > h[0] * 0.2, "a third harmonic: {:?}", &h[..4]);
    assert!(
        h[1] < h[2] * 0.1,
        "and no second, the fold being symmetric: {:?}",
        &h[..4]
    );
}

/// Symmetry biases the wave before the fold, which puts the even
/// harmonics in.
#[test]
fn symmetry_adds_the_even_harmonics() {
    let mut config = FoldConfig::new();
    config.drive_db = 18.0;
    config.symmetry = 0.6;
    let h = harmonics(&config, 1.0);
    assert!(h[1] > h[0] * 0.1, "a second harmonic: {:?}", &h[..4]);
}

/// Smooth is the corner: at 0 the fold is a crease (the triangle) with
/// harmonics far up the band, at 1 it is a sine's turn with far fewer.
/// Measured at a moderate drive — one fold — because at a large one the
/// sine fold is a high-index FM and as rich as the crease.
#[test]
fn smooth_takes_the_corner_off_the_fold() {
    let mut sharp = FoldConfig::new();
    sharp.drive_db = 8.0;
    sharp.smooth = 0.0;
    let mut soft = sharp;
    soft.smooth = 1.0;
    let high = |config: &FoldConfig| {
        let input = sine(500.0, 24_000);
        let (out, _) = through(config, input.clone(), input);
        let tail = &out[12_000..];
        (7..=24)
            .map(|h| energy_at(tail, 500.0 * h as f32))
            .sum::<f32>()
    };
    assert!(
        high(&sharp) > high(&soft) * 2.0,
        "{} against {}",
        high(&sharp),
        high(&soft)
    );
}

#[test]
fn output_is_a_trim_after_the_fold() {
    let mut config = FoldConfig::new();
    config.drive_db = 12.0;
    let mut trimmed = config;
    trimmed.output_db = -6.0;
    let a = level_at(500.0, 12_000, |l, r| through(&config, l, r));
    let b = level_at(500.0, 12_000, |l, r| through(&trimmed, l, r));
    assert!((db(b / a) + 6.0).abs() < 0.3, "{}", db(b / a));
}

#[test]
fn every_knob_at_the_top_is_finite() {
    let mut config = FoldConfig::new();
    config.drive_db = 40.0;
    config.symmetry = 1.0;
    config.smooth = 1.0;
    config.output_db = 24.0;
    let (out, _) = through(&config, sine(440.0, 12_000), sine(440.0, 12_000));
    assert!(out.iter().all(|s| s.is_finite()));
}
