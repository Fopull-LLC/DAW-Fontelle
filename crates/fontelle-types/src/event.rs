use crate::{NodeId, ParamAddress, Sample};

/// One entry in a `CompiledTimeline` (TDD §11.1). Read-only from the RT thread's
/// point of view — the whole `Vec` is built and handed over by `triple_buffer`
/// before playback reaches it, so nothing here allocates on the hot path.
#[derive(Debug, Clone)]
pub enum EventPayload {
    NoteOn {
        key: u8,
        velocity: u8,
        /// TDD §11.4: distinguishes overlapping clips on the same channel so a
        /// note-off only kills the voice it belongs to.
        voice_context: u32,
    },
    NoteOff {
        key: u8,
        voice_context: u32,
    },
    ParamValue {
        target: ParamAddress,
        value: f64,
    },
    ClipStart,
    ClipStop,
}

#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub sample: Sample,
    pub target: NodeId,
    pub payload: EventPayload,
}

/// The flat, immutable, sample-timestamped output of `fontelle-sequencer`'s
/// compilation pass (TDD §11). This is the only thing the audio RT thread ever
/// reads of the document — it never sees clips, prefabs, or the model (INVARIANT 3).
#[derive(Debug, Default, Clone)]
pub struct CompiledTimeline {
    /// Sorted by `sample`.
    pub events: Vec<TimedEvent>,
    /// Sparse seek index, one entry per bar: `(sample, first event index at or after it)`.
    pub index: Vec<(Sample, usize)>,
}

impl CompiledTimeline {
    pub fn empty() -> Self {
        Self::default()
    }
}
