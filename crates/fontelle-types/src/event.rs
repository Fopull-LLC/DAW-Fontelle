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
        /// Where this one note sits in the stereo field, in the range
        /// `Note::pan` is stored in: `-127` hard left, `0` centre, `127` hard
        /// right. [`pan_unit`](crate::pan_unit) is the one conversion into the
        /// `-1.0..=1.0` the pan law takes.
        ///
        /// It is on the note-on rather than a parameter of the node because it
        /// is §16.5's *per-note* pan — two notes sounding together on one
        /// channel may sit in different places, which a node-wide control
        /// cannot express. The channel's own pan is separate and live, and the
        /// two add (see `Voice::render_with_pan`).
        pan: i8,
        /// Cents off this note's own key, in the range `Note::fine_pitch` is
        /// stored in. `0` is the note as written, and it adds to the layer's
        /// tuning and the mod matrix's pitch routes — all three are cents, so
        /// a value here means the same interval wherever the note sits.
        fine_pitch: i16,
        /// How much longer than the instrument says this one note rings after
        /// its note-off, `0..=127`.
        ///
        /// `0` is *the patch's own* release rather than the shortest one, and
        /// that is a compatibility rule rather than an aesthetic one: `0` is
        /// the default every note ever written carries, so any other reading
        /// would change how existing projects sound the day this field
        /// started being read. Higher only ever lengthens.
        release: u8,
        /// §16.5's two free per-note modulation values, `0..=127`, reaching
        /// the voice as `ModSource::NoteModX` and `NoteModY`.
        ///
        /// Free means the *patch* decides what they do: nothing is routed
        /// from them by default, and a note that sets them under a patch that
        /// routes neither sounds exactly like one that does not.
        mod_x: u8,
        mod_y: u8,
        /// TDD §11.4: distinguishes overlapping clips on the same channel so a
        /// note-off only kills the voice it belongs to.
        voice_context: u32,
    },
    NoteOff {
        key: u8,
        voice_context: u32,
    },
    /// Bend whatever is sounding in `voice_context` to `key`, over
    /// `glide_samples` — a **slide note** (FL Studio's), compiled from
    /// `Note::slide`.
    ///
    /// It starts no voice and ends none, which is the whole of what makes it a
    /// slide: the note that was already playing arrives at a new pitch, and
    /// the note-off the score wrote for the *original* key still ends it. A
    /// slide with nothing sounding does nothing.
    ///
    /// In samples rather than seconds because everything else on this wire is:
    /// the RT side has a sample clock and no other unit.
    NoteSlide {
        key: u8,
        glide_samples: u32,
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

    /// The cursor value `events_for_block` should start from to render the
    /// block beginning at `sample` — the first event at or after it.
    ///
    /// This is what a seek needs and what a monotonically-advancing cursor
    /// cannot give: after playing to the end of a piece the cursor sits past
    /// every event in it, so seeking back to bar 1 without repositioning
    /// plays silence. Binary search over the already-sorted `events`, so it
    /// allocates nothing and is safe to call from the audio callback the
    /// moment it observes a seek (INVARIANT 1).
    pub fn cursor_at(&self, sample: Sample) -> usize {
        self.events.partition_point(|event| event.sample < sample)
    }
}

/// Where a note came from: the compiled timeline, or somebody playing.
///
/// The distinction exists for exactly one reason, and it is not cosmetic:
/// **transport stop and seek must cut the timeline's voices and leave the
/// player's alone.** A sequenced voice belongs to a moment in the song that
/// the playhead has left, so carrying it across a seek plays the wrong music
/// over the new position. A live voice belongs to a finger that is still on a
/// key, and cutting it leaves the player holding a silent keyboard until they
/// let go and press again.
///
/// It is a type rather than a reserved `voice_context` value on purpose. The
/// sequencer's contexts are clip indices counting from zero (TDD §11.4), so a
/// "live" sentinel would be a convention holding two unrelated numbering
/// schemes apart by nothing but the unlikelihood of a collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceOrigin {
    /// Started by the compiled timeline.
    #[default]
    Timeline,
    /// Started by live input — a MIDI device, or the UI's keyboard.
    Live,
}

/// Somewhere a live event can be put: a MIDI device's callback thread, a UI
/// keyboard, anything generating events outside the compiled timeline.
///
/// It is a trait here, in the shared vocabulary crate, rather than a concrete
/// queue, because of the dependency rule (TDD §4.1): the queue itself is an
/// RT-thread structure and belongs to `fontelle-engine`, while the things that
/// fill it — `fontelle-midi`, and later the UI — sit outside it and may not
/// depend on it. Both sides can name this.
pub trait EventSink: Send {
    /// Queues one event. Returns `false` if it could not be taken, which for a
    /// bounded queue means the far end has stopped reading. Implementations
    /// must never block: a caller may be a device callback with a deadline of
    /// its own.
    fn send(&mut self, event: TimedEvent) -> bool;
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
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            },
        }
    }

    #[test]
    fn cursor_at_finds_the_first_event_at_or_after_a_sample() {
        let timeline = CompiledTimeline {
            events: vec![note_on(0), note_on(100), note_on(100), note_on(300)],
            index: Vec::new(),
        };

        assert_eq!(timeline.cursor_at(0), 0);
        assert_eq!(
            timeline.cursor_at(100),
            1,
            "an event exactly at the seek target has not happened yet — it plays"
        );
        assert_eq!(
            timeline.cursor_at(101),
            3,
            "both events at 100 are behind us"
        );
        assert_eq!(timeline.cursor_at(1_000), 4, "past the end is the end");
    }

    #[test]
    fn cursor_at_rewinds_a_cursor_that_had_run_to_the_end() {
        let timeline = CompiledTimeline {
            events: vec![note_on(0), note_on(200)],
            index: Vec::new(),
        };
        let mut cursor = 0;
        timeline.events_for_block(&mut cursor, 0..1_000);
        assert_eq!(cursor, 2, "played through everything");

        cursor = timeline.cursor_at(0);
        let block = timeline.events_for_block(&mut cursor, 0..128);
        assert_eq!(
            block.len(),
            1,
            "seeking back to the start plays the piece again"
        );
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
