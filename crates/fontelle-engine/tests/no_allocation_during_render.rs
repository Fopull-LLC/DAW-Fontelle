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
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{
    BufferPool, BusSumNode, CompiledGraph, MasterNode, MixerTrackNode, RtGuardAllocator,
    SamplerNode, ScheduledNode,
};
use fontelle_types::{CompiledTimeline, EventPayload, NodeId, TimedEvent};
use slotmap::Key;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn build_graph() -> CompiledGraph {
    build_graph_with(false)
}

/// The two-node stereo shape real playback now uses: sampler writing a bus
/// pair, then a `MixerTrackNode` processing them in place.
fn build_stereo_graph_with_mixer() -> CompiledGraph {
    build_graph_with(true)
}

fn build_graph_with(stereo_mixer: bool) -> CompiledGraph {
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
        curve: EnvelopeCurve::Linear,
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
                interpolation: Some(Interpolation::Normal),
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

    let buses: Vec<usize> = if stereo_mixer { vec![0, 1] } else { vec![0] };
    let mut schedule = vec![ScheduledNode {
        id: NodeId::null(),
        node: Box::new(SamplerNode::new(sampler, Arc::new(store))),
        input_buffers: Vec::new(),
        output_buffers: buses.clone(),
    }];
    if stereo_mixer {
        schedule.push(ScheduledNode {
            id: NodeId::null(),
            node: Box::new(MixerTrackNode {
                gain_db: -3.0,
                pan: 0.25,
                ..MixerTrackNode::new()
            }),
            input_buffers: buses.clone(),
            output_buffers: buses.clone(),
        });
    }

    CompiledGraph {
        schedule,
        buffer_pool: BufferPool::with_capacity(buses.len(), BLOCK),
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

/// The same INVARIANT 1 check against the shape playback *actually* uses now:
/// a two-node stereo schedule (sampler → mixer track, processing in place),
/// driven from a real `CompiledTimeline` through
/// `CompiledTimeline::events_for_block` — exactly what
/// `AudioDevice`'s callback does per block, including the event-cursor walk
/// the single-node test above never touches.
///
/// Written as a regression guard *after* the code it covers, unlike the
/// red-first tests for the mixer's behaviour: the paths it protects
/// (`get_disjoint_mut` for the bus pair, `split_at_mut` in the mixer,
/// `split_first_mut` + `copy_from_slice` in the sampler node, the slice
/// return from `events_for_block`) are all ones where a careless later edit
/// could reintroduce a per-block `Vec`, which is precisely the bug this
/// file's original test caught on real hardware.
#[test]
fn the_stereo_sampler_into_mixer_chain_does_not_allocate_per_block() {
    let mut graph = build_stereo_graph_with_mixer();

    // A real timeline with events spread across many blocks, so the cursor
    // advances mid-run rather than being consumed entirely on block one.
    let mut events = Vec::new();
    for i in 0..16 {
        let sample = i * BLOCK as i64 * 7;
        events.push(TimedEvent {
            sample,
            target: NodeId::null(),
            payload: EventPayload::NoteOn {
                key: 60 + (i % 12) as u8,
                velocity: 100,
                voice_context: 0,
            },
        });
        events.push(TimedEvent {
            sample: sample + BLOCK as i64 * 3,
            target: NodeId::null(),
            payload: EventPayload::NoteOff {
                key: 60 + (i % 12) as u8,
                voice_context: 0,
            },
        });
    }
    events.sort_by_key(|e| e.sample);
    let timeline = CompiledTimeline {
        events,
        index: Vec::new(),
    };

    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
    };

    // Everything above is off-RT setup and may allocate freely.
    fontelle_engine::mark_current_thread_rt();

    let mut cursor = 0usize;
    for i in 0..800 {
        let start = (i * BLOCK) as i64;
        let range = start..start + BLOCK as i64;
        let block_events = timeline.events_for_block(&mut cursor, range.clone());
        graph.process_block(block_events, transport, range);
    }

    assert_eq!(
        cursor,
        timeline.events.len(),
        "the run must be long enough to consume the whole timeline"
    );

    // Same teardown caveat as the test above: real playback never drops the
    // graph mid-stream.
    fontelle_engine::unmark_current_thread_rt();
}

/// The full mixer shape playback uses now: a sampler on its own bus pair, its
/// track fader in place, a `BusSumNode` routing that pair into the master, the
/// master fader, and a `MasterNode` running a look-ahead limiter and the
/// meters.
///
/// Three of those nodes are newer than the two tests above and none of them is
/// covered by either. `BusSumNode` is the first node in the tree whose inputs
/// are a different set from its outputs, which is a whole new branch of
/// `process_block`'s buffer handling; `MasterNode` owns a delay line and a
/// ring buffer, and a limiter that sized either of them per block would be the
/// exact bug this file exists to catch.
fn build_full_mixer_graph() -> CompiledGraph {
    let source = build_graph_with(true);
    let mut schedule = source.schedule;
    // The sampler moves onto a track bus of its own.
    schedule[0].output_buffers = vec![2, 3];
    schedule[1].input_buffers = vec![2, 3];
    schedule[1].output_buffers = vec![2, 3];
    schedule.push(ScheduledNode {
        id: NodeId::null(),
        node: Box::new(BusSumNode),
        input_buffers: vec![2, 3],
        output_buffers: vec![0, 1],
    });
    schedule.push(ScheduledNode {
        id: NodeId::null(),
        node: Box::new(MixerTrackNode {
            pan_law: fontelle_types::PanLaw::Linear,
            ..MixerTrackNode::new()
        }),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });
    schedule.push(ScheduledNode {
        id: NodeId::null(),
        node: Box::new(MasterNode::new()),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });

    let mut graph = CompiledGraph {
        schedule,
        buffer_pool: BufferPool::with_capacity(4, BLOCK),
    };
    graph.prepare(SR, BLOCK as u32);
    graph
}

#[test]
fn the_full_track_to_master_chain_does_not_allocate_per_block() {
    let mut graph = build_full_mixer_graph();

    let events = [TimedEvent {
        sample: 0,
        target: NodeId::null(),
        payload: EventPayload::NoteOn {
            key: 60,
            velocity: 127,
            voice_context: 0,
        },
    }];
    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
    };

    fontelle_engine::mark_current_thread_rt();
    graph.process_block(&events, transport, 0..BLOCK as i64);
    for i in 1..800 {
        let start = (i * BLOCK) as i64;
        graph.process_block(&[], transport, start..start + BLOCK as i64);
    }
    fontelle_engine::unmark_current_thread_rt();
}
