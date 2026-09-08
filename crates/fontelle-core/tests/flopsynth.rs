//! Flopsynth's patch: the Init, the roles, and the address table
//! (`docs/flopsynth-plan.md` §6, §4).
//!
//! The claim under all of them is §2.1's: **Flopsynth is not a special case.**
//! It is an ordinary `Patch`, so what these tests measure is that it *is* one
//! — that it round-trips through the ordinary format, that its addresses go
//! through the ordinary `patch_params`, and that the ordinary voice plays it.

use fontelle_core::flopsynth::{self, LayerRole};
use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler, Source, patch_params};
use fontelle_dsp::{SynthSource, WavetableId};

const SR: f32 = 48_000.0;

fn render(patch: fontelle_core::Patch, key: u8, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let frames = (SR * seconds) as usize;
    let mut left = vec![0.0f32; frames];
    let mut right = vec![0.0f32; frames];
    {
        let (l, r) = (&mut left[..], &mut right[..]);
        sampler.render(&store, &mut [l, r]);
    }
    left
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

#[test]
fn the_init_patch_is_five_synth_layers_in_role_order() {
    let patch = flopsynth::flopsynth_init();
    assert_eq!(patch.layers.len(), 5);
    for (index, expected) in [
        LayerRole::OscA,
        LayerRole::OscB,
        LayerRole::OscC,
        LayerRole::Sub,
        LayerRole::Noise,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(flopsynth::layer_role(index), expected);
        assert!(
            matches!(patch.layers[index].source, Source::Synth(_)),
            "layer {index} is not a synth oscillator"
        );
    }
    // The roles are a convention, not a rule: a sixth layer is legal and is
    // what the hybrid instruments of §12 arrive as.
    assert_eq!(flopsynth::layer_role(5), LayerRole::Extra);
    assert!(flopsynth::is_flopsynth(&patch));

    // The noise layer is noise; the other four read tables.
    let Source::Synth(noise) = &patch.layers[4].source else {
        panic!("the noise layer is not a synth layer");
    };
    assert_eq!(noise.source, SynthSource::Noise);
    assert!(
        !noise.key_track,
        "noise has no pitch to track, and one that followed the keyboard would \
         be a filter sweep nobody asked for"
    );
}

/// The whole of §6's argument, measured: only oscillator A is up, and every
/// other layer is set up so that turning its level knob is the **only** step
/// to hearing it.
#[test]
fn only_oscillator_a_is_up_and_the_rest_are_one_knob_away() {
    let patch = flopsynth::flopsynth_init();
    assert!(patch.layers[0].gain_db > fontelle_core::SILENT_DB);
    for index in 1..5 {
        assert_eq!(
            patch.layers[index].gain_db,
            fontelle_core::SILENT_DB,
            "layer {index} arrives audible, which clips the first chord"
        );
    }

    // Turn each one up on its own and it sounds — nothing else to configure.
    for index in 1..5 {
        let mut one = flopsynth::flopsynth_init();
        one.layers[0].gain_db = fontelle_core::SILENT_DB;
        one.layers[index].gain_db = -12.0;
        let out = render(one, 60, 0.5);
        assert!(
            rms(&out) > 0.002,
            "turning up layer {index} alone has to make a sound; RMS {}",
            rms(&out)
        );
    }
}

#[test]
fn the_init_patch_leaves_headroom() {
    // A four-note chord at full velocity, which is what `basic_synth` and
    // `KIT_HEADROOM_DB` are held to.
    let store = SampleStore::new();
    let mut sampler = Sampler::new(flopsynth::flopsynth_init());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    for key in [60u8, 64, 67, 72] {
        sampler.trigger(NoteTrigger::new(key, 127));
    }
    let frames = (SR * 1.5) as usize;
    let mut left = vec![0.0f32; frames];
    let mut right = vec![0.0f32; frames];
    {
        let (l, r) = (&mut left[..], &mut right[..]);
        sampler.render(&store, &mut [l, r]);
    }
    let loudest = peak(&left).max(peak(&right));
    assert!(
        loudest < 0.8,
        "a four-note chord at velocity 127 has to leave room for the other \
         four layers; peaked at {loudest}"
    );
    assert!(loudest > 0.05, "and it has to actually sound: {loudest}");
}

#[test]
fn a_note_through_the_init_patch_plays_its_own_pitch() {
    for (key, hz) in [(60u8, 261.63f32), (72, 523.25), (48, 130.81)] {
        let out = render(flopsynth::flopsynth_init(), key, 0.5);
        // Zero crossings over the sustain, past the attack.
        let tail = &out[2_400..];
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let measured = crossings as f32 / (tail.len() as f32 / SR);
        assert!(
            (measured - hz).abs() < hz * 0.05,
            "key {key} should sound {hz} Hz; measured {measured}"
        );
    }
}

/// §4's table, held by construction: every address the panel offers is one
/// `patch_params::set` accepts and `value` reads back. A knob you can turn and
/// a lane cannot reach is the defect this makes inexpressible.
#[test]
fn every_address_the_panel_offers_round_trips() {
    let mut patch = flopsynth::flopsynth_init();
    // A route and an effect slot, so the two address families that only exist
    // when the patch has one are covered too.
    patch.fx.push(fontelle_core::PatchFx {
        config: fontelle_types::EffectConfig::new(fontelle_types::EffectKind::Chorus),
        enabled: true,
    });

    let addresses = flopsynth::addresses(&patch);
    assert!(addresses.len() > 100, "only {} addresses", addresses.len());

    let mut seen = addresses.clone();
    seen.sort();
    let before = seen.len();
    seen.dedup();
    assert_eq!(before, seen.len(), "an address is listed twice");

    for address in &addresses {
        assert!(
            patch_params::value(&patch, address).is_some(),
            "{address} is offered by the panel and cannot be read"
        );
        // **Write, read, write, read.** Not "what went in comes out", which is
        // false for a switch and for every chooser — a switch given 0.25 is
        // off, and it is *right* that it reads back 0. What has to hold is
        // that reading a control and writing what it said back is a no-op,
        // because that is what a panel does on every redraw: a control that
        // drifted a step each time it was drawn would walk across its own
        // range while nobody touched it.
        for probe in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let mut copy = patch.clone();
            assert!(
                patch_params::set(&mut copy, address, probe),
                "{address} is offered by the panel and cannot be written"
            );
            let read = patch_params::value(&copy, address)
                .unwrap_or_else(|| panic!("{address} wrote and then would not read"));
            let mut again = copy.clone();
            patch_params::set(&mut again, address, read);
            let settled = patch_params::value(&again, address).unwrap();
            assert!(
                (settled - read).abs() < 1e-4,
                "{address}: read {read}, wrote it back, read {settled}"
            );
        }
        // And the dial reaches both ends: a control whose two extremes are the
        // same value is a control that does nothing.
        let (mut low, mut high) = (patch.clone(), patch.clone());
        patch_params::set(&mut low, address, 0.0);
        patch_params::set(&mut high, address, 1.0);
        assert!(
            (patch_params::value(&low, address).unwrap()
                - patch_params::value(&high, address).unwrap())
            .abs()
                > 1e-4,
            "{address} reads the same at both ends of its travel"
        );
    }
}

