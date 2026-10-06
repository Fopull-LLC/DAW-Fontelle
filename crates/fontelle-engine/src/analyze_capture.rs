//! Analyze Musically's insert, the engine half: what plays through a track,
//! into a ring, for a writer thread to put on disk
//! (`docs/analyze-musically-plan.md` §6.1).
//!
//! > *"you should be able to add it to a mixer track as a plugin like you
//! > can with edison in fl to record into it"* — Ty
//!
//! # The shape
//!
//! [`AnalyzeCapture`] is shared, through an `Arc`, by three parties: the
//! insert's [`crate::EffectNode`] (which records pre-fader, at its place in
//! the chain), an [`AnalyzeCaptureNode`] scheduled after the track's fader
//! (which records post-fader, when the insert's switch says so), and one
//! reader off the audio thread (which drains it — the app's take writer).
//! It survives a graph rebuild the way the spectrum tap does: the app keeps
//! it by `(track, slot)` and hands the same one to the next graph, so a take
//! in progress carries on across an edit elsewhere in the song.
//!
//! # INVARIANT 1
//!
//! The audio thread writes into a ring preallocated at construction —
//! interleaved stereo frames as `f32` bits in atomics, the spectrum tap's
//! trick, so it needs no `unsafe` and no lock — and a second, small ring of
//! take boundaries. It never waits: a full ring **drops and counts** the
//! frames that did not fit ([`AnalyzeCapture::dropped_frames`]), which the
//! window must report — a take with a hole in it is worse when nobody says
//! so. One writer at a time is the contract, and the graph keeps it: only
//! one graph runs a block, and only a live graph is given a tap.

/// What one take event says, to [`AnalyzeCapture::drain`]'s sink.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyzeCaptureEvent<'a> {
    /// A take begins. `song_sample` is where the song was at its first
    /// frame, when the transport was rolling; `None` for a free take.
    Started { song_sample: Option<i64> },
    /// More of the take: interleaved stereo frames, in order.
    Audio(&'a [f32]),
    /// The take ends. `dropped_frames` is how many of its frames the full
    /// ring could not keep.
    Stopped { dropped_frames: u64 },
}

/// Where on the strip a capture call is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzeTapPoint {
    /// At the insert's place in the chain: after the inserts before it,
    /// before the fader (Edison's).
    PreFader,
    /// After the track's fader and pan.
    PostFader,
}

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// Channels a frame carries: a mixer bus is stereo.
const CHANNELS: usize = 2;
/// Take boundaries the event ring holds between drains. A take is two; a
/// reader that falls behind by thirty-two takes has bigger problems.
const EVENTS: usize = 64;
/// Words an event takes: kind, frame, and the song sample or the count.
const EVENT_WORDS: usize = 3;

const KIND_START: u64 = 1;
const KIND_START_FREE: u64 = 2;
const KIND_STOP: u64 = 3;

/// One Analyze Musically insert's capture: the ring, the arm switch and the
/// count of what was lost. See the module's note.
pub struct AnalyzeCapture {
    /// Interleaved stereo frames, `f32` bits.
    audio: Vec<AtomicU32>,
    capacity: u64,
    /// Frames ever pushed (the writer's), and ever drained (the reader's).
    written: AtomicU64,
    read: AtomicU64,
    /// Take boundaries, [`EVENT_WORDS`] words each, and their two counters.
    events: Vec<AtomicU64>,
    events_written: AtomicU64,
    events_read: AtomicU64,

    armed: AtomicBool,
    // What `configure` was last told.
    mode: AtomicU8,
    threshold: AtomicU32,
    release_ms: AtomicU32,
    post_fader: AtomicBool,
    bypassed: AtomicBool,

    // The writer's own state, in atomics so a rebuilt graph's node carries
    // on the take its predecessor started.
    recording: AtomicBool,
    stop_pending: AtomicBool,
    silent_frames: AtomicU64,
    take_dropped: AtomicU64,
    dropped: AtomicU64,
    /// The last block's peak at the insert's tap point, `f32` bits: the
    /// Record page's meter, armed or not.
    level: AtomicU32,
    /// `written` when the running take began.
    take_start: AtomicU64,
}

