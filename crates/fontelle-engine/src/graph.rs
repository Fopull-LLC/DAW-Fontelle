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
    /// Every event in this block, for *all* nodes. Use [`ProcessContext::events`]
    /// instead unless a node genuinely wants to see other nodes' traffic —
    /// `TimedEvent::target` is what says who an event is for, and a node that
    /// reads this field directly will play other instruments' parts.
    pub all_events: &'a [TimedEvent],
    /// The id of the node being processed, so it can pick its own events out.
    pub node: NodeId,
    pub transport: TransportSnapshot,
    pub sample_range: Range<Sample>,
}

impl ProcessContext<'_> {
    /// The events addressed to this node, in order.
    ///
    /// Filtering here rather than in each node keeps the routing rule in one
    /// place, and returning an iterator rather than a slice keeps it
    /// allocation-free: the events for one node are not contiguous, since the
    /// timeline is ordered by time and not by target.
    pub fn events(&self) -> impl Iterator<Item = &TimedEvent> {
        let node = self.node;
        self.all_events.iter().filter(move |e| e.target == node)
    }
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

    /// Several buffers at once, mutably. `indices` must be distinct — the
    /// scheduler guarantees that at compile time, and `get_disjoint_mut`
    /// enforces it here rather than trusting it.
    ///
    /// This is what lets a node read one set of buses and write another: the
    /// caller takes every buffer it needs in one call and demotes the input
    /// half to `&[f32]` afterwards. Asking for them one at a time would
    /// borrow the pool twice.
    pub fn buffers_mut<const N: usize>(&mut self, indices: [usize; N]) -> [&mut [f32]; N] {
        self.buffers
            .get_disjoint_mut(indices)
            .expect("buffer pool indices must be distinct and in range")
            .map(|b| b.as_mut_slice())
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
    /// **A node may also declare a different set of inputs than outputs**, and
    /// then it reads one bus and writes another: the compiled form of a
    /// track's `output` routing (TDD §13.1). Both sides must be the same
    /// width, since a width change is a downmix and that is a node's decision
    /// rather than the graph's.
    ///
    /// **Still scoped:** at most two buffers per side — enough for a mono or
    /// stereo bus, which is every bus in the mixer today. Anything outside the
    /// supported shape panics with a clear message rather than silently
    /// processing the wrong buffer.
    /// Off-RT. Gives every node the sample rate and maximum block size it needs
    /// to size its internal buffers, so `process_block` never has to allocate
    /// (INVARIANT 1).
    ///
    /// `AudioNode::prepare` existed from the start and nothing called it: the
    /// only source node in the graph was built from an already-prepared
    /// `Sampler`, so the omission was invisible until a node needed internal
    /// storage of its own. Every path that renders a graph must call this
    /// first.
    pub fn prepare(&mut self, sample_rate: f32, max_block_size: u32) {
        let ctx = PrepareContext {
            sample_rate,
            max_block_size,
        };
        for scheduled in self.schedule.iter_mut() {
            scheduled.node.prepare(&ctx);
        }
    }

    /// Silences every node and clears every bus — transport stop and seek.
    ///
    /// The buffers matter as much as the nodes: a bus still holds the last
    /// block that had sound in it, and a node that only adds into its output
    /// (every source does) would let that block through once more before the
    /// clear at the top of `process_block` caught up.
    ///
    /// RT-safe, so it can be called from the audio callback when it observes a
    /// transport change rather than having to be scheduled off-thread.
    pub fn reset(&mut self) {
        for scheduled in self.schedule.iter_mut() {
            scheduled.node.reset();
        }
        for index in 0..self.buffer_pool.len() {
            self.buffer_pool.buffer_mut(index).fill(0.0);
        }
    }

    pub fn process_block(
        &mut self,
        events: &[TimedEvent],
        transport: TransportSnapshot,
        sample_range: Range<Sample>,
    ) {
        // Buses are cleared once, here, and source nodes add into them. That
        // is what lets two instruments share an output: a source that
        // overwrote would mean whichever node ran last was the only one
        // anybody heard. The corollary is that without this clear, a block of
        // silence would replay the last block that had sound.
        let frames_to_clear = (sample_range.end - sample_range.start).max(0) as usize;
        for index in 0..self.buffer_pool.len() {
            let buffer = self.buffer_pool.buffer_mut(index);
            let frames = frames_to_clear.min(buffer.len());
            buffer[..frames].fill(0.0);
        }

        for scheduled in self.schedule.iter_mut() {
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
            // A node whose inputs are a *different* set from its outputs
            // reads one bus and writes another — the compiled form of a
            // track's `output` routing (TDD §13.1) and, later, of a send
            // (§13.2). Both sides must be the same width: a bus route is
            // stereo-to-stereo or mono-to-mono, and a width change is a
            // downmix, which is a node's job rather than the graph's.
            if !scheduled.input_buffers.is_empty()
                && scheduled.input_buffers != scheduled.output_buffers
            {
                assert_eq!(
                    scheduled.input_buffers.len(),
                    scheduled.output_buffers.len(),
                    "CompiledGraph::process_block routes between buses of the same channel \
                     count — node {:?} declares inputs {:?} against outputs {:?}",
                    scheduled.id,
                    scheduled.input_buffers,
                    scheduled.output_buffers
                );
                match (
                    scheduled.input_buffers.as_slice(),
                    scheduled.output_buffers.as_slice(),
                ) {
                    ([source], [dest]) => {
                        let [source, dest] = self.buffer_pool.buffers_mut([*source, *dest]);
                        let frames = frames.min(source.len()).min(dest.len());
                        let inputs: [&[f32]; 1] = [&source[..frames]];
                        let mut outputs = [&mut dest[..frames]];
                        let mut ctx = ProcessContext {
                            inputs: &inputs,
                            outputs: &mut outputs,
                            all_events: events,
                            node: scheduled.id,
                            transport,
                            sample_range: sample_range.clone(),
                        };
                        scheduled.node.process(&mut ctx);
                    }
                    ([source_l, source_r], [dest_l, dest_r]) => {
                        let [source_l, source_r, dest_l, dest_r] = self
                            .buffer_pool
                            .buffers_mut([*source_l, *source_r, *dest_l, *dest_r]);
                        let frames = frames
                            .min(source_l.len())
                            .min(source_r.len())
                            .min(dest_l.len())
                            .min(dest_r.len());
                        let inputs: [&[f32]; 2] = [&source_l[..frames], &source_r[..frames]];
                        let mut outputs = [&mut dest_l[..frames], &mut dest_r[..frames]];
                        let mut ctx = ProcessContext {
                            inputs: &inputs,
                            outputs: &mut outputs,
                            all_events: events,
                            node: scheduled.id,
                            transport,
                            sample_range: sample_range.clone(),
                        };
                        scheduled.node.process(&mut ctx);
                    }
                    _ => unreachable!("width equality and the <= 2 cap are asserted above"),
                }
                continue;
            }

            match scheduled.output_buffers.as_slice() {
                [] => {
                    let mut ctx = ProcessContext {
                        inputs: &[],
                        outputs: &mut [],
                        all_events: events,
                        node: scheduled.id,
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
                        all_events: events,
                        node: scheduled.id,
                        transport,
                        sample_range: sample_range.clone(),
                    };
                    scheduled.node.process(&mut ctx);
                }
                [left, right] => {
                    let [a, b] = self.buffer_pool.buffers_mut([*left, *right]);
                    let frames = frames.min(a.len()).min(b.len());
                    let mut outputs = [&mut a[..frames], &mut b[..frames]];
                    let mut ctx = ProcessContext {
                        inputs: &[],
                        outputs: &mut outputs,
                        all_events: events,
                        node: scheduled.id,
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
                    interpolation: Some(Interpolation::Draft),
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

        let mut graph = CompiledGraph {
            schedule: vec![ScheduledNode {
                id: NodeId::null(),
                node: Box::new(node),
                input_buffers: Vec::new(),
                output_buffers: vec![0],
            }],
            buffer_pool: BufferPool::with_capacity(1, 128),
        };
        graph.prepare(SR, 128);
        graph
    }

    fn playing() -> TransportSnapshot {
        TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
        }
    }

    /// A one-shot buffer at a constant `level`, so a summed bus can be read
    /// straight off the peak.
    fn flat_patch(store: &mut SampleStore, level: f32) -> Patch {
        let asset = store.insert(SampleBuffer {
            data: Arc::from(vec![level; 10_000]),
            sample_rate: SR as u32,
        });
        let disabled_filter = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: false,
        };
        let instant = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.0,
            curve: EnvelopeCurve::Linear,
        };
        Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Some(Interpolation::Draft),
                    end_offset: 64.0,
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

        let mut graph = CompiledGraph {
            schedule,
            buffer_pool: BufferPool::with_capacity(2, 128),
        };
        graph.prepare(SR, 128);
        graph
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
                    interpolation: Some(Interpolation::Draft),
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
        let mut graph = CompiledGraph {
            schedule: vec![ScheduledNode {
                id: NodeId::null(),
                node: Box::new(SamplerNode::new(sampler, Arc::new(store))),
                input_buffers: Vec::new(),
                output_buffers: vec![0],
            }],
            buffer_pool: BufferPool::with_capacity(1, 128),
        };
        graph.prepare(SR, 128);
        graph
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

    /// Two independent sampler nodes on the same stereo bus, so both the event
    /// routing and the summing are under test at once.
    fn two_sampler_graph(level_a: f32, level_b: f32) -> (CompiledGraph, NodeId, NodeId) {
        let mut store = SampleStore::new();
        let patch_a = flat_patch(&mut store, level_a);
        let patch_b = flat_patch(&mut store, level_b);
        let store = Arc::new(store);
        let id_a = NodeId::from(slotmap::KeyData::from_ffi(1));
        let id_b = NodeId::from(slotmap::KeyData::from_ffi(2));
        let prepared = |patch| {
            let mut sampler = Sampler::new(patch);
            sampler.prepare(&fontelle_core::PrepareContext {
                sample_rate: SR,
                max_block_size: 128,
            });
            sampler
        };

        let graph = CompiledGraph {
            schedule: vec![
                ScheduledNode {
                    id: id_a,
                    node: Box::new(SamplerNode::new(prepared(patch_a), store.clone())),
                    input_buffers: Vec::new(),
                    output_buffers: vec![0, 1],
                },
                ScheduledNode {
                    id: id_b,
                    node: Box::new(SamplerNode::new(prepared(patch_b), store)),
                    input_buffers: Vec::new(),
                    output_buffers: vec![0, 1],
                },
            ],
            buffer_pool: BufferPool::with_capacity(2, 128),
        };
        let mut graph = graph;
        graph.prepare(SR, 128);
        (graph, id_a, id_b)
    }

    fn note_on_for(target: NodeId, sample: i64) -> TimedEvent {
        TimedEvent {
            sample,
            target,
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 127,
                voice_context: 0,
            },
        }
    }

    #[test]
    fn a_node_only_receives_events_addressed_to_it() {
        // `TimedEvent::target` has always been there and the sequencer has
        // always filled it in, but the graph handed every event to every node.
        // With one sampler that is invisible; with two it means every
        // instrument plays every part.
        // Different levels, deliberately: with both at the same level a node
        // that wrongly plays the event is indistinguishable from one that
        // correctly ignores it.
        let (mut graph, id_a, _id_b) = two_sampler_graph(1.0, 0.25);
        graph.process_block(&[note_on_for(id_a, 0)], playing(), 0..64);

        // A centred layer is 0.707 a side on the constant-power pan law, so
        // full scale from the addressed node reads as that, not 1.0 — the
        // numbers to rule out are 0.884 (both summed) and 0.177 (the wrong
        // one overwriting).
        let centred = std::f32::consts::FRAC_1_SQRT_2;
        let peak = graph.buffer_pool.buffer_mut(0)[..64]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            (peak - centred).abs() < 1e-4,
            "only the addressed node should have sounded: full scale from the \
             addressed node, not 1.25 summed or 0.25 overwritten; got {peak}"
        );
    }

    #[test]
    fn source_nodes_sharing_a_bus_sum_rather_than_overwrite() {
        // Two instruments on one output is the ordinary case the moment a song
        // has more than one part. A source that overwrites means whichever
        // node runs last is the only one anybody hears.
        let (mut graph, id_a, id_b) = two_sampler_graph(1.0, 0.5);
        graph.process_block(
            &[note_on_for(id_a, 0), note_on_for(id_b, 0)],
            playing(),
            0..64,
        );

        let peak = graph.buffer_pool.buffer_mut(0)[..64]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        // Both patches are centred, so each arrives at 0.707 of its level.
        let expected = 1.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (peak - expected).abs() < 1e-3,
            "1.0 and 0.5 on the same bus should sum to {expected}, got {peak}"
        );
    }

    #[test]
    fn a_bus_does_not_carry_its_previous_block_into_the_next() {
        // The corollary of sources accumulating: something has to clear the
        // bus, or a block of silence replays the last block that had sound.
        let (mut graph, id_a, _) = two_sampler_graph(1.0, 0.25);
        graph.process_block(&[note_on_for(id_a, 0)], playing(), 0..64);
        assert!(
            graph.buffer_pool.buffer_mut(0)[..64]
                .iter()
                .any(|s| *s != 0.0)
        );

        // Second block, no events, and the one-shot sample has run out.
        graph.process_block(&[], playing(), 64..128);
        graph.process_block(&[], playing(), 128..192);
        let peak = graph.buffer_pool.buffer_mut(0)[..64]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert_eq!(peak, 0.0, "a silent block must be silent, got {peak}");
    }

    // --- Distinct input and output buffer sets -------------------------------

    /// A sampler on its own bus, routed into the master pair — the smallest
    /// schedule in which a node reads one buffer set and writes another.
    fn routed_sampler_graph() -> (CompiledGraph, NodeId) {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 0.5);
        let id = NodeId::from(slotmap::KeyData::from_ffi(1));
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        let mut graph = CompiledGraph {
            schedule: vec![
                ScheduledNode {
                    id,
                    node: Box::new(SamplerNode::new(sampler, Arc::new(store))),
                    input_buffers: Vec::new(),
                    output_buffers: vec![2, 3],
                },
                ScheduledNode {
                    id: NodeId::null(),
                    node: Box::new(crate::nodes::BusSumNode),
                    input_buffers: vec![2, 3],
                    output_buffers: vec![0, 1],
                },
            ],
            buffer_pool: BufferPool::with_capacity(4, 128),
        };
        graph.prepare(SR, 128);
        (graph, id)
    }

    #[test]
    fn a_node_may_read_one_buffer_set_and_write_another() {
        let (mut graph, id) = routed_sampler_graph();
        graph.process_block(&[note_on_for(id, 0)], playing(), 0..64);

        // A centred layer is 0.707 a side, so a 0.5 patch reads 0.354.
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        let master = graph.buffer_pool.buffer_mut(0)[0];
        assert!(
            (master - expected).abs() < 1e-4,
            "the track bus must reach the master: expected {expected}, got {master}"
        );
        let track = graph.buffer_pool.buffer_mut(2)[0];
        assert!(
            (track - expected).abs() < 1e-4,
            "a route must leave its source alone — a send taps a bus, it does \
             not consume it; track bus reads {track}"
        );
    }

    /// The master pair is the one buffer a route adds to rather than
    /// overwrites: every track in the song lands there.
    #[test]
    fn a_route_adds_into_its_destination_rather_than_replacing_it() {
        let (mut graph, id) = routed_sampler_graph();
        // A second route from the same track bus stands in for a second track
        // arriving at the master.
        graph.schedule.push(ScheduledNode {
            id: NodeId::null(),
            node: Box::new(crate::nodes::BusSumNode),
            input_buffers: vec![2, 3],
            output_buffers: vec![0, 1],
        });
        graph.process_block(&[note_on_for(id, 0)], playing(), 0..64);

        let expected = 2.0 * 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        let master = graph.buffer_pool.buffer_mut(0)[0];
        assert!(
            (master - expected).abs() < 1e-4,
            "two routes into one bus must sum: expected {expected}, got {master}"
        );
    }

    /// Two independent tracks, each with its own bus, fader and route to the
    /// master. **Deliberately unequal:** with both tracks at the same gain, a
    /// fader applied to the wrong bus would be invisible.
    fn two_track_graph(gain_a_db: f32, gain_b_db: f32) -> (CompiledGraph, NodeId, NodeId) {
        let mut store = SampleStore::new();
        let patch_a = flat_patch(&mut store, 1.0);
        let patch_b = flat_patch(&mut store, 1.0);
        let store = Arc::new(store);
        let id_a = NodeId::from(slotmap::KeyData::from_ffi(1));
        let id_b = NodeId::from(slotmap::KeyData::from_ffi(2));
        let prepared = |patch| {
            let mut sampler = Sampler::new(patch);
            sampler.prepare(&fontelle_core::PrepareContext {
                sample_rate: SR,
                max_block_size: 128,
            });
            sampler
        };
        let track = |id, sampler, bus: [usize; 2], gain_db| {
            vec![
                ScheduledNode {
                    id,
                    node: Box::new(SamplerNode::new(sampler, store.clone())),
                    input_buffers: Vec::new(),
                    output_buffers: bus.to_vec(),
                },
                ScheduledNode {
                    id: NodeId::null(),
                    node: Box::new(MixerTrackNode {
                        gain_db,
                        pan_law: fontelle_types::PanLaw::Linear,
                        ..MixerTrackNode::new()
                    }),
                    input_buffers: bus.to_vec(),
                    output_buffers: bus.to_vec(),
                },
                ScheduledNode {
                    id: NodeId::null(),
                    node: Box::new(crate::nodes::BusSumNode),
                    input_buffers: bus.to_vec(),
                    output_buffers: vec![0, 1],
                },
            ]
        };

        let mut schedule = track(id_a, prepared(patch_a), [2, 3], gain_a_db);
        schedule.extend(track(id_b, prepared(patch_b), [4, 5], gain_b_db));

        let mut graph = CompiledGraph {
            schedule,
            buffer_pool: BufferPool::with_capacity(6, 128),
        };
        graph.prepare(SR, 128);
        (graph, id_a, id_b)
    }

    #[test]
    fn each_track_has_its_own_fader_before_the_master_sums_them() {
        let (mut graph, id_a, id_b) = two_track_graph(0.0, -20.0);
        graph.process_block(
            &[note_on_for(id_a, 0), note_on_for(id_b, 0)],
            playing(),
            0..64,
        );

        // Both patches are centred, so each arrives at 0.707 of its level.
        let centred = std::f32::consts::FRAC_1_SQRT_2;
        let expected = centred * (1.0 + 10f32.powf(-20.0 / 20.0));
        let got = graph.buffer_pool.buffer_mut(0)[0];
        assert!(
            (got - expected).abs() < 1e-4,
            "a track 20 dB down must reach the master 20 dB down: expected \
             {expected}, got {got}"
        );
    }

    #[test]
    fn muting_one_track_leaves_the_other_alone() {
        // The point of per-track buses: before this, one fader was the only
        // fader, and muting it muted the song.
        let (mut graph, id_a, id_b) = two_track_graph(0.0, 0.0);
        graph.schedule[4] = ScheduledNode {
            id: NodeId::null(),
            node: Box::new(MixerTrackNode {
                mute: true,
                ..MixerTrackNode::new()
            }),
            input_buffers: vec![4, 5],
            output_buffers: vec![4, 5],
        };
        graph.process_block(
            &[note_on_for(id_a, 0), note_on_for(id_b, 0)],
            playing(),
            0..64,
        );

        let centred = std::f32::consts::FRAC_1_SQRT_2;
        let got = graph.buffer_pool.buffer_mut(0)[0];
        assert!(
            (got - centred).abs() < 1e-4,
            "muting track B must leave exactly track A at the master: \
             expected {centred}, got {got}"
        );
    }

    #[test]
    fn tracks_panned_apart_reach_opposite_sides_of_the_master() {
        let (mut graph, id_a, id_b) = two_track_graph(0.0, 0.0);
        for (index, pan) in [(1usize, -1.0f32), (4usize, 1.0f32)] {
            let bus = graph.schedule[index].input_buffers.clone();
            graph.schedule[index] = ScheduledNode {
                id: NodeId::null(),
                node: Box::new(MixerTrackNode {
                    pan,
                    pan_law: fontelle_types::PanLaw::Linear,
                    ..MixerTrackNode::new()
                }),
                input_buffers: bus.clone(),
                output_buffers: bus,
            };
        }
        graph.process_block(
            &[note_on_for(id_a, 0), note_on_for(id_b, 0)],
            playing(),
            0..64,
        );

        // Balance-style: hard left keeps its own left channel and silences its
        // right, so each track lands wholly on one side of the master.
        let centred = std::f32::consts::FRAC_1_SQRT_2;
        let left = graph.buffer_pool.buffer_mut(0)[0];
        let right = graph.buffer_pool.buffer_mut(1)[0];
        assert!(
            (left - centred).abs() < 1e-4 && (right - centred).abs() < 1e-4,
            "one track a side: expected {centred} each, got {left} / {right}"
        );
    }

    #[test]
    #[should_panic(expected = "same channel count")]
    fn a_route_between_buses_of_different_widths_is_rejected() {
        let (mut graph, _) = routed_sampler_graph();
        // A stereo bus routed into a single mono destination: the graph has no
        // business inventing a downmix, so it refuses rather than guessing.
        graph.schedule[1].output_buffers = vec![0];
        graph.process_block(&[], playing(), 0..64);
    }

    /// `SamplerNode::reset` was a `todo!()` — a panic waiting for the first
    /// thing that stopped or seeked the transport, on the audio thread.
    #[test]
    fn resetting_the_graph_silences_every_node_and_clears_every_bus() {
        let (mut graph, id_a, id_b) = two_sampler_graph(1.0, 0.5);
        graph.process_block(
            &[note_on_for(id_a, 0), note_on_for(id_b, 0)],
            playing(),
            0..64,
        );
        assert!(
            graph.buffer_pool.buffer_mut(0)[..64]
                .iter()
                .any(|s| *s != 0.0)
        );

        graph.reset();
        assert!(
            graph.buffer_pool.buffer_mut(0)[..64]
                .iter()
                .all(|s| *s == 0.0),
            "the bus must be cleared too, or the last block plays once more"
        );

        // And the voices are gone, not merely released: a further block with
        // no events must be silent even though the one-shot samples had plenty
        // left to play.
        graph.process_block(&[], playing(), 64..128);
        let peak = graph.buffer_pool.buffer_mut(0)[..64]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert_eq!(
            peak, 0.0,
            "a reset sampler must have nothing left, got {peak}"
        );
    }
}