#[test]
fn an_address_this_build_does_not_know_changes_nothing_and_is_not_an_error() {
    let mut patch = flopsynth::flopsynth_init();
    let before = patch.clone();
    // INVARIANT 7: a project naming a parameter a later build dropped has to
    // open rather than refuse.
    assert!(!patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/warble",
        0.5
    ));
    assert!(!patch_params::set(
        &mut patch,
        "patch/layer[9]/synth/position",
        0.5
    ));
    assert!(!patch_params::set(&mut patch, "patch/macro[7]", 0.5));
    assert!(patch_params::value(&patch, "patch/layer[0]/synth/warble").is_none());
    assert_eq!(patch, before);
}

/// The noise layer has no table, no position and no warp, and the panel is
/// built from the patch — so it must not offer them.
#[test]
fn the_noise_layer_offers_only_the_controls_it_has() {
    let patch = flopsynth::flopsynth_init();
    let addresses = flopsynth::addresses(&patch);
    assert!(addresses.contains(&"patch/layer[4]/synth/noise_colour".to_string()));
    for missing in [
        "patch/layer[4]/synth/table",
        "patch/layer[4]/synth/position",
        "patch/layer[4]/synth/warp",
    ] {
        assert!(
            !addresses.contains(&missing.to_string()),
            "{missing} is offered on the noise layer, which has no such control"
        );
    }
    // And the table address is refused rather than silently turning the noise
    // layer into an oscillator.
    let mut copy = patch.clone();
    assert!(!patch_params::set(
        &mut copy,
        "patch/layer[4]/synth/table",
        0.5
    ));
}

