//! The recordings the factory bank ships: a sampled grand, in the binary.
//!
//! > *"maybe you could use the osc sampling feature to make the piano sound
//! > more realistic if you can find a grand piano one shot to use."* — Ty,
//! > 2026-09-16
//!
//! `tests/user_sample.rs` is a recording the *user* dropped in, which the
//! patch carries whole. A recording the *bank* ships is different in one
//! way only: a patch that plays it names it rather than carrying it — a
//! project with a piano channel is not eight megabytes of base64, and the
//! shipped preset file is a page of JSON like every other. Everything else
//! (zones, roots, the voice's nearest-zone rule) is `UserSample` as before.

use std::sync::Arc;

use fontelle_core::factory_samples::FactorySampleSet;
use fontelle_core::{Patch, SampleZone, Source, UserSample, flopsynth};
use fontelle_dsp::SynthSource;

/// A patch playing `set` on its first oscillator.
fn patch_playing(set: FactorySampleSet) -> Patch {
    let mut patch = flopsynth::flopsynth_init();
    patch.samples = vec![set.sample()];
    let Source::Synth(osc) = &mut patch.layers[0].source else {
        unreachable!()
    };
    osc.source = SynthSource::Sample(0);
    patch
}

#[test]
fn the_grand_ships_two_layers_that_tile_the_keyboard() {
    for set in [FactorySampleSet::GrandSoft, FactorySampleSet::GrandHard] {
        let sample = set.sample();
        assert_eq!(sample.factory, Some(set));
        assert!(!sample.name.is_empty());
        assert!(
            sample.zones.len() >= 20,
            "{set:?}: a sampled piano is a recording every few keys, not {}",
            sample.zones.len()
        );
        // Sorted, contiguous, and every root inside its own range.
        let mut next = 0u16;
        for zone in &sample.zones {
            assert_eq!(
                u16::from(zone.key_range.0),
                next,
                "{set:?}: a gap or an overlap at key {}",
                zone.key_range.0
            );
            assert!(zone.key_range.0 <= zone.root_key && zone.root_key <= zone.key_range.1);
            next = u16::from(zone.key_range.1) + 1;
        }
        assert_eq!(next, 128, "{set:?}: the top of the keyboard is uncovered");
        // Middle C is a recording, not a transposition.
        assert!(sample.zones.iter().any(|zone| zone.root_key == 60));
        for zone in &sample.zones {
            let seconds = zone.samples.len() as f32 / zone.sample_rate as f32;
            assert!(
                seconds >= 1.2,
                "{set:?} root {}: {seconds} s is not a note",
                zone.root_key
            );
            assert!(zone.sample_rate >= 24_000);
            let peak = zone.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!(
                peak > 0.05 && peak <= 1.0,
                "{set:?} root {}: peak {peak}",
                zone.root_key
            );
            // Starts from rest: the recording begins before the hammer lands.
            assert!(
                zone.samples[0].abs() < 0.02,
                "{set:?} root {}: starts at {}",
                zone.root_key,
                zone.samples[0]
            );
            // And ends in silence, so a note that outlives it does not click.
            assert!(zone.samples[zone.samples.len() - 1].abs() < 0.002);
        }
        // The bass is kept longer than the top: it rings longer.
        let bottom = sample.zones.first().unwrap();
        let top = sample.zones.last().unwrap();
        assert!(
            bottom.samples.len() as f32 / bottom.sample_rate as f32
                > top.samples.len() as f32 / top.sample_rate as f32
        );
    }
}

#[test]
fn a_factory_set_is_shared_not_copied() {
    // Two patches playing the grand hold the same audio: the bank decodes it
    // once, and a project with four piano channels is not four pianos.
    let a = FactorySampleSet::GrandHard.sample();
    let b = FactorySampleSet::GrandHard.sample();
    assert!(Arc::ptr_eq(&a.zones[0].samples, &b.zones[0].samples));
}

#[test]
fn a_patch_names_a_factory_set_rather_than_carrying_it() {
    let patch = patch_playing(FactorySampleSet::GrandSoft);
    let data = patch.to_data(&Default::default()).unwrap();
    let text = serde_json::to_string(&data).unwrap();
    assert!(
        text.len() < 40_000,
        "the stored patch should name the set, not carry it: {} bytes",
        text.len()
    );
    assert!(text.contains("grand-soft"), "the set's name is in the file");
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.samples, patch.samples);
    assert!(Arc::ptr_eq(
        &back.samples[0].zones[0].samples,
        &patch.samples[0].zones[0].samples
    ));
}

#[test]
fn a_users_own_recording_is_still_carried_whole() {
    // The other side of the same rule: a recording that is nobody's factory
    // set has nowhere else to live.
    let mut patch = flopsynth::flopsynth_init();
    patch.samples = vec![UserSample {
        name: "Mine".to_string(),
        factory: None,
        zones: vec![SampleZone {
            root_key: 60,
            fine_cents: 0.0,
            key_range: (0, 127),
            sample_rate: 48_000,
            samples: (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect(),
        }],
    }];
    let data = patch.to_data(&Default::default()).unwrap();
    let text = serde_json::to_string(&data).unwrap();
    assert!(text.len() > 100_000, "{} bytes", text.len());
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.samples[0].factory, None);
    assert_eq!(back.samples[0].zones.len(), 1);
    // Sixteen-bit on the way through, as `tests/user_sample.rs` says.
    let (a, b) = (
        &back.samples[0].zones[0].samples,
        &patch.samples[0].zones[0].samples,
    );
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-4));
}

#[test]
fn a_set_this_build_does_not_know_reads_back_silent_not_broken() {
    // A file from a later build naming a set this one has not got: the
    // oscillator is silent and the rest of the patch is intact, which is the
    // same answer a damaged sample blob gets.
    let patch = patch_playing(FactorySampleSet::GrandSoft);
    let mut data = patch.to_data(&Default::default()).unwrap();
    let text = serde_json::to_string(&data.body).unwrap();
    data.body = serde_json::from_str(&text.replace("grand-soft", "harpsichord-1780")).unwrap();
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.samples.len(), 1);
    assert!(back.samples[0].zones.is_empty());
    assert_eq!(back.samples[0].factory, None);
    assert_eq!(back.layers.len(), patch.layers.len());
}
