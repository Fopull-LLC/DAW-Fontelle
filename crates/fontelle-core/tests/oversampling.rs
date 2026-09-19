//! Oversampling reaches the voice (`docs/flopsynth-next.md` §4.1): the
//! patch has one setting, an oscillator may have its own, the ladder follows
//! the patch's, and none of it is in the file until somebody turns it on.
//!
//! `fontelle-dsp/tests/synth_alias.rs` holds what the oversampling *does*;
//! what these tests hold is that the settings get where they are going —
//! through the patch, the two addresses and the voice — and that a patch
//! written before the setting existed is a patch at `Off`.

use fontelle_core::flopsynth::{self, LayerRole};
use fontelle_core::{
    NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source, patch_params,
};
use fontelle_dsp::{FilterModel, Oversampling, SynthSource, WarpMode, WavetableId, fft_in_place};

const SR: f32 = 48_000.0;
const FRAMES: usize = 16_384;

/// The Init with every layer but A silent, and A a saw hard-synced at 8× —
/// the loudest alias a table can make — through the ladder driven hard.
fn synced(oversampling: Oversampling, own: Oversampling) -> Patch {
    let mut patch = flopsynth::flopsynth_init();
    patch.oversampling = oversampling;
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        let Source::Synth(osc) = &mut layer.source else {
            continue;
        };
        if index == LayerRole::OscA as usize {
            osc.source = SynthSource::Table(WavetableId::Saw);
            osc.warp = WarpMode::Sync;
            osc.warp_amount = 1.0;
            osc.quality = own;
            layer.gain_db = 0.0;
        } else {
            layer.gain_db = -120.0;
        }
    }
    for slot in &mut patch.filters {
        slot.enabled = false;
    }
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch
}

/// C7, tuned the cent onto the transform's grid so the harmonics land on
/// bins, one voice, the left channel.
fn render(patch: Patch) -> (Vec<f32>, f32) {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    let key = 96u8;
    let played = 440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0);
    let on_grid = (played * FRAMES as f32 / SR).round() * SR / FRAMES as f32;
    let cents = (1_200.0 * (on_grid / played).log2()).round();
    sampler.trigger(NoteTrigger::new(key, 100).with_fine_pitch(cents as i16));
    let mut left = vec![0.0f32; FRAMES];
    let mut right = vec![0.0f32; FRAMES];
    {
        let (l, r) = (&mut left[..], &mut right[..]);
        sampler.render(&store, &mut [l, r]);
    }
    (left, played * 2f32.powf(cents / 1_200.0))
}

/// Off-grid energy over on-grid, in dB, between 30 Hz and 20 kHz — the
/// measure `fontelle-dsp/tests/synth_alias.rs` uses.
fn alias_db(samples: &[f32], f0: f32) -> f32 {
    let n = samples.len();
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let t = std::f32::consts::TAU * i as f32 / n as f32;
            let w =
                0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; n];
    fft_in_place(&mut re, &mut im);
    let bin_hz = SR / n as f32;
    let f0_bins = f0 / bin_hz;
    let top = (20_000.0 / bin_hz) as usize;
    let (mut signal, mut alias) = (0.0f64, 0.0f64);
    for k in 8..top {
        let power = f64::from(re[k] * re[k] + im[k] * im[k]);
        let nearest = (k as f32 / f0_bins).round() * f0_bins;
        if (k as f32 - nearest).abs() <= 4.0 {
            signal += power;
        } else {
            alias += power;
        }
    }
    10.0 * (alias / signal.max(1e-30)).log10() as f32
}

/// A patch through the stored form and back, as a project or a preset file
/// would carry it.
fn stored(patch: &Patch) -> (String, Patch) {
    let data = patch.to_data(&Default::default()).expect("serialises");
    let text = serde_json::to_string(&data.body).unwrap();
    let read = Patch::from_data(&data, |_| None).expect("reads back").patch;
    (text, read)
}

