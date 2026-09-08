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
use fontelle_types::{CompiledTimeline, EventPayload, EventSink, NodeId, TimedEvent};
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
        ..Default::default()
    };
    let env = EnvelopeConfig {
        delay_s: 0.001,
        attack_s: 0.001,
        hold_s: 0.001,
        decay_s: 0.001,
        sustain_level: 1.0,
        release_s: 0.001,
        curve: EnvelopeCurve::Linear,
        ..Default::default()
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
        ..Default::default()
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
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    }];
    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
        bpm: fontelle_types::DEFAULT_BPM,
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
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
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
        tempo: Vec::new(),
        audio: Vec::new(),
    };

    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
        bpm: fontelle_types::DEFAULT_BPM,
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
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    }];
    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
        bpm: fontelle_types::DEFAULT_BPM,
    };

    fontelle_engine::mark_current_thread_rt();
    graph.process_block(&events, transport, 0..BLOCK as i64);
    for i in 1..800 {
        let start = (i * BLOCK) as i64;
        graph.process_block(&[], transport, start..start + BLOCK as i64);
        // Interleaved with the render, because `CompiledGraph::reset` claims
        // to be RT-safe so the callback can act on a transport change itself
        // rather than scheduling one off-thread. A `reset` that allocated
        // would violate INVARIANT 1 exactly where it is hardest to notice.
        if i % 97 == 0 {
            graph.reset();
        }
    }
    fontelle_engine::unmark_current_thread_rt();
}

#[test]
fn driving_the_transport_through_stops_seeks_and_loops_does_not_allocate() {
    // The device callback observes transport changes itself rather than
    // scheduling them off-thread, so everything `TransportReader::next_step`
    // does happens under INVARIANT 1 — including the binary search that
    // rewinds the event cursor on a seek and the block split at a loop seam.
    // A `Vec` anywhere in there is the same bug this file was written for,
    // just one layer up.
    let mut graph = build_full_mixer_graph();
    let transport = fontelle_engine::Transport::new();
    let mut reader = fontelle_engine::TransportReader::new();

    // A timeline with real content, so `events_for_block` and `cursor_at`
    // have something to scan rather than short-circuiting on an empty slice.
    let timeline = CompiledTimeline {
        events: (0..512)
            .map(|i| TimedEvent {
                sample: i as i64 * 371,
                target: NodeId::null(),
                payload: if i % 2 == 0 {
                    EventPayload::NoteOn {
                        key: 60,
                        velocity: 127,
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
            })
            .collect(),
        index: Vec::new(),
        tempo: Vec::new(),
        audio: Vec::new(),
    };

    transport.play();
    transport.set_loop_range((0, 3_840), (0, 40_037));
    transport.set_looping(true);

    fontelle_engine::mark_current_thread_rt();
    for i in 0..2_000 {
        // Everything the model thread can do to the transport, at a rate no
        // user could manage, so each branch is covered many times over.
        match i % 211 {
            50 => transport.stop(),
            60 => transport.play(),
            100 => transport.seek(1_000_000),
            150 => transport.seek(0),
            _ => {}
        }

        let mut written = 0;
        while written < BLOCK {
            let step = reader.next_step(&transport, &timeline, BLOCK - written, BLOCK, false);
            if step.reset {
                graph.reset();
            }
            if step.process {
                graph.process_block(step.events, step.snapshot, step.range.clone());
            }
            written += step.frames.min(BLOCK - written);
        }
    }
    fontelle_engine::unmark_current_thread_rt();
}

#[test]
fn draining_live_input_does_not_allocate() {
    // The live queue is drained inside the audio callback, once per block, so
    // everything about it is under INVARIANT 1: the pop, the stamping, and the
    // scratch it collects into. The scratch is sized for every port's full
    // capacity at construction precisely so this holds — a `Vec` that grew on
    // a busy block would allocate on the RT thread only when someone played
    // hard, which is the worst possible time to find out.
    let mut graph = build_full_mixer_graph();
    let transport = fontelle_engine::Transport::new();
    let mut reader = fontelle_engine::TransportReader::new();
    let mut gate = fontelle_engine::IdleGate::new();
    let (mut source, mut ports) = fontelle_engine::live_event_channel(4, 64);
    // Armed, and drained with `recording` true below: mirroring a take into
    // the capture ring happens on this thread too, and a `clone` of the wrong
    // payload there would allocate.
    let (writer, mut capture) = fontelle_engine::live_capture_channel(64);
    source.arm_capture(writer);
    let timeline = CompiledTimeline::empty();

    let mut keyboards: Vec<fontelle_engine::LivePort> = (0..4)
        .map(|_| ports.claim().expect("a free port"))
        .collect();

    let mut taken: Vec<TimedEvent> = Vec::with_capacity(4 * 64);

    fontelle_engine::mark_current_thread_rt();
    for block in 0..1_000 {
        // Filling the queues is the device thread's job and allocates
        // nothing either — the ring is already there. Doing it inside the
        // tagged region covers both halves at once.
        for (index, keyboard) in keyboards.iter_mut().enumerate() {
            let key = 36 + ((block + index) % 60) as u8;
            keyboard.send(TimedEvent {
                sample: 0,
                target: NodeId::null(),
                payload: if block % 2 == 0 {
                    EventPayload::NoteOn {
                        key,
                        velocity: 100,
                        pan: 0,
                        fine_pitch: 0,
                        release: 0,
                        mod_x: 0,
                        mod_y: 0,
                        voice_context: u32::MAX,
                    }
                } else {
                    EventPayload::NoteOff {
                        key,
                        voice_context: u32::MAX,
                    }
                },
            });
        }

        // Emptied every few blocks, the way the model thread does, so the
        // ring never sits full and the drop path is not the only one covered.
        // Preallocated because it is being emptied inside the tagged region
        // here; the real model thread is not RT and may grow whatever it likes.
        if block % 8 == 0 {
            taken.clear();
            capture.drain_into(&mut taken);
        }

        let live = source.drain(reader.position(), true);
        let awake = gate.is_awake(live.len());
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, awake);
        if step.reset {
            graph.reset_sequenced();
        }
        if step.process {
            graph.process_block_with_live(step.events, live, step.snapshot, step.range.clone());
            gate.observe(0.5);
        }
    }
    fontelle_engine::unmark_current_thread_rt();
}
