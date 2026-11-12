//! Master, the mastering chain in one insert (Ty, 2026-11-12: *"songs made
//! in fontelle feel quieter on average. make sure theres a plugin
//! specifically for mastering that goes through different tools you might
//! want when you master including things like leveling out audio cutting
//! things that are inaudible to people, optimizing for multiple speaker
//! types, etc."*). One test per claim, each measured.

mod common;

use common::*;
use fontelle_fx::Master;
use fontelle_types::{MasterCheck, MasterConfig, MasterPreset};

/// Runs `config` over a stereo pair in blocks of 256, as the engine does.
fn through(config: &MasterConfig, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut master = Master::new();
    master.prepare(SR, config);
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    for start in (0..l.len()).step_by(256) {
        let end = (start + 256).min(l.len());
        let (a, b) = (&mut l[start..end], &mut r[start..end]);
        let mut channels: Vec<&mut [f32]> = vec![a, b];
        master.process(&mut channels, config);
    }
    (l, r)
}

/// ITU-R BS.1770's K-weighting at 48 kHz, written out here rather than
/// borrowed from the effect — a meter cannot vouch for itself.
fn k_weighted(x: &[f32]) -> Vec<f32> {
    let biquad = |x: &[f64], b: [f64; 3], a: [f64; 3]| {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        x.iter()
            .map(|&x0| {
                let y0 = b[0] * x0 + b[1] * x1 + b[2] * x2 - a[1] * y1 - a[2] * y2;
                x2 = x1;
                x1 = x0;
                y2 = y1;
                y1 = y0;
                y0
            })
            .collect::<Vec<f64>>()
    };
    let x: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let shelf = biquad(
        &x,
        [1.53512485958697, -2.69169618940638, 1.19839281085285],
        [1.0, -1.69065929318241, 0.73248077421585],
    );
    biquad(
        &shelf,
        [1.0, -2.0, 1.0],
        [1.0, -1.99004745483398, 0.99007225036621],
    )
    .into_iter()
    .map(|v| v as f32)
    .collect()
}

/// The loudness of a stereo stretch, in LUFS (ungated — the test signals
/// have no silence in them).
fn lufs(l: &[f32], r: &[f32]) -> f32 {
    let power = |x: &[f32]| {
        let k = k_weighted(x);
        k.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / k.len() as f64
    };
    (-0.691 + 10.0 * (power(l) + power(r)).log10()) as f32
}

/// A small mix: a bass, a mid and a top, as a song is.
fn a_mix(seconds: f32, level: f32) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            level
                * (0.6 * (std::f32::consts::TAU * 82.0 * t).sin()
                    + 0.3 * (std::f32::consts::TAU * 440.0 * t).sin()
                    + 0.15 * (std::f32::consts::TAU * 3_100.0 * t).sin())
        })
        .collect()
}

fn db(v: f32) -> f32 {
    20.0 * v.max(1e-9).log10()
}

#[test]
fn a_quiet_mix_comes_out_at_the_target_loudness() {
    let mix = a_mix(14.0, 0.05);
    let before = lufs(&mix[..48_000], &mix[..48_000]);
    assert!(before < -24.0, "the input is quiet: {before}");
    for target in [-14.0, -9.0] {
        let config = MasterConfig {
            target_lufs: target,
            ..MasterConfig::new()
        };
        let (l, r) = through(&config, &mix, &mix);
        let tail = l.len() - (4.0 * SR) as usize;
        let after = lufs(&l[tail..], &r[tail..]);
        assert!(
            (after - target).abs() < 1.5,
            "aimed at {target}, came out at {after}"
        );
    }
}

#[test]
fn a_loud_mix_is_brought_down_to_the_target_too() {
    let mix = a_mix(14.0, 0.7);
    let (l, r) = through(&MasterConfig::new(), &mix, &mix);
    let tail = l.len() - (4.0 * SR) as usize;
    let after = lufs(&l[tail..], &r[tail..]);
    assert!(
        (after - MasterConfig::new().target_lufs).abs() < 1.5,
        "{after}"
    );
}

#[test]
fn nothing_gets_past_the_ceiling() {
    let mix = a_mix(6.0, 0.9);
    for ceiling in [-1.0, -0.3, -3.0] {
        let config = MasterConfig {
            ceiling_db: ceiling,
            target_lufs: -6.0,
            ..MasterConfig::new()
        };
        let (l, r) = through(&config, &mix, &mix);
        let worst = db(peak(&l).max(peak(&r)));
        assert!(
            worst <= ceiling + 0.1,
            "ceiling {ceiling}, peaked at {worst}"
        );
    }
}

#[test]
fn leveling_off_leaves_the_loudness_alone() {
    let mix = a_mix(6.0, 0.1);
    let config = MasterConfig {
        auto_level: false,
        ..MasterConfig::new()
    };
    let (l, r) = through(&config, &mix, &mix);
    let tail = l.len() - (2.0 * SR) as usize;
    let moved = lufs(&l[tail..], &r[tail..]) - lufs(&mix[tail..], &mix[tail..]);
    assert!(moved.abs() < 1.0, "{moved}");
}

