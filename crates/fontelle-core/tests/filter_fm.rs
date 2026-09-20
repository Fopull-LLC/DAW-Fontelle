//! Filter FM from an oscillator (`docs/flopsynth-next.md` §4.4): a filter
//! slot's cutoff moved at audio rate by one of the patch's own layers,
//! read before that layer's level knob — the same tap the oscillators' FM
//! reads. The one filter feature Serum users ask for by name.

use fontelle_core::flopsynth::{self, LayerRole, flopsynth_init};
use fontelle_core::{
    NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source, patch_params,
};
use fontelle_dsp::{FilterModel, FilterRoute, SynthSource, WavetableId};

const SR: f32 = 48_000.0;

/// OSC A: a saw through a clean low-pass at 1 kHz. OSC B: a sine nobody
/// hears (its level is off) — the modulator, when it is one.
fn a_saw_through_a_filter() -> Patch {
    let mut patch = flopsynth_init();
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        let Source::Synth(osc) = &mut layer.source else {
            continue;
        };
        osc.unison.voices = 1;
        osc.random_phase = false;
        if index == LayerRole::OscA as usize {
            osc.source = SynthSource::Table(WavetableId::Saw);
            osc.filter_route = FilterRoute::F1;
            layer.gain_db = -6.0;
        } else if index == LayerRole::OscB as usize {
            osc.source = SynthSource::Table(WavetableId::SubSine);
            osc.filter_route = FilterRoute::Bypass;
            layer.gain_db = -120.0;
        } else {
            layer.gain_db = -120.0;
        }
    }
    patch.filters[0].enabled = true;
    patch.filters[0].model = FilterModel::Clean;
    patch.filters[0].cutoff_hz = 1_000.0;
    patch.filters[0].resonance = 0.707;
    patch.filters[1].enabled = false;
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].decay_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch.mod_matrix.routes.clear();
    patch
}

fn render(patch: Patch, key: u8) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let mut l = vec![0.0f32; SR as usize];
    let mut r = vec![0.0f32; SR as usize];
    sampler.render(&store, &mut [&mut l, &mut r]);
    l
}

fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

/// Energy above `hz`, as the sum over the harmonics of `f0` up there.
fn energy_above(samples: &[f32], f0: f32, hz: f32) -> f32 {
    let mut total = 0.0;
    let mut h = (hz / f0).ceil().max(1.0);
    while h * f0 < 12_000.0 {
        total += energy_at(samples, h * f0);
        h += 1.0;
    }
    total
}

