//! The vocal track's audio node: phrases sung ahead of time, played back at
//! the right sample.
//!
//! **Nothing is sung here.** A singer engine takes far longer than a block to
//! sing a phrase, so the app renders phrases on worker threads, ahead of the
//! playhead, and publishes a [`VocalTable`]: each phrase's audio and the song
//! second it starts at. [`VocalNode`] reads that table — from wherever the
//! playhead is, at the device's rate whatever rate the engine sang at, with an
//! equal-power crossfade where one phrase's lead-in (a consonant starts before
//! its note) runs under the last one's tail. A phrase not rendered yet is
//! simply not in the table, and is silence.
//!
//! # Placement in seconds
//!
//! A phrase's start is a song **second**, not a sample: the app works it out
//! through the tempo map once, and the node turns its own playhead into
//! seconds with its own rate — so the same table is right for the device and
//! for an export at another rate.
//!
//! # INVARIANT 1
//!
//! [`VocalPlayer`] is shared by the app and every node built for the channel,
//! and outlives graphs, the way `StudyPlayer` does. A new table is handed over
//! through a one-slot mailbox of a raw pointer, and the table it replaces goes
//! back the same way for the app to free: the node takes a new one only
//! while the return slot is empty, so it never frees anything. Reading is a
//! binary search and a linear interpolation per sample.

use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};
use crate::nodes::{CHANNEL_GAIN_MAX_DB, CHANNEL_GAIN_MIN_DB, EmptyParams};
use crate::transport::TransportState;

/// One phrase as sung: mono audio at the engine's rate, and the song second
/// its first sample sounds at.
#[derive(Debug, Clone)]
pub struct SungPhrase {
    pub start: f64,
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
}

/// The fade at a phrase's edge where nothing overlaps it: no click, no swell.
const EDGE_SECONDS: f64 = 0.004;

#[derive(Debug, Clone)]
struct Placed {
    start: f64,
    /// Seconds of audio.
    length: f64,
    rate: f64,
    samples: Arc<[f32]>,
    /// Fade lengths in seconds, at the front and the back.
    fade_in: f64,
    fade_out: f64,
}

impl Placed {
    fn end(&self) -> f64 {
        self.start + self.length
    }

    /// The gain at `t` seconds into the phrase: a quarter sine up over the
    /// front fade and a quarter cosine down over the back one, which is what
    /// makes two overlapping phrases sum to constant power.
    fn gain(&self, t: f64) -> f32 {
        let mut g = 1.0f64;
        if t < self.fade_in {
            g *= (std::f64::consts::FRAC_PI_2 * t / self.fade_in).sin();
        }
        let back = self.length - self.fade_out;
        if t > back && self.fade_out > 0.0 {
            g *= (std::f64::consts::FRAC_PI_2 * ((t - back) / self.fade_out).min(1.0)).cos();
        }
        g as f32
    }

    fn read(&self, t: f64) -> f32 {
        let at = t * self.rate;
        if at < 0.0 {
            return 0.0;
        }
        let i = at as usize;
        let Some(&a) = self.samples.get(i) else {
            return 0.0;
        };
        let b = self.samples.get(i + 1).copied().unwrap_or(0.0);
        a + (b - a) * (at - i as f64) as f32
    }
}

/// The phrases a vocal channel plays, in time order, with their fades worked
/// out. Built off the audio thread; immutable once published.
#[derive(Debug, Clone, Default)]
pub struct VocalTable {
    phrases: Vec<Placed>,
    /// The latest end of any phrase up to each index: what a binary search
    /// for "the first phrase still sounding at t" needs when one phrase can
    /// outlast the next.
    reach: Vec<f64>,
}