#[test]
fn what_nobody_can_hear_is_cut() {
    // The clean-up alone: no glue, no leveling.
    let clean = MasterConfig {
        auto_level: false,
        glue: 0.0,
        ..MasterConfig::new()
    };
    // 12 Hz rumble: under the low cut, gone.
    let rumble = sine(12.0, 96_000);
    let (l, _) = through(&clean, &rumble, &rumble);
    let cut = db(rms(&l[48_000..])) - db(rms(&rumble[48_000..]));
    assert!(cut < -12.0, "12 Hz only {cut} dB down");
    // A 100 Hz bass is left where it was.
    let bass = sine(100.0, 96_000);
    let (l, _) = through(&clean, &bass, &bass);
    let kept = db(rms(&l[48_000..])) - db(rms(&bass[48_000..]));
    assert!(kept.abs() < 1.0, "100 Hz moved {kept} dB");
}

#[test]
fn the_bass_is_kept_in_the_middle_for_every_speaker() {
    // A 60 Hz bass in one side only: with the bass kept mono it lands in
    // both sides alike, so a club's mono sub and a phone both get it.
    let bass = sine(60.0, 96_000);
    let silent = vec![0.0; 96_000];
    let config = MasterConfig {
        auto_level: false,
        mono_below_hz: 150.0,
        ..MasterConfig::new()
    };
    let (l, r) = through(&config, &bass, &silent);
    let side: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a - b) * 0.5).collect();
    let mid: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a + b) * 0.5).collect();
    assert!(
        db(rms(&side[48_000..])) < db(rms(&mid[48_000..])) - 15.0,
        "the bass is still in one side"
    );
}

#[test]
fn small_speakers_hear_the_bass_as_its_harmonics() {
    // A 50 Hz sine: a phone cannot play it. "Small speakers" adds its
    // harmonics, where a phone can.
    let bass = sine(50.0, 96_000);
    // The harmonics a phone can play, measured where they are: 150 to
    // 400 Hz, each by a single-bin DFT over the settled second.
    let band = |x: &[f32]| {
        let tail = &x[48_000..];
        let power: f32 = [150.0f32, 200.0, 250.0, 300.0, 350.0, 400.0]
            .iter()
            .map(|hz| {
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (n, v) in tail.iter().enumerate() {
                    let w = std::f64::consts::TAU * f64::from(*hz) * n as f64 / f64::from(SR);
                    re += f64::from(*v) * w.cos();
                    im += f64::from(*v) * w.sin();
                }
                ((re * re + im * im) / (tail.len() as f64).powi(2)) as f32
            })
            .sum();
        power.sqrt()
    };
    let plain = MasterConfig {
        auto_level: false,
        glue: 0.0,
        small_speakers: 0.0,
        ..MasterConfig::new()
    };
    let helped = MasterConfig {
        small_speakers: 80.0,
        ..plain
    };
    let (l0, _) = through(&plain, &bass, &bass);
    let (l1, _) = through(&helped, &bass, &bass);
    assert!(
        db(band(&l1)) > db(band(&l0)) + 20.0,
        "{} vs {}",
        db(band(&l1)),
        db(band(&l0))
    );
}

#[test]
fn checking_on_a_phone_is_mono_and_has_no_bass() {
    let bass = sine(60.0, 96_000);
    let top = sine(1_500.0, 96_000);
    let silent = vec![0.0; 96_000];
    let config = MasterConfig {
        auto_level: false,
        check: MasterCheck::Phone,
        ..MasterConfig::new()
    };
    let (l, r) = through(&config, &top, &silent);
    let apart = l
        .iter()
        .zip(&r)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(apart < 1e-4, "a phone is one speaker");
    let (lb, _) = through(&config, &bass, &bass);
    assert!(db(rms(&lb[48_000..])) < db(rms(&bass[48_000..])) - 18.0);
    // And Off is not a check at all.
    assert_eq!(MasterConfig::new().check, MasterCheck::Off);
}

#[test]
fn every_preset_lands_inside_its_ceiling_and_near_its_target() {
    let mix = a_mix(12.0, 0.08);
    for preset in MasterPreset::ALL {
        let config = MasterConfig::from_preset(preset);
        assert_eq!(config.check, MasterCheck::Off, "{preset:?} ships checking");
        let (l, r) = through(&config, &mix, &mix);
        let worst = db(peak(&l).max(peak(&r)));
        assert!(
            worst <= config.ceiling_db + 0.1,
            "{preset:?} peaked at {worst}"
        );
        if config.auto_level {
            let tail = l.len() - (4.0 * SR) as usize;
            let after = lufs(&l[tail..], &r[tail..]);
            assert!(
                (after - config.target_lufs).abs() < 2.0,
                "{preset:?}: aimed at {}, came out at {after}",
                config.target_lufs
            );
        }
    }
}

#[test]
fn its_latency_is_the_limiters_look_ahead() {
    let mut master = Master::new();
    master.prepare(SR, &MasterConfig::new());
    let mut limiter = fontelle_fx::Limiter::new();
    limiter.prepare(
        SR,
        &fontelle_fx::LimiterConfig {
            ceiling: 1.0,
            lookahead_ms: fontelle_types::LIMITER_LOOKAHEAD_MS,
            release_ms: 80.0,
        },
    );
    assert!(master.latency_samples() > 0);
    assert_eq!(master.latency_samples(), limiter.latency_samples());
}
