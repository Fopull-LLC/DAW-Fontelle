//! The instrument's own effects chain (`docs/flopsynth-plan.md` §2.2, §3.9).
//!
//! **A Serum preset without its chorus and reverb is not that preset**, so the
//! chain belongs to the patch and runs inside the instrument's node — after
//! the voice sum, before the channel's gain. What these tests hold is that it
//! is a *chain*: the order is audible, a bypassed slot is a wire, a patch with
//! no effects is bit-identical to the sampler alone, and the tail rings after
//! the last note is let go.

use fontelle_core::{NoteTrigger, PatchFx, SampleStore, Sampler, flopsynth};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SamplerNode, TransportSnapshot, TransportState,
};
use fontelle_types::NodeId;
use fontelle_types::{
    BitcrushConfig, DelayConfig, EffectConfig, EffectKind, EventPayload, FilterConfig,
    ReverbConfig, TimedEvent,
};
use std::sync::Arc;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn a_patch() -> fontelle_core::Patch {
    let mut patch = flopsynth::flopsynth_init();
    // A short note, so the tail is the chain's and not the envelope's.
    patch.envelopes[0].release_s = 0.01;
    patch
}

/// Renders `blocks` blocks of a node, with `notes` fired at the block index
/// each names. Returns the left channel.
fn render(patch: fontelle_core::Patch, notes: &[(usize, bool)], blocks: usize) -> Vec<f32> {
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let mut out = Vec::with_capacity(blocks * BLOCK);
    for block in 0..blocks {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let mut events: Vec<TimedEvent> = Vec::new();
        for (at, on) in notes {
            if *at != block {
                continue;
            }
            events.push(TimedEvent {
                sample: (block * BLOCK) as i64,
                target: NodeId::default(),
                payload: if *on {
                    EventPayload::NoteOn {
                        key: 60,
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
                        key: 60,
                        voice_context: 0,
                    }
                },
            });
        }
        {
            let (l, r) = (&mut left[..], &mut right[..]);
            let mut outputs: [&mut [f32]; 2] = [l, r];
            let at = (block * BLOCK) as i64;
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &events,
                live_events: &[],
                audio: &[],
                node: NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: at,
                    bpm: 120.0,
                },
                sample_range: at..at + BLOCK as i64,
            };
            node.process(&mut ctx);
        }
        out.extend_from_slice(&left);
    }
    out
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

fn slot(config: EffectConfig) -> PatchFx {
    PatchFx {
        config,
        enabled: true,
    }
}

