//! The audio thread's end of an analyser: a lock-free ring of recent samples.
//!
//! *"currently theres no eq monitor graph drawn to view the frequency spectrum
//! and make edits based off it and see in realtime."*
//!
//! # The split
//!
//! The RT side does the cheapest thing that can possibly work — **copy**. One
//! relaxed atomic store per frame into a fixed ring, and a counter. No lock, no
//! allocation, no transform, and nothing that can block the audio thread on a
//! window that has stopped reading (INVARIANT 1).
//!
//! The window does the rest: it takes the most recent [`SPECTRUM_SIZE`] samples
//! and transforms them, once a frame, and only while an EQ's window is open —
//! see [`fontelle_dsp::SpectrumAnalyser`]. An analyser that ran on the audio
//! thread would make every mix pay for a picture nobody is looking at.
//!
//! # What it is honest about
//!
//! A reader can be overtaken: if the window stalls for longer than the ring
//! holds, it reads a block with a seam in it. That is a **spectrum drawn one
//! frame wrong**, which nobody can see, and it is the right trade against any
//! amount of synchronisation on the audio thread. What it can never do is tear
//! *within* a sample, because each is one aligned 32-bit store.

use std::sync::atomic::{AtomicU64, Ordering};

use fontelle_dsp::SPECTRUM_SIZE;

/// How many samples the ring holds.
///
/// Twice the transform's window, so the reader always has a whole one behind
/// the write head even when it arrives just after a block landed.
const RING: usize = SPECTRUM_SIZE * 2;

/// One insert's analyser tap.
///
/// Shared: the graph's node holds one end and the session the other, both
/// through an `Arc`, in the same way [`crate::TrackControls`] is shared. It
/// survives a graph rebuild for the same reason that does — an EQ window open
/// while somebody adds a channel must not go blank.
pub struct SpectrumTap {
    /// f32 bits. `AtomicU32` rather than a `Vec<f32>` behind a lock, because
    /// the writer is the audio thread.
    samples: Vec<std::sync::atomic::AtomicU32>,
    /// How many frames have ever been written. Wraps at 2^64, which at 48 kHz
    /// is twelve million years.
    written: AtomicU64,
}

impl Default for SpectrumTap {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumTap {
    pub fn new() -> Self {
        Self {
            samples: (0..RING)
                .map(|_| std::sync::atomic::AtomicU32::new(0))
                .collect(),
            written: AtomicU64::new(0),
        }
    }

    /// Copies one block in, mono.
    ///
    /// **Mono**, summed and halved: an analyser is about *where the energy is*,
    /// not about the stereo image, and two overlaid curves at slightly
    /// different heights is a picture that reads as neither.
    ///
    /// RT-safe: one relaxed store per frame and one at the end, no allocation.
    pub fn write(&self, channels: &[&mut [f32]]) {
        let frames = channels.first().map_or(0, |c| c.len());
        if frames == 0 || self.samples.is_empty() {
            return;
        }
        let start = self.written.load(Ordering::Relaxed) as usize;
        let scale = 1.0 / channels.len().max(1) as f32;
        for frame in 0..frames {
            let mut sum = 0.0f32;
            for channel in channels {
                sum += channel.get(frame).copied().unwrap_or(0.0);
            }
            let slot = (start + frame) % self.samples.len();
            self.samples[slot].store((sum * scale).to_bits(), Ordering::Relaxed);
        }
        // Published last, so a reader that sees this count knows every sample
        // under it has been stored.
        self.written
            .store((start + frames) as u64, Ordering::Release);
    }

    /// The most recent [`SPECTRUM_SIZE`] samples, oldest first, into `out`.
    ///
    /// Off the audio thread. `out` is cleared and refilled rather than
    /// returned, so a window drawing every frame allocates nothing.
    ///
    /// Fewer than a full window is normal for the first fraction of a second
    /// after a graph is built, and the analyser pads rather than refusing —
    /// see [`fontelle_dsp::SpectrumAnalyser::analyse`].
    pub fn read(&self, out: &mut Vec<f32>) {
        out.clear();
        let written = self.written.load(Ordering::Acquire) as usize;
        let want = SPECTRUM_SIZE.min(written).min(self.samples.len());
        let start = written - want;
        out.reserve(want);
        for i in 0..want {
            let slot = (start + i) % self.samples.len();
            out.push(f32::from_bits(self.samples[slot].load(Ordering::Relaxed)));
        }
    }

    /// How many frames have gone through it — what tells a window whether
    /// anything is playing at all.
    pub fn frames_written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }
}
