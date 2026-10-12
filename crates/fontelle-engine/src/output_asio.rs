//! Playing through ASIO® by way of `fontelle-asio`, a separate program.
//!
//! Ty, 2026-10-12: ship ASIO under Steinberg's GPLv3 option, but as a
//! separate helper so Fontelle stays MIT/Apache and closed add-ons never
//! load into a GPL process. This is Fontelle's side of that, and it is
//! plain data only (`fontelle-audio-ipc`, `docs/asio.md`): Fontelle makes a
//! shared block and two doorbells, starts the helper beside `fontelle.exe`
//! with their name, and then
//!
//! - an **audio thread** of Fontelle's own waits for the helper's doorbell,
//!   renders the buffer straight into the shared block with the same
//!   [`Renderer`] every backend runs, and rings back — no allocation, no
//!   lock (INVARIANT 1). The helper's counters (device dropouts, late
//!   answers, the handoff's cost) are copied into [`OutputStats`] as it goes;
//! - a **supervisor** thread reads the helper's lines: what the driver
//!   opened as, its resets and changes, its errors, and its going.
//!
//! A helper that exits on its own — a crash, a driver that went, a device
//! unplugged — marks the output lost with its last words, and the session
//! plays through the system's default instead, saying so
//! (`AudioOutput::reopen_after_loss`). Fontelle going takes the helper with
//! it: its standard input closes, and on Windows it watches Fontelle's
//! process too.
//!
//! ASIO is a registered trademark of Steinberg Media Technologies GmbH.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fontelle_audio_ipc::control::HELPER_NAME;
use fontelle_audio_ipc::layout::Counters;
use fontelle_audio_ipc::{
    Bells, ClientSide, HelperEvent, HelperLine, Layout, PROTOCOL_VERSION, ReadyInfo, Region,
    RunArgs, Wake, channel_name,
};

use crate::device::DeviceError;
use crate::output::{OutputChoice, OutputStats, OutputStatus, Renderer};

/// The backend's name in Settings. The ® is the trademark's first use on the
/// page, as Steinberg's usage guidelines ask (§15).
pub const ASIO_HELPER_HOST: &str = "ASIO\u{ae} (via fontelle-asio helper)";

/// Frames the shared block has room for: past any ASIO driver's largest.
const MAX_FRAMES: u32 = 8192;

/// Channels exchanged: the studio is stereo.
const OUTPUTS: u32 = 2;

/// How long a driver may take to open. Some interfaces take seconds.
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the audio thread waits for a doorbell before looking at
/// whether it should stop.
const POLL: Duration = Duration::from_millis(50);

/// A helper that has not rung for this long while it should be playing has
/// stopped answering.
const SILENT_HELPER: Duration = Duration::from_secs(3);

static NEXT_CHANNEL: AtomicU32 = AtomicU32::new(1);

/// The helper program, if it is there to run: `FONTELLE_ASIO_HELPER` when
/// set (tests, and anyone keeping it elsewhere), else `fontelle-asio.exe`
/// beside Fontelle's own program, on Windows.
pub fn asio_helper_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FONTELLE_ASIO_HELPER") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    if !cfg!(windows) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(format!("{HELPER_NAME}.exe"));
    path.is_file().then_some(path)
}

/// The helper as a command: no console window of its own on Windows.
fn helper_command(path: &PathBuf) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The installed ASIO drivers, as the helper lists them. Empty without the
/// helper, or if it will not say within a few seconds.
pub fn asio_driver_names() -> Vec<String> {
    let Some(path) = asio_helper_path() else {
        return Vec::new();
    };
    let Ok(mut child) = helper_command(&path)
        .arg("list")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let stdout = child.stdout.take().expect("piped");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let names: Vec<String> = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| match HelperLine::parse(&line) {
                Some(HelperLine::Driver { name }) => Some(name),
                _ => None,
            })
            .collect();
        let _ = tx.send(names);
    });
    let names = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    names
}