/// The one that matters most, because it is the one nothing else guards: a
/// patch with no effects has to be **bit-identical** to the sampler alone.
/// Otherwise every existing instrument in the program has quietly changed.
#[test]
fn a_patch_with_no_effects_is_the_sampler_alone() {
    let plain = render(a_patch(), &[(0, true)], 8);
    // The same patch, rendered through the sampler with no node around it.
    let store = SampleStore::new();
    let mut sampler = Sampler::new(a_patch());
    sampler.prepare(&fontelle_core::PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    sampler.trigger(NoteTrigger::new(60, 100));
    let mut direct = Vec::with_capacity(8 * BLOCK);
    for _ in 0..8 {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        {
            let (l, r) = (&mut left[..], &mut right[..]);
            sampler.render(&store, &mut [l, r]);
        }
        direct.extend_from_slice(&left);
    }
    for (i, (a, b)) in plain.iter().zip(&direct).enumerate() {
        assert!(
            (a - b).abs() < 1e-9,
            "an empty chain must cost nothing and change nothing; differ at {i}: \
             {a} vs {b}"
        );
    }
}

/// §2.2's tail claim: a delay's repeats are audible **after** the last
/// note-off, with the transport stopped. The idle gate measures the graph's
/// output rather than asking nodes, so nothing has to report anything for this
/// to work — but the chain does have to still be running.
#[test]
fn the_chain_rings_after_the_last_note_off() {
    let mut patch = a_patch();
    let mut delay = DelayConfig::new();
    delay.time_ms = 120.0;
    delay.sync = false;
    delay.feedback = 0.6;
    delay.mix = 0.5;
    patch.fx.push(slot(EffectConfig::Delay(delay)));

    // On at block 0, off at block 8 — a fifth of a second of note.
    let out = render(patch, &[(0, true), (8, false)], 120);
    // Well past the note's own 10 ms release, and past the first repeat.
    let after = &out[(SR * 0.15) as usize..];
    assert!(
        rms(after) > 0.0005,
        "the delay's repeats have to outlive the note; RMS after the release \
         is {}",
        rms(after)
    );

    // And the same patch without the delay is quiet by then, so what is being
    // measured is the chain and not the envelope.
    let dry = render(a_patch(), &[(0, true), (8, false)], 120);
    let dry_after = &dry[(SR * 0.15) as usize..];
    assert!(
        rms(after) > rms(dry_after) * 20.0,
        "with no delay it should already be silent: {} against {}",
        rms(dry_after),
        rms(after)
    );
}

/// Two slots run **in order**, which is the whole reason a chain is a list and
/// not a set.
///
/// The case that shows it plainly: a **sample-rate reduction** scatters
/// aliases across the whole band, and a low-pass after it takes the ones above
/// its corner back out. Put the low-pass *first* and the aliases are made
/// after it, so they survive — and the high end of the two chains is nothing
/// alike.
#[test]
fn two_slots_run_in_the_order_the_patch_lists_them() {
    let build = |crush_first: bool| {
        let mut patch = a_patch();
        let mut crush = BitcrushConfig::new();
        // Well under the note's own harmonics, so the holding is real.
        crush.rate_hz = 5_000.0;
        crush.bits = 16.0;
        crush.mix = 1.0;
        let mut filter = FilterConfig::new();
        filter.cutoff_hz = 1_200.0;
        filter.mix = 1.0;
        let (a, b) = if crush_first {
            (EffectConfig::Bitcrush(crush), EffectConfig::Filter(filter))
        } else {
            (EffectConfig::Filter(filter), EffectConfig::Bitcrush(crush))
        };
        patch.fx.push(slot(a));
        patch.fx.push(slot(b));
        render(patch, &[(0, true)], 40)
    };

    // Energy above the low-pass's corner: what the filter is supposed to have
    // removed, and what the crush puts back when it runs last.
    let high = |s: &[f32]| {
        let tail = &s[BLOCK * 8..];
        let mut total = 0.0f32;
        let mut hz = 3_000.0;
        while hz < 15_000.0 {
            let n = tail.len() as f32;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, sample) in tail.iter().enumerate() {
                let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
                let phase = std::f32::consts::TAU * hz * i as f32 / SR;
                re += sample * window * phase.cos();
                im -= sample * window * phase.sin();
            }
            total += (re * re + im * im).sqrt() / n;
            hz += 500.0;
        }
        total
    };

    let crush_then_filter = high(&build(true));
    let filter_then_crush = high(&build(false));
    assert!(
        filter_then_crush > crush_then_filter * 3.0,
        "the order has to be audible: crushing then filtering left {crush_then_filter} \
         above the corner, filtering then crushing left {filter_then_crush}"
    );
}

#[test]
fn a_bypassed_slot_is_a_wire() {
    let mut patch = a_patch();
    patch.fx.push(PatchFx {
        config: EffectConfig::Reverb(ReverbConfig::new()),
        enabled: false,
    });
    let bypassed = render(patch, &[(0, true)], 16);
    let plain = render(a_patch(), &[(0, true)], 16);
    for (i, (a, b)) in bypassed.iter().zip(&plain).enumerate() {
        assert!(
            (a - b).abs() < 1e-9,
            "a switched-off slot must be exactly the absence of it; differ at \
             {i}: {a} vs {b}"
        );
    }
}

