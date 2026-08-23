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
    /// **M0 scope:** only source nodes are wired up — an empty `input_buffers`
    /// and at most one entry in `output_buffers` — enough for the vertical
    /// slice's one `SamplerNode`. A node declared outside that shape panics
    /// rather than silently mixing the wrong thing. Real multi-node buffer
    /// routing (an effect reading another node's output, disjoint-borrowing
    /// several buffers per node for a mixer send) is M4 work; see
    /// `PROGRESS.md`.
    pub fn process_block(
        &mut self,
        events: &[TimedEvent],
        transport: TransportSnapshot,
        sample_range: Range<Sample>,
    ) {
        for scheduled in self.schedule.iter_mut() {
            assert!(
                scheduled.input_buffers.is_empty(),
                "CompiledGraph::process_block only supports source nodes for now (TDD M0 scope) \
                 — node {:?} declares {} input buffer(s)",
                scheduled.id,
                scheduled.input_buffers.len()
            );
            assert!(
                scheduled.output_buffers.len() <= 1,
                "CompiledGraph::process_block only supports a single output buffer per node for \
                 now (TDD M0 scope) — node {:?} declares {}",
                scheduled.id,
                scheduled.output_buffers.len()
            );

            let mut outputs: Vec<&mut [f32]> = Vec::new();
            if let Some(&idx) = scheduled.output_buffers.first() {
                outputs.push(self.buffer_pool.buffer_mut(idx));
            }

            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                events,
                transport,
                sample_range: sample_range.clone(),
            };
            scheduled.node.process(&mut ctx);
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
    use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};
    use fontelle_types::EventPayload;
    use slotmap::Key;

    use super::*;
    use crate::nodes::SamplerNode;
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
