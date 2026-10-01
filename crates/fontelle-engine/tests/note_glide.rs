//! A note path's glide reaching a built-in instrument through the graph
//! (`docs/note-paths-plan.md`): `NoteGlide` bends the note it names, in the
//! sampler node, and leaves the rest of the chord alone.

use fontelle_core::{SampleStore, Sampler, flopsynth};
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

/// Upward zero crossings — cycles — over a stretch long enough to see a
/// whole tone (the performance-events trap: eight blocks cannot).
fn cycles(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count()
}

fn note_on(key: u8) -> EventPayload {
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
}

fn glide(key: u8, semitones: f32) -> EventPayload {
    EventPayload::NoteGlide {
        key,
        voice_context: 0,
        semitones,
        glide_samples: 0,
    }
}

#[test]
fn a_glide_bends_the_note_it_names_an_octave() {
    let plain = render(a_patch(), &[(0, note_on(48))], 100);
    let glided = render(a_patch(), &[(0, note_on(48)), (10, glide(48, 12.0))], 100);
    let before = cycles(&plain[30 * BLOCK..94 * BLOCK]) as f32;
    let after = cycles(&glided[30 * BLOCK..94 * BLOCK]) as f32;
    assert!(
        (after / before - 2.0).abs() < 0.1,
        "an octave up is twice the cycles: {before} → {after}"
    );
}

#[test]
fn a_glide_for_another_key_leaves_the_note_alone() {
    let plain = render(a_patch(), &[(0, note_on(48))], 100);
    let other = render(a_patch(), &[(0, note_on(48)), (10, glide(52, 12.0))], 100);
    assert_eq!(
        cycles(&plain[30 * BLOCK..94 * BLOCK]),
        cycles(&other[30 * BLOCK..94 * BLOCK])
    );
}
