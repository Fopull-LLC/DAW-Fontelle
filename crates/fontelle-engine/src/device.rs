use std::mem::ManuallyDrop;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::graph_channel::GraphSource;
use crate::live::LiveEventSource;
use crate::timeline_channel::TimelineSource;
use crate::transport::Transport;

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
    /// The output stream, when this device opened one — see
    /// [`crate::AudioOutput`].
    output: Option<crate::AudioOutput>,
    /// The capture stream, when one is open — see
    /// [`start_input_stream`](AudioDevice::start_input_stream). Separate from
    /// `stream` because they are opened and closed at different moments: the
    /// output lives for the session and the input only while a track is armed.
    input: Option<cpal::Stream>,
    /// Where the input callback's samples go so the graph can play them
    /// (TDD §15.4). See [`crate::InputMonitor`].
    ///
    /// On the *device* rather than passed to each call because both streams
    /// touch it and they are opened at different moments: the output callback
    /// asks it whether anything is being monitored, the input callback fills
    /// it. A device with none simply does not monitor.
    monitor: Option<Arc<crate::InputMonitor>>,
    /// The capture stream when it is a PipeWire node rather than a `cpal`
    /// device — see [`crate::PipeWireInput`]. At most one of this and
    /// `input` is open.
    #[cfg(target_os = "linux")]
    pipewire_input: Option<crate::PipeWireInput>,
}

impl AudioDevice {
    pub fn default_host() -> Self {
        Self {
            host: cpal::default_host(),
            output: None,
            input: None,
            monitor: None,
            #[cfg(target_os = "linux")]
            pipewire_input: None,
        }
    }

