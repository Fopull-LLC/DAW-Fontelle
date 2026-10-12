//! The output stream: which backend, which device, how big a buffer — chosen,
//! and changed while the studio runs.
//!
//! Reported: *"audio drivers not configurable enough so pretty sure its
//! defaulting to default audio drivers for a lot of users causing things to
//! sound like failing audio drivers sometimes"*. The output was always
//! `cpal`'s default host and its default device, asking for a 128-frame
//! buffer (2.7 ms at 48 kHz) whenever the device's range had room for it —
//! which nearly every range does. On Linux that host is ALSA and that device
//! is `default`, which on a PipeWire or PulseAudio desktop is the sound
//! server's ALSA plugin: a 2.7 ms period through a plugin bridge is one most
//! desktops cannot keep filled once a plugin or a busy moment takes a share
//! of it, and every period it misses is a click. Nothing said it was
//! happening: the backend's "underrun" went to `eprintln!`.
//!
//! So: [`DEFAULT_OUTPUT_BUFFER`] frames unless the person asks otherwise,
//! the backend and the device are theirs to choose ([`OutputChoice`]), a
//! choice that will not open falls back to the default with the reason
//! said, and every dropout the backend reports is counted
//! ([`OutputStats`]).
//!
//! # Changing it while the studio runs
//!
//! What the callback owns — the graph, the timeline, the transport's reader,
//! the live input — used to be moved into the stream's closure and leaked
//! with it, so a stream could be opened once. It lives in a
//! [`CallbackSlot`] now, which the closure borrows: the old stream is
//! parked out of it, dropped, and a new one borrows the same state. The
//! song does not stop; there is a moment of silence.
//!
//! # Backends of Fontelle's own
//!
//! Besides `cpal`'s, three that Windows users asked for by clicking
//! (*"clicky/crackly audio on Windows"*, 2026-10): WASAPI in **exclusive**
//! mode and WASAPI in **low-latency** shared mode ([`crate::wasapi`],
//! Windows only), and **ASIO** by way of the separate `fontelle-asio`
//! program ([`crate::output_asio`], `docs/asio.md`). All three run the same
//! render as `cpal`'s callback ([`Renderer`]) on a thread of their own.
//!
//! # Whose click was it?
//!
//! [`OutputStats`] counts a buffer the **device** lost (the backend said
//! so, or did not ask on time) apart from one **Fontelle** finished late
//! (rendering took longer than the buffer lasts), and keeps how much of each
//! buffer rendering takes ([`OutputHealth`]). A bigger buffer cures the
//! second and rarely the first.

use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fontelle_types::{CompiledTimeline, TimedEvent};

use crate::device::{BLOCK_SIZE, DeviceError};
use crate::graph_channel::GraphSource;
use crate::live::{IdleGate, LiveEventSource};
use crate::rt_guard::with_rt_thread;
use crate::timeline_channel::TimelineSource;
use crate::transport::{Transport, TransportReader, TransportState};

pub use crate::output_asio::{
    ASIO_HELPER_HOST, asio_driver_names, asio_helper_path, open_asio_panel,
};

/// WASAPI in exclusive mode: Fontelle alone on the device, at its own
/// buffer, nothing mixed in. Windows only.
pub const WASAPI_EXCLUSIVE_HOST: &str = "WASAPI (exclusive)";

/// WASAPI shared with other programs, at the smallest period the driver
/// allows (`IAudioClient3`) rather than Windows's 10 ms. Windows only.
pub const WASAPI_LOW_LATENCY_HOST: &str = "WASAPI (low latency)";

/// The buffer the output asks for when the person has not said: 512 frames,
/// 10.7 ms at 48 kHz. Four times the 128 that crackled; still well under
/// what a keyboard player notices.
pub const DEFAULT_OUTPUT_BUFFER: u32 = 512;

/// How long a callback the watchdog demoted stays on ordinary scheduling
/// before it asks rtkit for real-time again — long enough for the overload
/// that spent the budget to have passed.
#[cfg(target_os = "linux")]
const RT_REPROMOTE_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// What the person chose in Settings. `None` everywhere is the system's
/// default backend and device, at [`DEFAULT_OUTPUT_BUFFER`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputChoice {
    /// A backend by its `cpal` name: "ALSA", "JACK", "PulseAudio", "WASAPI",
    /// "CoreAudio".
    pub host: Option<String>,
    /// A device by the name [`output_device_names`] lists it under.
    pub device: Option<String>,
    /// Frames per buffer.
    pub buffer_frames: Option<u32>,
}

/// What is open, in words a person can check against their system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputStatus {
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    /// The buffer asked for and taken. `None` is the backend's own choice,
    /// when it would take no size Fontelle asked for.
    pub buffer_frames: Option<u32>,
    /// The device's own latencies in frames, input and output, where the
    /// backend says them (ASIO does).
    pub latencies: Option<(u32, u32)>,
    /// Why the choice was not what opened, when it was not: it would not open,
    /// and the default did.
    pub fell_back: Option<String>,
}

impl std::fmt::Display for OutputStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} \u{b7} {} \u{b7} ", self.host, self.device)?;
        match self.buffer_frames {
            Some(frames) => write!(
                f,
                "{frames} frames ({})",
                latency_label(frames, self.sample_rate)
            ),
            None => write!(f, "the backend's buffer"),
        }?;
        if let Some((input, output)) = self.latencies {
            write!(
                f,
                " \u{b7} in {}, out {}",
                latency_label(input, self.sample_rate),
                latency_label(output, self.sample_rate)
            )?;
        }
        Ok(())
    }
}

