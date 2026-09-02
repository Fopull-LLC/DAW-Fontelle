//! The filter insert's DSP (`docs/effects-catalogue.md` §2.2).
//!
//! Most of these are **magnitude responses**: a tone in at one frequency, and
//! what came out measured against what went in. That is the right measurement
//! for a filter and it is cheap, because a pure tone through a linear filter
//! comes out a pure tone and the ratio of the two levels *is* the response.
//!
//! The two that are not are the ones that matter most, because they are what
//! makes this more than an EQ band: `the_envelope_opens_the_filter_as_the_
//! signal_gets_loud` and `the_lfo_sweeps_the_corner` measure the response in
//! two different **windows of time** and assert that it moved. And
//! `the_drive_is_before_the_filter` measures a harmonic that only exists if
//! the two stages are in the order the module doc claims.

use fontelle_fx::Filter;
use fontelle_types::{FilterConfig, FilterShape, LfoWave, NoteDivision};

const SR: f32 = 48_000.0;

/// Half a second, which is two cycles of the slowest LFO these tests use.
const FRAMES: usize = 24_000;

const BPM: f32 = 120.0;

fn ms(milliseconds: f32) -> usize {
    (milliseconds * SR / 1000.0).round() as usize
}

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

/// The amplitude of one frequency component, by correlation against it.
///
/// A whole DFT would answer the same question for every frequency at once and
/// none of these tests want every frequency — they want "is there a third
/// harmonic", which is one number.
fn component(signal: &[f32], freq: f32) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (index, sample) in signal.iter().enumerate() {
        let angle = std::f64::consts::TAU * f64::from(freq) * index as f64 / f64::from(SR);
        re += f64::from(*sample) * angle.cos();
        im -= f64::from(*sample) * angle.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / signal.len() as f64) as f32
}

fn through(config: &FilterConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut filter = Filter::new();
    filter.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    filter.process(&mut channels, config, BPM);
    (left, right)
}

fn both(config: &FilterConfig, source: &[f32]) -> Vec<f32> {
    let (out, _) = through(config, source.to_vec(), source.to_vec());
    out
}

/// What the filter does to a tone at `freq`, in dB. Measured past the
/// settling, which for a resonant filter is a few dozen cycles.
fn response_db(config: &FilterConfig, freq: f32) -> f32 {
    let source = sine(freq, FRAMES);
    let out = both(config, &source);
    let settle = ms(100.0);
    db(rms(&out[settle..]) / rms(&source[settle..]))
}

// ------------------------------------------------------------------ at rest

#[test]
fn a_fresh_filter_is_a_wire() {
    // A 24 dB low-pass at the top of its knob, which is where the one control
    // a person came here for starts.
    let config = FilterConfig::new();
    for freq in [100.0, 1_000.0, 5_000.0] {
        let moved = response_db(&config, freq);
        assert!(
            moved.abs() < 0.2,
            "a fresh filter moved {freq} Hz by {moved} dB"
        );
    }
}

#[test]
fn silence_in_is_silence_out() {
    let config = FilterConfig {
        cutoff_hz: 800.0,
        resonance: 0.9,
        drive: 1.0,
        lfo_amount: 1.0,
        ..FilterConfig::new()
    };
    let (out_l, out_r) = through(&config, vec![0.0; FRAMES], vec![0.0; FRAMES]);
    assert_eq!(peak(&out_l), 0.0);
    assert_eq!(peak(&out_r), 0.0);
}

#[test]
fn an_empty_bus_is_not_a_panic() {
    let mut filter = Filter::new();
    filter.prepare(SR);
    let mut channels: Vec<&mut [f32]> = Vec::new();
    filter.process(&mut channels, &FilterConfig::new(), BPM);
}

// ------------------------------------------------------------------ shapes