impl VocalTable {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new(mut phrases: Vec<SungPhrase>) -> Self {
        phrases.retain(|p| p.sample_rate > 0 && !p.samples.is_empty() && p.start.is_finite());
        phrases.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut placed: Vec<Placed> = phrases
            .into_iter()
            .map(|p| {
                let rate = f64::from(p.sample_rate);
                let length = p.samples.len() as f64 / rate;
                let edge = EDGE_SECONDS.min(length / 2.0);
                Placed {
                    start: p.start,
                    length,
                    rate,
                    samples: p.samples,
                    fade_in: edge,
                    fade_out: edge,
                }
            })
            .collect();
        // Where one runs under the next, both fade across the overlap — the
        // same span, so the two curves are each other's complement.
        for i in 1..placed.len() {
            let overlap = placed[i - 1].end() - placed[i].start;
            if overlap > EDGE_SECONDS {
                let span = overlap
                    .min(placed[i - 1].length / 2.0)
                    .min(placed[i].length / 2.0);
                placed[i - 1].fade_out = span;
                placed[i].fade_in = span;
            }
        }
        let mut reach = Vec::with_capacity(placed.len());
        let mut furthest = f64::NEG_INFINITY;
        for p in &placed {
            furthest = furthest.max(p.end());
            reach.push(furthest);
        }
        Self {
            phrases: placed,
            reach,
        }
    }

    pub fn len(&self) -> usize {
        self.phrases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.phrases.is_empty()
    }
}

/// What the app and the nodes share. See the module doc.
#[derive(Debug)]
pub struct VocalPlayer {
    pending: AtomicPtr<VocalTable>,
    retired: AtomicPtr<VocalTable>,
    /// The newest table, for a node built after it was published (a graph
    /// rebuild, an export). Only ever touched off the audio thread.
    latest: Mutex<VocalTable>,
}

impl VocalPlayer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: AtomicPtr::new(std::ptr::null_mut()),
            retired: AtomicPtr::new(std::ptr::null_mut()),
            latest: Mutex::new(VocalTable::empty()),
        })
    }

    /// **Off the audio thread.** Hands `table` to the nodes; it is heard from
    /// the next block. Frees what a node handed back first.
    pub fn publish(&self, table: VocalTable) {
        self.reclaim();
        if let Ok(mut latest) = self.latest.lock() {
            *latest = table.clone();
        }
        let fresh = Box::into_raw(Box::new(table));
        let unread = self.pending.swap(fresh, Ordering::AcqRel);
        if !unread.is_null() {
            // SAFETY: from `Box::into_raw` above, swapped out of the slot, so
            // nobody else has it.
            drop(unsafe { Box::from_raw(unread) });
        }
    }

    /// **Off the audio thread.** Frees the table a node stopped playing.
    pub fn reclaim(&self) {
        let old = self.retired.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !old.is_null() {
            // SAFETY: a node stores here only a pointer it took from
            // `pending` (so from `Box::into_raw`) and no longer touches.
            drop(unsafe { Box::from_raw(old) });
        }
    }

    /// Whether a replaced table is waiting for [`reclaim`](Self::reclaim).
    pub fn has_retired(&self) -> bool {
        !self.retired.load(Ordering::Acquire).is_null()
    }

    /// How many phrases the newest table holds.
    pub fn phrase_count(&self) -> usize {
        self.latest.lock().map_or(0, |t| t.len())
    }

    fn latest(&self) -> VocalTable {
        self.latest.lock().map(|t| t.clone()).unwrap_or_default()
    }
}

impl Drop for VocalPlayer {
    fn drop(&mut self) {
        for slot in [&self.pending, &self.retired] {
            let p = slot.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !p.is_null() {
                // SAFETY: as in `publish` and `reclaim`.
                drop(unsafe { Box::from_raw(p) });
            }
        }
    }
}

/// The node: plays a vocal channel's table into its bus. See the module doc.
pub struct VocalNode {
    shared: Arc<VocalPlayer>,
    current: Box<VocalTable>,
    /// The block's mono mix before level and pan, sized in `prepare`.
    scratch: Vec<f32>,
    sample_rate: f64,
    gain_db: f32,
    pan: f32,
}

