//! Handing a freshly compiled timeline to a stream that is already running.
//!
//! "Loop and edit while playing" in the first-usable gate rests on this, and so
//! does every note the piano roll draws: the model thread recompiles, publishes,
//! and the RT thread picks the result up at its next block boundary.
//!
//! `triple_buffer`, per TDD §11.3, and the reasons are all INVARIANT 1:
//!
//! - **No lock.** The RT thread swaps an index; it never waits for the writer.
//! - **No allocation, and no deallocation.** [`TimelineSource`] hands out
//!   `&CompiledTimeline` and nothing else, so the RT side can never come to own
//!   one and drop it. The old events are freed inside [`TimelinePublisher::publish`],
//!   on the thread that published over them.
//! - **No backlog.** Dragging a note produces one of these per mouse-move. The
//!   RT thread takes the newest and the rest are simply overwritten — it must
//!   never work through a queue.

use fontelle_types::CompiledTimeline;

/// The model thread's end.
pub struct TimelinePublisher {
    input: triple_buffer::Input<CompiledTimeline>,
}

/// The RT thread's end.
pub struct TimelineSource {
    output: triple_buffer::Output<CompiledTimeline>,
}

/// Opens a channel, primed with `initial` so the RT side always has something
/// to read — there is no "no timeline yet" state to handle in the callback.
pub fn timeline_channel(initial: CompiledTimeline) -> (TimelinePublisher, TimelineSource) {
    let (input, output) = triple_buffer::triple_buffer(&initial);
    (TimelinePublisher { input }, TimelineSource { output })
}

impl TimelinePublisher {
    /// Publishes `timeline`, atomically as far as the reader is concerned.
    ///
    /// The timeline this overwrites is dropped **here**, on the calling thread.
    pub fn publish(&mut self, timeline: CompiledTimeline) {
        self.input.write(timeline);
    }
}

impl TimelineSource {
    /// **RT.** The newest published timeline, taking it if one has arrived.
    ///
    /// A swap of two indices in the worst case, and nothing at all when
    /// nothing has been published.
    pub fn current(&mut self) -> &CompiledTimeline {
        self.output.read()
    }

    /// Whether [`current`](Self::current) would pick something new up.
    ///
    /// Worth asking separately because taking a new timeline invalidates the
    /// reader's event cursor — see [`crate::TransportReader::retarget`] — and
    /// repositioning it every block would be exactly the per-block work a
    /// cursor exists to avoid.
    pub fn has_update(&self) -> bool {
        self.output.updated()
    }
}
