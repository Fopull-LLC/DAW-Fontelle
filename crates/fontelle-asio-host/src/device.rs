// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC. GPL-3.0-only; see `LICENSE` beside
// `Cargo.toml`.

//! What the run loop needs of a driver, and the test clock that stands in
//! for one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fontelle_audio_ipc::ReadyInfo;

use crate::pump::Pump;
use crate::sizes::{COMMON_RATES, fit_buffer_size, rate_refusal};

/// Something the driver said between buffers, collected for the run loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The driver wants to be stopped, closed and opened again (ASIO's
    /// `kAsioResetRequest`): its buffer size, rate or channels changed, often
    /// in its own control panel, or the device went and came back.
    Reset,
    /// The driver's sample rate moved.
    RateChanged,
    /// The driver's latencies moved.
    LatenciesChanged,
    /// The driver lost buffers: `n` overloads or resyncs it reported.
    Lost(u32),
}

/// A driver, as the run loop drives it. Every method is called on the
/// thread that made the device; only the [`Pump`] crosses to the driver's
/// own thread, between `start` and `stop`.
pub trait Device {
    /// Opens the driver at `rate` with a buffer of `frames` (`None`: the
    /// driver's own), `outputs` and `inputs` channels to exchange. Not
    /// started.
    fn open(
        &mut self,
        rate: u32,
        frames: Option<u32>,
        outputs: u32,
        inputs: u32,
    ) -> Result<ReadyInfo, String>;
    /// Starts calling `pump` once per buffer. On failure the pump comes back.
    fn start(&mut self, pump: Pump) -> Result<(), (String, Box<Pump>)>;
    /// Stops calling, and hands the pump back.
    fn stop(&mut self) -> Option<Pump>;
    /// Undoes `open`.
    fn close(&mut self);
    /// After `close`, lets the driver go entirely and loads it afresh — what
    /// a reset request asks for (the SDK's host: stop, dispose, exit, init),
    /// and what some drivers need before a new buffer size or rate holds.
    fn reload(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// What the driver said since the last call.
    fn notices(&mut self) -> Vec<Notice>;
    /// The driver's latencies now, in frames: input, output.
    fn latencies(&mut self) -> Option<(u32, u32)>;
    /// The driver's rate now.
    fn rate(&mut self) -> Option<u32>;
    /// Opens the driver's own settings window.
    fn control_panel(&mut self) -> Result<(), String>;
    /// Whether this process has a window open (a control panel).
    fn windows_open(&self) -> bool {
        false
    }
    /// Waits up to `wait`, doing whatever the platform needs done meanwhile
    /// (a COM driver's window messages, on Windows).
    fn idle(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }
}

/// The test clock's name, as `list` gives it.
pub const TEST_CLOCK: &str = "Fontelle test clock";

/// Whether the test clock is offered: only with `FONTELLE_ASIO_TEST_CLOCK=1`
/// in the environment, so nobody chooses it by accident.
pub fn test_clock_enabled() -> bool {
    std::env::var_os("FONTELLE_ASIO_TEST_CLOCK").is_some_and(|v| v == "1")
}

/// A pretend driver: calls once per buffer on a timer, plays nothing, and
/// can be told by the environment to misbehave the ways real drivers do —
/// `FONTELLE_ASIO_TEST_CLOCK_SKIP_EVERY=n` skips two buffers every `n`,
/// `…_RESET_AFTER=n` asks for a reset after `n` buffers (once), with
/// `…_RESET_FRAMES=f` as its new preferred size, and `…_STOP_AFTER=n` stops
/// calling after `n` buffers, as an unplugged device does.
pub struct ClockDevice {
    rate: u32,
    frames: u32,
    preferred: u32,
    skip_every: u32,
    reset_after: u32,
    reset_frames: u32,
    stop_after: u32,
    /// Buffers called since the clock was made, across resets.
    calls: Arc<AtomicU32>,
    reset_asked: Arc<AtomicBool>,
    reset_done: Arc<AtomicBool>,
    running: Option<(Arc<AtomicBool>, JoinHandle<Pump>)>,
}

fn env_number(name: &str) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

impl ClockDevice {
    pub fn from_env() -> Self {
        Self {
            rate: 48_000,
            frames: 512,
            preferred: 512,
            skip_every: env_number("FONTELLE_ASIO_TEST_CLOCK_SKIP_EVERY"),
            reset_after: env_number("FONTELLE_ASIO_TEST_CLOCK_RESET_AFTER"),
            reset_frames: env_number("FONTELLE_ASIO_TEST_CLOCK_RESET_FRAMES"),
            stop_after: env_number("FONTELLE_ASIO_TEST_CLOCK_STOP_AFTER"),
            calls: Arc::new(AtomicU32::new(0)),
            reset_asked: Arc::new(AtomicBool::new(false)),
            reset_done: Arc::new(AtomicBool::new(false)),
            running: None,
        }
    }
}

impl Device for ClockDevice {
    fn open(
        &mut self,
        rate: u32,
        frames: Option<u32>,
        outputs: u32,
        inputs: u32,
    ) -> Result<ReadyInfo, String> {
        if !COMMON_RATES.contains(&rate) {
            return Err(rate_refusal(TEST_CLOCK, &COMMON_RATES, rate));
        }
        if self.reset_done.load(Ordering::Relaxed) && self.reset_frames > 0 {
            self.preferred = self.reset_frames;
        }
        self.rate = rate;
        self.frames = fit_buffer_size(frames, 16, 8192, self.preferred, -1);
        Ok(ReadyInfo {
            driver: TEST_CLOCK.to_string(),
            frames: self.frames,
            rate,
            inputs,
            outputs,
            input_latency: self.frames,
            output_latency: self.frames,
            min_frames: 16,
            max_frames: 8192,
            preferred_frames: self.preferred,
            granularity: -1,
            format: "Float32LSB".to_string(),
        })
    }

