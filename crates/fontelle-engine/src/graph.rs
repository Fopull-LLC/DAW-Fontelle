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
    pub fn process_block(
        &mut self,
        _events: &[TimedEvent],
        _transport: TransportSnapshot,
        _sample_range: Range<Sample>,
    ) {
        todo!(
            "walk self.schedule in compiled order, wiring buffer_pool slices into each node's ProcessContext"
        )
    }
}