impl AnalyzeCapture {
    /// Room for `capacity_frames` stereo frames between drains.
    ///
    /// Allocates the whole ring now, off the audio thread. Ten seconds is a
    /// sensible size: the writer drains every few tens of milliseconds, and
    /// a disk that stalls for longer than the ring is a take that says it
    /// lost frames rather than one that stopped the mix.
    pub fn new(capacity_frames: usize) -> Self {
        let capacity = capacity_frames.max(1);
        Self {
            audio: (0..capacity * CHANNELS)
                .map(|_| AtomicU32::new(0))
                .collect(),
            capacity: capacity as u64,
            written: AtomicU64::new(0),
            read: AtomicU64::new(0),
            events: (0..EVENTS * EVENT_WORDS)
                .map(|_| AtomicU64::new(0))
                .collect(),
            events_written: AtomicU64::new(0),
            events_read: AtomicU64::new(0),
            armed: AtomicBool::new(false),
            mode: AtomicU8::new(0),
            threshold: AtomicU32::new(0.01f32.to_bits()),
            release_ms: AtomicU32::new(1_000.0f32.to_bits()),
            post_fader: AtomicBool::new(false),
            bypassed: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            stop_pending: AtomicBool::new(false),
            silent_frames: AtomicU64::new(0),
            take_dropped: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            level: AtomicU32::new(0),
            take_start: AtomicU64::new(0),
        }
    }

