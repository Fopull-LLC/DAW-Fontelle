//! Whose click was it?
//!
//! A click on Windows is one of two things, and the cure for each is the
//! other's opposite: the **device** did not get a buffer on time (a driver,
//! a USB hub, another program holding the bus — a bigger buffer rarely
//! helps), or **Fontelle** did not finish one on time (too many plugins for
//! the buffer — a bigger buffer is exactly the cure). Both counters exist so
//! the settings page, the status line and a bug report can say which.
//!
//! [`GapDetector`] reads the first from when the device calls; [`LoadMeter`]
//! reads the second from how long rendering took. Both are plain arithmetic
//! on numbers the caller measured, so they run on the audio thread without
//! allocating and test on any machine.

/// Spots buffers the device never asked for, from the time between its
/// calls.
///
/// A device that calls every `period` and then once after three periods has
/// skipped two buffers. Drivers jitter, and some deliver two calls close
/// together after one late one, so only a gap past [`GAP_THRESHOLD`] periods
/// counts, and it counts the whole buffers missing.
#[derive(Debug, Clone)]
pub struct GapDetector {
    period_ns: u64,
    last_ns: Option<u64>,
}

/// How many periods between two calls before the gap is a dropout.
pub const GAP_THRESHOLD: f64 = 1.8;

impl GapDetector {
    pub fn new(period_ns: u64) -> Self {
        Self {
            period_ns,
            last_ns: None,
        }
    }

    /// A new period (the buffer size or the rate changed); forgets the last
    /// call so the change itself is not a gap.
    pub fn set_period(&mut self, period_ns: u64) {
        self.period_ns = period_ns;
        self.last_ns = None;
    }

    /// The device called at `now_ns`. How many buffers it skipped since the
    /// last call: 0 nearly always.
    pub fn tick(&mut self, now_ns: u64) -> u32 {
        let last = self.last_ns.replace(now_ns);
        let (Some(last), true) = (last, self.period_ns > 0) else {
            return 0;
        };
        let periods = now_ns.saturating_sub(last) as f64 / self.period_ns as f64;
        if periods < GAP_THRESHOLD {
            return 0;
        }
        // The call that came is one buffer; the rest of the gap was skipped.
        (periods.round() as u32).saturating_sub(1).max(1)
    }

    /// Forget the last call — after a stop, a reset, or anything else that
    /// pauses the device on purpose.
    pub fn reset(&mut self) {
        self.last_ns = None;
    }
}

/// How busy rendering keeps the audio thread, and whether it was late.
///
/// `load` is the share of the buffer's own duration spent rendering it,
/// smoothed over about a second; `peak` the worst single buffer, falling
/// back slowly so a spike stays readable for a moment.
#[derive(Debug, Clone, Default)]
pub struct LoadMeter {
    load: f32,
    peak: f32,
}

/// Each buffer moves the load this share of the way to its own: about 50
/// buffers to settle, a quarter of a second at 256 frames.
const LOAD_SMOOTHING: f32 = 0.02;

/// The peak keeps this much of itself per buffer: a spike reads for a few
/// seconds.
const PEAK_FALL: f32 = 0.999;

impl LoadMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// One buffer took `busy_ns` of a `period_ns` buffer. Whether it was
    /// late — took the whole buffer or more, so the device had nothing new
    /// to play.
    pub fn note(&mut self, busy_ns: u64, period_ns: u64) -> bool {
        if period_ns == 0 {
            return false;
        }
        let share = busy_ns as f32 / period_ns as f32;
        self.load += (share - self.load) * LOAD_SMOOTHING;
        self.peak = (self.peak * PEAK_FALL).max(share);
        share >= 1.0
    }

    /// Smoothed share of the buffer spent rendering, 0.0 to (past) 1.0.
    pub fn load(&self) -> f32 {
        self.load
    }

    /// The worst recent buffer.
    pub fn peak(&self) -> f32 {
        self.peak
    }
}
