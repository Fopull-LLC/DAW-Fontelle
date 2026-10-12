//! One buffer's round trip.
//!
//! The driver calls the helper once per buffer. The helper:
//!
//! 1. writes the driver's input into input slot `seq & 1` and the frame
//!    count into `request_frames`;
//! 2. stores `request_seq = seq` (release) and rings Fontelle's doorbell;
//! 3. waits, up to the deadline, for `reply_seq == seq`;
//! 4. answered: copies output slot `seq & 1` to the driver. Late: plays
//!    silence and counts it in `late_replies`.
//!
//! Fontelle's audio thread waits for `request_seq` to move, renders into
//! the output slot, stores `render_ns`, stores `reply_seq = seq` (release)
//! and rings the helper's doorbell. That is the whole of it: synchronous,
//! inside the driver's own callback, so it adds no buffer of latency — the
//! cost is two wake-ups, measured into `handoff_last_ns`.
//!
//! A Fontelle that falls behind does not desynchronise anything: it answers
//! the newest `request_seq` it sees, and a cycle it never answered was
//! already counted late and played as silence.
//!
//! Ownership of the slots, which is what makes the `unsafe` in here sound:
//! between storing `request_seq = seq` and either seeing the reply or giving
//! up, slot `seq & 1` is Fontelle's; otherwise it is the helper's. A
//! Fontelle two whole buffers late could still be reading an input slot the
//! helper writes again — that costs a torn input buffer at a moment that is
//! already a dropout, never memory safety (the slots are plain `f32`).

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::layout::{Shared, State};

/// How one side wakes the other.
///
/// `ring` follows a store to `word`; `sleep` returns once `word` is no
/// longer `seen`, or after `timeout`, or spuriously — callers loop on the
/// word itself. On Linux it is a futex on the shared word; on Windows a
/// named auto-reset event (`crate::os`).
pub trait Doorbell: Send + Sync {
    fn ring(&self, word: &AtomicU32);
    fn sleep(&self, word: &AtomicU32, seen: u32, timeout: Duration);
}

/// How a cycle went, from the helper's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    /// Fontelle answered in time; the output was handed over.
    Answered,
    /// The deadline passed first: play silence. Counted.
    Late,
    /// Fontelle closed its end. Stop the driver.
    ClientGone,
}

/// The helper's end.
pub struct HelperSide<'a> {
    shared: &'a Shared,
    request: &'a dyn Doorbell,
    reply: &'a dyn Doorbell,
    seq: u32,
}

impl<'a> HelperSide<'a> {
    /// `request` is the bell Fontelle sleeps on; `reply` the one the helper
    /// sleeps on.
    pub fn new(shared: &'a Shared, request: &'a dyn Doorbell, reply: &'a dyn Doorbell) -> Self {
        let seq = shared.header().request_seq.load(Ordering::Acquire);
        Self {
            shared,
            request,
            reply,
            seq,
        }
    }

    /// One buffer: `fill_input` writes `frames × inputs` samples, and
    /// `take_output` is handed `frames × outputs` samples if Fontelle
    /// answers within `deadline`.
    pub fn exchange(
        &mut self,
        frames: usize,
        deadline: Duration,
        fill_input: impl FnOnce(&mut [f32]),
        take_output: impl FnOnce(&[f32]),
    ) -> Exchange {
        let h = self.shared.header();
        if self.shared.state() == Some(State::ClientClosed) {
            return Exchange::ClientGone;
        }
        let layout = self.shared.layout();
        let frames = frames.min(layout.max_frames as usize);
        let seq = self.seq.wrapping_add(1);
        self.seq = seq;
        let slot = (seq & 1) as usize;
        {
            // SAFETY: slot `seq & 1` is the helper's until `request_seq`
            // names it (module comment).
            let input = unsafe { self.shared.input_slot(slot) };
            fill_input(&mut input[..frames * layout.max_inputs as usize]);
        }
        h.request_frames.store(frames as u32, Ordering::Relaxed);
        h.deadline_ns
            .store(saturating_ns(deadline), Ordering::Relaxed);
        let started = Instant::now();
        h.request_seq.store(seq, Ordering::Release);
        h.cycles.fetch_add(1, Ordering::Relaxed);
        self.request.ring(&h.request_seq);

        let until = started + deadline;
        loop {
            let seen = h.reply_seq.load(Ordering::Acquire);
            if seen == seq {
                let round_trip = saturating_ns(started.elapsed());
                let render = h.render_ns.load(Ordering::Relaxed);
                let handoff = round_trip.saturating_sub(render);
                h.handoff_last_ns.store(handoff, Ordering::Relaxed);
                h.handoff_max_ns.fetch_max(handoff, Ordering::Relaxed);
                // SAFETY: answered — the slot is the helper's again, and
                // Fontelle's writes to it happened before its release store.
                let output = unsafe { self.shared.output_slot(slot) };
                take_output(&output[..frames * layout.max_outputs as usize]);
                return Exchange::Answered;
            }
            if self.shared.state() == Some(State::ClientClosed) {
                return Exchange::ClientGone;
            }
            let now = Instant::now();
            if now >= until {
                h.late_replies.fetch_add(1, Ordering::Relaxed);
                return Exchange::Late;
            }
            self.reply.sleep(&h.reply_seq, seen, until - now);
        }
    }

