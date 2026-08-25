use std::ops::Range;

use fontelle_types::{NodeId, ParamAddress, Sample, TimedEvent};

use crate::transport::TransportSnapshot;

pub struct PrepareContext {
    pub sample_rate: f32,
    pub max_block_size: u32,
}

/// Input/output buffer slices, the sample-accurate event slice for this block,
/// transport state, and the block's sample range — nothing in it requires the
/// heap (TDD §5.1). Internal processing is always 32-bit float, always
/// deinterleaved (planar); interleaving happens exactly once, at the device
/// boundary (§5.2).
pub struct ProcessContext<'a> {
    pub inputs: &'a [&'a [f32]],
    pub outputs: &'a mut [&'a mut [f32]],
    pub events: &'a [TimedEvent],
    pub transport: TransportSnapshot,
    pub sample_range: Range<Sample>,
}

pub trait ParamSet: Send + Sync {
    fn get(&self, addr: &ParamAddress) -> Option<f64>;
    /// Returns `false` if `addr` isn't one of this node's parameters.
    fn set(&self, addr: &ParamAddress, value: f64) -> bool;
}

/// The whole node abstraction the audio graph schedules (TDD §5.1). `prepare` runs
/// off-RT and may allocate; `process` runs on the RT thread and must not
/// (INVARIANT 1).
pub trait AudioNode: Send {
    fn prepare(&mut self, ctx: &PrepareContext);
    fn process(&mut self, ctx: &mut ProcessContext);
    /// Silence tails, clear internal state — called on transport stop/seek.
    fn reset(&mut self);
    fn latency_samples(&self) -> u32 {
        0
    }
    fn params(&self) -> &dyn ParamSet;
}

/// A pre-allocated pool of planar `f32` buffers, sized at `prepare()` time from
/// the compiled schedule's peak concurrent-buffer requirement. The scheduler
/// assigns indices via linear-scan register allocation at compile time — the RT
/// thread only ever indexes into the pool, never allocates one (TDD §5.2).
pub struct BufferPool {
    buffers: Vec<Vec<f32>>,
}

impl BufferPool {
    pub fn with_capacity(buffer_count: usize, block_size: usize) -> Self {
        Self {
            buffers: (0..buffer_count).map(|_| vec![0.0; block_size]).collect(),
        }
    }

    pub fn buffer_mut(&mut self, index: usize) -> &mut [f32] {
        &mut self.buffers[index]
    }