    /// Hands this device the ring that carries a live input into the graph.
    ///
    /// Given **before** either stream is opened, because it is what the input
    /// callback writes into and what the output callback reads to know whether
    /// the graph has to keep running while the transport is stopped.
    pub fn with_monitor(mut self, monitor: Arc<crate::InputMonitor>) -> Self {
        self.monitor = Some(monitor);
        self
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
    ///
    /// # On a PipeWire desktop, PipeWire is asked instead
    ///
    /// *"its not recognizing my logitech camera mic input / ... naming it
    /// something different sometimes than others."* The probe above has a
    /// blind spot a sound server makes permanent: PipeWire holds the
    /// hardware, so a device another program is listening to through it
    /// refuses a direct open and drops out of the list, and the alias that
    /// happens to open decides the name. So when PipeWire is running its
    /// sources are listed by their own descriptions — one name per device,
    /// every day — and opened through its ALSA plugin, which shares. See
    /// `crate::pipewire`. A machine without PipeWire gets the list below.
    pub fn input_names(&self) -> Vec<String> {
        let sources = crate::pipewire_sources();
        if !sources.is_empty() {
            return crate::source_menu(&sources);
        }
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
        let sources = crate::pipewire_sources();
        if !sources.is_empty() {
            // PipeWire's own default, spelled the way the menu spells it —
            // and the first source when it has no default, which is still a
            // microphone somebody can record from.
            let menu = crate::source_menu(&sources);
            let wanted = crate::pipewire_default_source();
            let index = wanted
                .and_then(|node| sources.iter().position(|s| s.node == node))
                .unwrap_or(0);
            return menu.get(index).cloned();
        }
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
        // A PipeWire source is opened through PipeWire — see `input_names`
        // for why, and `crate::pipewire` for how. The rate asked for is the
        // output's, because PipeWire will resample to it and a take at the
        // rate the song plays at is one less ratio to get right.
        // The PipeWire path is Linux's: the plugin it opens through is ALSA's,
        // and `pw-dump` answers nothing anywhere else, so every other platform
        // falls through to `cpal` below.
        #[cfg(target_os = "linux")]
        let sources = crate::pipewire_sources();
        #[cfg(target_os = "linux")]
        if !sources.is_empty() {
            let source = match name {
                Some(wanted) => crate::find_pipewire_source(&sources, wanted).ok_or_else(|| {
                    DeviceError(format!("no input called \u{201c}{wanted}\u{201d}"))
                })?,
                None => {
                    let default = crate::pipewire_default_source();
                    default
                        .and_then(|node| sources.iter().find(|s| s.node == node))
                        .or(sources.first())
                        .ok_or_else(|| DeviceError("no default input device".into()))?
                }
            };
            let wanted_rate = self
                .host
                .default_output_device()
                .and_then(|d| d.default_output_config().ok())
                .map_or(48_000, |c| c.sample_rate());
            let (input, rate, channels) = crate::PipeWireInput::open(
                &source.node,
                source.channels,
                wanted_rate,
                writer,
                self.monitor.clone(),
            )?;
            self.pipewire_input = Some(input);
            return Ok((rate, channels));
        }
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

        let supported = device
            .default_input_config()
            .map_err(|e| DeviceError(e.to_string()))?;
        let mut config = supported.config();
        let sample_rate = config.sample_rate;
        let channels = config.channels;
        // Ask for the same block the output runs at, because **monitoring
        // latency is one input period**: the reader holds enough slack to ride
        // out the gap between deliveries, and a device handing over a thousand
        // frames at a time costs twenty-one milliseconds of it.
        //
        // The device is *asked* rather than told — its own supported range,
        // not a guess — because a config it will not take is a stream that
        // fails to open, and no capture at all is far worse than a monitor
        // with more latency. Whatever it actually delivers is measured on the
        // way past; see `InputMonitor::device_block`.
        if let cpal::SupportedBufferSize::Range { min, max } = supported.buffer_size()
            && (*min..=*max).contains(&(BLOCK_SIZE as u32))
        {
            config.buffer_size = cpal::BufferSize::Fixed(BLOCK_SIZE as u32);
        }

        let mut writer = ManuallyDrop::new(writer);
        // Opened before the stream starts, so the first block the callback
        // delivers already finds a ring that knows what rate it is at.
        if let Some(monitor) = &self.monitor {
            monitor.open(sample_rate, channels);
        }
        // `ManuallyDrop` for the reason the output callback's clone is: this
        // closure is torn down on an audio thread too.
        let monitor = ManuallyDrop::new(self.monitor.clone());
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
                        // The same block into the second ring, so what is kept
                        // and what is heard are the same samples rather than
                        // two readings of the device.
                        if let Some(monitor) = &*monitor {
                            monitor.write(data);
                        }
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
        #[cfg(target_os = "linux")]
        {
            self.pipewire_input = None;
        }
        // Before the stream is really gone, so nothing that was still in
        // flight is played through whatever is opened next.
        if let Some(monitor) = &self.monitor {
            monitor.close();
        }
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
    /// The stream is stopped and dropped when `self` is dropped or `stop` is
    /// called. cpal tears the callback closure down *on the audio thread
    /// itself*, still tagged RT, so nothing the closure drops may free: what
    /// it plays lives in [`crate::AudioOutput`]'s `CallbackSlot`, never
    /// dropped, and the closure holds only counts on it. That is also what
    /// lets the stream be opened again on another device without the song
    /// going with it (`AudioOutput::reopen`).
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
        self.start_output(
            graph,
            timeline,
            sample_rate,
            transport,
            live,
            &crate::OutputChoice::default(),
        )
        .map(|_| ())
    }

    /// [`start_output_stream`](Self::start_output_stream) on the backend,
    /// device and buffer `choice` names — or the default, if that will not
    /// open; the status says which opened and why.
    pub fn start_output(
        &mut self,
        graph: GraphSource,
        timeline: TimelineSource,
        sample_rate: u32,
        transport: Arc<Transport>,
        live: Option<LiveEventSource>,
        choice: &crate::OutputChoice,
    ) -> Result<crate::OutputStatus, DeviceError> {
        let output = crate::AudioOutput::start(
            graph,
            timeline,
            sample_rate,
            transport,
            live,
            self.monitor.clone(),
            choice,
        )?;
        let status = output
            .status()
            .cloned()
            .ok_or_else(|| DeviceError("the output did not open".into()))?;
        self.output = Some(output);
        Ok(status)
    }

    /// The running output, for a caller that will change it while it plays
    /// (Settings) — taken out of the device, which then has none to stop.
    pub fn take_output(&mut self) -> Option<crate::AudioOutput> {
        self.output.take()
    }

    pub fn stop(&mut self) {
        self.output = None;
    }
}

impl Drop for AudioDevice {
    /// A device that goes takes its input stream with it, and **says so**
    /// to the monitor. Dropping the streams alone did the first half: the
    /// session used to let go of the device to close an input, and the ring
    /// went on answering "a stream is open" — to a `MonitorNode` that then
    /// waited forever to prime, and to the idle gate, which kept the graph
    /// running for a microphone nobody had any more.
    fn drop(&mut self) {
        self.stop_input();
    }
}