    /// Arms or disarms it. What arming does depends on the insert's
    /// [`ArmMode`](fontelle_types::ArmMode): record now, on play, or on
    /// input. Off the audio thread; takes effect on the next block.
    pub fn arm(&self, armed: bool) {
        self.armed.store(armed, Ordering::Relaxed);
    }

    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    /// Whether a take is running right now.
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }

    /// The last block's peak, linear, at the point the insert listens.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Frames in the take running now; 0 with none.
    pub fn take_frames(&self) -> u64 {
        if !self.is_recording() {
            return 0;
        }
        self.written
            .load(Ordering::Relaxed)
            .saturating_sub(self.take_start.load(Ordering::Relaxed))
    }

    /// Frames pushed and not yet drained: how far behind the writer is.
    pub fn unread_frames(&self) -> u64 {
        self.written
            .load(Ordering::Acquire)
            .saturating_sub(self.read.load(Ordering::Acquire))
    }

    /// Frames lost to a full ring since it was made.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// **RT.** The insert's settings for this block: called by the insert's
    /// node every block, before either capture point runs, so the
    /// post-fader node reads the same settings. `bypassed` counts as
    /// disarmed: a bypassed insert is switched off.
    pub fn configure(&self, config: &fontelle_types::AnalyzeConfig, bypassed: bool) {
        self.mode.store(config.arm.index() as u8, Ordering::Relaxed);
        let threshold = 10f32.powf(config.threshold_db / 20.0);
        self.threshold.store(threshold.to_bits(), Ordering::Relaxed);
        self.release_ms
            .store(config.release_ms.to_bits(), Ordering::Relaxed);
        self.post_fader.store(config.post_fader, Ordering::Relaxed);
        self.bypassed.store(bypassed, Ordering::Relaxed);
    }

    /// **RT.** One block as it is at `point`. Records only when `point` is
    /// the one the insert is set to; starts and stops takes as the arm mode
    /// says. Never allocates, never blocks.
    pub fn capture(
        &self,
        block: &[&mut [f32]],
        point: AnalyzeTapPoint,
        transport: &crate::TransportSnapshot,
        sample_rate: f32,
    ) {
        let wanted = if self.post_fader.load(Ordering::Relaxed) {
            AnalyzeTapPoint::PostFader
        } else {
            AnalyzeTapPoint::PreFader
        };
        if point != wanted {
            return;
        }
        let Some(left) = block.first() else {
            return;
        };
        let right: &[f32] = block.get(1).map_or(left, |r| r);
        let frames = left.len().min(right.len());
        let peak = left[..frames]
            .iter()
            .chain(&right[..frames])
            .fold(0.0f32, |p, s| p.max(s.abs()));
        self.level.store(peak.to_bits(), Ordering::Relaxed);

        // A stop that found the event ring full goes first, and nothing is
        // recorded until it has gone: a take's frames must not run into the
        // next one's.
        if self.stop_pending.load(Ordering::Relaxed) {
            if !self.push_event(
                KIND_STOP,
                self.written.load(Ordering::Relaxed),
                self.take_dropped.load(Ordering::Relaxed),
            ) {
                return;
            }
            self.stop_pending.store(false, Ordering::Relaxed);
        }

        let armed = self.armed.load(Ordering::Relaxed) && !self.bypassed.load(Ordering::Relaxed);
        let rolling = matches!(
            transport.state,
            crate::TransportState::Playing | crate::TransportState::Recording
        );
        let song = |offset: usize| rolling.then_some(transport.position_sample + offset as i64);

        if !armed {
            if self.is_recording() {
                self.stop();
            }
            return;
        }
        let mode = fontelle_types::ArmMode::ALL
            .get(usize::from(self.mode.load(Ordering::Relaxed)))
            .copied()
            .unwrap_or_default();
        let mut free = self.free_frames();
        match mode {
            fontelle_types::ArmMode::OnPlay | fontelle_types::ArmMode::Now => {
                if mode == fontelle_types::ArmMode::OnPlay && !rolling {
                    if self.is_recording() {
                        self.stop();
                    }
                    return;
                }
                if !self.is_recording() && !self.start(song(0)) {
                    return;
                }
                for frame in 0..frames {
                    self.push_frame(left[frame], right[frame], &mut free);
                }
                self.publish();
            }
            fontelle_types::ArmMode::OnInput => {
                let threshold = f32::from_bits(self.threshold.load(Ordering::Relaxed));
                let release_ms = f32::from_bits(self.release_ms.load(Ordering::Relaxed));
                let release = (release_ms / 1000.0 * sample_rate) as u64;
                for frame in 0..frames {
                    let loud = left[frame].abs().max(right[frame].abs()) >= threshold;
                    if !self.is_recording() {
                        if !loud || !self.start(song(frame)) {
                            continue;
                        }
                        self.silent_frames.store(0, Ordering::Relaxed);
                    }
                    self.push_frame(left[frame], right[frame], &mut free);
                    let silent = if loud {
                        0
                    } else {
                        self.silent_frames.load(Ordering::Relaxed) + 1
                    };
                    self.silent_frames.store(silent, Ordering::Relaxed);
                    if silent >= release.max(1) {
                        self.publish();
                        self.stop();
                    }
                }
                self.publish();
            }
        }
    }

    /// Room in the ring, in frames, as the writer sees it.
    fn free_frames(&self) -> u64 {
        let written = self.written.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        self.capacity - (written - read).min(self.capacity)
    }

    /// One frame in, or counted as lost. `free` is the room left, counted
    /// down here so the reader's position is read once a block.
    fn push_frame(&self, left: f32, right: f32, free: &mut u64) {
        if *free == 0 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.take_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        *free -= 1;
        let written = self.written.load(Ordering::Relaxed);
        let slot = (written % self.capacity) as usize * CHANNELS;
        self.audio[slot].store(left.to_bits(), Ordering::Relaxed);
        self.audio[slot + 1].store(right.to_bits(), Ordering::Relaxed);
        // Published by `publish`; the count itself is the writer's.
        self.written.store(written + 1, Ordering::Relaxed);
    }

    /// Makes every frame pushed so far visible to the reader.
    fn publish(&self) {
        let written = self.written.load(Ordering::Relaxed);
        self.written.store(written, Ordering::Release);
    }

    /// Starts a take at the next frame pushed. `false` when the event ring
    /// is full: the take starts on a later block instead.
    fn start(&self, song_sample: Option<i64>) -> bool {
        let (kind, value) = match song_sample {
            Some(at) => (KIND_START, at as u64),
            None => (KIND_START_FREE, 0),
        };
        if !self.push_event(kind, self.written.load(Ordering::Relaxed), value) {
            return false;
        }
        self.take_dropped.store(0, Ordering::Relaxed);
        self.silent_frames.store(0, Ordering::Relaxed);
        self.take_start
            .store(self.written.load(Ordering::Relaxed), Ordering::Relaxed);
        self.recording.store(true, Ordering::Relaxed);
        true
    }

    /// Ends the take after the frames pushed so far.
    fn stop(&self) {
        self.publish();
        self.recording.store(false, Ordering::Relaxed);
        let at = self.written.load(Ordering::Relaxed);
        let lost = self.take_dropped.load(Ordering::Relaxed);
        if !self.push_event(KIND_STOP, at, lost) {
            self.stop_pending.store(true, Ordering::Relaxed);
        }
    }

    fn push_event(&self, kind: u64, frame: u64, value: u64) -> bool {
        let written = self.events_written.load(Ordering::Relaxed);
        let read = self.events_read.load(Ordering::Acquire);
        if written - read >= EVENTS as u64 {
            return false;
        }
        let slot = (written % EVENTS as u64) as usize * EVENT_WORDS;
        self.events[slot].store(kind, Ordering::Relaxed);
        self.events[slot + 1].store(frame, Ordering::Relaxed);
        self.events[slot + 2].store(value, Ordering::Relaxed);
        self.events_written.store(written + 1, Ordering::Release);
        true
    }

    /// Off the audio thread, one reader at a time: everything captured since
    /// the last drain, in order, to `sink`.
    pub fn drain(&self, sink: &mut dyn FnMut(AnalyzeCaptureEvent<'_>)) {
        // Frames first, then events: a frame the reader can see was pushed
        // after every start event before it, so the start is seen too.
        let written = self.written.load(Ordering::Acquire);
        let events_written = self.events_written.load(Ordering::Acquire);
        let mut read = self.read.load(Ordering::Relaxed);
        let mut events_read = self.events_read.load(Ordering::Relaxed);
        let mut scratch: Vec<f32> = Vec::new();
        loop {
            let next = (events_read < events_written).then(|| {
                let slot = (events_read % EVENTS as u64) as usize * EVENT_WORDS;
                (
                    self.events[slot].load(Ordering::Relaxed),
                    self.events[slot + 1].load(Ordering::Relaxed),
                    self.events[slot + 2].load(Ordering::Relaxed),
                )
            });
            let until = next.map_or(written, |(_, frame, _)| frame.min(written));
            if until > read {
                scratch.clear();
                for frame in read..until {
                    let slot = (frame % self.capacity) as usize * CHANNELS;
                    scratch.push(f32::from_bits(self.audio[slot].load(Ordering::Relaxed)));
                    scratch.push(f32::from_bits(self.audio[slot + 1].load(Ordering::Relaxed)));
                }
                sink(AnalyzeCaptureEvent::Audio(&scratch));
                read = until;
                self.read.store(read, Ordering::Release);
            }
            match next {
                Some((kind, frame, value)) if frame <= read => {
                    sink(match kind {
                        KIND_START => AnalyzeCaptureEvent::Started {
                            song_sample: Some(value as i64),
                        },
                        KIND_START_FREE => AnalyzeCaptureEvent::Started { song_sample: None },
                        _ => AnalyzeCaptureEvent::Stopped {
                            dropped_frames: value,
                        },
                    });
                    events_read += 1;
                    self.events_read.store(events_read, Ordering::Release);
                }
                _ => break,
            }
        }
    }
}

/// The post-fader capture point: scheduled after a track's fader, it hands
/// the bus to the insert's [`AnalyzeCapture`] and changes nothing.
pub struct AnalyzeCaptureNode {
    tap: std::sync::Arc<AnalyzeCapture>,
    sample_rate: f32,
}

impl AnalyzeCaptureNode {
    pub fn new(tap: std::sync::Arc<AnalyzeCapture>) -> Self {
        Self {
            tap,
            sample_rate: 48_000.0,
        }
    }
}

impl crate::AudioNode for AnalyzeCaptureNode {
    fn prepare(&mut self, ctx: &crate::PrepareContext) {
        self.sample_rate = ctx.sample_rate;
    }

    fn process(&mut self, ctx: &mut crate::ProcessContext) {
        self.tap.capture(
            ctx.outputs,
            AnalyzeTapPoint::PostFader,
            &ctx.transport,
            self.sample_rate,
        );
    }

    /// A transport stop is not the end of a take (see `EffectState`'s
    /// reset); nothing here to clear.
    fn reset(&mut self) {}

    fn debug_name(&self) -> &'static str {
        "AnalyzeCaptureNode"
    }

    fn params(&self) -> &dyn crate::ParamSet {
        &crate::nodes::EmptyParams
    }
}