/// The mix knob at zero is the dry signal, and at one it is the effect alone —
/// and a slot's mix is addressable, which is what makes it automatable.
#[test]
fn a_slots_mix_blends_and_is_addressable() {
    let with_mix = |mix: f32| {
        let mut patch = a_patch();
        let mut reverb = ReverbConfig::new();
        reverb.mix = mix;
        reverb.size = 0.8;
        patch.fx.push(slot(EffectConfig::Reverb(reverb)));
        render(patch, &[(0, true), (8, false)], 80)
    };
    let dry = with_mix(0.0);
    let wet = with_mix(1.0);
    let plain = render(a_patch(), &[(0, true), (8, false)], 80);
    for (i, (a, b)) in dry.iter().zip(&plain).enumerate() {
        assert!(
            (a - b).abs() < 1e-6,
            "mix 0 is the signal that went in; differ at {i}: {a} vs {b}"
        );
    }
    let tail = (SR * 0.15) as usize;
    assert!(
        rms(&wet[tail..]) > rms(&plain[tail..]) * 5.0,
        "mix 1 is the tail alone, and there has to be one"
    );

    // And the address the panel offers is the address `patch_params` accepts.
    let mut patch = a_patch();
    patch
        .fx
        .push(slot(EffectConfig::Reverb(ReverbConfig::new())));
    assert!(fontelle_core::patch_params::set(
        &mut patch,
        "patch/fx[0]/mix",
        0.25
    ));
    let read = fontelle_core::patch_params::value(&patch, "patch/fx[0]/mix").expect("reads");
    assert!((read - 0.25).abs() < 0.01);
    assert!(fontelle_core::patch_params::set(
        &mut patch,
        "patch/fx[0]/enabled",
        0.0
    ));
    assert!(!patch.fx[0].enabled);
}

/// §3.9's list: only zero-latency kinds are offered, so the node's own latency
/// stays zero and nothing downstream has to be compensated. A plugin
/// *instrument* with latency is the one case §5.5 deliberately does not
/// compensate, and Flopsynth is an instrument.
#[test]
fn the_chain_adds_no_latency() {
    let mut patch = a_patch();
    for kind in [
        EffectKind::Chorus,
        EffectKind::Delay,
        EffectKind::Reverb,
        EffectKind::Filter,
    ] {
        patch.fx.push(slot(EffectConfig::new(kind)));
    }
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    assert_eq!(
        node.latency_samples(),
        0,
        "an instrument's chain has to be latency-free, because an instrument's \
         latency is the one thing §5.5 does not line up"
    );
}

/// The gate every preset in the bank has to pass, checked here because the
/// chain is in the node and the bank is in core: a four-note chord at full
/// velocity through the Init patch and its chain stays inside full scale.
#[test]
fn a_full_chord_through_a_chain_stays_inside_full_scale() {
    let mut patch = a_patch();
    patch.envelopes[0].release_s = 0.3;
    let mut reverb = ReverbConfig::new();
    reverb.size = 0.7;
    reverb.mix = 0.35;
    patch.fx.push(slot(EffectConfig::Chorus(
        fontelle_types::ChorusConfig::new(),
    )));
    patch.fx.push(slot(EffectConfig::Reverb(reverb)));

    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let mut loudest = 0.0f32;
    for block in 0..200 {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let events: Vec<TimedEvent> = if block == 0 {
            [60u8, 64, 67, 72]
                .into_iter()
                .map(|key| TimedEvent {
                    sample: 0,
                    target: NodeId::default(),
                    payload: EventPayload::NoteOn {
                        key,
                        velocity: 127,
                        pan: 0,
                        fine_pitch: 0,
                        release: 0,
                        mod_x: 0,
                        mod_y: 0,
                        voice_context: 0,
                    },
                })
                .collect()
        } else {
            Vec::new()
        };
        {
            let (l, r) = (&mut left[..], &mut right[..]);
            let mut outputs: [&mut [f32]; 2] = [l, r];
            let at = (block * BLOCK) as i64;
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &events,
                live_events: &[],
                audio: &[],
                node: NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: at,
                    bpm: 120.0,
                },
                sample_range: at..at + BLOCK as i64,
            };
            node.process(&mut ctx);
        }
        loudest = loudest.max(peak(&left)).max(peak(&right));
    }
    assert!(
        loudest <= 0.98,
        "a chord through the chain peaked at {loudest}"
    );
    assert!(loudest > 0.05, "and it has to sound: {loudest}");
}

