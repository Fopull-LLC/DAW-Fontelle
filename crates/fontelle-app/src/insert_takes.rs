//! Takes recorded by an Analyze Musically insert, onto disk
//! (`docs/analyze-musically-plan.md` §6.1).
//!
//! The engine's [`AnalyzeCapture`] is the ring; this is its reader: a
//! thread that drains it every few tens of milliseconds into one WAV a take
//! — `<dir>/Take N.wav`, the name device takes use, under a number nothing
//! in the folder has — through the same crash-safe [`WavWriter`] a device
//! take is written with, so a take cut short by a crash still opens.
//!
//! What a take is *for* — the study it lands in, the takes list, Send to
//! arrangement — is the window's: this hands back [`InsertTake`]s and does
//! nothing to the document.
//!
//! [`AnalyzeCapture`]: fontelle_engine::AnalyzeCapture
//! [`WavWriter`]: fontelle_assets::WavWriter

use std::path::PathBuf;
use std::sync::Arc;

use std::sync::atomic::{AtomicBool, Ordering};

use fontelle_engine::{AnalyzeCapture, AnalyzeCaptureEvent};

/// One finished take.
#[derive(Debug, Clone, PartialEq)]
pub struct InsertTake {
    pub path: PathBuf,
    /// Stereo frames in the file.
    pub frames: usize,
    pub sample_rate: u32,
    /// Where the song was at its first frame, when the transport rolled;
    /// `None` for a free take (Send to arrangement then uses the playhead).
    pub song_sample: Option<i64>,
    /// Frames the ring lost while it ran. Not zero is a take with a hole in
    /// it, and the window says so.
    pub dropped_frames: u64,
}

/// The single-threaded core: drains a capture into files when told to.
/// [`InsertTakeWriter`] runs one on a thread; a test can run one by hand.
pub struct TakeFiles {
    dir: PathBuf,
    sample_rate: u32,
    open: Option<OpenTake>,
}

/// The take being written.
struct OpenTake {
    writer: fontelle_assets::WavWriter,
    path: PathBuf,
    song_sample: Option<i64>,
    /// The first write error, kept until the take ends: one error reported
    /// once, not once a block.
    error: Option<String>,
}

/// Interleaved channels a take has: a mixer bus is stereo.
const CHANNELS: u16 = 2;

impl TakeFiles {
    /// Takes go in `dir` (made when the first take starts), at
    /// `sample_rate`.
    pub fn new(dir: PathBuf, sample_rate: u32) -> Self {
        Self {
            dir,
            sample_rate,
            open: None,
        }
    }

    /// Everything the capture holds, onto disk; the takes that ended.
    pub fn pump(&mut self, capture: &AnalyzeCapture) -> Vec<Result<InsertTake, String>> {
        let mut finished = Vec::new();
        capture.drain(&mut |event| match event {
            AnalyzeCaptureEvent::Started { song_sample } => {
                // A start with a take still open means its stop was never
                // seen; close it as it stands rather than run two into one.
                if let Some(done) = self.close(0) {
                    finished.push(done);
                }
                match self.create(song_sample) {
                    Ok(open) => self.open = Some(open),
                    Err(e) => finished.push(Err(e)),
                }
            }
            AnalyzeCaptureEvent::Audio(frames) => {
                if let Some(open) = &mut self.open
                    && open.error.is_none()
                    && let Err(e) = open.writer.write(frames)
                {
                    open.error = Some(e.to_string());
                }
            }
            AnalyzeCaptureEvent::Stopped { dropped_frames } => {
                if let Some(done) = self.close(dropped_frames) {
                    finished.push(done);
                }
            }
        });
        finished
    }

    /// Closes the take still being written, if one is, as it stands — what
    /// stopping the writer does to a take nobody ended.
    pub fn finish(&mut self) -> Option<Result<InsertTake, String>> {
        self.close(0)
    }