#[test]
fn the_destinations_the_matrix_offers_are_the_ones_this_patch_has() {
    let patch = flopsynth::flopsynth_init();
    let destinations = flopsynth::destinations(&patch);
    assert!(!destinations.is_empty());
    for (_, label) in &destinations {
        assert!(!label.is_empty());
    }
    // Every label distinct, or the matrix's drop-down has two rows that read
    // the same and do different things.
    let mut labels: Vec<&str> = destinations.iter().map(|(_, l)| l.as_str()).collect();
    let total = labels.len();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), total, "two destinations share a label");

    // The noise layer has no position or warp to offer.
    use fontelle_core::ModDest;
    assert!(
        destinations
            .iter()
            .any(|(d, _)| *d == ModDest::OscPosition(0))
    );
    assert!(
        !destinations
            .iter()
            .any(|(d, _)| *d == ModDest::OscPosition(4))
    );
    assert!(destinations.iter().any(|(d, _)| *d == ModDest::Amp));
}

/// §6's "the first thing somebody turns up does what they expect": the filter
/// envelope's route is already there, at depth zero, so turning one knob opens
/// the filter.
#[test]
fn the_filter_envelope_is_already_routed_at_depth_zero() {
    let patch = flopsynth::flopsynth_init();
    use fontelle_core::{ModDest, ModSource};
    let route = patch
        .mod_matrix
        .routes
        .iter()
        .find(|r| r.destination == ModDest::FilterCutoff(0))
        .expect("env 2 to filter 1 cutoff is in the Init patch");
    assert_eq!(route.source, ModSource::Envelope(1));
    assert_eq!(route.depth, 0.0);

    // And it is addressable, so turning it up is a knob rather than a matrix
    // row somebody has to know to add.
    assert!(
        flopsynth::addresses(&patch).contains(&"patch/mod[0]/depth".to_string()),
        "the depth of the route the Init patch wrote has to be a knob"
    );
}

/// A route's depth is a **parameter**; its source, destination, curve and via
/// are structure. §2.3 draws the line between an edit that goes on the live
/// wire and one that rebuilds, and it is drawn by which of them has an address.
#[test]
fn a_routes_depth_is_addressable_and_the_rest_of_it_is_not() {
    let patch = flopsynth::flopsynth_init();
    let addresses = flopsynth::addresses(&patch);
    assert!(addresses.contains(&"patch/mod[0]/depth".to_string()));
    for structure in ["source", "destination", "curve", "via"] {
        assert!(
            !addresses.contains(&format!("patch/mod[0]/{structure}")),
            "a route's {structure} is structure, not a parameter"
        );
    }
}

/// A layer's table is the one parameter that cannot go on the live wire:
/// choosing one means resolving it, and resolving one locks the bank and may
/// build half a megabyte.
#[test]
fn a_layers_table_is_the_one_parameter_that_has_to_rebuild() {
    assert!(!Sampler::is_live_param("patch/layer[0]/synth/table"));
    for live in [
        "patch/layer[0]/synth/position",
        "patch/filter[0]/cutoff",
        "patch/macro[0]",
        "patch/mod[0]/depth",
    ] {
        assert!(Sampler::is_live_param(live), "{live} should be live");
    }
}

#[test]
fn every_wavetable_can_be_chosen_by_address_and_read_back() {
    let mut patch = flopsynth::flopsynth_init();
    for (index, id) in WavetableId::ALL.iter().enumerate() {
        let value = patch_params::choice_value(index, WavetableId::ALL.len());
        assert!(patch_params::set(
            &mut patch,
            "patch/layer[0]/synth/table",
            value
        ));
        let Source::Synth(osc) = &patch.layers[0].source else {
            panic!("layer 0 stopped being a synth layer");
        };
        assert_eq!(
            osc.source,
            SynthSource::Table(*id),
            "choosing {} by address landed on the wrong table",
            id.label()
        );
    }
}

