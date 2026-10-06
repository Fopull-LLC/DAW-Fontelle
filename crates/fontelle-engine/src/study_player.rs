//! Analyze Musically's preview player (`docs/analyze-musically-plan.md`
//! §3.10): the study's audio — as recorded, or with the edits — played from
//! the window's cursor, over a range or round a loop, with a playhead the
//! window reads and an A/B switch.
//!
//! Ty, trying P1: *"theres no playhead in the analyze musically plugin so i
//! cant preview what im making in the window"*, and *"when i preview a note
//! its not playing that section repitched to the new note"*. This is what
//! plays that section.
//!
//! # Two halves, one shared
//!
//! [`StudyPlayer`] is shared (an `Arc`) by the window's thread and the node;
//! it outlives every graph, so a rebuild — any change to the mixer — neither
//! loses the audio nor moves the playhead. [`StudyPlayerNode`] is the node a
//! graph holds, on the master like the browser's preview voice: a listen is
//! not part of the song.
//!
//! # INVARIANT 1
//!
//! The window asks through atomics: a request counter, its range, the A/B
//! flag. New audio (an edit re-rendered) is handed over through a one-slot
//! mailbox of a raw pointer, and the audio it replaces goes back the same
//! way: the node never frees anything. It takes a new buffer only while the
//! return slot is empty, the rule `graph_channel` keeps for graphs, and the
//! window empties that slot before it posts the next. `prepare` sizes
//! nothing because nothing is needed: the audio is the window's, shared.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};
use crate::nodes::EmptyParams;

/// The audio a study plays: interleaved, at its own rate, the original and
/// the edited take of the same length (they are time-aligned, which is what
/// lets A/B switch in place).
#[derive(Debug, Clone)]
pub struct StudyAudio {
    pub channels: usize,
    pub sample_rate: u32,
    pub original: Arc<[f32]>,
    pub edited: Arc<[f32]>,
}

impl StudyAudio {
    fn frames(&self) -> usize {
        self.edited.len() / self.channels.max(1)
    }
}

/// The fade at a start, a stop, a loop's seam and an A/B switch: long
/// enough to be no click, short enough to be no swell.
const FADE_SECONDS: f32 = 0.005;
/// No range end.
const NONE: u64 = u64::MAX;
const PLAY: u8 = 1;
const STOP: u8 = 2;

/// What the window and the node share. See the module doc.
#[derive(Debug)]
pub struct StudyPlayer {
    /// New audio, posted by the window, taken by the node.
    pending: AtomicPtr<StudyAudio>,
    /// What the node stopped playing, for the window to free.
    retired: AtomicPtr<StudyAudio>,
    /// The newest audio, for a node built after it was posted (a graph
    /// rebuild). Only ever touched off the audio thread.
    latest: Mutex<Option<StudyAudio>>,
    /// Bumped by every play and stop; `kind` says which the last was.
    request: AtomicU64,
    kind: AtomicU8,
    from: AtomicU64,
    to: AtomicU64,
    looped: AtomicBool,
    /// Whether it is playing, as the node last said.
    playing: AtomicBool,
    original: AtomicBool,
    /// The playhead: a frame of the study's audio.
    position: AtomicU64,
}