/// Opens `driver`'s own control panel while nothing plays through it: a
/// helper of its own, which stays until the panel closes.
pub fn open_asio_panel(driver: &str) -> Result<(), String> {
    let path = asio_helper_path().ok_or("fontelle-asio is not beside Fontelle")?;
    let mut child = helper_command(&path)
        .args(["panel", "--driver", driver])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("fontelle-asio would not start: {e}"))?;
    // Reaped whenever the panel closes.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// What the supervisor knows, for the window.
#[derive(Default)]
struct View {
    /// Why the helper went, when it went on its own.
    gone: Option<String>,
    /// Its last error line.
    last_error: Option<String>,
}

/// A stream through the helper.
pub(crate) struct AsioStream {
    stop: Arc<AtomicBool>,
    audio: Option<JoinHandle<()>>,
    supervisor: Option<JoinHandle<()>>,
    child: Arc<Mutex<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    view: Arc<Mutex<View>>,
    region: Arc<Region>,
    /// Set by the audio thread: the helper stopped ringing.
    silent: Arc<AtomicBool>,
    pid: u32,
}

impl AsioStream {
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// The driver as it runs now, from the shared block's header: a reset
    /// may have moved its buffer and latencies.
    pub(crate) fn refresh(&self, status: &mut OutputStatus) {
        let h = self.region.shared().header();
        let frames = h.buffer_frames.load(Ordering::Relaxed);
        if frames > 0 {
            status.buffer_frames = Some(frames);
        }
        status.latencies = Some((
            h.input_latency.load(Ordering::Relaxed),
            h.output_latency.load(Ordering::Relaxed),
        ));
    }

    pub(crate) fn loss_reason(&self) -> Option<String> {
        let view = self.view.lock().ok()?;
        if let Some(gone) = &view.gone {
            return Some(gone.clone());
        }
        self.silent
            .load(Ordering::Relaxed)
            .then(|| "fontelle-asio stopped answering".to_string())
    }

    pub(crate) fn open_panel(&self) -> Result<(), String> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| "the helper's line is gone".to_string())?;
        let stdin = stdin.as_mut().ok_or("the helper has stopped")?;
        writeln!(stdin, "panel")
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("fontelle-asio did not hear: {e}"))
    }
}

impl Drop for AsioStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // The audio thread closes its end of the block on the way out, which
        // the helper sees within a buffer.
        if let Some(audio) = self.audio.take() {
            let _ = audio.join();
        }
        if let Ok(mut stdin) = self.stdin.lock()
            && let Some(mut pipe) = stdin.take()
        {
            let _ = writeln!(pipe, "stop");
        }
        // Given a moment to stop its driver cleanly, then made to.
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let Ok(mut child) = self.child.lock() else {
                break;
            };
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if Instant::now() >= until => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                Ok(None) => {}
            }
            drop(child);
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