    fn start(&mut self, mut pump: Pump) -> Result<(), (String, Box<Pump>)> {
        let stop = Arc::new(AtomicBool::new(false));
        let period = Duration::from_secs_f64(f64::from(self.frames) / f64::from(self.rate));
        let (skip_every, reset_after, stop_after) =
            (self.skip_every, self.reset_after, self.stop_after);
        let (calls, reset_asked, reset_done) = (
            Arc::clone(&self.calls),
            Arc::clone(&self.reset_asked),
            Arc::clone(&self.reset_done),
        );
        let stopping = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("fontelle-asio test clock".to_string())
            .spawn(move || {
                let mut next = Instant::now() + period;
                while !stopping.load(Ordering::Relaxed) {
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    }
                    next += period;
                    let n = calls.load(Ordering::Relaxed);
                    if stop_after > 0 && n >= stop_after {
                        // Unplugged: no more calls, ever.
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    if skip_every > 0 && n > 0 && n % skip_every == 0 {
                        // Two buffers the device never asked for.
                        std::thread::sleep(period * 2);
                        next += period * 2;
                    }
                    calls.fetch_add(1, Ordering::Relaxed);
                    pump.cycle(|input| input.fill(0.0), |_| {});
                    if reset_after > 0
                        && n + 1 == reset_after
                        && !reset_done.swap(true, Ordering::Relaxed)
                    {
                        reset_asked.store(true, Ordering::Relaxed);
                    }
                }
                pump
            })
            .expect("a thread for the test clock");
        self.running = Some((stop, thread));
        Ok(())
    }

    fn stop(&mut self) -> Option<Pump> {
        let (stop, thread) = self.running.take()?;
        stop.store(true, Ordering::Relaxed);
        thread.join().ok()
    }

    fn close(&mut self) {}

    fn notices(&mut self) -> Vec<Notice> {
        if self.reset_asked.swap(false, Ordering::Relaxed) {
            vec![Notice::Reset]
        } else {
            Vec::new()
        }
    }

    fn latencies(&mut self) -> Option<(u32, u32)> {
        Some((self.frames, self.frames))
    }

    fn rate(&mut self) -> Option<u32> {
        Some(self.rate)
    }

    fn control_panel(&mut self) -> Result<(), String> {
        Err("the test clock has no control panel".to_string())
    }
}
