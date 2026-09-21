//! Microphones on a PipeWire desktop, asked of PipeWire (TDD §15.4).
//!
//! Reported from using the window: *"for some reason its not recognizing my
//! logitech camera mic input / i think it actually is but it is naming it
//! something different sometimes than others."*
//!
//! Both halves were true and one cause explains both. The input list used to
//! be ALSA's own PCM names, filtered by asking each whether it would open. On
//! a PipeWire desktop the sound server **holds the hardware**: when another
//! program was listening to the camera through PipeWire, every direct ALSA
//! open of it failed and the camera vanished from the menu; when nothing
//! was, whichever of ALSA's several aliases for the same card opened first
//! gave the entry its name — *"Logi Webcam C920e, USB Audio"* one day, *"Logi
//! Webcam C920e"* the next — and a project saved with one could not find the
//! other.
//!
//! So on a machine running PipeWire the sources are asked of PipeWire. It
//! names each one **once** (`node.description`), it **shares** a device with
//! whoever else is listening, and it is what the output has been going
//! through all along — `AudioDevice::start_output_stream` opens ALSA's
//! `default`, which on this desktop *is* the PipeWire plugin. One sound
//! server on both ends, rather than PipeWire on the way out and a fight
//! over the hardware on the way in.
//!
//! # How it is read, and why not a library
//!
//! `pw-dump` prints the server's whole object graph as JSON, and reading that
//! is a few lines against `serde_json`. The alternative — binding
//! `libpipewire` — is a build-time dependency on the PipeWire headers and a
//! bindgen pass, for a question that is asked when a menu is clicked and
//! answered in twenty milliseconds. A machine without `pw-dump` gets no
//! sources from here and the ALSA list it always had.
//!
//! # How it is opened
//!
//! Through PipeWire's own ALSA plugin: `pipewire:NODE=<node.name>` is a PCM
//! that captures one node, and it opens whether or not anybody else has the
//! device. The stream runs on a thread of its own doing one thing — read a
//! period, push it into the take's ring and the monitor's — which is the
//! same contract the `cpal` input callback keeps.

#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
use crate::audio_input::InputWriter;
#[cfg(target_os = "linux")]
use crate::device::{BLOCK_SIZE, DeviceError};
#[cfg(target_os = "linux")]
use crate::input_monitor::InputMonitor;

/// One capture node, as PipeWire describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireSource {
    /// `node.name`: what opens it, and what never changes for a device.
    pub node: String,
    /// `node.description`: what a person calls it. The node's name when it
    /// has none.
    pub description: String,
    /// `api.alsa.card.name`, when the node is an ALSA card: the name the
    /// old ALSA list knew the same microphone by.
    pub card: Option<String>,
    /// How many channels it delivers, one or two. Two when it does not say,
    /// because every PipeWire source can be asked for two.
    pub channels: u16,
}

/// The sources in a `pw-dump`, in the server's order.
///
/// `Audio/Source` nodes and nothing else — not sinks, not devices, and not
/// another program's stream. A menu with *"ALSA plug-in [resolve]"* in it is
/// the thirty-two-row menu the ALSA filter existed to prevent, again.
///
/// Anything that is not the JSON `pw-dump` writes is no sources, never a
/// panic: this reads another program's output.
pub fn parse_pipewire_sources(dump: &str) -> Vec<PipeWireSource> {
    let Ok(serde_json::Value::Array(objects)) = serde_json::from_str::<serde_json::Value>(dump)
    else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|object| {
            if object.get("type")?.as_str()? != "PipeWire:Interface:Node" {
                return None;
            }
            let props = object.get("info")?.get("props")?;
            let class = props.get("media.class")?.as_str()?;
            if class != "Audio/Source" && !class.starts_with("Audio/Source/") {
                return None;
            }
            let node = props.get("node.name")?.as_str()?.trim().to_string();
            if node.is_empty() {
                return None;
            }
            let description = props
                .get("node.description")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| node.clone());
            let card = props
                .get("api.alsa.card.name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let channels = props
                .get("audio.channels")
                .and_then(|v| v.as_u64())
                .map_or(2, |n| n.clamp(1, 2) as u16);
            Some(PipeWireSource {
                node,
                description,
                card,
                channels,
            })
        })
        .collect()
}

/// The node PipeWire records from when nobody says which — its
/// `default.audio.source` — if the dump names one.
pub fn parse_pipewire_default_source(dump: &str) -> Option<String> {
    let serde_json::Value::Array(objects) = serde_json::from_str::<serde_json::Value>(dump).ok()?
    else {
        return None;
    };
    objects.iter().find_map(|object| {
        if object.get("type")?.as_str()? != "PipeWire:Interface:Metadata" {
            return None;
        }
        object
            .get("metadata")?
            .as_array()?
            .iter()
            .find_map(|entry| {
                if entry.get("key")?.as_str()? != "default.audio.source" {
                    return None;
                }
                Some(entry.get("value")?.get("name")?.as_str()?.to_string())
            })
    })
}