#[test]
fn every_shape_passes_what_its_name_says() {
    // One corner, three tones, eight shapes. Each row is the claim the name
    // makes: what a low pass does to something two octaves under its corner
    // is nothing, and what it does two octaves over is a lot.
    let corner = 1_000.0;
    for shape in FilterShape::ALL {
        let config = FilterConfig {
            shape,
            cutoff_hz: corner,
            resonance: 0.5,
            ..FilterConfig::new()
        };
        let below = response_db(&config, corner / 8.0);
        let at = response_db(&config, corner);
        let above = response_db(&config, corner * 8.0);
        match shape {
            FilterShape::LowPass12 | FilterShape::LowPass24 => {
                assert!(below > -1.0, "{shape:?} cut the bottom: {below} dB");
                assert!(above < -30.0, "{shape:?} passed the top: {above} dB");
            }
            FilterShape::HighPass12 | FilterShape::HighPass24 => {
                assert!(above > -1.0, "{shape:?} cut the top: {above} dB");
                assert!(below < -30.0, "{shape:?} passed the bottom: {below} dB");
            }
            FilterShape::BandPass12 | FilterShape::BandPass24 => {
                assert!(at > below + 12.0, "{shape:?} did not favour its corner");
                assert!(at > above + 12.0, "{shape:?} did not favour its corner");
            }
            FilterShape::Notch => {
                assert!(below > -1.0 && above > -1.0, "{shape:?} is not a notch");
                assert!(at < -12.0, "{shape:?} left its corner at {at} dB");
            }
            FilterShape::Peak => {
                assert!(below.abs() < 1.0 && above.abs() < 1.0, "{shape:?} is not a bell");
                assert!(at > 6.0, "{shape:?} did not lift its corner: {at} dB");
            }
        }
    }
}

#[test]
fn twenty_four_falls_twice_as_fast_as_twelve() {
    // The number in the name is decibels per octave, and it is measured two
    // octaves up where the difference has had somewhere to happen.
    let shallow = FilterConfig {
        shape: FilterShape::LowPass12,
        cutoff_hz: 1_000.0,
        ..FilterConfig::new()
    };
    let steep = FilterConfig {
        shape: FilterShape::LowPass24,
        ..shallow
    };
    let one = response_db(&shallow, 4_000.0);
    let two = response_db(&steep, 4_000.0);
    assert!(
        (one + 24.0).abs() < 3.0,
        "12 dB an octave gave {one} dB two octaves up"
    );
    assert!(
        (two + 48.0).abs() < 4.0,
        "24 dB an octave gave {two} dB two octaves up"
    );
}

#[test]
fn resonance_puts_a_peak_at_the_corner() {
    let flat = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 1_000.0,
        resonance: 0.0,
        ..FilterConfig::new()
    };
    let sharp = FilterConfig {
        resonance: 1.0,
        ..flat
    };
    let without = response_db(&flat, 1_000.0);
    let with = response_db(&sharp, 1_000.0);
    assert!(
        without < 0.5,
        "the knob at zero already had a peak: {without} dB"
    );
    assert!(
        with > 15.0,
        "full resonance lifted the corner by {with} dB, which is not a filter that sings"
    );
}

// ---------------------------------------------------------------- envelope

/// A tone that spends its first half quiet and its second half loud — an
/// auto-wah's whole world.
fn quiet_then_loud(freq: f32) -> Vec<f32> {
    sine(freq, FRAMES)
        .iter()
        .enumerate()
        .map(|(index, sample)| sample * if index < FRAMES / 2 { 0.01 } else { 1.0 })
        .collect()
}

#[test]
fn the_envelope_opens_the_filter_as_the_signal_gets_loud() {
    // The auto-wah, measured: with the corner down at 500 Hz a 5 kHz tone is
    // nowhere, and with the envelope up four octaves it is through.
    let config = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 500.0,
        env_amount: 1.0,
        env_attack_ms: 5.0,
        ..FilterConfig::new()
    };
    let source = quiet_then_loud(5_000.0);
    let out = both(&config, &source);

    let shut = FRAMES / 2 - ms(50.0)..FRAMES / 2;
    let open = FRAMES / 2 + ms(100.0)..FRAMES;
    let closed_db = db(rms(&out[shut.clone()]) / rms(&source[shut]));
    let open_db = db(rms(&out[open.clone()]) / rms(&source[open]));
    assert!(
        closed_db < -40.0,
        "the filter was not shut before the note: {closed_db} dB"
    );
    assert!(
        open_db > -6.0,
        "the envelope did not open it: {open_db} dB"
    );
}