    /// Says the helper has stopped and wakes Fontelle to see it.
    pub fn stop(&self) {
        self.shared.set_state(State::HelperStopped);
        self.request.ring(&self.shared.header().request_seq);
    }
}

/// What Fontelle's audio thread woke to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// A buffer to render.
    Cycle(Cycle),
    /// Nothing within the timeout. The helper may be gone: the caller checks.
    Idle,
    /// The helper stopped.
    Stopped,
}

/// One buffer to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycle {
    pub seq: u32,
    pub frames: usize,
}

/// Fontelle's end.
pub struct ClientSide<'a> {
    shared: &'a Shared,
    request: &'a dyn Doorbell,
    reply: &'a dyn Doorbell,
    seen: u32,
}

impl<'a> ClientSide<'a> {
    /// Starts from the last cycle answered, so a cycle the helper rang for
    /// before this thread was ready is still rendered.
    pub fn new(shared: &'a Shared, request: &'a dyn Doorbell, reply: &'a dyn Doorbell) -> Self {
        let seen = shared.header().reply_seq.load(Ordering::Acquire);
        Self {
            shared,
            request,
            reply,
            seen,
        }
    }

    /// Sleeps until the helper rings, it stops, or `timeout` passes.
    pub fn wait(&mut self, timeout: Duration) -> Wake {
        let h = self.shared.header();
        let until = Instant::now() + timeout;
        loop {
            if self.shared.state() == Some(State::HelperStopped) {
                return Wake::Stopped;
            }
            let seq = h.request_seq.load(Ordering::Acquire);
            if seq != self.seen {
                self.seen = seq;
                let frames = h.request_frames.load(Ordering::Relaxed) as usize;
                return Wake::Cycle(Cycle {
                    seq,
                    frames: frames.min(self.shared.layout().max_frames as usize),
                });
            }
            let now = Instant::now();
            if now >= until {
                return Wake::Idle;
            }
            self.request.sleep(&h.request_seq, seq, until - now);
        }
    }

    /// The cycle's input (`frames × inputs`) and output (`frames ×
    /// outputs`), interleaved.
    pub fn buffers(&mut self, cycle: Cycle) -> (&[f32], &mut [f32]) {
        let layout = self.shared.layout();
        let slot = (cycle.seq & 1) as usize;
        let frames = cycle.frames.min(layout.max_frames as usize);
        // SAFETY: the helper rang for `cycle.seq`, so its slot is ours until
        // `answer`; `&mut self` keeps this the only borrow on this side.
        unsafe {
            let input = &self.shared.input_slot(slot)[..frames * layout.max_inputs as usize];
            let output = &mut self.shared.output_slot(slot)[..frames * layout.max_outputs as usize];
            (input, output)
        }
    }

    /// Hands the cycle back: `render_ns` is how long rendering took.
    pub fn answer(&mut self, cycle: Cycle, render_ns: u32) {
        let h = self.shared.header();
        h.render_ns.store(render_ns, Ordering::Relaxed);
        h.reply_seq.store(cycle.seq, Ordering::Release);
        self.reply.ring(&h.reply_seq);
    }

    /// Says Fontelle has closed its end and wakes the helper to see it.
    pub fn close(&self) {
        self.shared.set_state(State::ClientClosed);
        self.reply.ring(&self.shared.header().reply_seq);
    }
}

/// A duration in nanoseconds, as the header's 32-bit fields hold it (4.2 s
/// at most — far past any buffer).
pub fn saturating_ns(d: Duration) -> u32 {
    u32::try_from(d.as_nanos()).unwrap_or(u32::MAX)
}
