//! A built-in instrument's note starts and stops on its own sample, not at
//! the top of the block it falls in.
//!
//! Found by the Analyze engine work: `SamplerNode::process` applied every
//! event of a block before rendering any of it, so a note whose event sat at
//! offset `k` of a 128-frame block sounded from offset 0 — up to a block
//! early (≈2.7 ms at 48 kHz per graph block, and the whole device buffer's
//! worth of jitter across a 512 buffer), which smears tight drums. Every
//! built-in instrument — Flopsynth, the Drum Machine, Osc3, a soundfont — is
//! a `Sampler` in this node, so this is all of them.

use std::sync::Arc;

use fontelle_core::{Patch, SampleStore, Sampler};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, RtGuardAllocator, SamplerNode, TransportSnapshot,
    TransportState,
};
use fontelle_types::{EventPayload, NodeId, TimedEvent};

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const SR: f32 = 48_000.0;

fn note(key: u8, sample: i64, on: bool) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: if on {
            EventPayload::NoteOn {
                key,
                velocity: 110,
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

fn patches() -> Vec<(&'static str, Patch)> {
    vec![
        ("basic synth", Patch::basic_synth()),
        ("Flopsynth", fontelle_core::flopsynth::flopsynth_init()),
    ]
}

/// Renders `blocks` blocks of `block` frames of `patch`, with `events` placed
/// at their own samples, and returns the left channel.
fn render(patch: &Patch, block: usize, blocks: usize, events: &[TimedEvent]) -> Vec<f32> {
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch.clone()), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: block as u32,
    });
    let mut out = Vec::with_capacity(block * blocks);
    let mut left = vec![0.0f32; block];
    let mut right = vec![0.0f32; block];
    for index in 0..blocks {
        let at = (index * block) as i64;
        let here: Vec<TimedEvent> = events
            .iter()
            .filter(|e| e.sample >= at && e.sample < at + block as i64)
            .cloned()
            .collect();
        left.fill(0.0);
        right.fill(0.0);
        let mut outputs: [&mut [f32]; 2] = [&mut left[..], &mut right[..]];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: &here,
            live_events: &[],
            audio: &[],
            node: NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: at,
                bpm: 120.0,
                ..Default::default()
            },
            sample_range: at..at + block as i64,
        };
        node.process(&mut ctx);
        out.extend_from_slice(&left);
    }
    out
}

fn onset(samples: &[f32]) -> Option<usize> {
    samples.iter().position(|s| s.abs() > 1e-6)
}

#[test]
fn a_note_on_at_offset_k_starts_at_k() {
    for (name, patch) in patches() {
        for block in [64usize, 128, 256] {
            for k in [0usize, 1, 37, block / 2, block - 1] {
                // In the second block, so the first is a block of silence.
                let at = block + k;
                let out = render(&patch, block, 4, &[note(60, at as i64, true)]);
                let start = onset(&out).unwrap_or_else(|| panic!("{name}: silent"));
                assert!(
                    start >= at && start <= at + 2,
                    "{name}, block {block}, offset {k}: sounded at {start}, the event is at {at}"
                );
            }
        }
    }
}

#[test]
fn a_note_off_at_offset_k_leaves_the_note_alone_until_k() {
    for (name, patch) in patches() {
        for block in [64usize, 128] {
            for k in [1usize, 37, block - 1] {
                let off = 6 * block + k;
                let held = render(&patch, block, 8, &[note(60, 0, true)]);
                let released = render(
                    &patch,
                    block,
                    8,
                    &[note(60, 0, true), note(60, off as i64, false)],
                );
                assert_eq!(
                    held[..off],
                    released[..off],
                    "{name}, block {block}, offset {k}: the note changed before its note-off"
                );
            }
        }
    }
}

/// Events at offset 0 are what they always were: the same render whether a
/// block is cut at them or not — the timing is the only thing that moved.
#[test]
fn a_note_at_the_top_of_a_block_renders_as_before() {
    for (name, patch) in patches() {
        let a = render(&patch, 128, 6, &[note(60, 128, true), note(60, 512, false)]);
        let b = render(&patch, 64, 12, &[note(60, 128, true), note(60, 512, false)]);
        // Two block sizes, both with the events at the top of a block.
        let close = a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4);
        assert!(close, "{name}");
    }
}

#[test]
fn splitting_a_block_at_its_events_does_not_allocate() {
    let patch = fontelle_core::flopsynth::flopsynth_init();
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch), store);
    const BLOCK: usize = 128;
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let blocks: Vec<Vec<TimedEvent>> = (0..200)
        .map(|index| {
            let at = (index * BLOCK) as i64;
            vec![
                note(48 + (index % 12) as u8, at + 5, true),
                note(60, at + 64, true),
                note(48 + ((index + 11) % 12) as u8, at + 90, false),
                note(60, at + 120, false),
            ]
        })
        .collect();
    let live = vec![note(72, 0, true)];
    fontelle_engine::mark_current_thread_rt();
    for (index, events) in blocks.iter().enumerate() {
        let at = (index * BLOCK) as i64;
        let mut outputs: [&mut [f32]; 2] = [&mut left[..], &mut right[..]];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: events,
            live_events: if index == 3 { &live } else { &[] },
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
    fontelle_engine::unmark_current_thread_rt();
}