#[test]
fn the_envelope_can_also_close_it() {
    // The half nobody ships. A negative amount sweeps the corner *down* as
    // the signal gets loud, which is a duck with a tone rather than a wah.
    let config = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 12_000.0,
        env_amount: -1.0,
        env_attack_ms: 5.0,
        ..FilterConfig::new()
    };
    let source = quiet_then_loud(5_000.0);
    let out = both(&config, &source);

    let quiet = FRAMES / 2 - ms(50.0)..FRAMES / 2;
    let loud = FRAMES / 2 + ms(100.0)..FRAMES;
    let before = db(rms(&out[quiet.clone()]) / rms(&source[quiet]));
    let after = db(rms(&out[loud.clone()]) / rms(&source[loud]));
    assert!(before > -1.0, "the quiet part was already filtered: {before} dB");
    assert!(
        after < -30.0,
        "a negative envelope did not close the filter: {after} dB"
    );
}

// --------------------------------------------------------------------- LFO

#[test]
fn the_lfo_sweeps_the_corner() {
    // Half a second of a two-hertz sine on the corner, with a 4 kHz tone to
    // hear it through. A quarter of the way in the corner is four octaves up
    // and the tone is through; three quarters in it is four octaves down and
    // the tone is gone.
    let config = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 500.0,
        lfo_amount: 1.0,
        lfo_rate_hz: 2.0,
        lfo_wave: LfoWave::Sine,
        ..FilterConfig::new()
    };
    let source = sine(4_000.0, FRAMES);
    let out = both(&config, &source);

    let top = ms(115.0)..ms(135.0);
    let bottom = ms(365.0)..ms(385.0);
    let open = db(rms(&out[top.clone()]) / rms(&source[top]));
    let shut = db(rms(&out[bottom.clone()]) / rms(&source[bottom]));
    assert!(open > -3.0, "the top of the sweep was still shut: {open} dB");
    assert!(shut < -40.0, "the bottom of the sweep was open: {shut} dB");
}

#[test]
fn sample_and_hold_jumps_rather_than_sweeps() {
    // The one wave that does not sweep. Its value is a memory rather than a
    // formula, which is why `LfoWave::value` returns zero for it and the DSP
    // has to be asked instead.
    assert_eq!(LfoWave::SampleHold.value(0.3), 0.0);
    let config = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 700.0,
        lfo_amount: 1.0,
        lfo_rate_hz: 16.0,
        lfo_wave: LfoWave::SampleHold,
        ..FilterConfig::new()
    };
    let out = both(&config, &sine(4_000.0, FRAMES));
    // Each hold is a sixteenth of a second; the level inside one is steady
    // and the level between two is not.
    let levels: Vec<f32> = (1..7)
        .map(|step| {
            let start = ms(62.5 * step as f32) + ms(20.0);
            db(rms(&out[start..start + ms(20.0)]))
        })
        .collect();
    let swing = levels
        .windows(2)
        .fold(0.0f32, |m, pair| m.max((pair[1] - pair[0]).abs()));
    assert!(
        swing > 6.0,
        "sample & hold held the same value throughout: {swing} dB of swing"
    );
}