/// §3.6: eight slots, and a level meter per slot — the peak of the block
/// after that slot ran, on the node's `VoiceMeter` beside the voice count,
/// so the rack's rows can show what each slot is putting out. A slot after
/// the last one filled, and a bypassed slot, read nothing.
#[test]
fn the_chain_holds_eight_slots_and_meters_each_one() {
    use fontelle_engine::VoiceMeter;
    assert_eq!(fontelle_core::MAX_PATCH_FX, 8);
    let mut patch = a_patch();
    patch.fx = vec![
        slot(EffectConfig::Filter(FilterConfig::default())),
        PatchFx {
            config: EffectConfig::Delay(DelayConfig::default()),
            enabled: false,
        },
    ];
    let store = Arc::new(SampleStore::new());
    let meter = Arc::new(VoiceMeter::new());
    let mut node = SamplerNode::new(Sampler::new(patch), store).with_meter(meter.clone());
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let levels = meter.fx_levels();
    assert_eq!(levels.len(), fontelle_core::MAX_PATCH_FX);
    assert!(
        levels.iter().all(|l| *l == 0.0),
        "silent before anything sounds"
    );
    // A note through the chain: the filter's slot reads its output's peak,
    // the bypassed delay reads nothing, and the six empty slots nothing.
    for block in 0..8 {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let events = if block == 0 {
            vec![TimedEvent {
                sample: 0,
                target: NodeId::default(),
                payload: EventPayload::NoteOn {
                    key: 60,
                    velocity: 100,
                    pan: 0,
                    fine_pitch: 0,
                    release: 0,
                    mod_x: 0,
                    mod_y: 0,
                    voice_context: 0,
                },
            }]
        } else {
            Vec::new()
        };
        let (l, r) = (&mut left[..], &mut right[..]);
        let mut outputs: [&mut [f32]; 2] = [l, r];
        let at = (block * BLOCK) as i64;
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: &events,
            live_events: &[],
            audio: &[],
            node: NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: at,
                bpm: 120.0,
            },
            sample_range: at..at + BLOCK as i64,
        };
        node.process(&mut ctx);
    }
    let levels = meter.fx_levels();
    assert!(
        levels[0] > 0.01,
        "the filter's slot is sounding: {levels:?}"
    );
    assert_eq!(levels[1], 0.0, "a bypassed slot reads nothing");
    assert!(levels[2..].iter().all(|l| *l == 0.0));
}

/// `ModDest::FxParam` (`docs/flopsynth-next.md` §4.2): a route to one of an
/// effect's own parameters, by slot and by the parameter's index in the
/// effect's spec. A macro on the delay's feedback changes the tail — the
/// instrument-wide sources reach the chain — and the destination has a
/// name in the window and an address on the wire.
#[test]
fn a_macro_on_the_delays_feedback_changes_the_tail() {
    use fontelle_core::{Curve, ModDest, ModRoute, ModSource};
    let mut patch = a_patch();
    let mut delay = DelayConfig::new();
    delay.time_ms = 120.0;
    delay.sync = false;
    delay.feedback = 0.0;
    delay.mix = 0.5;
    patch.fx.push(slot(EffectConfig::Delay(delay)));
    let feedback = EffectConfig::Delay(delay)
        .specs()
        .iter()
        .position(|spec| spec.id == "feedback")
        .expect("the delay has a feedback") as u8;
    // The destination is offered, named, and addressed to the slot's knob.
    let destinations = flopsynth::destinations(&patch);
    let (dest, label) = destinations
        .iter()
        .find(|(d, _)| *d == ModDest::FxParam(0, feedback))
        .expect("the delay's feedback is a destination");
    assert!(
        label.to_lowercase().contains("feedback") && label.contains("FX 1"),
        "{label}"
    );
    assert_eq!(
        flopsynth::dest_address_in(&patch, *dest).as_deref(),
        Some("patch/fx[0]/feedback")
    );
    // Macro 1 all the way up, routed at 0.8: the feedback goes from nought
    // to 80 %, and the tail from one repeat to many.
    patch.mod_matrix.routes.push(ModRoute {
        source: ModSource::Macro(0),
        destination: ModDest::FxParam(0, feedback),
        depth: 0.8,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    });
    let quiet = render(patch.clone(), &[(0, true), (8, false)], 200);
    patch.macros[0].value = 1.0;
    let ringing = render(patch.clone(), &[(0, true), (8, false)], 200);
    // Half a second in: three repeats at 120 ms have gone by, and with no
    // feedback there is nothing left.
    let late = |out: &[f32]| rms(&out[(SR * 0.45) as usize..(SR * 0.53) as usize]);
    assert!(
        late(&ringing) > late(&quiet) * 10.0,
        "the macro feeds the delay back: {} against {}",
        late(&ringing),
        late(&quiet)
    );
    // The patch's own knob is not moved by the route: it is a modulation.
    let EffectConfig::Delay(stored) = patch.fx[0].config else {
        unreachable!()
    };
    assert_eq!(stored.feedback, 0.0);
}
