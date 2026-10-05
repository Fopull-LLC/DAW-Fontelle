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

use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fontelle_types::{CompiledTimeline, TimedEvent};

use crate::device::{BLOCK_SIZE, DeviceError};
use crate::graph_channel::GraphSource;
use crate::live::{IdleGate, LiveEventSource};
use crate::rt_guard::with_rt_thread;
use crate::timeline_channel::TimelineSource;
use crate::transport::{Transport, TransportReader, TransportState};

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
        }
    }
}

/// How long `frames` last at `sample_rate`, as a person reads it: "10.7 ms".
pub fn latency_label(frames: u32, sample_rate: u32) -> String {
    let ms = f64::from(frames) * 1000.0 / f64::from(sample_rate.max(1));
    format!("{ms:.1} ms")
}

/// What the stream's error callback has seen, read by the window.
///
/// Atomics only: the error callback runs on the audio thread on most
/// backends, and it used to `eprintln!` there for every underrun — a lock and
/// a write on the RT thread, at exactly the moment it was already late.
#[derive(Debug, Default)]
pub struct OutputStats {
    xruns: AtomicU64,
    lost: AtomicBool,
}

impl OutputStats {
    /// Takes one error from the stream. A dropout is counted; a device that
    /// went away is noted for the window to act on.
    pub fn note_error(&self, error: &cpal::Error) {
        match error.kind() {
            cpal::ErrorKind::Xrun => {
                self.xruns.fetch_add(1, Ordering::Relaxed);
            }
            cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable => {
                self.lost.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Dropouts the backend has reported since the count was last cleared.
    pub fn xruns(&self) -> u64 {
        self.xruns.load(Ordering::Relaxed)
    }

    pub fn clear_xruns(&self) {
        self.xruns.store(0, Ordering::Relaxed);
    }

    /// Whether the device has gone away under the stream.
    pub fn lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
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

/// Every output backend this build can use on this machine, by name, in
/// `cpal`'s order. The default is among them.
pub fn output_host_names() -> Vec<String> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| id.name().to_string())
        .collect()
}

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

/// The output stream, and what it plays, kept apart so the stream can be
/// opened again on another device.
pub struct AudioOutput {
    // First, so it goes first: cpal drops the callback closure on the audio
    // thread, and while this struct still holds its own `Arc`s below, what
    // the closure drops there is only ever a count, never a free
    // (INVARIANT 1).
    stream: Option<cpal::Stream>,
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
            Err(first) if *choice != OutputChoice::default() => {
                // The buffer the person chose is kept: it is the device that
                // would not open, most likely, not the size.
                let fallback = OutputChoice {
                    buffer_frames: choice.buffer_frames,
                    ..OutputChoice::default()
                };
                match self
                    .open(&fallback)
                    .or_else(|_| self.open(&OutputChoice::default()))
                {
                    Ok((stream, mut status)) => {
                        status.fell_back = Some(first.0);
                        Ok((stream, status))
                    }
                    Err(e) => Err(DeviceError(format!("{} (and the default: {e})", first.0))),
                }
            }
            Err(e) => Err(e),
        };
        let (stream, status) = opened?;
        self.stream = Some(stream);
        self.status = Some(status.clone());
        Ok(status)
    }

    /// What is open, or `None` when nothing is.
    pub fn status(&self) -> Option<&OutputStatus> {
        self.status.as_ref()
    }

