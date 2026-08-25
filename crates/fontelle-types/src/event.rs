use std::ops::Range;

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

    /// Returns the contiguous sub-slice of `events` whose `sample` falls in
    /// `range` (half-open: `range.end` itself belongs to the *next* block),
    /// advancing `cursor` past them. Callers drive this once per audio
    /// callback block with sequential, non-overlapping ranges and a `cursor`
    /// they keep across calls — `AudioDevice::start_output_stream` is the
    /// real one (TDD §5.3's "nodes that can handle sample-accurate events
    /// internally do so"). Binary-search-free linear advance from wherever
    /// `cursor` already is, and a plain slice into the existing `Vec` as the
    /// return value — no allocation, so it's safe to call from the RT thread
    /// (INVARIANT 1).
    pub fn events_for_block(&self, cursor: &mut usize, range: Range<Sample>) -> &[TimedEvent] {
        while *cursor < self.events.len() && self.events[*cursor].sample < range.start {
            *cursor += 1;
        }
        let start = *cursor;
        while *cursor < self.events.len() && self.events[*cursor].sample < range.end {
            *cursor += 1;
        }
        &self.events[start..*cursor]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(sample: Sample) -> TimedEvent {
        TimedEvent {
            sample,
            target: NodeId::default(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 100,
                voice_context: 0,
            },
        }
    }

    #[test]
    fn empty_timeline_yields_no_events_and_leaves_cursor_at_zero() {
        let timeline = CompiledTimeline::empty();
        let mut cursor = 0;
        let slice = timeline.events_for_block(&mut cursor, 0..128);
        assert!(slice.is_empty());
        assert_eq!(cursor, 0);
    }

    #[test]
    fn sequential_blocks_partition_events_by_sample_and_advance_the_cursor() {
        let timeline = CompiledTimeline {
            events: vec![note_on(0), note_on(50), note_on(200)],
            index: Vec::new(),
        };

        let mut cursor = 0;
        let first_block = timeline.events_for_block(&mut cursor, 0..128);
        assert_eq!(first_block.len(), 2, "samples 0 and 50 fall in [0, 128)");
        assert_eq!(cursor, 2);

        let second_block = timeline.events_for_block(&mut cursor, 128..256);
        assert_eq!(second_block.len(), 1, "sample 200 falls in [128, 256)");
        assert_eq!(second_block[0].sample, 200);
        assert_eq!(cursor, 3);

        let third_block = timeline.events_for_block(&mut cursor, 256..384);
        assert!(third_block.is_empty());
        assert_eq!(cursor, 3);
    }

    #[test]
    fn a_boundary_event_belongs_to_the_block_it_starts_not_the_one_it_ends() {
        let timeline = CompiledTimeline {
            events: vec![note_on(128)],
            index: Vec::new(),
        };
        let mut cursor = 0;

        let first_block = timeline.events_for_block(&mut cursor, 0..128);
        assert!(
            first_block.is_empty(),
            "sample 128 is not < range.end (128), so it must not appear in [0, 128)"
        );

        let second_block = timeline.events_for_block(&mut cursor, 128..256);
        assert_eq!(second_block.len(), 1, "sample 128 belongs to [128, 256)");
    }

    #[test]
    fn multiple_events_at_the_same_sample_all_land_in_one_slice() {
        let timeline = CompiledTimeline {
            events: vec![note_on(10), note_on(10), note_on(10)],
            index: Vec::new(),
        };
        let mut cursor = 0;
        let block = timeline.events_for_block(&mut cursor, 0..128);
        assert_eq!(block.len(), 3);
    }
}