    /// `<dir>/Take N.wav` under a number nothing there has — the name a
    /// device take gets (`Session::take_path`).
    fn create(&self, song_sample: Option<i64>) -> Result<OpenTake, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = (1..100_000)
            .map(|n| self.dir.join(format!("Take {n}.wav")))
            .find(|path| !path.exists())
            .ok_or("the recordings folder is full")?;
        let writer = fontelle_assets::WavWriter::create(&path, self.sample_rate, CHANNELS)
            .map_err(|e| e.to_string())?;
        Ok(OpenTake {
            writer,
            path,
            song_sample,
            error: None,
        })
    }

    fn close(&mut self, dropped_frames: u64) -> Option<Result<InsertTake, String>> {
        let open = self.open.take()?;
        let frames = open.writer.frames() as usize;
        Some(match open.error {
            Some(e) => Err(format!("{}: {e}", open.path.display())),
            None => open
                .writer
                .finish()
                .map(|()| InsertTake {
                    path: open.path,
                    frames,
                    sample_rate: self.sample_rate,
                    song_sample: open.song_sample,
                    dropped_frames,
                })
                .map_err(|e| e.to_string()),
        })
    }
}

/// A thread draining one insert's capture into takes.
pub struct InsertTakeWriter {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    finished: std::sync::mpsc::Receiver<Result<InsertTake, String>>,
}

impl InsertTakeWriter {
    /// How often the thread drains the ring: a small fraction of the ten
    /// seconds the app's rings hold.
    pub const PERIOD: std::time::Duration = std::time::Duration::from_millis(20);

    /// Starts draining `capture` into `dir`. One writer per capture: the
    /// ring has one reader.
    pub fn spawn(capture: Arc<AnalyzeCapture>, dir: PathBuf, sample_rate: u32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (send, finished) = std::sync::mpsc::channel();
        let stopping = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("fontelle-insert-takes".into())
            .spawn(move || {
                let mut files = TakeFiles::new(dir, sample_rate);
                loop {
                    // Read before the drain, so the last drain after a stop
                    // sees everything recorded before it.
                    let last = stopping.load(Ordering::Acquire);
                    for take in files.pump(&capture) {
                        let _ = send.send(take);
                    }
                    if last {
                        if let Some(take) = files.finish() {
                            let _ = send.send(take);
                        }
                        break;
                    }
                    std::thread::sleep(Self::PERIOD);
                }
            })
            .ok();
        Self {
            stop,
            thread,
            finished,
        }
    }

    /// The takes finished since the last poll. Never blocks.
    pub fn poll(&self) -> Vec<Result<InsertTake, String>> {
        self.finished.try_iter().collect()
    }

    /// Drains what is left, closes an open take and ends the thread; every
    /// take not yet polled.
    pub fn stop(mut self) -> Vec<Result<InsertTake, String>> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.finished.try_iter().collect()
    }
}

impl Drop for InsertTakeWriter {
    /// A writer dropped without `stop` still closes its file.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ------------------------------------------------- a device's takes ---

/// What an input recorder is told from the window, read by its thread.
#[derive(Debug)]
pub struct InputControl {
    pub armed: AtomicBool,
    mode: std::sync::atomic::AtomicU8,
    threshold: std::sync::atomic::AtomicU32,
    release_ms: std::sync::atomic::AtomicU32,
    /// Written by the thread: a take is running, the last block's peak, the
    /// frames in the running take, and what the device's ring lost.
    pub recording: AtomicBool,
    level: std::sync::atomic::AtomicU32,
    pub take_frames: std::sync::atomic::AtomicU64,
    pub dropped: std::sync::atomic::AtomicU64,
}

impl Default for InputControl {
    fn default() -> Self {
        let config = fontelle_types::AnalyzeConfig::new();
        let this = Self {
            armed: AtomicBool::new(false),
            mode: std::sync::atomic::AtomicU8::new(0),
            threshold: std::sync::atomic::AtomicU32::new(0),
            release_ms: std::sync::atomic::AtomicU32::new(0),
            recording: AtomicBool::new(false),
            level: std::sync::atomic::AtomicU32::new(0),
            take_frames: std::sync::atomic::AtomicU64::new(0),
            dropped: std::sync::atomic::AtomicU64::new(0),
        };
        this.configure(&config);
        this
    }
}

impl InputControl {
    /// The arm mode, threshold and release, as an insert's are set.
    pub fn configure(&self, config: &fontelle_types::AnalyzeConfig) {
        self.mode.store(config.arm.index() as u8, Ordering::Relaxed);
        self.threshold
            .store(config.threshold_db.to_bits(), Ordering::Relaxed);
        self.release_ms
            .store(config.release_ms.to_bits(), Ordering::Relaxed);
    }