/// Starts the helper on `choice`'s driver and plays `renderer` through it.
pub(crate) fn open(
    choice: &OutputChoice,
    sample_rate: u32,
    mut renderer: Renderer,
    stats: Arc<OutputStats>,
) -> Result<(AsioStream, OutputStatus), DeviceError> {
    let path = asio_helper_path().ok_or_else(|| {
        DeviceError("fontelle-asio is not beside Fontelle, so ASIO cannot play".to_string())
    })?;
    let driver = match &choice.device {
        Some(driver) => driver.clone(),
        None => asio_driver_names()
            .into_iter()
            .next()
            .ok_or_else(|| DeviceError("no ASIO driver is installed".to_string()))?,
    };
    let channel = channel_name(
        std::process::id(),
        NEXT_CHANNEL.fetch_add(1, Ordering::Relaxed),
    );
    let layout = Layout::new(MAX_FRAMES, 0, OUTPUTS).expect("a fixed, valid layout");
    let region = Arc::new(
        Region::create(&channel, layout)
            .map_err(|e| DeviceError(format!("the shared audio block would not open: {e}")))?,
    );
    let bells = Arc::new(
        Bells::create(&channel)
            .map_err(|e| DeviceError(format!("the audio doorbells would not open: {e}")))?,
    );
    let args = RunArgs {
        driver: driver.clone(),
        channel,
        rate: sample_rate,
        frames: choice.buffer_frames,
        inputs: 0,
        outputs: OUTPUTS,
        parent: std::process::id(),
    };
    let mut child = helper_command(&path)
        .args(args.to_args())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Fontelle's own, which the session log tees: a helper that panics
        // says so where a bug report will find it.
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| DeviceError(format!("fontelle-asio would not start: {e}")))?;
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped");
    let child = Arc::new(Mutex::new(child));

    // The supervisor: the first decisive line goes to the opener, the rest
    // to the view, and the helper's end to the stats.
    let stop = Arc::new(AtomicBool::new(false));
    let view = Arc::new(Mutex::new(View::default()));
    let (first_tx, first_rx) = std::sync::mpsc::channel::<Result<ReadyInfo, String>>();
    let supervisor = {
        let stop = Arc::clone(&stop);
        let view = Arc::clone(&view);
        let child = Arc::clone(&child);
        let stats = Arc::clone(&stats);
        let driver = driver.clone();
        std::thread::Builder::new()
            .name("fontelle-asio supervisor".to_string())
            .spawn(move || {
                supervise(
                    BufReader::new(stdout),
                    first_tx,
                    &view,
                    &child,
                    &stop,
                    &stats,
                    &driver,
                )
            })
            .map_err(|e| DeviceError(format!("could not watch fontelle-asio: {e}")))?
    };
    let fail = |why: String, supervisor: JoinHandle<()>| {
        stop.store(true, Ordering::Relaxed);
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = supervisor.join();
        Err(DeviceError(why))
    };
    let ready = match first_rx.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(ready)) => ready,
        Ok(Err(why)) => return fail(why, supervisor),
        Err(RecvTimeoutError::Timeout) => {
            return fail(
                format!(
                    "{driver} did not open within {} seconds",
                    OPEN_TIMEOUT.as_secs()
                ),
                supervisor,
            );
        }
        Err(RecvTimeoutError::Disconnected) => {
            return fail(
                "fontelle-asio stopped before the driver opened".to_string(),
                supervisor,
            );
        }
    };

    // The audio thread.
    let silent = Arc::new(AtomicBool::new(false));
    let audio = {
        let stop = Arc::clone(&stop);
        let silent = Arc::clone(&silent);
        let region = Arc::clone(&region);
        let bells = Arc::clone(&bells);
        let stats = Arc::clone(&stats);
        std::thread::Builder::new()
            .name("fontelle audio (ASIO)".to_string())
            .spawn(move || {
                let shared = region.shared();
                let mut client = ClientSide::new(shared, &*bells.request, &*bells.reply);
                let mut seen = shared.counters();
                let mut last_ring = Instant::now();
                let channels = OUTPUTS as usize;
                while !stop.load(Ordering::Relaxed) {
                    match client.wait(POLL) {
                        Wake::Cycle(cycle) => {
                            let started = Instant::now();
                            let (_, output) = client.buffers(cycle);
                            renderer.render(output, channels);
                            client.answer(
                                cycle,
                                u32::try_from(started.elapsed().as_nanos()).unwrap_or(u32::MAX),
                            );
                            seen = note_counters(&stats, seen, shared.counters());
                            last_ring = Instant::now();
                        }
                        Wake::Stopped => break,
                        Wake::Idle => {
                            if last_ring.elapsed() > SILENT_HELPER {
                                silent.store(true, Ordering::Relaxed);
                                stats.set_lost();
                                break;
                            }
                        }
                    }
                }
                // Our end closed, so the helper stops its driver and exits.
                client.close();
            })
            .map_err(|e| DeviceError(format!("no thread for the ASIO stream: {e}")))?
    };

    let status = OutputStatus {
        host: ASIO_HELPER_HOST.to_string(),
        device: ready.driver.clone(),
        sample_rate,
        buffer_frames: Some(ready.frames),
        latencies: Some((ready.input_latency, ready.output_latency)),
        fell_back: None,
    };
    eprintln!(
        "fontelle: ASIO through fontelle-asio: {} at {} frames, {} Hz, {}, latency in {} out {} \
         frames",
        ready.driver,
        ready.frames,
        ready.rate,
        ready.format,
        ready.input_latency,
        ready.output_latency
    );
    Ok((
        AsioStream {
            stop,
            audio: Some(audio),
            supervisor: Some(supervisor),
            child,
            stdin: Mutex::new(stdin),
            view,
            region,
            silent,
            pid,
        },
        status,
    ))
}