    /// The counts the stream's error callback keeps.
    pub fn stats(&self) -> Arc<OutputStats> {
        Arc::clone(&self.stats)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
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

    fn open(&self, choice: &OutputChoice) -> Result<(cpal::Stream, OutputStatus), DeviceError> {
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
            Arc::clone(&self.slot),
            Arc::clone(&self.stats),
            sample_rate,
        )
        .map_err(|e| DeviceError(format!("{name}: {}", e.0)))?;
        stream
            .play()
            .map_err(|e| DeviceError(format!("{name}: {e}")))?;
        Ok((
            stream,
            OutputStatus {
                host: host_name,
                device: name,
                sample_rate,
                buffer_frames,
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

/// One stream over the slot's state. Everything per-thread — the real-time
/// promotion, the watchdog's count, whether the callback has panicked — is
/// the stream's own; everything the song needs is the slot's.
fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    slot: Arc<CallbackSlot<ManuallyDrop<OutputState>>>,
    stats: Arc<OutputStats>,
    sample_rate: u32,
) -> Result<cpal::Stream, DeviceError> {
    let channels = config.channels as usize;
    let mut first_callback = true;
    let mut rt_handle = RtHandleSlot(ManuallyDrop::new(None));
    // The watchdog's count as this thread last saw it, and when it was
    // demoted — see `rt_budget`. A demoted callback waits out a few seconds
    // on ordinary scheduling and then asks rtkit again.
    #[cfg(target_os = "linux")]
    let mut demotions_seen = crate::rt_budget::demotions();
    #[cfg(target_os = "linux")]
    let mut demoted_at: Option<std::time::Instant> = None;
    let mut poisoned = false;
    // Held by the closure, and by the `AudioOutput` that outlives it, so
    // what the closure drops on the audio thread is a count and never a free.
    let slot = ManuallyDrop::new(slot);
    let errors = ManuallyDrop::new(Arc::clone(&stats));

    device
        .build_output_stream(
            config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                if poisoned {
                    data.fill(0.0);
                    return;
                }

                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if first_callback {
                        // Treat the very first callback as warm-up, not
                        // steady-state RT processing: promoting priority goes
                        // through rtkit over D-Bus on Linux, which
                        // legitimately allocates for the one-time handshake,
                        // and some backends do their own first-use lazy setup
                        // on this call too. None of that is what INVARIANT 1
                        // is meant to catch. Do the promotion, output one
                        // silent buffer, and only start tagging/enforcing RT
                        // from the second callback on.
                        *rt_handle.0 = audio_thread_priority::promote_current_thread_to_real_time(
                            BLOCK_SIZE as u32,
                            sample_rate,
                        )
                        .ok();
                        // The promotion leaves a budget of one block's CPU
                        // time before SIGXCPU — a core dump by default.
                        // Widened, and watched: an overrun is a demotion, not
                        // an exit (`rt_budget`).
                        #[cfg(target_os = "linux")]
                        {
                            crate::rt_budget::widen_budget();
                            crate::rt_budget::arm_current_thread();
                        }
                        first_callback = false;
                        data.fill(0.0);
                        return;
                    }
                    // Demoted by the watchdog: run on ordinary scheduling for
                    // a few seconds — the overload that spent the budget is
                    // likely still there — then ask again. The D-Bus round
                    // trip allocates, which is why this sits outside
                    // `with_rt_thread`, like the first promotion.
                    #[cfg(target_os = "linux")]
                    {
                        let demotions = crate::rt_budget::demotions();
                        if demotions != demotions_seen {
                            demotions_seen = demotions;
                            demoted_at = Some(std::time::Instant::now());
                        }
                        if let Some(since) = demoted_at
                            && since.elapsed() >= RT_REPROMOTE_AFTER
                        {
                            demoted_at = None;
                            *rt_handle.0 =
                                audio_thread_priority::promote_current_thread_to_real_time(
                                    BLOCK_SIZE as u32,
                                    sample_rate,
                                )
                                .ok();
                            crate::rt_budget::widen_budget();
                        }
                    }
                    // The tag covers exactly our own processing and no more.
                    // The backend owns this thread between callbacks and
                    // legitimately allocates on it — notably, cpal's ALSA
                    // worker drops its `StreamWorkerContext` (a
                    // `Box<[pollfd]>`) here as it exits. Leaving the tag set
                    // turned that teardown into a phantom INVARIANT 1
                    // violation; see `rt_guard::with_rt_thread`.
                    with_rt_thread(|| {
                        // Parked (the output is being moved to another
                        // device): silence, and hands off the state.
                        let played = slot.with(|state| {
                            let OutputState {
                                graph,
                                timeline,
                                transport,
                                live,
                                reader,
                                gate,
                                monitor,
                                lent,
                            } = &mut **state;
                            let frames_total = data.len() / channels.max(1);

                            // Drained once for the whole callback, not once
                            // per step: these events arrived while the audio
                            // thread was away, they belong to this callback,
                            // and a loop seam splitting the callback in two
                            // must not deliver them a second time — which
                            // would retrigger every key currently going down.
                            let live_events: &[TimedEvent] = match live.as_mut() {
                                Some(source) => source.drain(
                                    reader.position(),
                                    transport.state() == TransportState::Recording,
                                ),
                                None => &[],
                            };
                            // Before any decision is taken: what a player is
                            // holding down is what keeps the graph running
                            // through the silent start of an attack. See
                            // `IdleGate::held`.
                            gate.take_live(live_events);
                            // Asked once a callback rather than assumed: the
                            // window opens and closes the input while this
                            // stream runs, and a gate holding a stale answer
                            // is either an idle window burning a core or a
                            // microphone nobody can hear.
                            gate.set_monitoring(monitor.as_ref().is_some_and(|m| m.is_live()));
                            // And whether somebody is at a plugin's own
                            // controls — see `IdleGate::set_attended`.
                            gate.set_attended(transport.is_attended());
                            let mut live_pending = !live_events.is_empty();

                            // Once per callback, not once per step: taking a
                            // newly published timeline is a swap, but the
                            // binary search that repositions the cursor into
                            // it is not free, and nothing is republished
                            // mid-callback.
                            // The instruments, not the notes: a channel added
                            // or an instrument chosen in the window rebuilds
                            // the graph, and this is where the running stream
                            // picks the new one up. The graph it stops using
                            // goes back to the publisher to be freed — never
                            // here (INVARIANT 1).
                            graph.take_update();
                            let graph = graph.current();
                            // A render is playing the studio's own plugins
                            // (`Transport::hold`): their processors go back
                            // to their bays, once, and the stream is silent
                            // until it is done. A node takes its processor
                            // again on the first block after.
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
                                // The cursor indexed into the events we just
                                // stopped using. Without this the next block
                                // either replays notes or skips them.
                                reader.retarget(timeline);
                            }

                            let mut written = 0;
                            while written < frames_total {
                                // A stopped transport still has to make sound
                                // when someone is playing the keyboard, and
                                // still has to cost nothing when nobody is.
                                let awake = gate.is_awake(usize::from(live_pending));
                                let step = reader.next_step(
                                    transport,
                                    timeline,
                                    frames_total - written,
                                    BLOCK_SIZE,
                                    awake,
                                );
                                let frames = step.frames;

                                // Before processing, not after: a stop or a
                                // seek means the audio that was in flight
                                // belongs to a different moment in the song,
                                // and letting its release tail ring over the
                                // new position is the audible form of the bug.
                                if step.reset {
                                    // Scoped: a stop, a seek or a loop seam
                                    // cuts the notes the *song* was playing
                                    // and leaves the ones a player is holding.
                                    // The gate is deliberately not cleared
                                    // here — a live voice may well still be
                                    // sounding through this, and the next
                                    // block's own measurement is what decides
                                    // whether anything still is.
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

                                    // Interleave the graph's planar buses into
                                    // the device's frame layout — the one and
                                    // only place format conversion happens
                                    // (TDD §5.2). Device channel `c` reads bus
                                    // `c`, clamped to whatever the pool
                                    // actually holds: a stereo graph into a
                                    // mono device drops the right bus, and a
                                    // mono graph into a multi-channel device
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
                                    // Measured off the samples already being
                                    // copied, so knowing whether the graph is
                                    // still making sound costs nothing beyond
                                    // the compare. It is what lets a stopped
                                    // transport go back to idle on its own
                                    // once a live note has died away.
                                    gate.observe(peak);
                                } else {
                                    // Stopped: no nodes run at all. This is
                                    // the near-zero idle CPU target (TDD §6.3,
                                    // §19) and the whole reason the check is
                                    // here rather than inside the graph.
                                    let from = written * channels;
                                    data[from..from + frames * channels].fill(0.0);
                                }

                                written += frames;
                            }
                        });
                        if played.is_none() {
                            data.fill(0.0);
                        }
                    });

                    let _ = &rt_handle; // held for the stream's life; never dropped (see above)
                }));

                if let Err(payload) = result {
                    let message = payload
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "non-string panic payload".to_string());
                    eprintln!(
                        "Fontelle: audio callback panicked, silencing this stream for the \
                         rest of its life rather than crash the process: {message}"
                    );
                    poisoned = true;
                    data.fill(0.0);
                }
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