/// A layer routed `Bypass` ignores a closed filter — which is what a sub is
/// for, and the whole reason the route is per layer.
#[test]
fn a_bypassed_layer_ignores_a_closed_filter() {
    let closed = |route: fontelle_dsp::FilterRoute| {
        let mut patch = flopsynth::flopsynth_init();
        // Only the sub, and shut the filter right down.
        patch.layers[0].gain_db = fontelle_core::SILENT_DB;
        patch.layers[3].gain_db = -6.0;
        let Source::Synth(osc) = &mut patch.layers[3].source else {
            unreachable!()
        };
        osc.filter_route = route;
        patch.filters[0].cutoff_hz = 60.0;
        patch.filters[0].enabled = true;
        rms(&render(patch, 60, 0.5))
    };
    let bypassed = closed(fontelle_dsp::FilterRoute::Bypass);
    let filtered = closed(fontelle_dsp::FilterRoute::Serial);
    assert!(
        bypassed > filtered * 2.0,
        "a bypassed layer must not be filtered: {bypassed} against {filtered}"
    );
}

/// The reverse layer walk, heard: an oscillator naming a **later** layer as
/// its FM modulator gets that layer's sample from *this* frame, so FM is
/// audible with no second pass and no one-sample delay.
#[test]
fn fm_from_a_later_layer_is_audible() {
    let mut patch = flopsynth::flopsynth_init();
    let Source::Synth(a) = &mut patch.layers[0].source else {
        unreachable!()
    };
    a.source = SynthSource::Table(WavetableId::Sine);
    a.warp = fontelle_dsp::WarpMode::Fm;
    a.warp_amount = 0.7;
    a.modulator = Some(1);
    let Source::Synth(b) = &mut patch.layers[1].source else {
        unreachable!()
    };
    b.source = SynthSource::Table(WavetableId::Sine);
    b.semitones = 12;
    // **A pure modulator**: B is at the floor, so nothing of it reaches the
    // mix and all of it reaches A. A modulator's level is what it contributes
    // to what you hear, not whether it modulates — which is the whole setup
    // "FM Growl" and "EP Tine" are built on.
    patch.layers[1].gain_db = fontelle_core::SILENT_DB;

    let with_fm = render(patch.clone(), 60, 0.5);

    let mut without = patch.clone();
    let Source::Synth(a) = &mut without.layers[0].source else {
        unreachable!()
    };
    a.warp_amount = 0.0;
    let plain = render(without, 60, 0.5);

    // FM adds sidebands, which shows as a brighter spectrum. A crude but
    // unambiguous measure: the mean absolute first difference.
    let brightness =
        |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / s.len() as f32;
    assert!(
        brightness(&with_fm) > brightness(&plain) * 1.3,
        "FM at 0.7 has to be audible: {} against {}",
        brightness(&with_fm),
        brightness(&plain)
    );
}

/// `ModSource::Random` reads a **different** value per note and the same
/// values for the same sequence of notes — which is what makes it usable and
/// what makes it testable.
#[test]
fn random_differs_per_note_and_repeats_for_the_same_sequence() {
    use fontelle_core::{Curve, ModDest, ModRoute, ModSource};
    let mut patch = flopsynth::flopsynth_init();
    patch.mod_matrix.routes.push(ModRoute {
        source: ModSource::Random,
        destination: ModDest::LayerPitch(0),
        depth: 1.0,
        curve: Curve::Linear,
        via: None,
        invert: false,
    });

    let sequence = || {
        let store = SampleStore::new();
        let mut sampler = Sampler::new(patch.clone());
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });
        let mut runs = Vec::new();
        for _ in 0..3 {
            sampler.trigger(NoteTrigger::new(60, 100));
            let mut left = vec![0.0f32; 2_048];
            let mut right = vec![0.0f32; 2_048];
            {
                let (l, r) = (&mut left[..], &mut right[..]);
                sampler.render(&store, &mut [l, r]);
            }
            sampler.release_all();
            let mut settle = vec![0.0f32; 8_192];
            let mut settle_r = vec![0.0f32; 8_192];
            {
                let (l, r) = (&mut settle[..], &mut settle_r[..]);
                sampler.render(&store, &mut [l, r]);
            }
            runs.push(left);
        }
        runs
    };

    let first = sequence();
    assert!(
        first[0] != first[1] || first[1] != first[2],
        "three notes must not all draw the same random value"
    );
    let again = sequence();
    assert_eq!(
        first, again,
        "the same sequence of notes has to draw the same random values, or \
         nothing about this is measurable"
    );
}

