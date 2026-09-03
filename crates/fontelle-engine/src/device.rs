use std::mem::ManuallyDrop;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fontelle_types::{CompiledTimeline, TimedEvent};

use crate::graph_channel::GraphSource;
use crate::live::{IdleGate, LiveEventSource};
use crate::rt_guard::with_rt_thread;
use crate::timeline_channel::TimelineSource;
use crate::transport::TransportState;
use crate::transport::{Transport, TransportReader};

/// The fixed block size the M0 vertical slice targets (TDD §22: "128 frames /
/// 48 kHz"). `CompiledGraph`'s `BufferPool` is sized to this; a backend that
/// won't honour the requested fixed buffer size still works — the callback
/// below processes in chunks of at most this size regardless of what the
/// device actually delivers per call.
pub const BLOCK_SIZE: usize = 128;

#[derive(Debug)]
pub struct DeviceError(pub String);

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceError {}

/// Thin wrapper over `cpal`'s host/device selection (TDD §3.1: native PipeWire
/// backend, auto-selecting PipeWire > PulseAudio > ALSA on Linux; ASIO on Windows;
/// CoreAudio on macOS).
pub struct AudioDevice {
    host: cpal::Host,
    stream: Option<cpal::Stream>,
    /// The capture stream, when one is open — see
    /// [`start_input_stream`](AudioDevice::start_input_stream). Separate from
    /// `stream` because they are opened and closed at different moments: the
    /// output lives for the session and the input only while a track is armed.
    input: Option<cpal::Stream>,
}

impl AudioDevice {
    pub fn default_host() -> Self {
        Self {
            host: cpal::default_host(),
            stream: None,
            input: None,
        }
    }

