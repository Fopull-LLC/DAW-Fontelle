//! The analyser behind the EQ's curve, checked by arithmetic.

use fontelle_dsp::{SPECTRUM_FLOOR_DB, SPECTRUM_SIZE, SpectrumAnalyser, bin_width_hz};

const SR: f32 = 48_000.0;

/// A sine at `hz`, `SPECTRUM_SIZE` samples of it.
fn sine(hz: f32, amplitude: f32) -> Vec<f32> {
    (0..SPECTRUM_SIZE)
        .map(|i| amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / SR).sin())
        .collect()
}

#[test]
fn a_tone_lands_in_its_own_bin() {
    let mut analyser = SpectrumAnalyser::new();
    // Exactly on a bin centre, so there is nothing to leak.
    let bin = 64;
    let hz = bin as f32 * bin_width_hz(SR);
    let magnitudes = analyser.analyse(&sine(hz, 1.0)).to_vec();

    let loudest = magnitudes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)
        .unwrap();
    assert_eq!(loudest, bin, "a {hz} Hz tone is loudest in bin {bin}");
}

/// A full-scale sine reads about 0 dB, so the graph's scale means something.
#[test]
fn a_full_scale_tone_reads_about_zero_decibels() {
    let mut analyser = SpectrumAnalyser::new();
    let hz = 64.0 * bin_width_hz(SR);
    let magnitudes = analyser.analyse(&sine(hz, 1.0)).to_vec();
    let peak = magnitudes.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        peak.abs() < 1.0,
        "a full-scale sine should read about 0 dB, got {peak}"
    );
}

/// Half the amplitude is six decibels down, wherever it is.
#[test]
fn halving_the_amplitude_is_six_decibels() {
    let mut analyser = SpectrumAnalyser::new();
    let hz = 100.0 * bin_width_hz(SR);
    let loud = analyser
        .analyse(&sine(hz, 1.0))
        .iter()
        .cloned()
        .fold(f32::MIN, f32::max);
    let quiet = analyser
        .analyse(&sine(hz, 0.5))
        .iter()
        .cloned()
        .fold(f32::MIN, f32::max);
    assert!(
        ((loud - quiet) - 6.02).abs() < 0.2,
        "half the amplitude is 6 dB down, got {}",
        loud - quiet
    );
}

#[test]
fn silence_is_the_floor() {
    let mut analyser = SpectrumAnalyser::new();
    let magnitudes = analyser.analyse(&vec![0.0; SPECTRUM_SIZE]).to_vec();
    assert!(
        magnitudes.iter().all(|db| *db <= SPECTRUM_FLOOR_DB + 0.001),
        "nothing in is nothing out"
    );
}

/// A window that has only just opened has fewer samples than the transform
/// wants, and gets a graph rather than a panic.
#[test]
fn a_short_block_is_padded_rather_than_refused() {
    let mut analyser = SpectrumAnalyser::new();
    let magnitudes = analyser.analyse(&sine(1_000.0, 1.0)[..200]).to_vec();
    assert_eq!(magnitudes.len(), SPECTRUM_SIZE / 2);
    assert!(magnitudes.iter().any(|db| *db > SPECTRUM_FLOOR_DB));
}

/// And an empty one — the first frame after the tap is made.
#[test]
fn nothing_at_all_is_still_a_graph() {
    let mut analyser = SpectrumAnalyser::new();
    let magnitudes = analyser.analyse(&[]).to_vec();
    assert_eq!(magnitudes.len(), SPECTRUM_SIZE / 2);
}