/// The helper's counters since `seen`, into `stats`. On the audio thread:
/// atomics only.
fn note_counters(stats: &OutputStats, seen: Counters, now: Counters) -> Counters {
    stats.note_device_xruns(now.device_xruns.saturating_sub(seen.device_xruns));
    stats.note_late(now.late_replies.saturating_sub(seen.late_replies));
    stats.set_handoff(now.handoff_last_ns);
    now
}

/// The supervisor's whole life: read the helper's lines until it closes
/// them, then see how it ended.
fn supervise(
    lines: impl BufRead,
    first: std::sync::mpsc::Sender<Result<ReadyInfo, String>>,
    view: &Mutex<View>,
    child: &Mutex<Child>,
    stop: &AtomicBool,
    stats: &OutputStats,
    driver: &str,
) {
    let mut first = Some(first);
    let mut bye: Option<String> = None;
    for line in lines.lines() {
        let Ok(line) = line else { break };
        let Some(parsed) = HelperLine::parse(&line) else {
            continue;
        };
        match parsed {
            HelperLine::Hello { version } if version != PROTOCOL_VERSION => {
                let why = format!(
                    "fontelle-asio speaks protocol {version} and this Fontelle {PROTOCOL_VERSION}; \
                     install the two from the same download"
                );
                if let Some(first) = first.take() {
                    let _ = first.send(Err(why));
                }
            }
            HelperLine::Hello { .. } | HelperLine::Driver { .. } => {}
            HelperLine::Ready(ready) => {
                if let Some(first) = first.take() {
                    let _ = first.send(Ok(ready));
                }
            }
            HelperLine::Error(words) => {
                eprintln!("fontelle: fontelle-asio: {words}");
                if let Some(first) = first.take() {
                    let _ = first.send(Err(words.clone()));
                }
                if let Ok(mut view) = view.lock() {
                    view.last_error = Some(words);
                }
            }
            HelperLine::Event(event) => {
                let said = match event {
                    HelperEvent::Reset => "the driver reset itself".to_string(),
                    HelperEvent::RateChanged(hz) => format!("the driver moved to {hz} Hz"),
                    HelperEvent::BufferChanged(n) => format!("the driver's buffer is now {n}"),
                    HelperEvent::LatenciesChanged { input, output } => {
                        format!("the driver's latencies are now {input} in, {output} out")
                    }
                    HelperEvent::Overload => "the driver reported an overload".to_string(),
                };
                eprintln!("fontelle: fontelle-asio ({driver}): {said}");
            }
            HelperLine::Bye(words) => bye = Some(words),
        }
    }
    // The lines closed: the helper is exiting or gone.
    let code = {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let status = child
                .lock()
                .ok()
                .and_then(|mut c| c.try_wait().ok().flatten());
            if let Some(status) = status {
                break status.code();
            }
            if Instant::now() >= until {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    if let Some(first) = first.take() {
        let _ = first.send(Err(match code {
            Some(code) => format!("fontelle-asio stopped before the driver opened (exit {code})"),
            None => "fontelle-asio stopped before the driver opened".to_string(),
        }));
        return;
    }
    if stop.load(Ordering::Relaxed) {
        return;
    }
    // Not asked to go: whatever it said last is why.
    let last_error = view.lock().ok().and_then(|v| v.last_error.clone());
    let why = match (last_error, bye, code) {
        (Some(error), _, _) => format!("fontelle-asio stopped: {error}"),
        (None, Some(bye), _) => format!("fontelle-asio stopped: {bye}"),
        (None, None, Some(code)) => {
            format!("fontelle-asio stopped unexpectedly (exit code {code})")
        }
        (None, None, None) => "fontelle-asio stopped unexpectedly".to_string(),
    };
    eprintln!("fontelle: {why}");
    if let Ok(mut view) = view.lock() {
        view.gone = Some(why);
    }
    stats.set_lost();
}
