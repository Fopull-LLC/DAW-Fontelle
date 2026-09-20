//! Drawn LFO shapes and LFOs on LFOs (`docs/flopsynth-next.md` §4.2): the
//! voice plays `Lfo.shape` in place of the wave when there is one, a
//! two-point ramp is `SawUp`, a tension bends a segment by the law the
//! envelope shapes use, and one LFO can drive another's rate.

use fontelle_core::flopsynth::{LayerRole, flopsynth_init};
use fontelle_core::{
    Curve, ModDest, ModRoute, ModSource, NoteTrigger, Patch, PrepareContext, RenderClock,
    SampleStore, Sampler, Source,
};
use fontelle_dsp::{SynthSource, WavetableId, shape_progress};
use fontelle_types::{LfoPoint, LfoShape, LfoShapeMode, LfoWave};

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn route(source: ModSource, destination: ModDest, depth: f32) -> ModRoute {
    ModRoute {
        source,
        destination,
        depth,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    }
}

/// The Init with only A up, a sine, no filter, an instant amp envelope.
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

/// C6 at velocity 100 for `seconds`, in the engine's blocks with the
/// clock moving, the left channel.
fn render(patch: Patch, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    sampler.trigger(NoteTrigger::new(84, 100));
    let frames = (seconds * SR) as usize;
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        sampler.set_clock(RenderClock {
            bpm: 120.0,
            position_sample: out.len() as u64,
        });
        let mut l = [0.0f32; BLOCK];
        let mut r = [0.0f32; BLOCK];
        sampler.render(&store, &mut [&mut l, &mut r]);
        out.extend_from_slice(&l);
    }
    out.truncate(frames);
    out
}

/// The amplitude envelope: the peak of each millisecond.
fn envelope(out: &[f32]) -> Vec<f32> {
    out.chunks((SR * 0.001) as usize)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect()
}

#[test]
fn a_two_point_ramp_is_saw_up() {
    let ramp = LfoShape {
        points: vec![
            LfoPoint {
                x: 0.0,
                y: -1.0,
                tension: 0.0,
            },
            LfoPoint {
                x: 1.0,
                y: 1.0,
                tension: 0.0,
            },
        ],
        grid: 0,
        mode: LfoShapeMode::Smooth,
    };
    for i in 0..100 {
        let phase = i as f32 / 100.0;
        assert!(
            (ramp.value(phase) - LfoWave::SawUp.value(phase)).abs() < 1e-4,
            "at {phase}: {} against {}",
            ramp.value(phase),
            LfoWave::SawUp.value(phase)
        );
    }
}

#[test]
fn a_tension_bends_a_segment_by_the_envelopes_law() {
    for tension in [-1.0f32, -0.5, 0.5, 1.0] {
        let ramp = LfoShape {
            points: vec![
                LfoPoint {
                    x: 0.0,
                    y: -1.0,
                    tension,
                },
                LfoPoint {
                    x: 1.0,
                    y: 1.0,
                    tension: 0.0,
                },
            ],
            grid: 0,
            mode: LfoShapeMode::Smooth,
        };
        for i in 1..100 {
            let t = i as f32 / 100.0;
            let expected = -1.0 + 2.0 * shape_progress(t, tension);
            assert!(
                (ramp.value(t) - expected).abs() < 1e-4,
                "tension {tension} at {t}: {} against {expected}",
                ramp.value(t)
            );
        }
    }
}

/// A drawn pulse — up for the first quarter of the cycle, down for the
/// rest — on the amp at 4 Hz: the note is loud a quarter of every cycle,
/// which a sine on the same route is not.
#[test]
fn the_voice_plays_a_drawn_shape_in_place_of_the_wave() {
    let pulse = LfoShape {
        points: vec![
            LfoPoint {
                x: 0.0,
                y: 1.0,
                tension: 0.0,
            },
            LfoPoint {
                x: 0.25,
                y: -1.0,
                tension: 0.0,
            },
        ],
        grid: 0,
        mode: LfoShapeMode::Step,
    };
    let mut patch = a_sine();
    patch.lfos[0].rate_hz = 4.0;
    patch.lfos[0].sync = false;
    patch.lfos[0].wave = LfoWave::Sine;
    patch.lfos[0].shape = Some(pulse);
    // ±24 dB on the amp: loud when the pulse is up, near silence down.
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::Lfo(0), ModDest::Amp, 1.0));
    // Within a decibel of the top: a step is there a quarter of the time,
    // a sine (over ±24 dB) only while it is above 0.96 — a tenth.
    let at_top = |patch: Patch| {
        let env = envelope(&render(patch, 1.0));
        let top = env.iter().fold(0.0f32, |m, v| m.max(*v));
        let loud: Vec<bool> = env.iter().map(|v| *v > top * 0.89).collect();
        let share = loud.iter().filter(|l| **l).count() as f32 / loud.len() as f32;
        // Falling edges: the pulse starts up, so its first rise is at
        // the note's start and not an edge.
        let edges = loud.windows(2).filter(|w| w[0] && !w[1]).count();
        (share, edges)
    };
    let (share, edges) = at_top(patch.clone());
    assert!(
        (0.2..=0.32).contains(&share),
        "at the top a quarter of the time, got {share:.2}"
    );
    // Four cycles a second: four falling edges.
    assert_eq!(edges, 4, "four pulses in a second");

    // The same patch without the shape plays the sine.
    patch.lfos[0].shape = None;
    let (share, _) = at_top(patch);
    assert!(
        share < 0.15,
        "a sine is at its top a tenth of the time, got {share:.2}"
    );
}

/// LFO 1, held at its top, on LFO 2's rate: LFO 2 turns at twice the rate
/// its knob says — one LFO reads as frequency modulation of the other.
#[test]
fn an_lfo_on_another_lfos_rate_is_frequency_modulation() {
    let mut patch = a_sine();
    // LFO 1: a sine at 0 Hz parked a quarter in — a constant 1.0.
    patch.lfos[0].rate_hz = 0.0;
    patch.lfos[0].phase = 0.25;
    patch.lfos[0].sync = false;
    // LFO 2: a square at 3 Hz on the amp, ±24 dB.
    patch.lfos[1].rate_hz = 3.0;
    patch.lfos[1].sync = false;
    patch.lfos[1].mode = fontelle_core::LfoMode::Retrigger;
    patch.lfos[1].wave = LfoWave::Square;
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::Lfo(1), ModDest::Amp, 1.0));
    let pulses = |patch: Patch| {
        let env = envelope(&render(patch, 2.0));
        let top = env.iter().fold(0.0f32, |m, v| m.max(*v));
        let loud: Vec<bool> = env.iter().map(|v| *v > top * 0.5).collect();
        loud.windows(2).filter(|w| !w[0] && w[1]).count()
    };
    let plain = pulses(patch.clone());
    assert!(
        (5..=6).contains(&plain),
        "3 Hz for two seconds: {plain} pulses"
    );
    // Full depth is three octaves; a third of it is one, so twice the rate.
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::Lfo(0), ModDest::LfoRate(1), 1.0 / 3.0));
    let doubled = pulses(patch);
    assert!(
        (11..=12).contains(&doubled),
        "LFO 1 at its top doubles LFO 2's rate: {doubled} pulses in two seconds"
    );
}