impl StudyPlayer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: AtomicPtr::new(std::ptr::null_mut()),
            retired: AtomicPtr::new(std::ptr::null_mut()),
            latest: Mutex::new(None),
            request: AtomicU64::new(0),
            kind: AtomicU8::new(STOP),
            from: AtomicU64::new(0),
            to: AtomicU64::new(NONE),
            looped: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            original: AtomicBool::new(false),
            position: AtomicU64::new(0),
        })
    }

    /// **Off the audio thread.** Hands `audio` to the node; it is heard from
    /// the next block on, at the same place. Frees what the node handed
    /// back since the last call first, so the node can always swap.
    pub fn submit(&self, audio: StudyAudio) {
        self.reclaim();
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(audio.clone());
        }
        let fresh = Box::into_raw(Box::new(audio));
        let unread = self.pending.swap(fresh, Ordering::AcqRel);
        if !unread.is_null() {
            // SAFETY: a pointer in `pending` came from `Box::into_raw` here
            // and the node takes it only by swapping it out; swapped out of
            // the slot by us, nobody else has it.
            drop(unsafe { Box::from_raw(unread) });
        }
    }

    /// **Off the audio thread.** Frees the audio the node has stopped
    /// playing. Called by `submit`, and worth calling now and then (a
    /// window's frame) so a replaced take is not held for long.
    pub fn reclaim(&self) {
        let old = self.retired.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !old.is_null() {
            // SAFETY: the node puts a pointer here (from `pending`, so from
            // `Box::into_raw`) only when the slot is empty, and no longer
            // touches it; we swapped it out, so it is ours alone.
            drop(unsafe { Box::from_raw(old) });
        }
    }

    /// Plays from frame `from` of the study's audio, to `to` (or the end),
    /// round and round between them when `looped`. While it plays, this is
    /// a seek.
    pub fn play(&self, from: u64, to: Option<u64>, looped: bool) {
        self.from.store(from, Ordering::Relaxed);
        self.to.store(to.unwrap_or(NONE), Ordering::Relaxed);
        self.looped.store(looped && to.is_some(), Ordering::Relaxed);
        self.position.store(from, Ordering::Relaxed);
        self.kind.store(PLAY, Ordering::Relaxed);
        self.playing.store(true, Ordering::Relaxed);
        self.request.fetch_add(1, Ordering::Release);
    }

    pub fn stop(&self) {
        self.kind.store(STOP, Ordering::Relaxed);
        self.playing.store(false, Ordering::Relaxed);
        self.request.fetch_add(1, Ordering::Release);
    }

    /// A/B: the original rather than the edits, from the same place.
    pub fn set_original(&self, original: bool) {
        self.original.store(original, Ordering::Relaxed);
    }

    pub fn original(&self) -> bool {
        self.original.load(Ordering::Relaxed)
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// The playhead, a frame of the study's audio.
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// The newest audio submitted, for a node being built.
    fn latest(&self) -> Option<StudyAudio> {
        self.latest.lock().ok().and_then(|latest| latest.clone())
    }
}

impl Drop for StudyPlayer {
    fn drop(&mut self) {
        for slot in [&self.pending, &self.retired] {
            let p = slot.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !p.is_null() {
                // SAFETY: as in `submit` and `reclaim`; nothing else can
                // reach the slots once the last `Arc` is going.
                drop(unsafe { Box::from_raw(p) });
            }
        }
    }
}

/// The node: reads the study's audio into the master bus. See the module
/// doc.
pub struct StudyPlayerNode {
    shared: Arc<StudyPlayer>,
    /// What it plays. Replaced only by a swap through the shared slots.
    current: Option<Box<StudyAudio>>,
    /// The last request seen.
    seen: u64,
    active: bool,
    pos: f64,
    to: u64,
    from: u64,
    looped: bool,
    /// 0..1: the start/stop fade, and the A/B mix (1 = original).
    gain: f32,
    mix: f32,
    /// The last moments before a loop's seam, still sounding as the start
    /// fades in: where they are read from and how many samples are left.
    tail: f64,
    tail_left: u32,
    fade: u32,
    sample_rate: f32,
}

impl StudyPlayerNode {
    /// **Off the audio thread** (a graph being built): starts with the
    /// newest audio and wherever the playhead is.
    pub fn new(shared: Arc<StudyPlayer>) -> Self {
        let current = shared.latest().map(Box::new);
        let seen = shared.request.load(Ordering::Acquire);
        let active = shared.playing();
        Self {
            seen,
            active,
            pos: shared.position() as f64,
            from: shared.from.load(Ordering::Relaxed),
            to: shared.to.load(Ordering::Relaxed),
            looped: shared.looped.load(Ordering::Relaxed),
            current,
            shared,
            gain: 0.0,
            mix: 0.0,
            tail: 0.0,
            tail_left: 0,
            fade: 240,
            sample_rate: 48_000.0,
        }
    }

    /// New audio, if the window posted some and there is room to hand back
    /// what it replaces.
    fn take_audio(&mut self) {
        if !self.shared.retired.load(Ordering::Acquire).is_null() {
            return;
        }
        let fresh = self
            .shared
            .pending
            .swap(std::ptr::null_mut(), Ordering::AcqRel);
        if fresh.is_null() {
            return;
        }
        // SAFETY: from `Box::into_raw` in `submit`, and swapped out of the
        // slot, so ours alone.
        let fresh = unsafe { Box::from_raw(fresh) };
        if let Some(old) = self.current.replace(fresh) {
            // Handed back, not freed: the slot was empty a moment ago and
            // only this node fills it.
            self.shared
                .retired
                .store(Box::into_raw(old), Ordering::Release);
        }
    }

