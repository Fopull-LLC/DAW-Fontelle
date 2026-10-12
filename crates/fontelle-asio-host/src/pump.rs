// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC. GPL-3.0-only; see `LICENSE` beside
// `Cargo.toml`.

//! One driver buffer's work, the same for a real driver and the test clock:
//! note whether the device skipped any buffers, hand the buffer to Fontelle
//! through the shared block, and give the driver what came back — or
//! silence, if nothing came back in time.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use fontelle_audio_ipc::{Doorbell, Exchange, GapDetector, HelperSide, Shared};

/// How much of a buffer the helper waits for Fontelle's answer. The rest is
/// for converting it and for the driver: an ASIO driver plays the buffer it
/// handed over one buffer later, so the whole wait fits inside a buffer
/// that would have been late anyway.
pub const DEADLINE_SHARE: f64 = 0.75;

/// What a driver's callback holds while the driver runs.
pub struct Pump {
    side: HelperSide<'static>,
    shared: &'static Shared,
    gaps: GapDetector,
    origin: Instant,
    frames: usize,
    deadline: Duration,
    client_gone: bool,
}

impl Pump {
    pub fn new(
        shared: &'static Shared,
        request: &'static dyn Doorbell,
        reply: &'static dyn Doorbell,
    ) -> Self {
        Self {
            side: HelperSide::new(shared, request, reply),
            shared,
            gaps: GapDetector::new(0),
            origin: Instant::now(),
            frames: 0,
            deadline: Duration::ZERO,
            client_gone: false,
        }
    }

    /// The driver's buffer and rate, now or after a reset.
    pub fn set_buffer(&mut self, frames: u32, rate: u32) {
        let period = Duration::from_secs_f64(f64::from(frames) / f64::from(rate.max(1)));
        self.frames = frames as usize;
        self.deadline = period.mul_f64(DEADLINE_SHARE);
        self.gaps.set_period(period.as_nanos() as u64);
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The device paused on purpose (stopped, reset): the gap before its
    /// next call is not a dropout.
    pub fn pause(&mut self) {
        self.gaps.reset();
    }

    /// Whether Fontelle has closed its end.
    pub fn client_gone(&self) -> bool {
        self.client_gone
    }

    /// The driver called for a buffer. `fill_input` writes the driver's input
    /// (interleaved, the block's input channels); `take_output` is handed
    /// Fontelle's output, or `None` for silence.
    pub fn cycle(
        &mut self,
        fill_input: impl FnOnce(&mut [f32]),
        take_output: impl FnOnce(Option<&[f32]>),
    ) -> Exchange {
        let lost = self.gaps.tick(self.origin.elapsed().as_nanos() as u64);
        if lost > 0 {
            self.count_device_xruns(u64::from(lost));
        }
        if self.client_gone {
            take_output(None);
            return Exchange::ClientGone;
        }
        let mut take = Some(take_output);
        let how = self
            .side
            .exchange(self.frames, self.deadline, fill_input, |out| {
                if let Some(take) = take.take() {
                    take(Some(out));
                }
            });
        if let Some(take) = take {
            take(None);
        }
        if how == Exchange::ClientGone {
            self.client_gone = true;
        }
        how
    }

    /// Buffers the device lost by its own account — an overload or resync
    /// the driver reported.
    pub fn count_device_xruns(&self, n: u64) {
        self.shared
            .header()
            .device_xruns
            .fetch_add(n, Ordering::Relaxed);
    }

    /// Says the helper has stopped, so Fontelle's audio thread stops
    /// waiting for it.
    pub fn stop(&self) {
        self.side.stop();
    }
}