/// How long `frames` last at `sample_rate`, as a person reads it: "10.7 ms".
pub fn latency_label(frames: u32, sample_rate: u32) -> String {
    let ms = f64::from(frames) * 1000.0 / f64::from(sample_rate.max(1));
    format!("{ms:.1} ms")
}

/// What the stream has seen, read by the window: dropouts by whose they
/// were, how busy rendering is, and whether the device went away.
///
/// Atomics only: everything here is written on the audio thread, and it
/// used to `eprintln!` there for every underrun — a lock and a write on the
/// RT thread, at exactly the moment it was already late. The load and peak
/// have one writer, the audio thread, so a load-then-store is enough.
#[derive(Debug, Default)]
pub struct OutputStats {
    device_xruns: AtomicU64,
    late: AtomicU64,
    lost: AtomicBool,
    /// `f32` bits: the share of each buffer spent rendering, smoothed.
    load: AtomicU32,
    /// `f32` bits: the worst recent buffer, falling back slowly.
    peak: AtomicU32,
    /// The ASIO helper's last handoff, ns, plus one; 0 is none.
    handoff: AtomicU32,
}

/// Each buffer moves the load this share of the way to its own.
const LOAD_SMOOTHING: f32 = 0.02;
/// The peak keeps this much of itself per buffer.
const PEAK_FALL: f32 = 0.999;

/// [`OutputStats`] at one moment, for Settings, the status line and the log.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OutputHealth {
    /// Buffers the device lost: the backend reported an underrun, the driver
    /// an overload or resync, or the device did not ask on time.
    pub device_xruns: u64,
    /// Buffers Fontelle finished after they were due.
    pub late: u64,
    /// The share of each buffer spent rendering, smoothed (1.0 is all of it).
    pub load: f32,
    /// The worst recent buffer's share.
    pub peak: f32,
    /// What handing a buffer to the ASIO helper and back costs, beyond
    /// rendering it, ns.
    pub handoff_ns: Option<u32>,
}

impl OutputHealth {
    /// The Dropouts row: "None", or whose they were.
    pub fn dropouts_label(&self) -> String {
        match (self.device_xruns, self.late) {
            (0, 0) => "None".to_string(),
            (d, 0) => format!("{d} from the device"),
            (0, l) => format!("{l} late (Fontelle)"),
            (d, l) => format!("{d} from the device, {l} late (Fontelle)"),
        }
    }

    /// The load row: "12% (peak 40%)".
    pub fn load_label(&self) -> String {
        format!("{:.0}% (peak {:.0}%)", self.load * 100.0, self.peak * 100.0)
    }

    /// One line for the log.
    pub fn log_line(&self) -> String {
        let mut line = format!(
            "{} device dropout{}, {} late buffer{}, load {}",
            self.device_xruns,
            if self.device_xruns == 1 { "" } else { "s" },
            self.late,
            if self.late == 1 { "" } else { "s" },
            self.load_label()
        );
        if let Some(ns) = self.handoff_ns {
            line.push_str(&format!(", handoff {} \u{b5}s", (ns + 500) / 1000));
        }
        line
    }
}