    fn take_request(&mut self) {
        let request = self.shared.request.load(Ordering::Acquire);
        if request == self.seen {
            return;
        }
        self.seen = request;
        if self.shared.kind.load(Ordering::Relaxed) == PLAY {
            let from = self.shared.from.load(Ordering::Relaxed);
            // A seek while sounding: the old place fades out under the new.
            if self.active && self.gain > 0.0 {
                self.tail = self.pos;
                self.tail_left = self.fade;
            }
            self.from = from;
            self.to = self.shared.to.load(Ordering::Relaxed);
            self.looped = self.shared.looped.load(Ordering::Relaxed);
            self.pos = from as f64;
            self.active = true;
        } else {
            self.active = false;
        }
    }
}

/// Channel `c` (0 or 1) of `data` at fractional frame `at`: a mono source on
/// both sides; silence outside it.
fn read(data: &[f32], channels: usize, frames: usize, at: f64, c: usize) -> f32 {
    if at < 0.0 {
        return 0.0;
    }
    let i = at as usize;
    if i >= frames {
        return 0.0;
    }
    let c = c.min(channels - 1);
    let a = data[i * channels + c];
    let b = if i + 1 < frames {
        data[(i + 1) * channels + c]
    } else {
        a
    };
    a + (b - a) * (at - i as f64) as f32
}

impl AudioNode for StudyPlayerNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate.max(1.0);
        self.fade = (FADE_SECONDS * self.sample_rate).round().max(1.0) as u32;
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.take_audio();
        self.take_request();
        // Out of its slot while it is read, so the rest of the node can
        // change around it; put back below. A move, not a copy.
        let Some(audio) = self.current.take() else {
            return;
        };
        let channels = audio.channels.max(1);
        let frames = audio.frames();
        let step = f64::from(audio.sample_rate) / f64::from(self.sample_rate);
        let block = ctx.outputs.first().map_or(0, |b| b.len());
        let fade_step = 1.0 / self.fade as f32;
        let want_original = self.shared.original();
        let end = if self.to == NONE {
            frames as u64
        } else {
            self.to.min(frames as u64)
        };
        let mut ended = false;
        for i in 0..block {
            self.gain = if self.active {
                (self.gain + fade_step).min(1.0)
            } else {
                (self.gain - fade_step).max(0.0)
            };
            if self.gain <= 0.0 && !self.active {
                self.tail_left = 0;
                continue;
            }
            self.mix = if want_original {
                (self.mix + fade_step).min(1.0)
            } else {
                (self.mix - fade_step).max(0.0)
            };
            let mix = self.mix;
            let at = |pos: f64, c: usize| -> f32 {
                let edited = read(&audio.edited, channels, frames, pos, c);
                if mix <= 0.0 {
                    return edited;
                }
                let original = read(&audio.original, channels, frames, pos, c);
                edited + (original - edited) * mix
            };
            let mut sides = [at(self.pos, 0), at(self.pos, 1)];
            if self.tail_left > 0 {
                // A seam (a loop come round, a seek): what was playing
                // fades out as the new place fades in.
                let w = self.tail_left as f32 * fade_step;
                let fresh = 1.0 - w;
                sides = [
                    sides[0] * fresh + at(self.tail, 0) * w,
                    sides[1] * fresh + at(self.tail, 1) * w,
                ];
                self.tail += step;
                self.tail_left -= 1;
            }
            for (side, value) in sides.iter().enumerate() {
                if let Some(out) = ctx.outputs.get_mut(side) {
                    out[i] += value * self.gain;
                }
            }
            self.pos += step;
            if self.active && self.pos >= end as f64 {
                if self.looped && end > self.from {
                    self.tail = self.pos;
                    self.tail_left = self.fade;
                    self.pos = self.from as f64 + (self.pos - end as f64);
                } else {
                    self.active = false;
                    ended = true;
                }
            }
        }
        self.current = Some(audio);
        let shown = if self.to != NONE && self.looped {
            (self.pos as u64).clamp(self.from, self.to.saturating_sub(1))
        } else {
            self.pos as u64
        };
        self.shared.position.store(shown, Ordering::Relaxed);
        // Said stopped, unless the window has asked for something since.
        if ended && self.shared.request.load(Ordering::Acquire) == self.seen {
            self.shared.playing.store(false, Ordering::Relaxed);
        }
    }

    /// A transport stop or a device reset leaves a listen alone: it is
    /// the window's, not the song's.
    fn reset(&mut self) {}

    fn debug_name(&self) -> &'static str {
        "study-player"
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}
