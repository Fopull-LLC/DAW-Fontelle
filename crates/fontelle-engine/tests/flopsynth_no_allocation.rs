//! INVARIANT 1, with Flopsynth on the channel
//! (`docs/flopsynth-plan.md` §1 gate 6, §9.3).
//!
//! Everything Flopsynth added to the render path is a new place for a `Vec` to
//! appear: five oscillators of eight unison voices each, a wavetable resolved
//! per layer, four filter buses through three filter slots, four LFOs with
//! their own state, a mod matrix with twenty destinations, and an effects
//! chain inside the instrument's own node. Every one of those is a fixed-size
//! array or a buffer sized in `prepare` — and this is the only kind of test
//! that can say so, because it is the only one with the guarding allocator
//! actually installed.
//!
//! Mirrors `plugin_no_allocation.rs` in shape, and runs the **heaviest** patch
//! the plan budgets for (§10): three eight-voice unison stacks, both filters,
//! every LFO turning, and a full chain.

use std::sync::Arc;

use fontelle_core::{Curve, PatchFx, SampleStore, Sampler, Source, flopsynth};
use fontelle_dsp::{FilterModel, FilterRoute, FilterSlope, SynthSource, WarpMode, WavetableId};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, RtGuardAllocator, SamplerNode, TransportSnapshot,
    TransportState,
};
use fontelle_types::{
    ChorusConfig, DelayConfig, EffectConfig, EventPayload, NodeId, ReverbConfig, TimedEvent,
};

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

/// The heaviest patch §10 budgets for, plus every path the voice has: a
/// bypassed layer, a serial pair, an FM modulator reading a later layer, both
/// filters on different models, all four LFOs turning, and a chain.
fn the_heaviest_patch() -> fontelle_core::Patch {
    use fontelle_core::{ModDest, ModRoute, ModSource};
    let mut patch = flopsynth::flopsynth_init();

    for index in 0..3 {
        let Source::Synth(osc) = &mut patch.layers[index].source else {
            unreachable!()
        };
        osc.unison.voices = 8;
        osc.unison.detune_cents = 25.0;
        osc.unison.blend = 0.9;
        osc.unison.width = 0.8;
        osc.random_phase = true;
        osc.source = SynthSource::Table(match index {
            0 => WavetableId::Choir,
            1 => WavetableId::Reese,
            _ => WavetableId::Growl,
        });
        // Every route used at least once, so no bus is untested.
        osc.filter_route = match index {
            0 => FilterRoute::Serial,
            1 => FilterRoute::F1,
            _ => FilterRoute::F2,
        };
        patch.layers[index].gain_db = -18.0;
    }
    // Oscillator A takes its FM from a later layer, which is the reverse walk.
    let Source::Synth(a) = &mut patch.layers[0].source else {
        unreachable!()
    };
    a.warp = WarpMode::Fm;
    a.warp_amount = 0.4;
    a.modulator = Some(1);

    patch.layers[3].gain_db = -18.0;
    patch.layers[4].gain_db = -30.0;

    // Two filters, two models — the ladder's `tanh` and the comb's delay line.
    patch.filters[0].model = FilterModel::Ladder;
    patch.filters[0].slope = FilterSlope::Db24;
    patch.filters[0].cutoff_hz = 1_800.0;
    patch.filters[0].resonance = 0.7;
    patch.filters[0].drive = 0.5;
    patch.filters[0].key_track = 0.4;
    patch.filters[0].character = 0.6;
    patch.filters[1].model = FilterModel::Comb;
    patch.filters[1].enabled = true;
    patch.filters[1].cutoff_hz = 220.0;
    patch.filters[1].character = 0.7;

    // Every LFO read by something, so none is skipped by `sources_in_use` —
    // which is what makes this test cover the LFO path at all.
    patch.lfos[0].sync = false;
    patch.lfos[0].rate_hz = 5.0;
    patch.lfos[1].sync = true;
    patch.lfos[1].mode = fontelle_core::LfoMode::Free;
    patch.lfos[2].wave = fontelle_types::LfoWave::SampleHold;
    patch.lfos[2].smooth = 0.4;
    patch.lfos[3].mode = fontelle_core::LfoMode::OneShot;
    patch.lfos[3].fade_s = 0.5;
    patch.lfos[3].delay_s = 0.1;

    let route = |source: ModSource, destination: ModDest, depth: f32| ModRoute {
        source,
        destination,
        depth,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    };
    patch.mod_matrix.routes.extend([
        route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.4),
        route(ModSource::Lfo(1), ModDest::OscPosition(0), 0.5),
        route(ModSource::Lfo(2), ModDest::OscWarp(0), 0.3),
        route(ModSource::Lfo(3), ModDest::Amp, 0.3),
        route(ModSource::Macro(0), ModDest::FilterCharacter(1), 0.5),
        route(ModSource::Random, ModDest::LayerPitch(2), 0.05),
        route(ModSource::NoteOnCounter, ModDest::OscUnisonDetune(1), 0.4),
        route(ModSource::Velocity, ModDest::FilterDrive(0), 0.5),
        route(ModSource::Envelope(2), ModDest::OscUnisonBlend(0), 0.3),
        route(ModSource::Envelope(3), ModDest::LfoPhase(0), 0.2),
        // The §4.2 generators, every one read, so their state is under the
        // guard too: the attractor's sub-steps, the walk's draws, the
        // follower's feed and a synced and a free sequencer.
        route(ModSource::Chaos, ModDest::OscPosition(1), 0.3),
        route(ModSource::RandomWalk, ModDest::LayerPan(2), 0.5),
        route(ModSource::EnvelopeFollower, ModDest::FilterCutoff(1), 0.3),
        route(ModSource::StepSeq(0), ModDest::LayerPitch(0), 0.05),
        route(ModSource::StepSeq(1), ModDest::OscWarp(2), 0.4),
        // And LFOs on LFOs, and a drawn shape, and a looping envelope.
        route(ModSource::Lfo(0), ModDest::LfoRate(1), 0.3),
        route(ModSource::Lfo(7), ModDest::Amp, 0.1),
        route(ModSource::Envelope(5), ModDest::FilterResonance(0), 0.2),
    ]);
    patch.sequencers[0].sync = true;
    patch.sequencers[1].sync = false;
    patch.sequencers[1].rate_hz = 7.0;
    patch.sequencers[1].smooth = 0.3;
    patch.lfos[7].shape = Some(fontelle_types::LfoShape::from_wave(
        fontelle_types::LfoWave::Triangle,
        16,
    ));
    patch.envelopes[5].loop_stages = Some((
        fontelle_dsp::EnvStage::Attack,
        fontelle_dsp::EnvStage::Decay,
    ));
    patch.macros[0].value = 0.5;

    // And the chain, full.
    let mut reverb = ReverbConfig::new();
    reverb.size = 0.8;
    reverb.mix = 0.35;
    let mut delay = DelayConfig::new();
    delay.mix = 0.25;
    for config in [
        EffectConfig::Chorus(ChorusConfig::new()),
        EffectConfig::Delay(delay),
        EffectConfig::Reverb(reverb),
    ] {
        patch.fx.push(PatchFx {
            config,
            enabled: true,
        });
    }
    // Sixteen voices of this is what §10's budget is written against.
    patch.voice_config.polyphony = 16;
    patch
}

