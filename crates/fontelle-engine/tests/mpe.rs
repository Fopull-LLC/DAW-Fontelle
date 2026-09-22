//! MPE through the graph (`docs/flopsynth-next.md` §4.2, phase 5): a
//! `NoteMod` event reaches the sampler node and moves the note it names.
//! A take does not keep one — a recording is notes, and per-note
//! expression is a lane the roll cannot draw yet — the same call the
//! wheels get in `live.rs`.

use fontelle_core::{Curve, ModDest, ModRoute, ModSource, SampleStore, Sampler, flopsynth};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SamplerNode, TransportSnapshot, TransportState,
};
use fontelle_types::{EventPayload, NodeId, TimedEvent};
use std::sync::Arc;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn a_patch() -> fontelle_core::Patch {
    let mut patch = flopsynth::flopsynth_init();
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].decay_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch.mod_matrix.routes.clear();
    // Pressure is loudness, so a pressed note reads on a meter.
    patch.mod_matrix.routes.push(ModRoute {
        source: ModSource::Aftertouch,
        destination: ModDest::LayerGain(0),
        depth: -0.625,
        curve: Curve::Linear,
        via: None,
        bypass: false,
        invert: false,
    });
    patch
}

fn render(
    patch: fontelle_core::Patch,
    events_at: &[(usize, EventPayload)],
    blocks: usize,
) -> Vec<f32> {
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
        let events: Vec<TimedEvent> = events_at
            .iter()
            .filter(|(at, _)| *at == block)
            .map(|(_, payload)| TimedEvent {
                sample: (block * BLOCK) as i64,
                target: NodeId::default(),
                payload: payload.clone(),
            })
            .collect();
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
                    ..Default::default()
                },
                sample_range: at..at + BLOCK as i64,
            };
            node.process(&mut ctx);
        }
        out.extend_from_slice(&left);
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

fn note_on() -> EventPayload {
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
}

#[test]
fn a_note_mod_event_moves_the_note_it_names() {
    let plain = render(a_patch(), &[(0, note_on())], 40);
    let pressed = render(
        a_patch(),
        &[
            (0, note_on()),
            (
                20,
                EventPayload::NoteMod {
                    key: 60,
                    voice_context: 0,
                    pressure: Some(127),
                    bend: None,
                    slide: None,
                    mod_x: None,
                },
            ),
        ],
        40,
    );
    let before = peak(&plain[30 * BLOCK..]);
    let after = peak(&pressed[30 * BLOCK..]);
    assert!(
        after < before * 0.1,
        "full pressure took the note down: {before} → {after}"
    );
    // For a note nobody is playing, nothing.
    let other = render(
        a_patch(),
        &[
            (0, note_on()),
            (
                20,
                EventPayload::NoteMod {
                    key: 64,
                    voice_context: 0,
                    pressure: Some(127),
                    bend: None,
                    slide: None,
                    mod_x: None,
                },
            ),
        ],
        40,
    );
    assert!((peak(&other[30 * BLOCK..]) - before).abs() < before * 0.05);
}
