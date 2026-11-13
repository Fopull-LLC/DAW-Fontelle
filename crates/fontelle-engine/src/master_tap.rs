//! What a Master insert's window reads: how loud it comes out, how far the
//! leveler is riding it, and how hard the ceiling held (Ty, 2026-11-12:
//! *"loudness readout on master would be good"*).
//!
//! `disgusting_beat_tap.rs`'s smaller sibling and the same honesty: relaxed
//! atomic stores, no lock, no allocation (INVARIANT 1). Shared through an
//! `Arc` the session keeps across a graph rebuild, so an open window does not
//! go back to silence when somebody adds a channel.

use std::sync::atomic::{AtomicU32, Ordering};

use fontelle_fx::MasterReading;

/// One Master insert's reading.
pub struct MasterTap {
    loudness: AtomicU32,
    level: AtomicU32,
    /// The most held since the window last read, so a short squash between
    /// two frames is still seen.
    reduction: AtomicU32,
}

impl Default for MasterTap {
    fn default() -> Self {
        Self::new()
    }
}

impl MasterTap {
    pub fn new() -> Self {
        let silent = MasterReading::SILENT;
        Self {
            loudness: AtomicU32::new(silent.loudness_lufs.to_bits()),
            level: AtomicU32::new(silent.level_db.to_bits()),
            reduction: AtomicU32::new(0),
        }
    }

    /// **RT.** One block's reading. Single writer, so the most-held needs no
    /// compare-exchange loop.
    pub fn write(&self, reading: MasterReading) {
        self.loudness
            .store(reading.loudness_lufs.to_bits(), Ordering::Relaxed);
        self.level
            .store(reading.level_db.to_bits(), Ordering::Relaxed);
        let held = f32::from_bits(self.reduction.load(Ordering::Relaxed));
        if reading.reduction_db > held {
            self.reduction
                .store(reading.reduction_db.to_bits(), Ordering::Relaxed);
        }
    }

    /// The window's read: the latest loudness and leveling, and the most the
    /// ceiling held since the last read.
    pub fn read(&self) -> MasterReading {
        MasterReading {
            loudness_lufs: f32::from_bits(self.loudness.load(Ordering::Relaxed)),
            level_db: f32::from_bits(self.level.load(Ordering::Relaxed)),
            reduction_db: f32::from_bits(self.reduction.swap(0, Ordering::Relaxed)),
        }
    }
}