fn a_note(key: u8, sample: i64, on: bool) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: if on {
            EventPayload::NoteOn {
                key,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            }
        } else {
            EventPayload::NoteOff {
                key,
                voice_context: 0,
            }
        },
    }
}

#[test]
fn rendering_flopsynth_does_not_allocate() {
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(the_heaviest_patch()), store);
    // Off-RT setup, which may allocate freely — including building every
    // wavetable the patch names and every effect in its chain.
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });

    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    // Sixteen voices' worth of notes, spread out so voices are allocated,
    // stolen and released mid-run rather than all at once.
    let events: Vec<Vec<TimedEvent>> = (0..600)
        .map(|block| {
            let mut out = Vec::new();
            if block % 12 == 0 {
                out.push(a_note(48 + (block % 24) as u8, 0, true));
            }
            if block % 12 == 9 {
                out.push(a_note(48 + ((block - 9) % 24) as u8, 0, false));
            }
            out
        })
        .collect();

    fontelle_engine::mark_current_thread_rt();
    for (block, block_events) in events.iter().enumerate() {
        let at = (block * BLOCK) as i64;
        let (l, r) = (&mut left[..], &mut right[..]);
        let mut outputs: [&mut [f32]; 2] = [l, r];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: block_events,
            live_events: &[],
            audio: &[],
            node: NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: at,
                bpm: 128.0,
                ..Default::default()
            },
            sample_range: at..at + BLOCK as i64,
        };
        node.process(&mut ctx);
    }
    fontelle_engine::unmark_current_thread_rt();
}

/// A parameter arriving on the **live wire** mid-note is the whole point of
/// §2.3, and it must not allocate either — an address is matched as a
/// subslice of a `&str`, never built.
#[test]
fn a_parameter_on_the_wire_does_not_allocate() {
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(the_heaviest_patch()), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });

    // Built off-RT: a `ParamAddress` owns a `String`, and cloning one on the
    // audio thread is exactly what INVARIANT 1 forbids.
    let addresses: Vec<fontelle_types::ParamAddress> = [
        "channel:1/patch/filter[0]/cutoff",
        "channel:1/patch/layer[0]/synth/position",
        "channel:1/patch/macro[0]",
        "channel:1/patch/fx[2]/mix",
        "channel:1/patch/mod[0]/depth",
    ]
    .into_iter()
    .map(fontelle_types::ParamAddress::new)
    .collect();
    let sweeps: Vec<Vec<TimedEvent>> = (0..400)
        .map(|block| {
            let mut out = Vec::new();
            if block == 0 {
                out.push(a_note(60, 0, true));
            }
            // One of each address, every block — a knob being dragged.
            for (index, address) in addresses.iter().enumerate() {
                out.push(TimedEvent {
                    sample: 0,
                    target: NodeId::default(),
                    payload: EventPayload::ParamValue {
                        target: address.clone(),
                        value: ((block + index) % 100) as f64 / 100.0,
                    },
                });
            }
            out
        })
        .collect();

    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    fontelle_engine::mark_current_thread_rt();
    for (block, block_events) in sweeps.iter().enumerate() {
        let at = (block * BLOCK) as i64;
        let (l, r) = (&mut left[..], &mut right[..]);
        let mut outputs: [&mut [f32]; 2] = [l, r];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: block_events,
            live_events: &[],
            audio: &[],
            node: NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: at,
                bpm: 128.0,
                ..Default::default()
            },
            sample_range: at..at + BLOCK as i64,
        };
        node.process(&mut ctx);
    }
    fontelle_engine::unmark_current_thread_rt();
}