    pub fn mode(&self) -> fontelle_types::ArmMode {
        fontelle_types::ArmMode::ALL
            .get(usize::from(self.mode.load(Ordering::Relaxed)))
            .copied()
            .unwrap_or_default()
    }

    pub fn threshold_db(&self) -> f32 {
        f32::from_bits(self.threshold.load(Ordering::Relaxed))
    }

    pub fn release_ms(&self) -> f32 {
        f32::from_bits(self.release_ms.load(Ordering::Relaxed))
    }

    /// The last block's peak, linear.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

/// Takes from an input device, decided as an insert's are (now, on play, on
/// input) but on the reader's side: a device's callback hands over every
/// block whatever happens, so this is where a take starts and stops. Pure
/// apart from the files; [`InputTakeWriter`] runs one on a thread.
pub struct InputTakes {
    dir: PathBuf,
    sample_rate: u32,
    channels: u16,
    open: Option<OpenTake>,
    silent: u64,
}

impl InputTakes {
    pub fn new(dir: PathBuf, sample_rate: u32, channels: u16) -> Self {
        Self {
            dir,
            sample_rate,
            channels: channels.max(1),
            open: None,
            silent: 0,
        }
    }

    /// One block from the device (interleaved), with what the window and the
    /// transport say now; the takes that ended.
    pub fn feed(
        &mut self,
        block: &[f32],
        control: &InputControl,
        rolling: bool,
        song_sample: i64,
    ) -> Vec<Result<InsertTake, String>> {
        let mut finished = Vec::new();
        let channels = usize::from(self.channels);
        let peak = block.iter().fold(0.0f32, |p, s| p.max(s.abs()));
        control.level.store(peak.to_bits(), Ordering::Relaxed);
        let armed = control.armed.load(Ordering::Relaxed);
        let mode = control.mode();
        let stop = |this: &mut Self, finished: &mut Vec<_>| {
            if let Some(done) = this.close() {
                finished.push(done);
            }
        };
        if !armed || (mode == fontelle_types::ArmMode::OnPlay && !rolling) {
            stop(self, &mut finished);
            control.recording.store(false, Ordering::Relaxed);
            return finished;
        }
        let at = rolling.then_some(song_sample);
        match mode {
            fontelle_types::ArmMode::OnPlay | fontelle_types::ArmMode::Now => {
                if self.open.is_none() {
                    match self.create(at) {
                        Ok(open) => self.open = Some(open),
                        Err(e) => finished.push(Err(e)),
                    }
                }
                self.write(block);
            }
            fontelle_types::ArmMode::OnInput => {
                let threshold = 10f32.powf(control.threshold_db() / 20.0);
                let release =
                    (control.release_ms() / 1000.0 * self.sample_rate as f32).max(1.0) as u64;
                for (i, frame) in block.chunks(channels).enumerate() {
                    let loud = frame.iter().any(|s| s.abs() >= threshold);
                    if self.open.is_none() {
                        if !loud {
                            continue;
                        }
                        match self.create(at.map(|a| a + i as i64)) {
                            Ok(open) => self.open = Some(open),
                            Err(e) => {
                                finished.push(Err(e));
                                continue;
                            }
                        }
                        self.silent = 0;
                    }
                    self.write(frame);
                    self.silent = if loud { 0 } else { self.silent + 1 };
                    if self.silent >= release {
                        stop(self, &mut finished);
                    }
                }
            }
        }
        control
            .recording
            .store(self.open.is_some(), Ordering::Relaxed);
        control.take_frames.store(
            self.open.as_ref().map_or(0, |o| o.writer.frames()),
            Ordering::Relaxed,
        );
        finished
    }

