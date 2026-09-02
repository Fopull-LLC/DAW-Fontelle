//! The audio thread's end of an **external sidechain**: one track's bus,
//! left where another track's insert can read it in the same block
//! (`docs/effects-catalogue.md` §2.1, TDD §13.4's "sidechain input from any
//! mixer track").
//!
//! # Why a tap rather than a second input on the node
//!
//! The graph hands a node its buffers by index, and its contract is deliberately
//! narrow: a node either processes its bus in place or routes one bus to
//! another of the same width, at most two buffers a side
//! (`CompiledGraph::process_block`). A key is a *third* set — a bus this node
//! reads and does not write — and widening that dispatch would put a new shape
//! into the hottest, most carefully argued loop in the engine for one feature.
//!
//! So the key travels the way the analyser's samples already do: a small node
//! on the source track's bus copies the block into a shared ring, and the
//! insert reads it out. The two are guaranteed to run in the right order
//! because the compiler treats a key as a feeding edge like a send, so the
//! source track is scheduled first — see `depth_to_master` in
//! `fontelle-app/src/realise.rs`.
//!
//! # What it is honest about
//!
//! Both ends are the audio thread, in the same pass, one after the other, so
//! there is no reader to overtake and no stall to survive — unlike
//! [`crate::SpectrumTap`], whose reader is a window. The atomics are here for
//! the `Arc`, not for a race: they are relaxed loads and stores, which on
//! every target this runs on are ordinary moves.
//!
//! A key read before its source has ever run reads **silence**, which is the
//! right answer: a compressor keyed to a track that is not playing should not
//! be compressing.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// One insert's view of another track's bus.
///
/// Shared through an `Arc`, like [`crate::TrackControls`], and kept across a
/// graph rebuild for the same reason: a key that went silent for a block every
/// time somebody added a channel would be a duck that stuttered.
pub struct KeyTap {
    /// The most recent block, **mono**. A detector asks "how loud is this",
    /// not "where is it", and every detector in `fontelle-fx` takes a single
    /// slice for exactly that reason.
    ///
    /// `f32` bits in atomics rather than a `Vec<f32>` behind a lock, because
    /// both ends are the audio thread and a lock there is INVARIANT 1's
    /// whole subject.
    samples: Vec<AtomicU32>,
    /// How many of them this block wrote.
    frames: AtomicUsize,
}

impl KeyTap {
    /// Room for `capacity` frames — the graph's maximum block size.
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: (0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect(),
            frames: AtomicUsize::new(0),
        }
    }

    /// Copies one block in, summed to mono.
    ///
    /// Summed rather than averaged: a mono source panned centre arrives on
    /// both sides at the same level, and halving it would make a kick keying a
    /// compressor read six decibels quieter than the same kick on a mono bus.
    /// The **loudest side** is what a stereo-linked detector wants, and that is
    /// what this takes.
    ///
    /// RT-safe: one relaxed store per frame, no allocation.
    pub fn write(&self, channels: &[&mut [f32]]) {
        let frames = channels
            .iter()
            .map(|c| c.len())
            .min()
            .unwrap_or(0)
            .min(self.samples.len());
        for frame in 0..frames {
            let mut peak = 0.0f32;
            for channel in channels {
                let sample = channel[frame];
                if sample.abs() > peak.abs() {
                    peak = sample;
                }
            }
            self.samples[frame].store(peak.to_bits(), Ordering::Relaxed);
        }
        self.frames.store(frames, Ordering::Release);
    }

    /// Copies this block's key into `out`, and says how many frames it wrote.
    ///
    /// The rest of `out` is silenced rather than left holding the last block:
    /// a short block after a long one would otherwise key the tail of the one
    /// before it.
    pub fn read_into(&self, out: &mut [f32]) -> usize {
        let frames = self
            .frames
            .load(Ordering::Acquire)
            .min(out.len())
            .min(self.samples.len());
        for (frame, slot) in out.iter_mut().enumerate().take(frames) {
            *slot = f32::from_bits(self.samples[frame].load(Ordering::Relaxed));
        }
        out[frames..].fill(0.0);
        frames
    }

    /// How many frames the source wrote last time it ran. Zero before it has
    /// ever run, which reads as silence.
    pub fn frames(&self) -> usize {
        self.frames.load(Ordering::Acquire)
    }

    /// Forgets the last block. What transport stop leaves behind.
    pub fn silence(&self) {
        self.frames.store(0, Ordering::Release);
    }
}

/// The node that fills a [`KeyTap`] from the bus it is scheduled on.
///
/// It changes nothing: it is scheduled in place on the source track's bus and
/// copies what is there, the way a send takes a copy rather than moving the
/// signal. A key that altered the track it listened to would be a sidechain
/// you could hear on the wrong channel.
pub struct KeyTapNode {
    tap: std::sync::Arc<KeyTap>,
}

impl KeyTapNode {
    pub fn new(tap: std::sync::Arc<KeyTap>) -> Self {
        Self { tap }
    }
}

impl crate::AudioNode for KeyTapNode {
    /// Nothing to size: the ring is allocated with the tap, off the audio
    /// thread, at the graph's maximum block size.
    fn prepare(&mut self, _ctx: &crate::PrepareContext) {}

    fn process(&mut self, ctx: &mut crate::ProcessContext) {
        self.tap.write(ctx.outputs);
    }

    /// A transport stop leaves the key silent rather than holding the last
    /// block, so a compressor keyed to a stopped track opens instead of
    /// staying ducked on a stale kick.
    fn reset(&mut self) {
        self.tap.silence();
    }

    fn debug_name(&self) -> &'static str {
        "KeyTapNode"
    }

    fn params(&self) -> &dyn crate::ParamSet {
        &crate::nodes::EmptyParams
    }
}