impl VocalNode {
    /// **Off the audio thread** (a graph being built): starts with the newest
    /// table.
    pub fn new(shared: Arc<VocalPlayer>) -> Self {
        let current = Box::new(shared.latest());
        Self {
            shared,
            current,
            scratch: Vec::new(),
            sample_rate: 48_000.0,
            gain_db: 0.0,
            pan: 0.0,
        }
    }

    /// The channel's own level and placement (`Channel::gain_db`, `pan`).
    pub fn on_channel(mut self, gain_db: f32, pan: f32) -> Self {
        self.gain_db = gain_db;
        self.pan = pan;
        self
    }

    fn take_table(&mut self) {
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
        // SAFETY: from `Box::into_raw` in `publish`, swapped out, so ours.
        let fresh = unsafe { Box::from_raw(fresh) };
        let old = std::mem::replace(&mut self.current, fresh);
        // Handed back, not freed: the slot was empty a moment ago and only a
        // node fills it.
        self.shared
            .retired
            .store(Box::into_raw(old), Ordering::Release);
    }

    fn take_automation(&mut self, ctx: &ProcessContext) {
        for event in ctx.events() {
            let fontelle_types::EventPayload::ParamValue { target, value } = &event.payload else {
                continue;
            };
            if !value.is_finite() {
                continue;
            }
            let address = target.as_str();
            if address.ends_with("/gain") {
                self.gain_db = CHANNEL_GAIN_MIN_DB
                    + *value as f32 * (CHANNEL_GAIN_MAX_DB - CHANNEL_GAIN_MIN_DB);
            } else if address.ends_with("/pan") {
                self.pan = *value as f32 * 2.0 - 1.0;
            }
        }
    }
}

impl AudioNode for VocalNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = f64::from(ctx.sample_rate.max(1.0));
        self.scratch = vec![0.0; ctx.max_block_size.max(1) as usize];
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.take_table();
        self.take_automation(ctx);
        // Only while the song moves: a count-in holds the playhead still,
        // and a stopped song sings nothing.
        if !matches!(
            ctx.transport.state,
            TransportState::Playing | TransportState::Recording | TransportState::Rendering
        ) {
            return;
        }
        let frames = ctx
            .outputs
            .iter()
            .map(|o| o.len())
            .min()
            .unwrap_or(0)
            .min(self.scratch.len());
        if frames == 0 || self.current.is_empty() {
            return;
        }
        let sr = self.sample_rate;
        let t0 = ctx.sample_range.start as f64 / sr;
        let t1 = t0 + frames as f64 / sr;
        let table = &*self.current;
        let mono = &mut self.scratch[..frames];
        mono.fill(0.0);
        let first = table.reach.partition_point(|end| *end <= t0);
        let mut any = false;
        for phrase in &table.phrases[first..] {
            if phrase.start >= t1 {
                break;
            }
            if phrase.end() <= t0 {
                continue;
            }
            any = true;
            for (i, out) in mono.iter_mut().enumerate() {
                let t = t0 + i as f64 / sr - phrase.start;
                if t < 0.0 || t >= phrase.length {
                    continue;
                }
                *out += phrase.read(t) * phrase.gain(t);
            }
        }
        if !any {
            return;
        }
        let gain = if self.gain_db == 0.0 {
            1.0
        } else {
            10f32.powf(self.gain_db / 20.0)
        };
        let stereo = ctx.outputs.len() > 1;
        let (left, right) = fontelle_types::PanLaw::Minus3Db.gains(self.pan);
        for (index, channel) in ctx.outputs.iter_mut().enumerate() {
            let side = match (stereo, index) {
                (false, _) => 1.0,
                (true, 0) => left,
                (true, 1) => right,
                _ => continue,
            };
            let scale = gain * side;
            for (out, s) in channel[..frames].iter_mut().zip(mono.iter()) {
                *out += s * scale;
            }
        }
    }

    fn reset(&mut self) {}

    fn debug_name(&self) -> &'static str {
        "vocal"
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}