#[test]
fn a_slot_with_no_modulator_is_the_filter_it_was() {
    let plain = a_saw_through_a_filter();
    assert_eq!(plain.filters[0].fm_from, None);
    assert_eq!(plain.filters[0].fm_amount, 0.0);
    // Naming a modulator at amount 0 changes nothing, sample for sample.
    let mut named = plain.clone();
    named.filters[0].fm_from = Some(LayerRole::OscB as u8);
    let (a, b) = (render(plain, 57), render(named, 57));
    let apart = a
        .iter()
        .zip(&b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(apart < 1e-6, "amount 0 is no modulation: {apart}");
}

/// A sine at the note's pitch swinging a 1 kHz low-pass by three octaves
/// scatters the saw's harmonics above the corner — the corner spends half
/// of every cycle up at 8 kHz. Measured as energy above 2 kHz: nothing
/// much without, plenty with.
#[test]
fn the_cutoff_moves_at_the_modulators_rate() {
    let f0 = 220.0;
    let plain = render(a_saw_through_a_filter(), 57);
    let mut fm = a_saw_through_a_filter();
    fm.filters[0].fm_from = Some(LayerRole::OscB as u8);
    fm.filters[0].fm_amount = 0.75;
    let fm = render(fm, 57);
    let tail = |s: &Vec<f32>| s[SR as usize / 2..].to_vec();
    let (plain, fm) = (tail(&plain), tail(&fm));
    let (quiet, bright) = (
        energy_above(&plain, f0, 2_000.0),
        energy_above(&fm, f0, 2_000.0),
    );
    assert!(
        bright > quiet * 4.0,
        "FM opens the filter at audio rate: {bright:.4} above 2 kHz against {quiet:.4}"
    );
    // The modulator itself is not heard: its level is off, and FM reads it
    // before the level knob. Its own line at 220 Hz is the saw's, no louder.
    let (plain_f0, fm_f0) = (energy_at(&plain, f0), energy_at(&fm, f0));
    assert!(
        fm_f0 < plain_f0 * 2.0,
        "the modulator is a modulator, not a voice: {fm_f0:.4} against {plain_f0:.4}"
    );
}

/// The modulator is read before its level knob, so a modulator turned
/// down to silence still modulates — the same rule as oscillator FM, for
/// the same reason (an operator nobody hears).
#[test]
fn the_modulator_is_read_before_its_level() {
    let f0 = 220.0;
    let build = |gain_db: f32| {
        let mut patch = a_saw_through_a_filter();
        patch.layers[LayerRole::OscB as usize].gain_db = gain_db;
        patch.filters[0].fm_from = Some(LayerRole::OscB as u8);
        patch.filters[0].fm_amount = 0.75;
        render(patch, 57)[SR as usize / 2..].to_vec()
    };
    let (silent, audible) = (build(-120.0), build(-30.0));
    let (a, b) = (
        energy_above(&silent, f0, 2_000.0),
        energy_above(&audible, f0, 2_000.0),
    );
    assert!(
        (a / b - 1.0).abs() < 0.2,
        "the level knob does not change the modulation: {a:.4} against {b:.4}"
    );
}

#[test]
fn the_addresses_reach_it() {
    let mut patch = a_saw_through_a_filter();
    let offered = flopsynth::addresses(&patch);
    assert!(offered.iter().any(|a| a == "patch/filter[0]/fm_from"));
    assert!(offered.iter().any(|a| a == "patch/filter[0]/fm"));
    // Position 0 is none, then the layers in order.
    assert!(patch_params::set(
        &mut patch,
        "patch/filter[0]/fm_from",
        0.0
    ));
    assert_eq!(patch.filters[0].fm_from, None);
    let choices = fontelle_core::patch_params::FILTER_FM_CHOICES;
    let osc_b = patch_params::choice_value(LayerRole::OscB as usize + 1, choices);
    assert!(patch_params::set(
        &mut patch,
        "patch/filter[0]/fm_from",
        osc_b
    ));
    assert_eq!(patch.filters[0].fm_from, Some(LayerRole::OscB as u8));
    assert_eq!(
        patch_params::value(&patch, "patch/filter[0]/fm_from"),
        Some(osc_b)
    );
    assert!(patch_params::set(&mut patch, "patch/filter[0]/fm", 0.4));
    assert!((patch.filters[0].fm_amount - 0.4).abs() < 1e-6);
    assert_eq!(patch_params::value(&patch, "patch/filter[0]/fm"), Some(0.4));
    // Every address the patch offers still answers.
    for address in flopsynth::addresses(&patch) {
        assert!(
            patch_params::value(&patch, &address).is_some(),
            "{address} is offered and cannot be read"
        );
    }
}

/// A patch with no filter FM writes what it always wrote (§0 rule 7).
#[test]
fn a_patch_without_filter_fm_writes_no_new_fields() {
    let patch = a_saw_through_a_filter();
    let data = patch.to_data(&Default::default()).unwrap();
    let text = data.body.to_string();
    assert!(!text.contains("fm_from"), "{text}");
    assert!(!text.contains("fm_amount"), "{text}");
    let mut with = a_saw_through_a_filter();
    with.filters[0].fm_from = Some(1);
    with.filters[0].fm_amount = 0.5;
    let data = with.to_data(&Default::default()).unwrap();
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.filters[0].fm_from, Some(1));
    assert_eq!(back.filters[0].fm_amount, 0.5);
}