/// Asks the running PipeWire for its sources. Empty when there is no
/// `pw-dump`, no server, or nothing to record from — all of which are states
/// and not failures.
pub fn pipewire_sources() -> Vec<PipeWireSource> {
    parse_pipewire_sources(&dump())
}

/// The running PipeWire's default source, by node name.
pub fn pipewire_default_source() -> Option<String> {
    parse_pipewire_default_source(&dump())
}

fn dump() -> String {
    std::process::Command::new("pw-dump")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// What the input menu shows: each source's description, in the server's
/// order, told apart when two describe themselves alike.
///
/// Two identical USB microphones are two rows that would otherwise read the
/// same, and a menu where choosing the second chooses the first is worse
/// than a number after the name.
pub fn source_menu(sources: &[PipeWireSource]) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(sources.len());
    for source in sources {
        let mut name = source.description.clone();
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{} ({n})", source.description);
            n += 1;
        }
        names.push(name);
    }
    names
}

/// The source a saved name means, if it is here.
///
/// Three readings, in order: the name a menu row was made from (which is the
/// description, numbered when two were alike); a plain description; and the
/// name the **old ALSA list** would have written into a project — *"Logi
/// Webcam C920e, USB Audio"*, the card in front of the comma — so a project
/// saved before this still finds its microphone.
pub fn find_pipewire_source<'a>(
    sources: &'a [PipeWireSource],
    name: &str,
) -> Option<&'a PipeWireSource> {
    let name = name.trim();
    if let Some(index) = source_menu(sources).iter().position(|row| row == name) {
        return sources.get(index);
    }
    if let Some(found) = sources.iter().find(|s| s.description == name) {
        return Some(found);
    }
    let card = name.split(',').next().map(str::trim).unwrap_or(name);
    if card.is_empty() {
        return None;
    }
    sources
        .iter()
        .find(|s| s.card.as_deref() == Some(card))
        .or_else(|| sources.iter().find(|s| s.description.starts_with(card)))
}

/// The ALSA PCM that captures one PipeWire node.
pub fn pipewire_pcm(node: &str) -> String {
    format!("pipewire:NODE={node}")
}

/// A capture stream on one PipeWire node, running on its own thread.
///
/// Dropping it stops the thread: the flag is set, the read that is in
/// progress returns within a period, and the thread is joined. A period is
/// [`BLOCK_SIZE`] frames where the server allows it, for the reason the
/// `cpal` path asks for the same: monitoring latency is one input period.
///
/// # The stream is never closed on the thread that read it
///
/// > *"if you try changing the input it often just crashed for me when i
/// > set it to no input briefly"*
///
/// The core dump was `SIGXCPU` on `fontelle-input`, inside `snd_pcm_close`
/// → `pw_stream_destroy` → `malloc_trim`. The capture thread runs at
/// real-time priority, and a real-time thread has a budget of CPU time
/// (`RLIMIT_RTTIME`, 200 ms as rtkit sets it) it may spend without blocking
/// before the kernel kills the **whole process**. Reading a period at a time
/// never comes near it. Closing the stream did: PipeWire's teardown trims the
/// heap, and on a DAW that has scanned a thousand plugins that is more than
/// the budget. So the thread hands the stream back through its join and
/// exits, and the stream is closed by [`drop_off_thread`] — on a thread with
/// no budget to blow, and not the window's either, so choosing another input
/// costs the window nothing while the old one goes.
#[cfg(target_os = "linux")]
pub struct PipeWireInput {
    stop: Arc<AtomicBool>,
    /// Returns what the thread must not close: the stream, and the take's
    /// ring with it (freeing a ring is a free on a real-time thread,
    /// INVARIANT 1).
    thread: Option<std::thread::JoinHandle<Leftovers>>,
}

/// What a capture thread hands back rather than dropping: the open stream
/// and the ring it wrote. Closed by whoever joins — see [`PipeWireInput`].
#[cfg(target_os = "linux")]
struct Leftovers {
    _pcm: alsa::pcm::PCM,
    _writer: InputWriter,
}

/// Drops `what` on a thread of its own, at ordinary priority, and says which.
///
/// For anything whose drop is expensive — a PipeWire stream's close trims
/// the whole heap — and which is owned, at the moment it has to go, by a
/// thread that cannot afford it: the window's, which would freeze, or a
/// real-time one, which the kernel would kill. The handle is returned so a
/// caller that needs the drop to have *happened* can wait for it; nothing in
/// the program does.
pub fn drop_off_thread<T: Send + 'static>(what: T) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("fontelle-closer".to_string())
        .spawn(move || drop(what))
        .expect("a thread to drop on")
}

