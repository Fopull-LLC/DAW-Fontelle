//! The analyser's tap: the audio thread's end of the EQ's spectrum graph.
//!
//! *"currently theres no eq monitor graph drawn to view the frequency spectrum
//! and make edits based off it and see in realtime."*
//!
//! What is checked here is the **seam**, because that is where this kind of
//! feature dies: an insert copies its input into a ring, and something off the
//! audio thread reads the last window of it back. The transform itself is
//! `fontelle-dsp/tests/spectrum.rs`; the picture is
//! `fontelle-ui/tests/inserts.rs`. This is the only part that involves two
//! threads' worth of ownership, so it is the part worth a test of its own.

use std::sync::Arc;

use fontelle_engine::{AudioNode, EffectNode, PrepareContext, SpectrumTap};
use fontelle_types::{BandChannel, BandType, EffectConfig, EqBand, EqConfig};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

fn sine(freq: f32, frames: usize, from: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * (i + from) as f32 / SR).sin())
        .collect()
}

/// An insert with a tap on it, prepared.
fn tapped(config: EqConfig) -> (EffectNode, Arc<SpectrumTap>) {
    let tap = Arc::new(SpectrumTap::new());
    let mut node = EffectNode::new(EffectConfig::Eq(config)).with_spectrum(Arc::clone(&tap));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    (node, tap)
}

/// Runs `blocks` blocks of a 1 kHz sine through `node`.
fn play(node: &mut EffectNode, blocks: usize) {
    for block in 0..blocks {
        let mut left = sine(1_000.0, BLOCK, block * BLOCK);
        let mut right = left.clone();
        let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
        process(node, &mut channels);
    }
}

#[test]
fn a_tap_collects_what_goes_through_the_insert() {
    let (mut node, tap) = tapped(EqConfig::new());
    let mut samples = Vec::new();
    tap.read(&mut samples);
    assert!(samples.is_empty(), "nothing has played yet");

    play(&mut node, 8);
    tap.read(&mut samples);
    assert_eq!(
        samples.len(),
        fontelle_dsp::SPECTRUM_SIZE,
        "a full window once enough has gone through"
    );
    assert!(
        samples.iter().any(|s| s.abs() > 0.5),
        "and it is the signal, not silence"
    );
}

/// The transform over what the tap collected finds the tone that was played.
#[test]
fn the_tap_and_the_transform_agree_about_what_was_playing() {
    let (mut node, tap) = tapped(EqConfig::new());
    play(&mut node, 8);

    let mut samples = Vec::new();
    tap.read(&mut samples);
    let mut analyser = fontelle_dsp::SpectrumAnalyser::new();
    let magnitudes = analyser.analyse(&samples).to_vec();
    let loudest = magnitudes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(bin, _)| bin)
        .unwrap();
    let hz = loudest as f32 * fontelle_dsp::bin_width_hz(SR);
    assert!(
        (hz - 1_000.0).abs() < 40.0,
        "a 1 kHz tone through the insert reads as 1 kHz, got {hz}"
    );
}

/// Fewer samples than the transform wants is what the first frames after a
/// graph is built look like, and it is not an error.
#[test]
fn a_partly_filled_tap_hands_back_what_it_has() {
    let (mut node, tap) = tapped(EqConfig::new());
    play(&mut node, 1);
    let mut samples = Vec::new();
    tap.read(&mut samples);
    assert_eq!(samples.len(), BLOCK);
    assert_eq!(tap.frames_written(), BLOCK as u64);
}

/// **What arrives, not what leaves.** The curve is drawn over the signal you
/// are shaping, so a cut you have just made has to leave a dip in the *curve*
/// against an unchanged spectrum — rather than flattening the spectrum and
/// leaving nothing to aim at.
#[test]
fn the_tap_is_taken_before_the_effect() {
    let mut eq = EqConfig::new();
    // A deep cut right where the tone is.
    eq.bands[0] = EqBand {
        band_type: BandType::Bell,
        freq_hz: 1_000.0,
        gain_db: -24.0,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    };
    let (mut node, tap) = tapped(eq);
    play(&mut node, 8);

    let mut samples = Vec::new();
    tap.read(&mut samples);
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak > 0.8,
        "the analyser sees the signal going in, not the one the cut left: {peak}"
    );
}

/// A bypassed insert still feeds it: switching an EQ off while you look for
/// the frequency is exactly when the picture matters most.
#[test]
fn a_bypassed_insert_still_feeds_its_analyser() {
    let (mut node, tap) = tapped(EqConfig::new());
    node.set_bypassed(true);
    play(&mut node, 8);
    assert_eq!(tap.frames_written(), (BLOCK * 8) as u64);
}