    /// Two buffers at once, mutably. `indices` must be distinct — the
    /// scheduler guarantees that at compile time, and `get_disjoint_mut`
    /// enforces it here rather than trusting it.
    pub fn buffer_pair_mut(&mut self, indices: [usize; 2]) -> [&mut [f32]; 2] {
        let [a, b] = self
            .buffers
            .get_disjoint_mut(indices)
            .expect("buffer pool indices must be distinct and in range");
        [a.as_mut_slice(), b.as_mut_slice()]
    }

    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

/// One entry in the compiled, topologically-sorted schedule the RT thread walks —
/// it never traverses a graph or resolves connections at runtime (TDD §5.1).
pub struct ScheduledNode {
    pub id: NodeId,
    pub node: Box<dyn AudioNode>,
    pub input_buffers: Vec<usize>,
    pub output_buffers: Vec<usize>,
}

pub struct CompiledGraph {
    pub schedule: Vec<ScheduledNode>,
    pub buffer_pool: BufferPool,
}

impl CompiledGraph {
    /// RT: walks `schedule` in order, feeding each node its assigned buffer
    /// indices. No allocation, no traversal beyond a linear scan (INVARIANT 1).
    ///
    /// **Nodes process in place.** A node that consumes another's output
    /// declares the *same* buffer indices in `input_buffers` and
    /// `output_buffers`; its buffers arrive already carrying the upstream
    /// signal, and it reads, modifies, and writes them back. This is the
    /// default contract in every plugin API (VST/CLAP/AU all work this way),
    /// and it's what lets the RT thread avoid both a copy per node and the
    /// aliasing problem of handing out `&[f32]` and `&mut [f32]` to the same
    /// pool slot. A source node (the sampler) declares no inputs and simply
    /// overwrites its outputs.
    ///
    /// **Still scoped:** at most two buffers per node — enough for a mono or
    /// stereo bus, which is what M0's sampler → mixer track → device chain
    /// needs. A node wanting a *different* set of inputs than outputs (a
    /// mixer send tapping one bus into another, a sidechain input) needs real
    /// disjoint multi-set borrowing and belongs to M4 alongside sends and
    /// insert chains. Anything outside the supported shape panics with a
    /// clear message rather than silently processing the wrong buffer.
    pub fn process_block(
        &mut self,
        events: &[TimedEvent],
        transport: TransportSnapshot,
        sample_range: Range<Sample>,
    ) {
        for scheduled in self.schedule.iter_mut() {
            assert!(
                scheduled.input_buffers.is_empty()
                    || scheduled.input_buffers == scheduled.output_buffers,
                "CompiledGraph::process_block requires a node's inputs to either be empty (a \
                 source) or identical to its outputs (in-place) — node {:?} declares inputs {:?} \
                 against outputs {:?}. Distinct input/output sets are M4 work; see PROGRESS.md.",
                scheduled.id,
                scheduled.input_buffers,
                scheduled.output_buffers
            );
            assert!(
                scheduled.output_buffers.len() <= 2,
                "CompiledGraph::process_block supports at most 2 buffers per node (mono or \
                 stereo) for now — node {:?} declares {}",
                scheduled.id,
                scheduled.output_buffers.len()
            );

            // Nodes are handed exactly `frames` samples, not their buffer's
            // full capacity. The device is under no obligation to deliver
            // `BLOCK_SIZE` frames per callback and generally doesn't — real
            // ALSA hardware here delivers 85 against a 128-frame buffer. A
            // node that rendered the whole buffer anyway would advance its
            // voices 128 samples while the caller emitted 85, discarding the
            // difference every callback: 34% of the signal, with a phase jump
            // each time. That was a real bug, audible as harsh
            // bitcrushed-sounding distortion, and it is what
            // `a_short_block_renders_the_same_audio_as_a_full_one` pins down.
            let frames = (sample_range.end - sample_range.start).max(0) as usize;

            // Fixed-size stack arrays, not `Vec`s — `Vec::push` on an empty
            // `Vec` allocates, and this runs once per block per node on the RT
            // thread. That's exactly the bug this file's
            // `no_allocation_during_render` test exists to catch (it did,
            // against an earlier version of this function).
            match scheduled.output_buffers.as_slice() {
                [] => {
                    let mut ctx = ProcessContext {
                        inputs: &[],
                        outputs: &mut [],
                        events,
                        transport,
                        sample_range: sample_range.clone(),
                    };
                    scheduled.node.process(&mut ctx);
                }
                [only] => {
                    let buffer = self.buffer_pool.buffer_mut(*only);
                    let frames = frames.min(buffer.len());
                    let mut outputs = [&mut buffer[..frames]];
                    let mut ctx = ProcessContext {
                        inputs: &[],
                        outputs: &mut outputs,
                        events,
                        transport,
                        sample_range: sample_range.clone(),
                    };
                    scheduled.node.process(&mut ctx);
                }
                [left, right] => {
                    let [a, b] = self.buffer_pool.buffer_pair_mut([*left, *right]);
                    let frames = frames.min(a.len()).min(b.len());
                    let mut outputs = [&mut a[..frames], &mut b[..frames]];
                    let mut ctx = ProcessContext {
                        inputs: &[],
                        outputs: &mut outputs,
                        events,
                        transport,
                        sample_range: sample_range.clone(),
                    };
                    scheduled.node.process(&mut ctx);
                }
                _ => unreachable!("output_buffers.len() <= 2 asserted above"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fontelle_core::{
        FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
        Sampler, Source, VoiceConfig,
    };
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
    use fontelle_types::EventPayload;
    use slotmap::Key;

    use super::*;
    use crate::nodes::{MixerTrackNode, SamplerNode};
    use crate::transport::TransportState;

    const SR: f32 = 48_000.0;

    fn one_sampler_graph() -> CompiledGraph {
        let mut store = SampleStore::new();
        let asset = store.insert(SampleBuffer {
            data: Arc::from(vec![1.0; 10_000]),
            sample_rate: SR as u32,
        });
        let disabled_filter = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
        };
        let instant = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
        };
        let patch = Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Interpolation::Draft,
                    end_offset: 10_000.0,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter, disabled_filter],
            envelopes: vec![instant, instant],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig::default(),
        };

        let mut sampler = Sampler::new(patch);
        sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });
        let node = SamplerNode::new(sampler, Arc::new(store));