#[test]
fn the_setting_is_off_by_default_and_absent_from_the_file_until_set() {
    let patch = flopsynth::flopsynth_init();
    assert_eq!(patch.oversampling, Oversampling::Off);
    let (text, read) = stored(&patch);
    assert!(
        !text.contains("oversampling"),
        "the file grew a field nobody set"
    );
    assert!(
        !text.contains("\"quality\""),
        "an oscillator grew a field nobody set"
    );
    // A patch written before the setting existed reads as one at Off.
    assert_eq!(read.oversampling, Oversampling::Off);

    let mut set = patch.clone();
    set.oversampling = Oversampling::X2;
    if let Source::Synth(osc) = &mut set.layers[0].source {
        osc.quality = Oversampling::X4;
    }
    let (text, read) = stored(&set);
    assert!(text.contains("\"oversampling\":\"X2\""), "{text}");
    assert!(text.contains("\"quality\":\"X4\""), "{text}");
    assert_eq!(read.oversampling, Oversampling::X2);
    let Source::Synth(osc) = &read.layers[0].source else {
        panic!("layer A is an oscillator");
    };
    assert_eq!(osc.quality, Oversampling::X4);
}

#[test]
fn the_two_addresses_step_through_the_factors_and_are_offered() {
    let mut patch = flopsynth::flopsynth_init();
    let addresses = flopsynth::addresses(&patch);
    assert!(addresses.iter().any(|a| a == "patch/oversampling"));
    assert!(
        addresses
            .iter()
            .any(|a| a == "patch/layer[0]/synth/quality")
    );
    // The noise layer has no read to oversample, and offers nothing.
    let noise = LayerRole::Noise as usize;
    assert!(
        !addresses
            .iter()
            .any(|a| *a == format!("patch/layer[{noise}]/synth/quality"))
    );

    for (value, expect) in [
        (0.0, Oversampling::Off),
        (0.5, Oversampling::X2),
        (1.0, Oversampling::X4),
    ] {
        assert!(patch_params::set(&mut patch, "patch/oversampling", value));
        assert_eq!(patch.oversampling, expect, "at {value}");
        assert!(patch_params::set(
            &mut patch,
            "patch/layer[0]/synth/quality",
            value
        ));
        let Source::Synth(osc) = &patch.layers[0].source else {
            panic!("layer A is an oscillator");
        };
        assert_eq!(osc.quality, expect, "at {value}");
    }
    assert_eq!(patch_params::value(&patch, "patch/oversampling"), Some(1.0));
    assert_eq!(
        patch_params::value(&patch, "patch/layer[0]/synth/quality"),
        Some(1.0)
    );
}

#[test]
fn the_patch_setting_reaches_an_oscillator_that_did_not_choose() {
    let (off, f0) = render(synced(Oversampling::Off, Oversampling::Off));
    let (from_patch, _) = render(synced(Oversampling::X4, Oversampling::Off));
    let (own, _) = render(synced(Oversampling::Off, Oversampling::X4));
    let off_db = alias_db(&off, f0);
    let patch_db = alias_db(&from_patch, f0);
    assert!(
        patch_db <= off_db - 20.0,
        "patch at 4\u{d7}: {patch_db:.1} dB against {off_db:.1} at Off"
    );
    // The oscillator's own answer is the same render as the patch's: one
    // setting, resolved before the note.
    assert_eq!(own, from_patch);
}

#[test]
fn the_oscillators_own_setting_wins_over_the_patch() {
    let (from_patch, f0) = render(synced(Oversampling::X4, Oversampling::Off));
    let (own, _) = render(synced(Oversampling::X4, Oversampling::X2));
    let (twice, _) = render(synced(Oversampling::X2, Oversampling::Off));
    assert_eq!(
        own, twice,
        "an oscillator at 2\u{d7} under a patch at 4\u{d7} renders at 2\u{d7}"
    );
    assert!(alias_db(&own, f0) > alias_db(&from_patch, f0));
}

#[test]
fn the_ladder_follows_the_patch() {
    // The source at 4× either way, so the difference is the filter's.
    let ladder = |oversampling| {
        let mut patch = synced(oversampling, Oversampling::X4);
        patch.filters[0].enabled = true;
        patch.filters[0].model = FilterModel::Ladder;
        patch.filters[0].cutoff_hz = 12_000.0;
        // Short of self-oscillation — see the dsp test.
        patch.filters[0].resonance = 0.7;
        patch.filters[0].drive = 1.0;
        patch.filters[0].character = 1.0;
        patch.filters[0].key_track = 0.0;
        patch
    };
    let (off, f0) = render(ladder(Oversampling::Off));
    let (over, _) = render(ladder(Oversampling::X4));
    let off_db = alias_db(&off, f0);
    let over_db = alias_db(&over, f0);
    assert!(
        over_db <= off_db - 6.0,
        "the ladder under a patch at 4\u{d7}: {over_db:.1} dB against {off_db:.1}"
    );
}
