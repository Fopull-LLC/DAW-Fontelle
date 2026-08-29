//! Handing a freshly built graph to a stream that is already running.
//!
//! [`crate::timeline_channel`] does this for the notes. This does it for the
//! *instruments*: adding a channel, choosing a soundfont, changing a preset or
//! moving a fader all rebuild the [`CompiledGraph`], and until this existed the
//! only way to get the result to the audio thread was to tear the device down
//! and open it again.
//!
//! # Why this is not a `triple_buffer`
//!
//! `triple_buffer` needs `T: Clone` — it clones the initial value into three
//! slots. A `CompiledGraph` is a `Vec<Box<dyn AudioNode>>`; a sampler node
//! holds a `Patch` and an `Arc<SampleStore>`, and there is no meaningful clone
//! of one. So this is the other standard shape: a bounded SPSC queue forward,
//! and **a second one back**.
//!
//! # The return queue is the whole point
//!
//! Swapping the graph is trivial. Not *freeing* the old one on the audio thread
//! is the hard part, and it is INVARIANT 1 exactly: dropping a `CompiledGraph`
//! frees every node, every `Patch`, and every buffer in it — a deallocation
//! storm on a thread that must never deallocate at all. So the RT side moves
//! the graph it stopped using into a return queue, and
//! [`GraphPublisher::reclaim`] frees it on the thread that built its
//! replacement.
//!
//! That is why [`GraphSource::take_update`] checks for **room to return the old
//! graph before it takes a new one**. With the return queue full, the honest
//! thing is to keep playing the graph we have; a swap we cannot undo would put
//! the free on the audio thread, which is the one outcome that is not allowed.
//! The publisher empties it on its next visit and the swap goes through then.
//!
//! This replaces the `ManuallyDrop` leak `AudioDevice` used for the graph — the
//! documented, deliberate leak that was acceptable only because the process
//! exited moments after `stop()`. A DAW that runs for hours and changes its
//! instruments cannot leak one graph per change.

use rtrb::{Consumer, Producer, RingBuffer};

use crate::graph::CompiledGraph;

/// How many graphs may be in flight in each direction.
///
/// Small on purpose: a graph rebuild is a structural change — a channel added,
/// an instrument chosen, a fader moved — not something that happens per
/// mouse-move the way a timeline recompile does. Four is room for a burst of
/// them between two audio callbacks, which is about 12 ms.
pub const GRAPH_QUEUE_CAPACITY: usize = 4;

/// The model thread's end.
pub struct GraphPublisher {
    outgoing: Producer<CompiledGraph>,
    returned: Consumer<CompiledGraph>,
    /// A graph that did not fit in the queue, kept so it is not lost. Only ever
    /// the newest — an older one still waiting here is replaced (and freed)
    /// rather than queued behind it.
    pending: Option<CompiledGraph>,
}

/// The RT thread's end. Owns the live graph.
pub struct GraphSource {
    current: CompiledGraph,
    incoming: Consumer<CompiledGraph>,
    returning: Producer<CompiledGraph>,
}

/// Opens a channel around `initial`, which is the graph the RT side plays until
/// something else is published — there is no "no graph yet" state in the
/// callback.
///
/// `initial` and everything published afterwards must already have been through
/// [`CompiledGraph::prepare`]: that is where nodes size their internal buffers,
/// it allocates, and it therefore belongs on this side of the channel.
pub fn graph_channel(initial: CompiledGraph) -> (GraphPublisher, GraphSource) {
    let (outgoing, incoming) = RingBuffer::new(GRAPH_QUEUE_CAPACITY);
    let (returning, returned) = RingBuffer::new(GRAPH_QUEUE_CAPACITY);
    (
        GraphPublisher {
            outgoing,
            returned,
            pending: None,
        },
        GraphSource {
            current: initial,
            incoming,
            returning,
        },
    )
}

