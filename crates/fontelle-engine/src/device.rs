use std::mem::ManuallyDrop;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fontelle_types::CompiledTimeline;

use crate::graph::CompiledGraph;
use crate::rt_guard::with_rt_thread;
use crate::transport::{TransportSnapshot, TransportState};

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
}

impl AudioDevice {
    pub fn default_host() -> Self {
        Self {
            host: cpal::default_host(),
            stream: None,
        }
    }

    pub fn default_output_name(&self) -> Option<String> {
        self.host
            .default_output_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string())
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
    /// `timeline` is walked by sample position each block via
    /// `CompiledTimeline::events_for_block` — a monotonically-advancing
    /// cursor into its already-sorted `events`, no allocation, no traversal
    /// beyond a linear scan (same RT-safety shape as `CompiledGraph::
    /// process_block` itself). Passing `CompiledTimeline::empty()` plays
    /// silence unless a node already has an active voice from some other
    /// trigger (the manual hardware test still does its note-on that way).
    pub fn start_output_stream(
        &mut self,
        graph: CompiledGraph,
        timeline: CompiledTimeline,
        sample_rate: u32,
    ) -> Result<(), DeviceError> {
        // Off-RT, before the stream exists: nodes size their internal buffers
        // here so the callback never has to.
        let mut graph = graph;
        graph.prepare(sample_rate as f32, BLOCK_SIZE as u32);
        let mut graph = ManuallyDrop::new(graph);
        let timeline = ManuallyDrop::new(timeline);
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
        let mut sample_counter: i64 = 0;
        let mut event_cursor: usize = 0;
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
                            let mut written = 0;
                            while written < frames_total {
                                let chunk = (frames_total - written).min(BLOCK_SIZE);
                                let transport = TransportSnapshot {
                                    state: TransportState::Playing,
                                    position_sample: sample_counter,
                                };
                                let block_range = sample_counter..sample_counter + chunk as i64;
                                let events = timeline
                                    .events_for_block(&mut event_cursor, block_range.clone());
                                graph.process_block(events, transport, block_range);

                                // Interleave the graph's planar buses into the
                                // device's frame layout — the one and only
                                // place format conversion happens (TDD §5.2).
                                // Device channel `c` reads bus `c`, clamped to
                                // whatever the pool actually holds: a stereo
                                // graph into a mono device drops the right
                                // bus, and a mono graph into a multi-channel
                                // device duplicates across all of them.
                                let buses = graph.buffer_pool.len();
                                for c in 0..channels {
                                    let bus = c.min(buses.saturating_sub(1));
                                    let block = graph.buffer_pool.buffer_mut(bus);
                                    for i in 0..chunk {
                                        data[(written + i) * channels + c] = block[i];
                                    }
                                }

                                written += chunk;
                                sample_counter += chunk as i64;
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