impl OutputStats {
    /// Takes one error from the stream. A dropout is the device's; a device
    /// that went away is noted for the window to act on.
    pub fn note_error(&self, error: &cpal::Error) {
        match error.kind() {
            cpal::ErrorKind::Xrun => {
                self.device_xruns.fetch_add(1, Ordering::Relaxed);
            }
            cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable => {
                self.lost.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// One buffer rendered in `busy`, of a buffer lasting `period`: the
    /// load, and a late buffer if it took the whole of it or more.
    pub fn note_cycle(&self, busy: Duration, period: Duration) {
        if self.note_load(busy, period) {
            self.late.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The load alone, for a backend whose own count of late buffers is the
    /// truer one (the ASIO helper's deadline). Whether it was late.
    pub(crate) fn note_load(&self, busy: Duration, period: Duration) -> bool {
        if period.is_zero() {
            return false;
        }
        let share = busy.as_secs_f32() / period.as_secs_f32();
        let load = f32::from_bits(self.load.load(Ordering::Relaxed));
        let load = load + (share - load) * LOAD_SMOOTHING;
        self.load.store(load.to_bits(), Ordering::Relaxed);
        let peak = f32::from_bits(self.peak.load(Ordering::Relaxed));
        self.peak
            .store((peak * PEAK_FALL).max(share).to_bits(), Ordering::Relaxed);
        share >= 1.0
    }

    /// `n` buffers the device lost by a count of its own.
    pub fn note_device_xruns(&self, n: u64) {
        if n > 0 {
            self.device_xruns.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// `n` buffers Fontelle was late with, by a count of the backend's.
    pub fn note_late(&self, n: u64) {
        if n > 0 {
            self.late.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn set_handoff(&self, ns: u32) {
        self.handoff.store(ns.saturating_add(1), Ordering::Relaxed);
    }

    /// Every dropout since the count was last cleared, whoever's.
    pub fn xruns(&self) -> u64 {
        self.device_xruns.load(Ordering::Relaxed) + self.late.load(Ordering::Relaxed)
    }

    /// The counts and the load, read once.
    pub fn health(&self) -> OutputHealth {
        OutputHealth {
            device_xruns: self.device_xruns.load(Ordering::Relaxed),
            late: self.late.load(Ordering::Relaxed),
            load: f32::from_bits(self.load.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.peak.load(Ordering::Relaxed)),
            handoff_ns: match self.handoff.load(Ordering::Relaxed) {
                0 => None,
                n => Some(n - 1),
            },
        }
    }

    /// Starts the counts again, and the peak with them.
    pub fn clear_xruns(&self) {
        self.device_xruns.store(0, Ordering::Relaxed);
        self.late.store(0, Ordering::Relaxed);
        self.peak.store(0f32.to_bits(), Ordering::Relaxed);
    }

    /// Whether the device has gone away under the stream.
    pub fn lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    pub(crate) fn set_lost(&self) {
        self.lost.store(true, Ordering::Relaxed);
    }

    pub fn clear_lost(&self) {
        self.lost.store(false, Ordering::Relaxed);
    }
}

/// State a real-time callback borrows, and a caller can take back.
///
/// The callback asks for it with [`with`](Self::with) — one compare-and-swap,
/// no lock, no wait — and gets `None` once the slot is
/// [parked](Self::park). Parking waits for a callback already inside to
/// leave, so after it returns the state is the caller's alone
/// ([`with_parked`](Self::with_parked)) until [`unpark`](Self::unpark).
pub struct CallbackSlot<T> {
    state: UnsafeCell<T>,
    gate: AtomicU8,
}

const IDLE: u8 = 0;
const BUSY: u8 = 1;
const PARKED: u8 = 2;

// SAFETY: the state is reached only by whoever holds the gate: the callback
// between a successful IDLE -> BUSY and its store of IDLE, or the caller while
// it is PARKED. The two can never hold it at once.
unsafe impl<T: Send> Sync for CallbackSlot<T> {}

impl<T> CallbackSlot<T> {
    pub fn new(state: T) -> Self {
        Self {
            state: UnsafeCell::new(state),
            gate: AtomicU8::new(IDLE),
        }
    }

    /// The callback's side: `f` on the state, or `None` if it is parked.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if self
            .gate
            .compare_exchange(IDLE, BUSY, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        // Released however `f` leaves, a panic included: the callback catches
        // its own panics, and a gate left BUSY would hang the next park.
        struct Leave<'a>(&'a AtomicU8);
        impl Drop for Leave<'_> {
            fn drop(&mut self) {
                self.0.store(IDLE, Ordering::Release);
            }
        }
        let _leave = Leave(&self.gate);
        // SAFETY: the gate is ours until `_leave` drops.
        Some(f(unsafe { &mut *self.state.get() }))
    }

    /// Takes the state away from the callback, waiting for one already
    /// inside to finish. A callback that comes after is handed `None`.
    pub fn park(&self) {
        loop {
            match self
                .gate
                .compare_exchange(IDLE, PARKED, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) | Err(PARKED) => return,
                Err(_) => std::thread::yield_now(),
            }
        }
    }

    /// Gives the state back to the callback.
    pub fn unpark(&self) {
        let _ = self
            .gate
            .compare_exchange(PARKED, IDLE, Ordering::Release, Ordering::Relaxed);
    }

    /// The caller's side, while parked: `f` on the state, or `None` if the
    /// slot is not parked.
    pub fn with_parked<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if self.gate.load(Ordering::Acquire) != PARKED {
            return None;
        }
        // SAFETY: parked, so no callback can be inside, and only the owner
        // of the slot parks it.
        Some(f(unsafe { &mut *self.state.get() }))
    }
}

/// Every output backend this build can use on this machine, by name: `cpal`'s
/// in its order, the default among them, then Fontelle's own — the two
/// WASAPI modes on Windows, and ASIO when `fontelle-asio` is beside
/// Fontelle ([`asio_helper_path`]).
pub fn output_host_names() -> Vec<String> {
    let mut hosts: Vec<String> = cpal::available_hosts()
        .into_iter()
        .map(|id| id.name().to_string())
        .collect();
    if cfg!(windows) {
        hosts.push(WASAPI_LOW_LATENCY_HOST.to_string());
        hosts.push(WASAPI_EXCLUSIVE_HOST.to_string());
    }
    if asio_helper_path().is_some() {
        hosts.push(ASIO_HELPER_HOST.to_string());
    }
    hosts
}

/// Which of Fontelle's own backends `name` is, if it is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnBackend {
    WasapiExclusive,
    WasapiLowLatency,
    AsioHelper,
}

impl OwnBackend {
    pub(crate) fn named(name: &str) -> Option<Self> {
        match name {
            WASAPI_EXCLUSIVE_HOST => Some(Self::WasapiExclusive),
            WASAPI_LOW_LATENCY_HOST => Some(Self::WasapiLowLatency),
            ASIO_HELPER_HOST => Some(Self::AsioHelper),
            _ => None,
        }
    }
}

/// The shared-mode period to ask `IAudioClient3` for: `requested` frames
/// (Automatic: [`LOW_LATENCY_AUTOMATIC`]) rounded **up** onto the engine's
/// `fundamental` steps from `min`, inside `[min, max]`. Up, because a period
/// shorter than asked is a crackle nobody chose.
pub fn low_latency_period(requested: Option<u32>, fundamental: u32, min: u32, max: u32) -> u32 {
    let max = max.max(min);
    let want = requested.unwrap_or(LOW_LATENCY_AUTOMATIC);
    if want <= min || fundamental == 0 {
        return min.max(want.min(max));
    }
    let steps = (want - min).div_ceil(fundamental);
    (min + steps * fundamental).min(max)
}

/// What Automatic means in low-latency mode: 240 frames, 5 ms at 48 kHz —
/// half of Windows's usual 10 ms, and above the 2–3 ms periods that only a
/// quiet machine keeps fed.
pub const LOW_LATENCY_AUTOMATIC: u32 = 240;

/// What Automatic means in exclusive mode: [`DEFAULT_OUTPUT_BUFFER`].
pub const EXCLUSIVE_AUTOMATIC: u32 = DEFAULT_OUTPUT_BUFFER;

/// The backend "Automatic" means here: `cpal`'s default.
pub fn default_output_host_name() -> String {
    cpal::default_host().id().name().to_string()
}

fn host_named(name: &str) -> Result<cpal::Host, DeviceError> {
    let id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name() == name)
        .ok_or_else(|| DeviceError(format!("{name} is not available on this system")))?;
    cpal::host_from_id(id).map_err(|e| DeviceError(format!("{name}: {e}")))
}

fn device_name(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|desc| desc.name().to_string())
}

/// Every output device `host` offers (the default host for `None`) that will
/// say how it plays, by name, each once. Empty for a backend that is not
/// there or not running.
pub fn output_device_names(host: Option<&str>) -> Vec<String> {
    // Fontelle's own: ASIO's are its drivers; the WASAPI modes play the
    // same endpoints `cpal`'s WASAPI lists, by the same names.
    match host.and_then(OwnBackend::named) {
        Some(OwnBackend::AsioHelper) => return asio_driver_names(),
        Some(OwnBackend::WasapiExclusive | OwnBackend::WasapiLowLatency) => {
            return output_device_names(Some("WASAPI"));
        }
        None => {}
    }
    let host = match host {
        Some(name) => match host_named(name) {
            Ok(host) => host,
            Err(_) => return Vec::new(),
        },
        None => cpal::default_host(),
    };
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    // Asked whether each will play, the way the input list asks
    // (`AudioDevice::input_names`): ALSA lists every plugin it has, and a
    // card the sound server holds refuses to open. Named first, because the
    // probe is the expensive half.
    let mut names: Vec<String> = Vec::new();
    for device in devices {
        let Some(name) = device_name(&device).filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        if names.contains(&name) || device.default_output_config().is_err() {
            continue;
        }
        names.push(name);
    }
    names
}

/// The buffer sizes to ask the output device for, best first: the size
/// asked for, then [`DEFAULT_OUTPUT_BUFFER`], each brought inside the
/// device's range, then whatever the device picks itself — so a card that
/// will take neither still plays.
///
/// The callback walks whatever length it is handed a block at a time, so a
/// size costs latency, not correctness.
pub fn output_buffer_sizes(
    supported: &cpal::SupportedBufferSize,
    requested: Option<u32>,
) -> Vec<cpal::BufferSize> {
    let fit = |frames: u32| match supported {
        cpal::SupportedBufferSize::Range { min, max } => frames.clamp(*min, (*max).max(*min)),
        cpal::SupportedBufferSize::Unknown => frames,
    };
    let mut sizes = Vec::with_capacity(3);
    for frames in requested.into_iter().chain([DEFAULT_OUTPUT_BUFFER]) {
        let size = cpal::BufferSize::Fixed(fit(frames));
        if !sizes.contains(&size) {
            sizes.push(size);
        }
    }
    sizes.push(cpal::BufferSize::Default);
    sizes
}

/// What the callback owns that outlives any one stream.
struct OutputState {
    graph: GraphSource,
    timeline: TimelineSource,
    transport: Arc<Transport>,
    live: Option<LiveEventSource>,
    reader: TransportReader,
    gate: IdleGate,
    monitor: Option<Arc<crate::InputMonitor>>,
    /// Whether the current graph has handed its plugins to a render — see
    /// `Transport::hold`.
    lent: bool,
}

/// A running stream, of whichever kind.
enum Stream {
    /// Held for what dropping it does: stopping the stream.
    Cpal(#[allow(dead_code)] cpal::Stream),
    Asio(crate::output_asio::AsioStream),
    /// Held for what dropping it does, as `Cpal`.
    #[cfg(windows)]
    Wasapi(#[allow(dead_code)] crate::wasapi::WasapiStream),
}

impl Stream {
    /// What is open, now: a driver that reset itself may have moved its
    /// buffer since the stream opened.
    fn refresh(&self, status: &mut OutputStatus) {
        if let Self::Asio(asio) = self {
            asio.refresh(status);
        }
    }
}

/// The output stream, and what it plays, kept apart so the stream can be
/// opened again on another device.
pub struct AudioOutput {
    // First, so it goes first: cpal drops the callback closure on the audio
    // thread, and while this struct still holds its own `Arc`s below, what
    // the closure drops there is only ever a count, never a free
    // (INVARIANT 1).
    stream: Option<Stream>,
    /// `ManuallyDrop` inside: the graph is never freed, here or anywhere —
    /// the same deliberate leak the stream's closure always had, because a
    /// plugin's teardown at exit is a crash waiting to happen and the
    /// process is about to give the memory back anyway.
    slot: Arc<CallbackSlot<ManuallyDrop<OutputState>>>,
    stats: Arc<OutputStats>,
    sample_rate: u32,
    status: Option<OutputStatus>,
}

impl AudioOutput {
    /// Opens the output `choice` asks for, or the default if it will not
    /// open, playing `graph` through the real audio callback.
    ///
    /// See [`crate::AudioDevice::start_output_stream`] for what the callback
    /// does with each of these.
    pub fn start(
        graph: GraphSource,
        timeline: TimelineSource,
        sample_rate: u32,
        transport: Arc<Transport>,
        live: Option<LiveEventSource>,
        monitor: Option<Arc<crate::InputMonitor>>,
        choice: &OutputChoice,
    ) -> Result<Self, DeviceError> {
        // Off-RT, before the stream exists: nodes size their internal buffers
        // here so the callback never has to. Everything published later is
        // prepared by `GraphPublisher`'s caller, on its own thread.
        let mut graph = graph;
        graph
            .current()
            .prepare(sample_rate as f32, BLOCK_SIZE as u32);
        let state = OutputState {
            graph,
            timeline,
            transport,
            live,
            reader: TransportReader::new(),
            gate: IdleGate::new(),
            monitor,
            lent: false,
        };
        let mut output = Self {
            stream: None,
            slot: Arc::new(CallbackSlot::new(ManuallyDrop::new(state))),
            stats: Arc::new(OutputStats::default()),
            sample_rate,
            status: None,
        };
        output.reopen(choice)?;
        Ok(output)
    }

    /// Closes the stream and opens `choice` in its place, or the system's
    /// default when `choice` will not open — [`OutputStatus::fell_back`]
    /// says why. An error is neither opening: the studio is silent until
    /// something else is chosen, and nothing it was playing is lost.
    pub fn reopen(&mut self, choice: &OutputChoice) -> Result<OutputStatus, DeviceError> {
        self.close();
        self.stats.clear_lost();
        let opened = match self.open(choice) {
            Ok(opened) => Ok(opened),
            Err(first) if *choice != OutputChoice::default() => self.fall_back(choice, first.0),
            Err(e) => Err(e),
        };
        let (stream, status) = opened?;
        self.stream = Some(stream);
        self.status = Some(status.clone());
        Ok(status)
    }

    /// After the output was lost ([`OutputStats::lost`]): `choice` again —
    /// unless what was lost was the ASIO helper, which is not started again
    /// on its own (a helper that crashes would crash again, every few
    /// seconds); the default plays instead, saying why. Choosing ASIO again
    /// in Settings ([`reopen`](Self::reopen)) tries it again.
    pub fn reopen_after_loss(
        &mut self,
        choice: &OutputChoice,
    ) -> Result<OutputStatus, DeviceError> {
        let helper_gone = choice.host.as_deref() == Some(ASIO_HELPER_HOST)
            && matches!(self.stream, Some(Stream::Asio(_)));
        let Some(why) = self.loss_reason().filter(|_| helper_gone) else {
            return self.reopen(choice);
        };
        self.close();
        self.stats.clear_lost();
        let (stream, status) = self.fall_back(choice, why)?;
        self.stream = Some(stream);
        self.status = Some(status.clone());
        Ok(status)
    }

    /// The default in place of `choice`, which would not open because of
    /// `why`. The buffer the person chose is kept: it is the device that
    /// would not open, most likely, not the size.
    fn fall_back(
        &self,
        choice: &OutputChoice,
        why: String,
    ) -> Result<(Stream, OutputStatus), DeviceError> {
        let fallback = OutputChoice {
            buffer_frames: choice.buffer_frames,
            ..OutputChoice::default()
        };
        match self
            .open(&fallback)
            .or_else(|_| self.open(&OutputChoice::default()))
        {
            Ok((stream, mut status)) => {
                status.fell_back = Some(why);
                Ok((stream, status))
            }
            Err(e) => Err(DeviceError(format!("{why} (and the default: {e})"))),
        }
    }

    /// What is open, or `None` when nothing is — as it is now, so a driver
    /// that changed its own buffer says the new one.
    pub fn status(&self) -> Option<OutputStatus> {
        let mut status = self.status.clone()?;
        if let Some(stream) = &self.stream {
            stream.refresh(&mut status);
        }
        Some(status)
    }

    /// The counts the stream keeps.
    pub fn stats(&self) -> Arc<OutputStats> {
        Arc::clone(&self.stats)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Why the stream was lost, when the backend can say — the ASIO helper
    /// exiting, with its last words.
    pub fn loss_reason(&self) -> Option<String> {
        match &self.stream {
            Some(Stream::Asio(asio)) => asio.loss_reason(),
            _ => None,
        }
    }

    /// Something the backend said while it plays on that a person should
    /// read — the ASIO helper's error about a control panel the driver does
    /// not have. Each once.
    pub fn take_note(&self) -> Option<String> {
        match &self.stream {
            Some(Stream::Asio(asio)) => asio.take_note(),
            _ => None,
        }
    }

    /// The ASIO helper's process, while one plays.
    pub fn helper_pid(&self) -> Option<u32> {
        match &self.stream {
            Some(Stream::Asio(asio)) => Some(asio.pid()),
            _ => None,
        }
    }

    /// Opens the ASIO driver's own control panel, through the helper that
    /// is playing it. Not playing through ASIO, [`open_asio_panel`] opens
    /// one by name.
    pub fn open_driver_panel(&mut self) -> Result<(), String> {
        match &self.stream {
            Some(Stream::Asio(asio)) => asio.open_panel(),
            _ => Err("not playing through ASIO".to_string()),
        }
    }

    /// Closes the stream, keeping what it was playing for the next one.
    pub fn close(&mut self) {
        if self.stream.is_none() {
            return;
        }
        // Parked first, so a callback already inside finishes and any after
        // it plays silence, whatever the backend does about joining its
        // thread when the stream goes.
        self.slot.park();
        self.stream = None;
        self.status = None;
        self.slot.unpark();
    }

    fn renderer(&self, count_late: bool) -> Renderer {
        Renderer::new(
            Arc::clone(&self.slot),
            Arc::clone(&self.stats),
            self.sample_rate,
            count_late,
        )
    }

    fn open(&self, choice: &OutputChoice) -> Result<(Stream, OutputStatus), DeviceError> {
        match choice.host.as_deref().and_then(OwnBackend::named) {
            Some(OwnBackend::AsioHelper) => {
                return crate::output_asio::open(
                    choice,
                    self.sample_rate,
                    self.renderer(false),
                    Arc::clone(&self.stats),
                )
                .map(|(stream, status)| (Stream::Asio(stream), status));
            }
            #[cfg(windows)]
            Some(own @ (OwnBackend::WasapiExclusive | OwnBackend::WasapiLowLatency)) => {
                return crate::wasapi::open(
                    own == OwnBackend::WasapiExclusive,
                    choice,
                    self.sample_rate,
                    self.renderer(true),
                    Arc::clone(&self.stats),
                )
                .map(|(stream, status)| (Stream::Wasapi(stream), status));
            }
            #[cfg(not(windows))]
            Some(OwnBackend::WasapiExclusive | OwnBackend::WasapiLowLatency) => {
                return Err(DeviceError("WASAPI is Windows's".to_string()));
            }
            None => {}
        }
        let host = match &choice.host {
            Some(name) => host_named(name)?,
            None => cpal::default_host(),
        };
        let host_name = host.id().name().to_string();
        let device = match &choice.device {
            Some(name) => host
                .output_devices()
                .map_err(|e| DeviceError(format!("{host_name}: {e}")))?
                .find(|d| device_name(d).as_deref() == Some(name.as_str()))
                .ok_or_else(|| DeviceError(format!("{name} is not there on {host_name}")))?,
            None => host
                .default_output_device()
                .ok_or_else(|| DeviceError(format!("{host_name} has no default output")))?,
        };
        let name = device_name(&device).unwrap_or_else(|| "(unnamed)".to_string());
        let sample_rate = self.sample_rate;

        // A float configuration at the studio's rate, stereo if there is
        // one. The callback writes `f32`; a device that only takes another
        // format is one this cannot play, and the default stands in.
        let supported = device
            .supported_output_configs()
            .map_err(|e| DeviceError(format!("{name}: {e}")))?
            .filter(|c| {
                c.sample_format() == cpal::SampleFormat::F32
                    && (c.min_sample_rate()..=c.max_sample_rate()).contains(&sample_rate)
            })
            .min_by_key(|c| (c.channels() != 2, c.channels()))
            .map(|c| c.with_sample_rate(sample_rate));
        let supported = match supported {
            Some(supported) => supported,
            // Asked the old way: the device's default, at the studio's rate.
            // Some backends convert on the way in and say little about it.
            None => device
                .default_output_config()
                .map_err(|e| DeviceError(format!("{name}: {e}")))?,
        };
        let mut config = supported.config();
        config.sample_rate = sample_rate;
        config.buffer_size = first_buffer_size_that_opens(
            &device,
            config,
            &output_buffer_sizes(supported.buffer_size(), choice.buffer_frames),
        );
        let buffer_frames = match config.buffer_size {
            cpal::BufferSize::Fixed(frames) => Some(frames),
            cpal::BufferSize::Default => None,
        };
        let stream = build_stream(
            &device,
            config,
            self.renderer(true),
            Arc::clone(&self.stats),
        )
        .map_err(|e| DeviceError(format!("{name}: {}", e.0)))?;
        stream
            .play()
            .map_err(|e| DeviceError(format!("{name}: {e}")))?;
        Ok((
            Stream::Cpal(stream),
            OutputStatus {
                host: host_name,
                device: name,
                sample_rate,
                buffer_frames,
                latencies: None,
                fell_back: None,
            },
        ))
    }
}

/// The first of `sizes` a stream opens with, the last taken on trust.
///
/// Tried with a silent stream that is never started, so a refusal costs
/// nothing but the probe.
fn first_buffer_size_that_opens(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    sizes: &[cpal::BufferSize],
) -> cpal::BufferSize {
    let (last, rest) = sizes
        .split_last()
        .expect("output_buffer_sizes always ends with the default");
    for size in rest {
        let trial = cpal::StreamConfig {
            buffer_size: *size,
            ..config
        };
        let opened = device.build_output_stream(
            trial,
            |data: &mut [f32], _: &cpal::OutputCallbackInfo| data.fill(0.0),
            |_| {},
            None,
        );
        match opened {
            Ok(_) => return *size,
            Err(e) => eprintln!(
                "fontelle: the output would not open with {size:?} ({e}); trying the next size"
            ),
        }
    }
    *last
}

/// The real-time promotion handle, in the slot the output callback keeps it in.
///
/// Made **inside** the callback, on the audio thread, and never touched by
/// any other: the slot is moved into the closure empty and filled on the
/// first call. That is what makes it sound to declare it `Send` where the
/// platform's handle is not — on Windows it holds the raw AvRt task handle,
/// which the crate rightly refuses to mark. Never dropped either: cpal drops
/// the closure on the audio thread. One is leaked per stream opened, which
/// is a few bytes per change of device.
struct RtHandleSlot(ManuallyDrop<Option<audio_thread_priority::RtPriorityHandle>>);

// SAFETY: see the type's own note — the handle is created and used on one
// thread, the audio thread, and the slot crosses to it while still `None`.
unsafe impl Send for RtHandleSlot {}

/// The render every backend runs: one buffer of the song, interleaved into
/// `data`. Everything per-thread — the real-time promotion, the watchdog's
/// count, whether the callback has panicked — is the renderer's own;
/// everything the song needs is the slot's.
///
/// Held by exactly one audio thread. Its `Arc`s are `ManuallyDrop` and the
/// [`AudioOutput`] that made it holds its own, so wherever the renderer is
/// dropped — cpal drops its closure on the audio thread — what goes is a
/// count, never a free (INVARIANT 1).
pub(crate) struct Renderer {
    slot: ManuallyDrop<Arc<CallbackSlot<ManuallyDrop<OutputState>>>>,
    stats: ManuallyDrop<Arc<OutputStats>>,
    sample_rate: u32,
    /// Whether this renderer's own timing says a buffer was late. Not where
    /// the backend keeps a truer count (the ASIO helper's deadline).
    count_late: bool,
    first_callback: bool,
    rt_handle: RtHandleSlot,
    /// The watchdog's count as this thread last saw it, and when it was
    /// demoted — see `rt_budget`. A demoted callback waits out a few seconds
    /// on ordinary scheduling and then asks rtkit again.
    #[cfg(target_os = "linux")]
    demotions_seen: u64,
    #[cfg(target_os = "linux")]
    demoted_at: Option<Instant>,
    poisoned: bool,
}

impl Renderer {
    fn new(
        slot: Arc<CallbackSlot<ManuallyDrop<OutputState>>>,
        stats: Arc<OutputStats>,
        sample_rate: u32,
        count_late: bool,
    ) -> Self {
        Self {
            slot: ManuallyDrop::new(slot),
            stats: ManuallyDrop::new(stats),
            sample_rate,
            count_late,
            first_callback: true,
            rt_handle: RtHandleSlot(ManuallyDrop::new(None)),
            #[cfg(target_os = "linux")]
            demotions_seen: crate::rt_budget::demotions(),
            #[cfg(target_os = "linux")]
            demoted_at: None,
            poisoned: false,
        }
    }

    /// Fills `data` (`channels` to a frame) with the next stretch of the song.
    pub(crate) fn render(&mut self, data: &mut [f32], channels: usize) {
        if self.poisoned {
            data.fill(0.0);
            return;
        }
        let started = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.render_inner(data, channels)
        }));
        match result {
            // Timed from the second callback on: the first is the promotion.
            Ok(true) => {
                let frames = data.len() / channels.max(1);
                let period =
                    Duration::from_secs_f64(frames as f64 / f64::from(self.sample_rate.max(1)));
                if self.count_late {
                    self.stats.note_cycle(started.elapsed(), period);
                } else {
                    self.stats.note_load(started.elapsed(), period);
                }
            }
            Ok(false) => {}
            Err(payload) => {
                let message = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".to_string());
                eprintln!(
                    "Fontelle: audio callback panicked, silencing this stream for the \
                     rest of its life rather than crash the process: {message}"
                );
                self.poisoned = true;
                data.fill(0.0);
            }
        }
    }

    /// The render itself; whether it was a buffer worth timing.
    fn render_inner(&mut self, data: &mut [f32], channels: usize) -> bool {
        let sample_rate = self.sample_rate;
        if self.first_callback {
            // Treat the very first callback as warm-up, not steady-state RT
            // processing: promoting priority goes through rtkit over D-Bus on
            // Linux, which legitimately allocates for the one-time handshake,
            // and some backends do their own first-use lazy setup on this call
            // too. None of that is what INVARIANT 1 is meant to catch. Do the
            // promotion, output one silent buffer, and only start
            // tagging/enforcing RT from the second callback on.
            *self.rt_handle.0 = audio_thread_priority::promote_current_thread_to_real_time(
                BLOCK_SIZE as u32,
                sample_rate,
            )
            .ok();
            // The promotion leaves a budget of one block's CPU time before
            // SIGXCPU — a core dump by default. Widened, and watched: an
            // overrun is a demotion, not an exit (`rt_budget`).
            #[cfg(target_os = "linux")]
            {
                crate::rt_budget::widen_budget();
                crate::rt_budget::arm_current_thread();
            }
            self.first_callback = false;
            data.fill(0.0);
            return false;
        }
        // Demoted by the watchdog: run on ordinary scheduling for a few
        // seconds — the overload that spent the budget is likely still
        // there — then ask again. The D-Bus round trip allocates, which is
        // why this sits outside `with_rt_thread`, like the first promotion.
        #[cfg(target_os = "linux")]
        {
            let demotions = crate::rt_budget::demotions();
            if demotions != self.demotions_seen {
                self.demotions_seen = demotions;
                self.demoted_at = Some(Instant::now());
            }
            if let Some(since) = self.demoted_at
                && since.elapsed() >= RT_REPROMOTE_AFTER
            {
                self.demoted_at = None;
                *self.rt_handle.0 = audio_thread_priority::promote_current_thread_to_real_time(
                    BLOCK_SIZE as u32,
                    sample_rate,
                )
                .ok();
                crate::rt_budget::widen_budget();
            }
        }
        let slot = &self.slot;
        // The tag covers exactly our own processing and no more. The backend
        // owns this thread between callbacks and legitimately allocates on
        // it — notably, cpal's ALSA worker drops its `StreamWorkerContext` (a
        // `Box<[pollfd]>`) here as it exits. Leaving the tag set turned that
        // teardown into a phantom INVARIANT 1 violation; see
        // `rt_guard::with_rt_thread`.
        with_rt_thread(|| {
            // Parked (the output is being moved to another device): silence,
            // and hands off the state.
            let played = slot.with(|state| render_state(state, data, channels));
            if played.is_none() {
                data.fill(0.0);
            }
        });
        true
    }
}

/// One buffer of the song from `state` into `data`.
fn render_state(state: &mut OutputState, data: &mut [f32], channels: usize) {
    let OutputState {
        graph,
        timeline,
        transport,
        live,
        reader,
        gate,
        monitor,
        lent,
    } = state;
    let frames_total = data.len() / channels.max(1);

    // Drained once for the whole callback, not once per step: these events
    // arrived while the audio thread was away, they belong to this callback,
    // and a loop seam splitting the callback in two must not deliver them a
    // second time — which would retrigger every key currently going down.
    let live_events: &[TimedEvent] = match live.as_mut() {
        Some(source) => source.drain(
            reader.position(),
            transport.state() == TransportState::Recording,
        ),
        None => &[],
    };
    // Before any decision is taken: what a player is holding down is what
    // keeps the graph running through the silent start of an attack. See
    // `IdleGate::held`.
    gate.take_live(live_events);
    // Asked once a callback rather than assumed: the window opens and closes
    // the input while this stream runs, and a gate holding a stale answer is
    // either an idle window burning a core or a microphone nobody can hear.
    gate.set_monitoring(monitor.as_ref().is_some_and(|m| m.is_live()));
    // And whether somebody is at a plugin's own controls — see
    // `IdleGate::set_attended`.
    gate.set_attended(transport.is_attended());
    let mut live_pending = !live_events.is_empty();

    // Once per callback, not once per step: taking a newly published
    // timeline is a swap, but the binary search that repositions the cursor
    // into it is not free, and nothing is republished mid-callback.
    // The instruments, not the notes: a channel added or an instrument chosen
    // in the window rebuilds the graph, and this is where the running stream
    // picks the new one up. The graph it stops using goes back to the
    // publisher to be freed — never here (INVARIANT 1).
    graph.take_update();
    let graph = graph.current();
    // A render is playing the studio's own plugins (`Transport::hold`): their
    // processors go back to their bays, once, and the stream is silent until
    // it is done. A node takes its processor again on the first block after.
    if transport.is_held() {
        if !*lent {
            graph.retire();
            *lent = true;
        }
        data.fill(0.0);
        return;
    }
    *lent = false;

    let republished = timeline.has_update();
    let timeline: &CompiledTimeline = timeline.current();
    if republished {
        // The cursor indexed into the events we just stopped using. Without
        // this the next block either replays notes or skips them.
        reader.retarget(timeline);
    }

    let mut written = 0;
    while written < frames_total {
        // A stopped transport still has to make sound when someone is
        // playing the keyboard, and still has to cost nothing when nobody is.
        let awake = gate.is_awake(usize::from(live_pending));
        let step = reader.next_step(
            transport,
            timeline,
            frames_total - written,
            BLOCK_SIZE,
            awake,
        );
        let frames = step.frames;

        // Before processing, not after: a stop or a seek means the audio
        // that was in flight belongs to a different moment in the song, and
        // letting its release tail ring over the new position is the audible
        // form of the bug.
        if step.reset {
            // Scoped: a stop, a seek or a loop seam cuts the notes the *song*
            // was playing and leaves the ones a player is holding. The gate
            // is deliberately not cleared here — a live voice may well still
            // be sounding through this, and the next block's own measurement
            // is what decides whether anything still is.
            graph.reset_sequenced();
        }

        if step.process {
            let this_step = if live_pending { live_events } else { &[] };
            live_pending = false;
            graph.process_block_with_audio(
                step.events,
                this_step,
                step.audio,
                step.snapshot,
                step.range.clone(),
            );

            // Interleave the graph's planar buses into the device's frame
            // layout — the one and only place format conversion happens (TDD
            // §5.2). Device channel `c` reads bus `c`, clamped to whatever the
            // pool actually holds: a stereo graph into a mono device drops the
            // right bus, and a mono graph into a multi-channel device
            // duplicates across all of them.
            let buses = graph.buffer_pool.len();
            let mut peak = 0.0f32;
            for c in 0..channels {
                let bus = c.min(buses.saturating_sub(1));
                let block = graph.buffer_pool.buffer_mut(bus);
                for i in 0..frames {
                    let sample = block[i];
                    peak = peak.max(sample.abs());
                    data[(written + i) * channels + c] = sample;
                }
            }
            // Measured off the samples already being copied, so knowing
            // whether the graph is still making sound costs nothing beyond
            // the compare. It is what lets a stopped transport go back to
            // idle on its own once a live note has died away.
            gate.observe(peak);
        } else {
            // Stopped: no nodes run at all. This is the near-zero idle CPU
            // target (TDD §6.3, §19) and the whole reason the check is here
            // rather than inside the graph.
            let from = written * channels;
            data[from..from + frames * channels].fill(0.0);
        }

        written += frames;
    }
}

/// One `cpal` stream around a [`Renderer`].
fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut renderer: Renderer,
    stats: Arc<OutputStats>,
) -> Result<cpal::Stream, DeviceError> {
    let channels = config.channels as usize;
    // Held by the closure, and by the `AudioOutput` that outlives it, so
    // what the closure drops on the audio thread is a count and never a free.
    let errors = ManuallyDrop::new(stats);
    device
        .build_output_stream(
            config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                renderer.render(data, channels);
            },
            // A dropout is a count, not a line on a terminal: this runs on
            // the audio thread on most backends. Anything else is rare and
            // worth the line.
            move |err| {
                errors.note_error(&err);
                if !matches!(err.kind(), cpal::ErrorKind::Xrun) {
                    eprintln!("Fontelle: audio stream error: {err}");
                }
            },
            None,
        )
        .map_err(|e| DeviceError(e.to_string()))
}