    pub fn default_output_name(&self) -> Option<String> {
        self.host
            .default_output_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string())
    }

    /// Every input the host can see, by name (TDD §15.4).
    ///
    /// *"i click a input button that lets my select my mic input."* Names
    /// rather than handles, because the answer is written into the document —
    /// a project reopened tomorrow has to find the same microphone, and a
    /// device index is not the same device twice.
    ///
    /// A machine with no microphone gets an empty list, which is a state and
    /// not a failure.
    ///
    /// # Why this is filtered, and how
    ///
    /// Found by opening the menu on a real machine: ALSA offered **thirty-two**
    /// inputs. Four of them were the same Scarlett; most of the rest were
    /// plumbing — *"Rate Converter Plugin Using Libav/FFmpeg Library"*,
    /// *"Plugin for channel upmix (4,6,8)"* — and half of them cannot capture
    /// at all. A menu like that is one nobody can find their microphone in.
    ///
    /// So each device is **asked whether it will open**, which is a real
    /// question rather than a guess at what a name means, and the list is
    /// deduplicated. On that machine it goes from thirty-two rows to seven.
    /// The probe costs an open per device, which is fine for something that
    /// happens when a menu is clicked and would not be if it happened per
    /// frame.
    pub fn input_names(&self) -> Vec<String> {
        let Ok(devices) = self.host.input_devices() else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for device in devices {
            // The order matters: `default_input_config` is the expensive half
            // and the one that makes ALSA print its own complaints, so a
            // device with no usable name is dropped before it is probed.
            let Some(name) = device
                .description()
                .ok()
                .map(|desc| desc.name().to_string())
                .filter(|name| !name.trim().is_empty())
            else {
                continue;
            };
            if names.contains(&name) {
                continue;
            }
            if device.default_input_config().is_err() {
                continue;
            }
            names.push(name);
        }
        names
    }

    /// The host's default input, **if it is one this program would offer**.
    ///
    /// The filter is not pedantry: on the machine this was written on, ALSA's
    /// `default` PCM describes itself as *"Default Audio Device"* and then
    /// refuses to open for capture. Naming it anyway would put a device in
    /// front of somebody that cannot record, which is the one thing this
    /// question must not do — so the two answers agree by construction.
    pub fn default_input_name(&self) -> Option<String> {
        let name = self
            .host
            .default_input_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string())
            .filter(|name| !name.trim().is_empty())?;
        self.input_names().contains(&name).then_some(name)
    }

    /// Opens an input stream on the device called `name`, pushing every block
    /// it delivers into `writer` (TDD §15.4).
    ///
    /// Returns the rate and channel count the device actually opened at, which
    /// are **not** negotiable the way the output's are: a microphone runs at
    /// what its interface runs at, and asking for something else either fails
    /// or resamples behind your back. The take records the rate it was
    /// captured at and the clip player reads it at the ratio to the device's —
    /// the same arrangement an imported file gets, for the same reason.
    ///
    /// The callback does one thing: push and return. §15.4's rule — *"the RT
    /// thread never touches the filesystem"* — and INVARIANT 1's. Everything
    /// else, the WAV included, happens on a thread that is allowed to be slow.
    ///
    /// Wrapped in `catch_unwind` for the reason the output callback is: a Rust
    /// panic unwinding across ALSA's C boundary is undefined behaviour and
    /// hard-aborts the process regardless of its cause.
    pub fn start_input_stream(
        &mut self,
        name: Option<&str>,
        writer: crate::InputWriter,
    ) -> Result<(u32, u16), DeviceError> {
        let device = match name {
            Some(wanted) => self
                .host
                .input_devices()
                .map_err(|e| DeviceError(e.to_string()))?
                .find(|device| {
                    device
                        .description()
                        .map(|d| d.name() == wanted)
                        .unwrap_or(false)
                })
                .ok_or_else(|| DeviceError(format!("no input called \u{201c}{wanted}\u{201d}")))?,
            None => self
                .host
                .default_input_device()
                .ok_or_else(|| DeviceError("no default input device".into()))?,
        };

        let config = device
            .default_input_config()
            .map_err(|e| DeviceError(e.to_string()))?
            .config();
        let sample_rate = config.sample_rate;
        let channels = config.channels;

        let mut writer = ManuallyDrop::new(writer);
        let mut poisoned = false;
        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    if poisoned {
                        return;
                    }
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        writer.write(data);
                    }));
                    if result.is_err() {
                        eprintln!("fontelle: the input callback panicked; capture stopped");
                        poisoned = true;
                    }
                },
                |e| eprintln!("fontelle: input stream error: {e}"),
                None,
            )
            .map_err(|e| DeviceError(e.to_string()))?;
        stream.play().map_err(|e| DeviceError(e.to_string()))?;
        self.input = Some(stream);
        Ok((sample_rate, channels))
    }

    /// Closes the input stream, if one is open.
    pub fn stop_input(&mut self) {
        self.input = None;
    }

    /// Builds and starts the output stream, driving `graph` from the real
    /// audio callback. Promotes the callback thread to RT priority
    /// (`audio_thread_priority`) and tags it via `rt_guard::mark_current_thread_rt`
    /// on its first invocation — both must happen *inside* the callback since
    /// cpal creates that OS thread itself; there's no separate "thread
    /// started" hook to do it from ahead of time.
    ///
    /// The callback's whole body runs inside `catch_unwind`, unconditionally
    /// (not just in debug builds). This isn't only about `RtGuardAllocator`:
    /// on Linux, ALSA calls this closure through a C function pointer, and a
    /// Rust panic unwinding across that boundary is undefined behaviour — the
    /// runtime detects it and hard-aborts the whole process no matter how
    /// clean the panic message is. Confirmed the hard way: fixing the
    /// double-panic in `rt_guard` and a teardown-drop panic (both real bugs,
    /// both still fixed below) didn't stop the crash, because *any* panic
    /// reaching ALSA's callback aborts regardless of its cause. Catching it
    /// here — reporting once, then outputting silence for the rest of the
    /// stream's life — is standard practice for audio callbacks generally,
    /// not a debug-only aid.
    ///
    /// The returned stream is stopped and dropped when `self` is dropped or
    /// `stop` is called — cpal has no separate "close" step, and (confirmed on
    /// real hardware) it tears down the callback closure *on the audio thread
    /// itself*, which is still tagged RT at that point. Dropping `graph`
    /// there — freeing its `Patch`es, sample buffers, everything — would
    /// violate INVARIANT 1 just as surely as allocating would. `graph`,
    /// `timeline`, and the RT-priority handle are therefore wrapped in
    /// `ManuallyDrop` so that implicit teardown drop does nothing. This is a
    /// real, deliberate leak — acceptable for now because every current
    /// caller (`fontelle-app --play-sf2`, the manual test) exits the whole
    /// process shortly after `stop()`, so the OS reclaims the memory anyway.
    /// A long-running DAW process needs a proper deferred-drop ("trash bin":
    /// hand the old graph to a channel a non-RT thread actually frees)
    /// instead — not built yet, see `PROGRESS.md`.
    ///
    /// `transport` is the shared state the callback is driven *by*: play,
    /// stop, seek and loop all reach the audio thread through it, and the
    /// playhead comes back the same way. The whole per-block decision lives in
    /// `TransportReader` rather than here, because code inside this closure
    /// can only be run by a real sound card and therefore can only be tested
    /// by ear. Everything below is the loop around it plus the interleave.
    ///
    /// `live` is the consumer end of the live-input channel (TDD §14.1) — a
    /// MIDI keyboard's events, drained once per callback and handed to the
    /// graph alongside the timeline's. Pass `None` for a pure playback stream.
    ///
    /// `timeline` is walked by sample position each block via
    /// `CompiledTimeline::events_for_block` — a cursor into its already-sorted
    /// `events`, no allocation, no traversal beyond a linear scan (same
    /// RT-safety shape as `CompiledGraph::process_block` itself), repositioned
    /// by binary search when the reader observes a seek. Passing
    /// `CompiledTimeline::empty()` plays silence unless a node already has an
    /// active voice from some other trigger (the manual hardware test still
    /// does its note-on that way).
    pub fn start_output_stream(
        &mut self,
        graph: GraphSource,
        timeline: TimelineSource,
        sample_rate: u32,
        transport: Arc<Transport>,
        live: Option<LiveEventSource>,
    ) -> Result<(), DeviceError> {
        // Off-RT, before the stream exists: nodes size their internal buffers
        // here so the callback never has to. Everything published later is
        // prepared by `GraphPublisher`'s caller, on its own thread, for the
        // same reason.
        let mut graph = graph;
        graph
            .current()
            .prepare(sample_rate as f32, BLOCK_SIZE as u32);
        let mut graph = ManuallyDrop::new(graph);
        let mut timeline = ManuallyDrop::new(timeline);
        let device = self
            .host
            .default_output_device()
            .ok_or_else(|| DeviceError("no default output device".into()))?;

        let mut config = device
            .default_output_config()
            .map_err(|e| DeviceError(e.to_string()))?
            .config();
        config.sample_rate = sample_rate;
        config.buffer_size = cpal::BufferSize::Fixed(BLOCK_SIZE as u32);
        let channels = config.channels as usize;

        let mut first_callback = true;
        let mut rt_handle: ManuallyDrop<Option<audio_thread_priority::RtPriorityHandle>> =
            ManuallyDrop::new(None);
        // The caller keeps its own clone for the stream's life, so dropping
        // this one on the audio thread at teardown is a refcount decrement and
        // never a free — but it is wrapped like everything else the closure
        // owns so that stays true no matter what the caller does with theirs.
        let transport = ManuallyDrop::new(transport);
        let mut live = ManuallyDrop::new(live);
        let mut reader = TransportReader::new();
        let mut gate = IdleGate::new();
        let mut poisoned = false;

        let stream = device
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
                            // steady-state RT processing: promoting priority
                            // goes through rtkit over D-Bus on Linux, which
                            // legitimately allocates for the one-time
                            // handshake, and some backends do their own
                            // first-use lazy setup (format conversion buffers
                            // etc.) on this call too. None of that is what
                            // INVARIANT 1 is meant to catch. Do the promotion,
                            // output one silent block, and only start
                            // tagging/enforcing RT from the second callback
                            // on — ~2.7ms of silence at 128 samples/48kHz,
                            // not audible.
                            *rt_handle =
                                audio_thread_priority::promote_current_thread_to_real_time(
                                    BLOCK_SIZE as u32,
                                    sample_rate,
                                )
                                .ok();
                            first_callback = false;
                            data.fill(0.0);
                            return;
                        }
                        // The tag covers exactly our own processing and no
                        // more. The backend owns this thread between
                        // callbacks and legitimately allocates on it —
                        // notably, cpal's ALSA worker drops its
                        // `StreamWorkerContext` (a `Box<[pollfd]>`) here as it
                        // exits. Leaving the tag set turned that teardown into
                        // a phantom INVARIANT 1 violation that looked for a
                        // long time like a per-block allocation; see
                        // `rt_guard::with_rt_thread` and `PROGRESS.md`.
                        with_rt_thread(|| {
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
                                    &transport,
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

                        let _ = &rt_handle; // held for the stream's life; never dropped (see doc comment above)
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
                |err| eprintln!("Fontelle: audio stream error: {err}"),
                None,
            )
            .map_err(|e| DeviceError(e.to_string()))?;

        stream.play().map_err(|e| DeviceError(e.to_string()))?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }
}
