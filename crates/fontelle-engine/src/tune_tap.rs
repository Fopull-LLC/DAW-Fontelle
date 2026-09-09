//! The audio thread's end of the pitch trace: a lock-free ring of what the
//! corrector did, hop by hop (`docs/tune-plan.md` §7.3).
//!
//! The analyser tap's sibling, and the same honesty. The RT side does the
//! cheapest thing that can possibly work — four relaxed atomic stores per hop
//! and a counter. No lock, no allocation, and nothing that can block the audio
//! thread on a window that has stopped reading (INVARIANT 1).
//!
//! A reader that stalls for longer than the ring holds reads a trace with a
//! seam in it. That is **a pitch line drawn one frame wrong**, which nobody can
//! see, and it is the right trade against any amount of synchronisation on the
//! audio thread. What it can never do is tear *within* a field, because each is
//! one aligned 32-bit store.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use fontelle_types::TuneFrame;

/// How many hops the ring holds.
///
/// Four seconds is what the window draws, and the fastest a hop can arrive is
/// every 32 samples at 96 kHz — three thousand a second. Twelve thousand
/// covers that, and it is sized for it once rather than resized when somebody
/// changes the mode.
pub const TUNE_TRACE_FRAMES: usize = 12_000;

/// One insert's pitch trace.
///
/// Shared: the graph's node holds one end and the session the other, both
/// through an `Arc`, in the same way [`crate::SpectrumTap`] is. It survives a
/// graph rebuild for the same reason that does — a window open while somebody
/// adds a channel must not go blank.
pub struct TuneTap {
    /// Four words a frame, interleaved: sung, out, target, flags. `f32::bits`
    /// for the three floats, because the writer is the audio thread.
    words: Vec<AtomicU32>,
    /// How many frames have ever been written.
    written: AtomicU64,
}

impl Default for TuneTap {
    fn default() -> Self {
        Self::new()
    }
}

impl TuneTap {
    pub fn new() -> Self {
        Self {
            words: (0..TUNE_TRACE_FRAMES * 4)
                .map(|_| AtomicU32::new(0))
                .collect(),
            written: AtomicU64::new(0),
        }
    }

    /// Copies this block's hops in. RT-safe: four relaxed stores a frame and
    /// one release at the end, no allocation.
    pub fn write(&self, frames: &[TuneFrame]) {
        if frames.is_empty() || self.words.is_empty() {
            return;
        }
        let start = self.written.load(Ordering::Relaxed) as usize;
        for (index, frame) in frames.iter().enumerate() {
            let slot = ((start + index) % TUNE_TRACE_FRAMES) * 4;
            self.words[slot].store(frame.sung_cents.to_bits(), Ordering::Relaxed);
            self.words[slot + 1].store(frame.out_cents.to_bits(), Ordering::Relaxed);
            self.words[slot + 2].store(frame.target_cents.to_bits(), Ordering::Relaxed);
            self.words[slot + 3].store(frame.flags, Ordering::Relaxed);
        }
        // Published last, so a reader that sees this count knows every frame
        // under it has been stored.
        self.written
            .store((start + frames.len()) as u64, Ordering::Release);
    }

    /// The most recent `want` frames, oldest first.
    ///
    /// Off the audio thread, once a frame, which is what `spectrum` already
    /// does — the allocation is the window's and costs the mix nothing.
    pub fn read(&self, want: usize) -> Vec<TuneFrame> {
        let written = self.written.load(Ordering::Acquire) as usize;
        let want = want.min(written).min(TUNE_TRACE_FRAMES);
        let start = written - want;
        (0..want)
            .map(|i| {
                let slot = ((start + i) % TUNE_TRACE_FRAMES) * 4;
                TuneFrame {
                    sung_cents: f32::from_bits(self.words[slot].load(Ordering::Relaxed)),
                    out_cents: f32::from_bits(self.words[slot + 1].load(Ordering::Relaxed)),
                    target_cents: f32::from_bits(self.words[slot + 2].load(Ordering::Relaxed)),
                    flags: self.words[slot + 3].load(Ordering::Relaxed),
                }
            })
            .collect()
    }

    /// How many hops have gone through it — what tells a window whether
    /// anything is playing at all.
    pub fn frames_written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }
}
