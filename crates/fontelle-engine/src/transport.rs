use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};

use fontelle_types::{Sample, Tick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportState {
    Stopped = 0,
    Playing = 1,
    Recording = 2,
    Rendering = 3,
}

impl TransportState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Playing,
            2 => Self::Recording,
            3 => Self::Rendering,
            _ => Self::Stopped,
        }
    }
}

/// Read by the RT thread, written by the model thread — an atomic struct, no lock
/// (TDD §6.3). When `Stopped`, the graph is not processed: the callback fills
/// silence and returns immediately, which is what delivers the near-zero idle-CPU
/// target. This must be designed in from the start, not optimised in later.
pub struct Transport {
    state: AtomicU8,
    position_sample: AtomicI64,
    loop_start_tick: AtomicI64,
    loop_end_tick: AtomicI64,
    looping: AtomicU8,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(TransportState::Stopped as u8),
            position_sample: AtomicI64::new(0),
            loop_start_tick: AtomicI64::new(0),
            loop_end_tick: AtomicI64::new(0),
            looping: AtomicU8::new(0),
        }
    }

    pub fn state(&self) -> TransportState {
        TransportState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_state(&self, state: TransportState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub fn position_sample(&self) -> Sample {
        self.position_sample.load(Ordering::Acquire)
    }

    pub fn set_position_sample(&self, sample: Sample) {
        self.position_sample.store(sample, Ordering::Release);
    }

    pub fn loop_range_tick(&self) -> (Tick, Tick) {
        (
            self.loop_start_tick.load(Ordering::Acquire),
            self.loop_end_tick.load(Ordering::Acquire),
        )
    }

    pub fn is_looping(&self) -> bool {
        self.looping.load(Ordering::Acquire) != 0
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping as u8, Ordering::Release);
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

/// A cheap-to-copy read of `Transport`, handed to nodes through `ProcessContext`
/// once per block rather than re-touching the atomics per sample.
#[derive(Debug, Clone, Copy)]
pub struct TransportSnapshot {
    pub state: TransportState,
    pub position_sample: Sample,
}
