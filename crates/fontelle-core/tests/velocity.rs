//! The velocity curve and the velocity window (`docs/flopsynth-next.md`
//! §4.2): how hard a note was played becomes a gain through the patch's
//! own curve rather than SF2's square, and a layer plays only inside its
//! window, fading in and out at its edges — the sampled grand's two
//! layers as the ordinary case.

use fontelle_core::flopsynth::{LayerRole, flopsynth_init};
use fontelle_core::{
    NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source, VelocityCurve, patch_params,
    velocity_gain,
};
use fontelle_dsp::{SynthSource, WavetableId};

const SR: f32 = 48_000.0;

fn a_sine() -> Patch {
    let mut patch = flopsynth_init();
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        let Source::Synth(osc) = &mut layer.source else {
            continue;
        };
        if index == LayerRole::OscA as usize {
            osc.source = SynthSource::Table(WavetableId::SubSine);
            osc.unison.voices = 1;
            layer.gain_db = -6.0;
        } else {
            layer.gain_db = -120.0;
        }
    }
    for slot in &mut patch.filters {
        slot.enabled = false;
    }
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].decay_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch.mod_matrix.routes.clear();
    patch
}

/// The steady level of C4 at `velocity`.
fn level(patch: Patch, velocity: u8) -> f32 {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(60, velocity));
    let mut l = vec![0.0f32; 4800];
    let mut r = vec![0.0f32; 4800];
    sampler.render(&store, &mut [&mut l, &mut r]);
    l[2400..].iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn the_curves_are_what_they_say() {
    // Square is what every patch had: SF2's default, velocity 64 about
    // twelve decibels down.
    assert_eq!(VelocityCurve::default(), VelocityCurve::Square);
    let half = 64.0 / 127.0;
    assert!((velocity_gain(64, VelocityCurve::Square) - half * half).abs() < 1e-6);
    assert!((velocity_gain(64, VelocityCurve::Linear) - half).abs() < 1e-6);
    assert!(
        velocity_gain(64, VelocityCurve::Soft) > half,
        "soft is louder low down"
    );
    assert!(
        velocity_gain(64, VelocityCurve::Hard) < half * half,
        "hard is quieter"
    );
    for curve in VelocityCurve::ALL {
        assert_eq!(
            velocity_gain(0, curve),
            0.0,
            "{curve:?}: nought is a note-off"
        );
        assert!(
            (velocity_gain(127, curve) - 1.0).abs() < 1e-6,
            "{curve:?} tops at one"
        );
        let mut last = 0.0;
        for v in 1..=127 {
            let g = velocity_gain(v, curve);
            assert!(g >= last, "{curve:?} never goes down: {v}");
            last = g;
        }
    }
    // Custom: four points, the gains at velocities 32, 64, 96 and 127,
    // straight lines between and from nought.
    let custom = VelocityCurve::Custom([0.5, 0.6, 0.7, 1.0]);
    assert!((velocity_gain(32, custom) - 0.5).abs() < 1e-6);
    assert!((velocity_gain(64, custom) - 0.6).abs() < 1e-6);
    assert!((velocity_gain(16, custom) - 0.25).abs() < 1e-6);
    assert!((velocity_gain(80, custom) - 0.65).abs() < 1e-6);
}

#[test]
fn the_patch_plays_its_own_curve() {
    let base = a_sine();
    let square = level(base.clone(), 64) / level(base.clone(), 127);
    let mut linear = base.clone();
    linear.voice_config.velocity_curve = VelocityCurve::Linear;
    let linear = level(linear, 64) / level(base.clone(), 127);
    assert!(
        (square - 0.254).abs() < 0.02 && (linear - 0.504).abs() < 0.02,
        "square {square:.3}, linear {linear:.3}"
    );
    // Addresses: the chooser, and the four custom points.
    let mut patch = base;
    let addresses = fontelle_core::flopsynth::addresses(&patch);
    for address in [
        "patch/voice/velocity_curve",
        "patch/voice/velocity_point[0]",
        "patch/voice/velocity_point[3]",
    ] {
        assert!(addresses.iter().any(|a| a == address), "no {address}");
        assert!(patch_params::value(&patch, address).is_some());
    }
    assert!(patch_params::set(
        &mut patch,
        "patch/voice/velocity_curve",
        1.0
    ));
    assert!(matches!(
        patch.voice_config.velocity_curve,
        VelocityCurve::Custom(_)
    ));
    assert!(patch_params::set(
        &mut patch,
        "patch/voice/velocity_point[1]",
        0.9
    ));
    let VelocityCurve::Custom(points) = patch.voice_config.velocity_curve else {
        unreachable!()
    };
    assert!((points[1] - 0.9).abs() < 1e-6);
    // The file: absent at the default, kept when set.
    let data = a_sine().to_data(&Default::default()).unwrap();
    assert!(!data.body.to_string().contains("velocity_curve"));
    let data = patch.to_data(&Default::default()).unwrap();
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(
        back.voice_config.velocity_curve,
        patch.voice_config.velocity_curve
    );
}

/// A layer's velocity window has a fade at its edges: inside the fade the
/// layer comes in over `vel_fade` velocities rather than switching on,
/// which is what lets two recordings cross over.
#[test]
fn a_layers_window_fades_at_its_edges() {
    let mut patch = a_sine();
    let a = LayerRole::OscA as usize;
    patch.layers[a].vel_range = (60, 127);
    patch.layers[a].playback.vel_fade = 20;
    let full = level(patch.clone(), 127);
    // Below the window: nothing. At its edge: nothing yet. Inside the
    // fade: on the way. Past the fade: the curve alone.
    assert_eq!(level(patch.clone(), 59), 0.0);
    let edge = level(patch.clone(), 60) / full;
    let inside = level(patch.clone(), 70) / full;
    let past = level(patch.clone(), 80) / full;
    let curve_at = |v: u8| velocity_gain(v, VelocityCurve::Square);
    assert!(edge < 0.05, "at the edge: {edge:.3}");
    assert!(
        inside > 0.3 * curve_at(70) && inside < 0.7 * curve_at(70),
        "halfway through the fade: {inside:.3} against {:.3}",
        curve_at(70)
    );
    assert!(
        (past - curve_at(80)).abs() < 0.02,
        "past the fade the window does nothing: {past:.3} against {:.3}",
        curve_at(80)
    );
    // The same at the top edge, and the addresses on the window.
    patch.layers[a].vel_range = (0, 100);
    let top = level(patch.clone(), 100) / level(patch.clone(), 60);
    let below = level(patch.clone(), 80) / level(patch.clone(), 60);
    assert!(
        top < 0.1 && below > 1.0,
        "the top fades too: {top:.3} / {below:.3}"
    );
    let addresses = fontelle_core::flopsynth::addresses(&patch);
    for tail in ["vel_low", "vel_high", "vel_fade"] {
        let address = format!("patch/layer[{a}]/{tail}");
        assert!(addresses.contains(&address), "no {address}");
        assert!(patch_params::value(&patch, &address).is_some());
    }
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/vel_fade",
        0.5
    ));
    assert_eq!(patch.layers[0].playback.vel_fade, 64);
    let data = a_sine().to_data(&Default::default()).unwrap();
    assert!(
        !data.body.to_string().contains("vel_fade"),
        "absent at nought"
    );
}
