//! What a DisgustingBeat's window is shown: the memory, and the read head on
//! it (`docs/disgusting-beat-plan.md` §7.2).
//!
//! `tune_tap.rs`'s sibling and the same honesty: the RT side does the cheapest
//! thing that can work — relaxed atomic stores, no lock, no allocation, and
//! nothing that can block the audio thread on a window that has stopped
//! reading (INVARIANT 1). A reader that misses a block draws one frame late,
//! which nobody can see.
//!
//! The picture is the point. A hold's read head stops dead on the sample it
//! froze; a reverse runs backwards over the waveform; a stutter jumps back
//! three times. Somebody who watches that once does not have to be told what
//! the curve means — which is the thing the plugin this is modelled on never
//! shows you.

use std::sync::atomic::{AtomicU32, Ordering};

use fontelle_fx::{DISGUSTING_BEAT_BUCKETS, DisgustingBeatFrame};

/// One insert's memory picture.
///
/// Shared through an `Arc` like [`crate::TuneTap`], and it survives a graph
/// rebuild for the same reason: a window open while somebody adds a channel
/// must not go blank.
pub struct DisgustingBeatTap {
    /// Peak and RMS per bucket, interleaved, as `f32::to_bits`.
    buckets: Vec<AtomicU32>,
    /// Where the read head was when each bucket was written, in buckets
    /// behind it — the same ring, so the two are read together.
    trail: Vec<AtomicU32>,
    /// Phase, offset, rate, filled seconds, and the clamp flag.
    phase: AtomicU32,
    offset: AtomicU32,
    rate: AtomicU32,
    filled: AtomicU32,
    clamped: AtomicU32,
    /// Where the newest bucket is, so the window can draw oldest-first.
    head: AtomicU32,
}

impl Default for DisgustingBeatTap {
    fn default() -> Self {
        Self::new()
    }
}

impl DisgustingBeatTap {
    pub fn new() -> Self {
        Self {
            buckets: (0..DISGUSTING_BEAT_BUCKETS * 2)
                .map(|_| AtomicU32::new(0))
                .collect(),
            trail: (0..DISGUSTING_BEAT_BUCKETS)
                .map(|_| AtomicU32::new(0))
                .collect(),
            phase: AtomicU32::new(0),
            offset: AtomicU32::new(0),
            rate: AtomicU32::new(1.0f32.to_bits()),
            filled: AtomicU32::new(0),
            clamped: AtomicU32::new(0),
            head: AtomicU32::new(0),
        }
    }

    /// **RT.** One block's worth. Relaxed stores only.
    pub fn write(
        &self,
        frame: DisgustingBeatFrame,
        buckets: &[(f32, f32)],
        trail: &[f32],
        head: usize,
    ) {
        self.phase.store(frame.phase.to_bits(), Ordering::Relaxed);
        self.offset.store(frame.offset.to_bits(), Ordering::Relaxed);
        self.rate.store(frame.rate.to_bits(), Ordering::Relaxed);
        self.filled
            .store(frame.filled_seconds.to_bits(), Ordering::Relaxed);
        self.clamped
            .store(u32::from(frame.clamped), Ordering::Relaxed);
        self.head.store(head as u32, Ordering::Relaxed);
        for (index, (peak, rms)) in buckets.iter().take(DISGUSTING_BEAT_BUCKETS).enumerate() {
            self.buckets[index * 2].store(peak.to_bits(), Ordering::Relaxed);
            self.buckets[index * 2 + 1].store(rms.to_bits(), Ordering::Relaxed);
        }
        for (index, behind) in trail.iter().take(DISGUSTING_BEAT_BUCKETS).enumerate() {
            self.trail[index].store(behind.to_bits(), Ordering::Relaxed);
        }
    }

    /// What the window draws. Oldest bucket first, so it is a picture of time
    /// running left to right without the reader having to know where the ring
    /// wrapped.
    pub fn read(&self) -> DisgustingBeatView {
        let head = self.head.load(Ordering::Relaxed) as usize % DISGUSTING_BEAT_BUCKETS.max(1);
        let mut buckets = Vec::with_capacity(DISGUSTING_BEAT_BUCKETS);
        let mut trail = Vec::with_capacity(DISGUSTING_BEAT_BUCKETS);
        for step in 0..DISGUSTING_BEAT_BUCKETS {
            let index = (head + 1 + step) % DISGUSTING_BEAT_BUCKETS;
            buckets.push((
                f32::from_bits(self.buckets[index * 2].load(Ordering::Relaxed)),
                f32::from_bits(self.buckets[index * 2 + 1].load(Ordering::Relaxed)),
            ));
            trail.push(f32::from_bits(self.trail[index].load(Ordering::Relaxed)));
        }
        DisgustingBeatView {
            frame: DisgustingBeatFrame {
                phase: f32::from_bits(self.phase.load(Ordering::Relaxed)),
                offset: f32::from_bits(self.offset.load(Ordering::Relaxed)),
                rate: f32::from_bits(self.rate.load(Ordering::Relaxed)),
                clamped: self.clamped.load(Ordering::Relaxed) != 0,
                filled_seconds: f32::from_bits(self.filled.load(Ordering::Relaxed)),
            },
            buckets,
            trail,
        }
    }
}

/// One frame of what a DisgustingBeat is doing, as the window reads it.
#[derive(Debug, Clone)]
pub struct DisgustingBeatView {
    pub frame: DisgustingBeatFrame,
    /// Peak and RMS per bucket, oldest first.
    pub buckets: Vec<(f32, f32)>,
    /// How far behind the write head the read was at each of those buckets,
    /// in buckets. Oldest first, the same order.
    pub trail: Vec<f32>,
}