        CompiledGraph {
            schedule: vec![ScheduledNode {
                id: NodeId::null(),
                node: Box::new(node),
                input_buffers: Vec::new(),
                output_buffers: vec![0],
            }],
            buffer_pool: BufferPool::with_capacity(1, 128),
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn a_note_on_reaches_the_sampler_node_and_produces_sound() {
        let mut graph = one_sampler_graph();
        let events = [TimedEvent {
            sample: 0,
            target: NodeId::null(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 127,
                voice_context: 0,
            },
        }];
        let transport = TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
        };

        graph.process_block(&events, transport, 0..128);

        assert!(
            rms(graph.buffer_pool.buffer_mut(0)) > 0.5,
            "the compiled graph must carry a NoteOn through to real sampler output"
        );
    }

    /// The M0 gate's "→ mixer track →" step, as an actual two-node schedule:
    /// a `SamplerNode` writing a stereo pair, then a `MixerTrackNode`
    /// processing those same buffers in place.
    fn sampler_into_mixer_graph(mixer: MixerTrackNode) -> CompiledGraph {
        let source = one_sampler_graph();
        let mut schedule = source.schedule;
        // The sampler now feeds a stereo pair rather than a single buffer.
        schedule[0].output_buffers = vec![0, 1];
        schedule.push(ScheduledNode {
            id: NodeId::null(),
            node: Box::new(mixer),
            // Same buffers in and out: this node processes in place.
            input_buffers: vec![0, 1],
            output_buffers: vec![0, 1],
        });

        CompiledGraph {
            schedule,
            buffer_pool: BufferPool::with_capacity(2, 128),
        }
    }

    fn note_on_events() -> [TimedEvent; 1] {
        [TimedEvent {
            sample: 0,
            target: NodeId::null(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 127,
                voice_context: 0,
            },
        }]
    }

    #[test]
    fn a_sampler_feeding_a_mixer_track_is_attenuated_by_that_tracks_fader() {
        let transport = TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
        };

        let mut unity = sampler_into_mixer_graph(MixerTrackNode::new());
        unity.process_block(&note_on_events(), transport, 0..128);
        let unity_rms = rms(unity.buffer_pool.buffer_mut(0));

        let mut quiet = sampler_into_mixer_graph(MixerTrackNode {
            gain_db: -6.0,
            ..MixerTrackNode::new()
        });
        quiet.process_block(&note_on_events(), transport, 0..128);
        let quiet_rms = rms(quiet.buffer_pool.buffer_mut(0));

