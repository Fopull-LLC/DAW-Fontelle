//! The test that should have existed from the start: does the *actual* render
//! path (`CompiledGraph::process_block` → `SamplerNode::process` →
//! `Sampler::render` → `Voice::render`) really hold INVARIANT 1? Every unit
//! test elsewhere in this workspace exercises this path under the plain
//! system allocator, which can't tell you that — only a binary with
//! `RtGuardAllocator` actually installed can. Its own binary (every
//! `tests/*.rs` file is), so it's safe to install one here.
//!
//! This is what caught the real bug on real hardware
//! (`cargo run -p fontelle-app -- --play-sf2 <real file>`, not the synthetic
//! `manual_audio_output` test, which never installed the guard at all): a
//! fresh `Vec<&mut [f32]>` built with `.push()` inside `process_block`,
//! reallocated every single block.

use std::sync::Arc;

use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};
use fontelle_engine::{BufferPool, CompiledGraph, RtGuardAllocator, SamplerNode, ScheduledNode};
use fontelle_types::{EventPayload, NodeId, TimedEvent};
use slotmap::Key;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn build_graph() -> CompiledGraph {
    let mut store = SampleStore::new();
    // A real-shaped, non-looping, multi-thousand-sample buffer — the same
    // shape that exposed the bug (a real SF2 layer), not the tiny looped
    // synthetic buffer `manual_audio_output.rs` uses.
    let asset = store.insert(SampleBuffer {
        data: Arc::from(vec![0.5; 21_573]),
        sample_rate: 44_100,
    });
    let disabled_filter = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let env = EnvelopeConfig {
        delay_s: 0.001,
        attack_s: 0.001,
        hold_s: 0.001,
        decay_s: 0.001,
        sustain_level: 1.0,
        release_s: 0.001,
    };
    let patch = Patch {
        layers: vec![Layer {
            source: Source::Sample { file: asset },
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 77,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Off,
                interpolation: Interpolation::Normal,
                end_offset: 21_573.0,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled_filter, disabled_filter],
        envelopes: vec![env, env],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    };

    let mut sampler = Sampler::new(patch);
    sampler.prepare(&fontelle_core::PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });

    CompiledGraph {
        schedule: vec![ScheduledNode {
            id: NodeId::null(),
            node: Box::new(SamplerNode::new(sampler, Arc::new(store))),
            input_buffers: Vec::new(),
            output_buffers: vec![0],
        }],
        buffer_pool: BufferPool::with_capacity(1, BLOCK),
    }
}

#[test]
fn process_block_does_not_allocate_across_many_real_blocks() {
    let mut graph = build_graph();
    // Off-RT setup (building the graph, importing, etc.) is exempt — only
    // tag the thread RT once we're doing the thing INVARIANT 1 is actually
    // about: repeatedly rendering blocks, the way the real audio callback does.
    fontelle_engine::mark_current_thread_rt();

    let events = [TimedEvent {
        sample: 0,
        target: NodeId::null(),
        payload: EventPayload::NoteOn {
            key: 60,
            velocity: 100,
            voice_context: 0,
        },
    }];
    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
    };

    // First block carries the NoteOn; render enough further blocks to cover
    // attack/decay/sustain and well into the note (not just one lucky call).
    // 800 blocks @ 128/48kHz ≈ 2.1s — matching (and exceeding) the real
    // `--play-sf2` run's duration, since the crash it hit wasn't reproducible
    // at this test's original 200-block (~0.5s) length.
    graph.process_block(&events, transport, 0..BLOCK as i64);
    for i in 1..800 {
        let start = (i * BLOCK) as i64;
        graph.process_block(&[], transport, start..start + BLOCK as i64);
    }

    // Real playback never drops `graph` mid-stream — it's dropped when the
    // stream stops, a currently-unaddressed teardown gap (see
    // `unmark_current_thread_rt`'s doc comment and PROGRESS.md), not the
    // steady-state rendering this test checks. Un-tag before `graph`'s own
    // drop so *that* known gap doesn't fail *this* test.
    fontelle_engine::unmark_current_thread_rt();
}
