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