        assert!(
            unity_rms > 0.1,
            "the sampler must actually reach the mixer track, got {unity_rms}"
        );
        let ratio = quiet_rms / unity_rms;
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!(
            (ratio - expected).abs() < 0.01,
            "a -6dB mixer track should scale the sampler's output by ~{expected}, got {ratio}"
        );
    }

    #[test]
    fn a_muted_mixer_track_silences_the_sampler_feeding_it() {
        let mut graph = sampler_into_mixer_graph(MixerTrackNode {
            mute: true,
            ..MixerTrackNode::new()
        });
        graph.process_block(
            &note_on_events(),
            TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 0,
            },
            0..128,
        );
        assert_eq!(rms(graph.buffer_pool.buffer_mut(0)), 0.0);
        assert_eq!(rms(graph.buffer_pool.buffer_mut(1)), 0.0);
    }

    #[test]
    fn a_mono_sampler_reaches_both_channels_of_a_stereo_pair() {
        let mut graph = sampler_into_mixer_graph(MixerTrackNode::new());
        graph.process_block(
            &note_on_events(),
            TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 0,
            },
            0..128,
        );
        let left = rms(graph.buffer_pool.buffer_mut(0));
        let right = rms(graph.buffer_pool.buffer_mut(1));
        assert!(left > 0.1, "left channel must carry audio, got {left}");
        assert!(
            (left - right).abs() < 1e-5,
            "a centred mono source must reach both channels equally: {left} vs {right}"
        );
    }

    /// A sampler over a *ramp* buffer: every source sample is distinct, so a
    /// position error shows up as wrong values rather than being masked by a
    /// constant.
    fn ramp_graph() -> CompiledGraph {
        let mut store = SampleStore::new();
        let asset = store.insert(SampleBuffer {
            data: Arc::from(
                (0..10_000)
                    .map(|i| (i % 256) as f32 / 256.0)
                    .collect::<Vec<f32>>(),
            ),
            sample_rate: SR as u32,
        });
        let disabled_filter = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
        };
        let instant = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 10.0,
            curve: EnvelopeCurve::Linear,
        };
        let patch = Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Interpolation::Draft,
                    end_offset: 10_000.0,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter, disabled_filter],
            envelopes: vec![instant, instant],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig::default(),
        };
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });
        CompiledGraph {
            schedule: vec![ScheduledNode {
                id: NodeId::null(),
                node: Box::new(SamplerNode::new(sampler, Arc::new(store))),
                input_buffers: Vec::new(),
                output_buffers: vec![0],
            }],
            buffer_pool: BufferPool::with_capacity(1, 128),
        }
    }

    /// Renders `total` frames in `chunk`-sized calls, concatenating what each
    /// call actually produced — the way the device consumes it.
    fn render_in_chunks(total: usize, chunk: usize) -> Vec<f32> {
        let mut graph = ramp_graph();
        let transport = TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
        };
        graph.process_block(&note_on_events(), transport, 0..chunk as i64);

        let mut out = Vec::new();
        out.extend_from_slice(&graph.buffer_pool.buffer_mut(0)[..chunk]);
        let mut at = chunk;
        while at < total {
            let n = chunk.min(total - at);
            graph.process_block(&[], transport, at as i64..(at + n) as i64);
            out.extend_from_slice(&graph.buffer_pool.buffer_mut(0)[..n]);
            at += n;
        }
        out
    }

    /// **The device does not hand us `BLOCK_SIZE` frames.** On real ALSA
    /// hardware it delivered 85. A node must therefore render exactly as many
    /// frames as `sample_range` asks for — not its buffer's full capacity —
    /// or every short callback advances the voices further than the audio it
    /// emits, silently discarding the difference. At 85 frames against a
    /// 128-frame buffer that is 34% of the signal thrown away per callback,
    /// which sounds exactly as bad as it is.
    #[test]
    fn a_short_block_renders_the_same_audio_as_a_full_one() {
        let total = 128 * 6;
        let full = render_in_chunks(total, 128);
        let short = render_in_chunks(total, 85);

        assert_eq!(full.len(), total);
        assert_eq!(short.len(), total);
        for (i, (a, b)) in full.iter().zip(short.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "frame {i}: rendering in 85-frame chunks must produce identical audio to \
                 128-frame chunks, got {a} vs {b}"
            );
        }
    }

    #[test]
    fn silence_with_no_events() {
        let mut graph = one_sampler_graph();
        let transport = TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
        };

        graph.process_block(&[], transport, 0..128);

        assert_eq!(rms(graph.buffer_pool.buffer_mut(0)), 0.0);
    }
}
