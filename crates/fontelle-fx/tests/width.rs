//! Width (`docs/flopsynth-next.md` §4.5): mid/side width with the bass
//! kept in the middle, and a gain on each half.

mod common;

use common::*;
use fontelle_fx::Width;
use fontelle_types::WidthConfig;

fn through(config: &WidthConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut width = Width::new();
    width.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    width.process(&mut channels, config);
    (left, right)
}

fn side_and_mid(l: &[f32], r: &[f32]) -> (f32, f32) {
    let side: Vec<f32> = l.iter().zip(r).map(|(a, b)| (a - b) * 0.5).collect();
    let mid: Vec<f32> = l.iter().zip(r).map(|(a, b)| (a + b) * 0.5).collect();
    (peak(&side[side.len() / 2..]), peak(&mid[mid.len() / 2..]))
}

#[test]
fn a_fresh_width_is_a_wire() {
    let config = WidthConfig::new();
    let (l, r) = through(&config, sine(440.0, 4_800), vec![0.0; 4_800]);
    let apart = l
        .iter()
        .zip(sine(440.0, 4_800).iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(apart < 1e-5 && peak(&r) < 1e-5);
}

#[test]
fn width_scales_the_side_and_leaves_the_middle() {
    let one_sided = || (sine(1_000.0, 9_600), vec![0.0; 9_600]);
    let (l, r) = one_sided();
    let (side0, mid0) = side_and_mid(&l, &r);
    let mut narrow = WidthConfig::new();
    narrow.width = 0.0;
    let (l, r) = through(&narrow, one_sided().0, one_sided().1);
    let (side, mid) = side_and_mid(&l, &r);
    assert!(
        side < 1e-4 && (mid - mid0).abs() < 1e-3,
        "mono: {side} {mid}"
    );
    let mut wide = WidthConfig::new();
    wide.width = 2.0;
    let (l, r) = through(&wide, one_sided().0, one_sided().1);
    let (side, mid) = side_and_mid(&l, &r);
    assert!(
        (side / side0 - 2.0).abs() < 0.02 && (mid - mid0).abs() < 1e-3,
        "double: {side} {mid}"
    );
}

/// Bass mono: the side under the corner is taken out, the side above kept.
#[test]
fn the_bass_is_kept_in_the_middle() {
    let mut config = WidthConfig::new();
    config.mono_below_hz = 200.0;
    let side_level = |hz: f32| {
        let l = sine(hz, 24_000);
        let r: Vec<f32> = l.iter().map(|s| -s).collect();
        let (l, r) = through(&config, l, r);
        side_and_mid(&l, &r).0
    };
    assert!(
        db(side_level(50.0)) < -12.0,
        "50 Hz side: {}",
        db(side_level(50.0))
    );
    assert!(
        db(side_level(2_000.0)).abs() < 1.0,
        "2 kHz side: {}",
        db(side_level(2_000.0))
    );
}

#[test]
fn the_mid_and_side_gains_are_gains() {
    let mut config = WidthConfig::new();
    config.mid_db = -6.0;
    config.side_db = 6.0;
    let (l, r) = through(&config, sine(1_000.0, 9_600), vec![0.0; 9_600]);
    let (side, mid) = side_and_mid(&l, &r);
    assert!(
        (db(side / 0.5) - 6.0).abs() < 0.2,
        "side {}",
        db(side / 0.5)
    );
    assert!((db(mid / 0.5) + 6.0).abs() < 0.2, "mid {}", db(mid / 0.5));
}