#[test]
fn the_patch_trim_moves_the_whole_instrument_and_nothing_else() {
    let quiet = {
        let mut patch = flopsynth::flopsynth_init();
        patch.output_db = -12.0;
        rms(&render(patch, 60, 0.5))
    };
    let plain = rms(&render(flopsynth::flopsynth_init(), 60, 0.5));
    let ratio = 20.0 * (quiet / plain).log10();
    assert!(
        (ratio + 12.0).abs() < 0.5,
        "a −12 dB trim has to be −12 dB; measured {ratio}"
    );
}

/// A knob and the destination that modulates it are **the same control**, and
/// this is the join between them: drag-to-assign lands on a knob, and what it
/// has to write into the matrix is the `ModDest` that moves that knob.
///
/// The test that matters is the one below it — that every destination's
/// address is a control the panel actually draws. Two lists that name the same
/// thing and drift apart is the defect this file has caught four times.
#[test]
fn every_destination_names_the_control_it_moves() {
    let patch = flopsynth::flopsynth_init();
    let addresses = flopsynth::addresses(&patch);
    for (dest, label) in flopsynth::destinations(&patch) {
        let Some(address) = flopsynth::dest_address(dest) else {
            // A destination with no knob of its own is allowed — the amp is
            // the level the envelope already owns — but it has to be a
            // deliberate `None` rather than a wrong string.
            assert_eq!(dest, fontelle_core::ModDest::Amp, "{label} has no address");
            continue;
        };
        assert!(
            addresses.contains(&address),
            "{label} says it moves {address}, which the panel does not draw"
        );
    }
}

#[test]
fn a_control_that_nothing_can_modulate_has_no_destination() {
    // The output trim and the voice mode are not per-voice values, so they are
    // not destinations — and drag-to-assign has to be able to say so, or a
    // source dropped on one would light up and then do nothing.
    let patch = flopsynth::flopsynth_init();
    let destinations = flopsynth::destinations(&patch);
    let has = |address: &str| {
        destinations
            .iter()
            .any(|(dest, _)| flopsynth::dest_address(*dest).as_deref() == Some(address))
    };
    assert!(has("patch/filter[0]/cutoff"));
    assert!(!has("patch/output"));
    assert!(!has("patch/voice/mode"));
}

/// A layer nobody can hear and nobody reads costs nothing to render.
///
/// The Init patch is five layers with **one** of them up: the other four sit
/// at the floor waiting to be turned on. Rendering them anyway is four table
/// reads, four unison stacks and four filter feeds per sample for silence —
/// which is most of what a Flopsynth voice was spending (`docs/flopsynth-plan.md`
/// §10).
///
/// The rule has an exception and this test is mostly about the exception: a
/// layer at the floor that another layer **modulates with** is still rendered,
/// because a modulator's level is how much of it you hear and not whether it
/// modulates. `fm_from_a_later_layer_is_audible` is the other half of that
/// claim; this one says the skip does not change the sound.
#[test]
fn a_silent_layer_nobody_reads_changes_nothing() {
    let patch = flopsynth::flopsynth_init();
    let with_silent = render(patch.clone(), 60, 0.8);

    // The same patch with the silent layers taken out by hand. If the skip is
    // right, the two are the same audio to the last bit — a layer at the floor
    // contributes exactly nothing, so removing it is not an approximation.
    let mut trimmed = patch.clone();
    trimmed
        .layers
        .retain(|layer| layer.gain_db > fontelle_core::SILENT_DB);
    assert!(
        trimmed.layers.len() < patch.layers.len(),
        "the Init patch should have layers at the floor"
    );
    let without = render(trimmed, 60, 0.8);

    assert_eq!(with_silent.len(), without.len());
    for (index, (a, b)) in with_silent.iter().zip(without.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-6,
            "sample {index} differs: {a} against {b}"
        );
    }
}