    /// Closes a take still running.
    pub fn finish(&mut self) -> Option<Result<InsertTake, String>> {
        self.close()
    }

    fn write(&mut self, samples: &[f32]) {
        if let Some(open) = &mut self.open
            && open.error.is_none()
            && let Err(e) = open.writer.write(samples)
        {
            open.error = Some(e.to_string());
        }
    }

    fn create(&self, song_sample: Option<i64>) -> Result<OpenTake, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = (1..100_000)
            .map(|n| self.dir.join(format!("Take {n}.wav")))
            .find(|path| !path.exists())
            .ok_or("the recordings folder is full")?;
        let writer = fontelle_assets::WavWriter::create(&path, self.sample_rate, self.channels)
            .map_err(|e| e.to_string())?;
        Ok(OpenTake {
            writer,
            path,
            song_sample,
            error: None,
        })
    }

    fn close(&mut self) -> Option<Result<InsertTake, String>> {
        let open = self.open.take()?;
        let frames = open.writer.frames() as usize;
        Some(match open.error {
            Some(e) => Err(format!("{}: {e}", open.path.display())),
            None => open
                .writer
                .finish()
                .map(|()| InsertTake {
                    path: open.path,
                    frames,
                    sample_rate: self.sample_rate,
                    song_sample: open.song_sample,
                    dropped_frames: 0,
                })
                .map_err(|e| e.to_string()),
        })
    }
}

/// Where an input recorder's blocks come from: a tap on a device a track
/// already has open, or a stream of its own.
pub type InputSource = Box<dyn FnMut(&mut Vec<f32>) -> u64 + Send>;

/// A thread turning an input device's blocks into takes.
pub struct InputTakeWriter {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    finished: std::sync::mpsc::Receiver<Result<InsertTake, String>>,
    pub control: Arc<InputControl>,
}

impl InputTakeWriter {
    /// Drains `source` (which answers the frames its ring has lost) every
    /// [`InsertTakeWriter::PERIOD`] into takes in `dir`.
    pub fn spawn(
        mut source: InputSource,
        channels: u16,
        sample_rate: u32,
        dir: PathBuf,
        control: Arc<InputControl>,
        transport: Option<Arc<fontelle_engine::Transport>>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (send, finished) = std::sync::mpsc::channel();
        let stopping = Arc::clone(&stop);
        let held = Arc::clone(&control);
        let thread = std::thread::Builder::new()
            .name("fontelle-input-takes".into())
            .spawn(move || {
                let mut takes = InputTakes::new(dir, sample_rate, channels);
                let mut block = Vec::new();
                loop {
                    let last = stopping.load(Ordering::Acquire);
                    block.clear();
                    let lost = source(&mut block);
                    held.dropped.store(lost, Ordering::Relaxed);
                    let (rolling, at) = transport
                        .as_ref()
                        .map_or((false, 0), |t| (t.is_playing(), t.position_sample()));
                    for take in takes.feed(&block, &held, rolling, at) {
                        let _ = send.send(take);
                    }
                    if last {
                        if let Some(take) = takes.finish() {
                            let _ = send.send(take);
                        }
                        held.recording.store(false, Ordering::Relaxed);
                        break;
                    }
                    std::thread::sleep(InsertTakeWriter::PERIOD);
                }
            })
            .ok();
        Self {
            stop,
            thread,
            finished,
            control,
        }
    }

    pub fn poll(&self) -> Vec<Result<InsertTake, String>> {
        self.finished.try_iter().collect()
    }

    pub fn stop(mut self) -> Vec<Result<InsertTake, String>> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.finished.try_iter().collect()
    }
}

impl Drop for InputTakeWriter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