#[cfg(target_os = "linux")]
impl PipeWireInput {
    /// Opens `node` for capture and starts reading it into `writer` — and
    /// into `monitor`, when there is one, in the same breath.
    ///
    /// Returns the rate and channel count the stream actually opened at.
    /// `wanted_rate` is asked for and the server's answer is taken: PipeWire
    /// resamples to whatever the graph runs at, so asking for the output's
    /// rate gets a take at the output's rate.
    pub fn open(
        node: &str,
        channels: u16,
        wanted_rate: u32,
        mut writer: InputWriter,
        monitor: Option<Arc<InputMonitor>>,
    ) -> Result<(Self, u32, u16), DeviceError> {
        use alsa::pcm::{Access, Format, HwParams, PCM};
        use alsa::{Direction, ValueOr};

        let name = pipewire_pcm(node);
        let describe = |e: alsa::Error| DeviceError(format!("{name}: {e}"));
        let pcm = PCM::new(&name, Direction::Capture, false).map_err(describe)?;
        let (rate, channels, period) = {
            let hwp = HwParams::any(&pcm).map_err(describe)?;
            hwp.set_access(Access::RWInterleaved).map_err(describe)?;
            hwp.set_format(Format::FloatLE).map_err(describe)?;
            hwp.set_channels(u32::from(channels.clamp(1, 2)))
                .map_err(describe)?;
            let rate = hwp
                .set_rate_near(wanted_rate.max(1), ValueOr::Nearest)
                .map_err(describe)?;
            let period = hwp
                .set_period_size_near(BLOCK_SIZE as alsa::pcm::Frames, ValueOr::Nearest)
                .map_err(describe)?;
            // Four periods of room: enough that a scheduling hiccup on the
            // reading thread is not an overrun, and little enough that an
            // overrun is caught within a few milliseconds.
            let _ = hwp.set_buffer_size_near(period * 4);
            pcm.hw_params(&hwp).map_err(describe)?;
            (rate, hwp.get_channels().map_err(describe)? as u16, period)
        };
        pcm.prepare().map_err(describe)?;
        pcm.start().map_err(describe)?;

        if let Some(monitor) = &monitor {
            monitor.open(rate, channels);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let period = period.max(1) as usize;
        let thread = std::thread::Builder::new()
            .name("fontelle-input".to_string())
            .spawn(move || {
                // Best effort, like the output callback's: a capture thread
                // that is late is an overrun, and an overrun is a hole in a
                // take.
                let _rt =
                    audio_thread_priority::promote_current_thread_to_real_time(period as u32, rate)
                        .ok();
                // A period's worth of budget before SIGXCPU, widened and
                // watched — see `rt_budget`. This thread is not promoted
                // again after a demotion: a capture at ordinary priority
                // still captures, and the output callback's is the one that
                // matters for the sound.
                crate::rt_budget::widen_budget();
                crate::rt_budget::arm_current_thread();
                // Scoped so the reader's borrow of the stream ends before the
                // stream is handed back — and handed back even when it would
                // not read, because a close is a close wherever it fails.
                'reading: {
                    let Ok(io) = pcm.io_f32() else {
                        eprintln!("fontelle: {name}: the stream would not read as float");
                        break 'reading;
                    };
                    let mut buffer = vec![0.0f32; period * usize::from(channels)];
                    while !flag.load(Ordering::Relaxed) {
                        match io.readi(&mut buffer) {
                            Ok(frames) => {
                                let block = &buffer[..frames * usize::from(channels)];
                                writer.write(block);
                                // The same block into the second ring, so
                                // what is kept and what is heard are the
                                // same samples.
                                if let Some(monitor) = &monitor {
                                    monitor.write(block);
                                }
                            }
                            Err(e) => {
                                // An overrun is recoverable and costs the
                                // frames that were lost; anything else ends
                                // the stream, which the next
                                // `sync_audio_input` reports.
                                if pcm.recover(e.errno(), true).is_err() {
                                    eprintln!("fontelle: {name}: capture stopped: {e}");
                                    break;
                                }
                            }
                        }
                    }
                }
                crate::rt_budget::disarm_current_thread();
                // Handed back, not closed: see the type's own note.
                Leftovers {
                    _pcm: pcm,
                    _writer: writer,
                }
            })
            .map_err(|e| DeviceError(format!("could not start the input thread: {e}")))?;
        Ok((
            Self {
                stop,
                thread: Some(thread),
            },
            rate,
            channels,
        ))
    }
}

#[cfg(target_os = "linux")]
impl Drop for PipeWireInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && let Ok(leftovers) = thread.join()
        {
            drop_off_thread(leftovers);
        }
    }
}