#[test]
fn every_lfo_wave_is_a_shape_of_its_own() {
    // Rule 1, as a measurement: two positions of a chooser that do the same
    // thing are one position.
    let sampled: Vec<Vec<f32>> = LfoWave::ALL
        .iter()
        .map(|wave| (0..16).map(|i| wave.value(i as f32 / 16.0)).collect())
        .collect();
    for (index, wave) in LfoWave::ALL.iter().enumerate() {
        for value in &sampled[index] {
            assert!(
                (-1.0..=1.0).contains(value),
                "{wave:?} left the range at {value}"
            );
        }
        for (other, sibling) in LfoWave::ALL.iter().enumerate().skip(index + 1) {
            // Sample & hold's closed form is silence on purpose; every other
            // pair has to differ.
            if *wave == LfoWave::SampleHold || *sibling == LfoWave::SampleHold {
                continue;
            }
            assert_ne!(
                sampled[index], sampled[other],
                "{wave:?} and {sibling:?} are the same wave"
            );
        }
    }
}

#[test]
fn a_synced_lfo_follows_the_tempo() {
    let config = FilterConfig {
        lfo_sync: true,
        lfo_division: NoteDivision::Quarter,
        ..FilterConfig::new()
    };
    assert!((config.effective_lfo_hz(120.0) - 2.0).abs() < 1e-4);
    assert!((config.effective_lfo_hz(60.0) - 1.0).abs() < 1e-4);
    let free = FilterConfig { lfo_sync: false, ..config };
    assert_eq!(free.effective_lfo_hz(60.0), free.lfo_rate_hz);
}

// ------------------------------------------------------------------- drive

#[test]
fn the_drive_is_before_the_filter() {
    // The ordering claim, and the only measurement that can tell the two
    // apart: a `tanh` on a sine makes a third harmonic, and a filter under it
    // takes that harmonic away. If the drive were after the filter, the
    // harmonic would survive whatever the corner was set to.
    let open = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 20_000.0,
        drive: 1.0,
        ..FilterConfig::new()
    };
    let closed = FilterConfig {
        cutoff_hz: 800.0,
        ..open
    };
    let clean = FilterConfig { drive: 0.0, ..open };
    let source: Vec<f32> = sine(500.0, FRAMES).iter().map(|s| s * 0.9).collect();
    let settle = ms(100.0);

    let none = component(&both(&clean, &source)[settle..], 1_500.0);
    let made = component(&both(&open, &source)[settle..], 1_500.0);
    let removed = component(&both(&closed, &source)[settle..], 1_500.0);

    assert!(none < 1e-4, "a clean filter made harmonics: {none}");
    assert!(made > 0.02, "the drive made no third harmonic: {made}");
    assert!(
        removed < made * 0.2,
        "the filter did not take the drive's harmonic away, so it is not after it"
    );
}

#[test]
fn the_output_trims_what_the_drive_added() {
    // Rule 4: anything nonlinear has a gain either side of it, or the drive
    // knob is a volume.
    let source: Vec<f32> = sine(500.0, FRAMES).iter().map(|s| s * 0.5).collect();
    let unity = FilterConfig::new();
    let trimmed = FilterConfig {
        output_db: -6.0,
        ..unity
    };
    let settle = ms(50.0);
    let moved = db(rms(&both(&trimmed, &source)[settle..]) / rms(&both(&unity, &source)[settle..]));
    assert!((moved + 6.0).abs() < 0.01, "the trim moved {moved} dB");
}

// ------------------------------------------------------------------ stereo

#[test]
fn the_corner_is_stereo_linked() {
    // One side loud and one quiet: the envelope moves both corners together,
    // because a corner that moved per side would make the image swim.
    let config = FilterConfig {
        shape: FilterShape::LowPass24,
        cutoff_hz: 500.0,
        env_amount: 1.0,
        ..FilterConfig::new()
    };
    let loud = sine(5_000.0, FRAMES);
    let quiet: Vec<f32> = loud.iter().map(|s| s * 0.01).collect();
    let (_, out_r) = through(&config, loud, quiet.clone());
    let settle = ms(200.0)..;
    let heard = db(rms(&out_r[settle.clone()]) / rms(&quiet[settle]));
    assert!(
        heard > -6.0,
        "the quiet side was left shut while the loud one opened: {heard} dB"
    );
}