impl GraphPublisher {
    /// Publishes `graph`, already prepared, and frees whatever has come back.
    ///
    /// Never blocks and never fails: a graph that does not fit in the queue is
    /// held as [`pending`](Self::pump) and goes out on the next
    /// [`pump`](Self::pump) or `publish`. What it does *not* do is queue two of
    /// them — a superseded graph is freed here, immediately, because the only
    /// one worth playing is the newest.
    pub fn publish(&mut self, graph: CompiledGraph) {
        self.reclaim();
        // Dropped here, on this thread, and before the new one is stored: an
        // older pending graph is a graph nobody will ever hear.
        self.pending = Some(graph);
        self.pump();
    }

    /// Tries again to send a graph that did not fit, and frees what has come
    /// back. Cheap enough to call once a frame, which is what the window does.
    ///
    /// Returns whether anything is still waiting to go out.
    pub fn pump(&mut self) -> bool {
        self.reclaim();
        if let Some(graph) = self.pending.take()
            && let Err(rtrb::PushError::Full(graph)) = self.outgoing.push(graph)
        {
            self.pending = Some(graph);
        }
        self.pending.is_some()
    }

    /// Frees every graph the RT side has handed back. Returns how many.
    ///
    /// **This is where a `CompiledGraph` dies.** Nothing else in the workspace
    /// may drop one that has been live.
    pub fn reclaim(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(graph) = self.returned.pop() {
            drop(graph);
            freed += 1;
        }
        freed
    }
}

impl GraphSource {
    /// **RT.** The graph to render this block with.
    pub fn current(&mut self) -> &mut CompiledGraph {
        &mut self.current
    }

    /// **RT.** Swaps in the newest published graph, if there is one and there
    /// is room to hand back the one it replaces.
    ///
    /// Drains rather than taking one: a caller that woke to find several
    /// waiting should arrive at the newest in this block, not walk a backlog of
    /// structural changes one audio block at a time.
    ///
    /// Returns whether the live graph changed — the caller's cue to silence
    /// whatever the old one was still sounding, since the voices belonged to
    /// nodes that no longer exist.
    pub fn take_update(&mut self) -> bool {
        let mut swapped = false;
        // The room check comes first, every time round: without somewhere to
        // put the old graph, taking a new one would mean freeing it here.
        while self.returning.slots() > 0 {
            let Ok(next) = self.incoming.pop() else { break };
            let old = std::mem::replace(&mut self.current, next);
            if let Err(rtrb::PushError::Full(old)) = self.returning.push(old) {
                // Unreachable: this is the only producer and the slot was just
                // counted. Leaking beats freeing on the audio thread, so if the
                // impossible happens it stays leaked and says so.
                debug_assert!(false, "the return queue lost a slot between check and push");
                std::mem::forget(old);
            }
            swapped = true;
        }
        swapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::BufferPool;

    fn empty_graph() -> CompiledGraph {
        CompiledGraph {
            schedule: Vec::new(),
            buffer_pool: BufferPool::with_capacity(1, 1),
        }
    }

    /// The guard that keeps a free off the audio thread, tested directly.
    ///
    /// `GraphPublisher::publish` empties the return queue before it fills the
    /// forward one, and the two queues are the same size — so this state is
    /// unreachable through the public API, which is the point of the design and
    /// the reason it cannot be reached from `tests/graph_channel.rs`. It is
    /// still the property everything else rests on, so it is checked here,
    /// where the queue can be filled by hand.
    #[test]
    fn a_full_return_queue_blocks_the_swap_rather_than_freeing_on_the_rt_side() {
        let (mut publisher, mut source) = graph_channel(empty_graph());
        // Published first, then the return queue filled behind the publisher's
        // back — `publish` empties it, which is exactly why this state cannot
        // be built the other way round.
        publisher.publish(empty_graph());
        for _ in 0..GRAPH_QUEUE_CAPACITY {
            source
                .returning
                .push(empty_graph())
                .expect("the return queue starts empty");
        }
        assert!(
            !source.take_update(),
            "with nowhere to hand the old graph back, the RT side must keep the \
             one it is playing"
        );
        // And the graph that was published is still there, waiting, not lost.
        assert_eq!(publisher.reclaim(), GRAPH_QUEUE_CAPACITY);
        assert!(source.take_update());
    }
}
