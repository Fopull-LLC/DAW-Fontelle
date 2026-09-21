//! Flopsynth's wavetables (`docs/flopsynth-plan.md` §3.2).
//!
//! # No files
//!
//! Every table here is **generated** — from a harmonic recipe or from a shape
//! sampled directly — so Flopsynth works on a fresh install and a preset can
//! never break a link. That is the drum machine's position (`Source::Drum`
//! names no file and therefore never needs relinking, TDD §17.4) taken to the
//! oscillators, and it is what lets a hundred and thirty factory presets ship
//! as two hundred bytes of JSON each instead of as a sample library.
//!
//! # The pyramid
//!
//! A [`Wavetable`] is *frames* × *mip levels*. A frame is one cycle of a
//! waveform; moving across the frames is what a position knob does. Each frame
//! carries [`WAVETABLE_LEVELS`] band-limited copies of itself, the first
//! [`WAVETABLE_LEN`] samples long and each one after that half as long, so
//! level *k* can hold harmonics up to `(WAVETABLE_LEN >> k) / 2` and — this is
//! the point — **cannot hold anything above that**, because there is no room
//! for it. Playing a note picks the finest level whose top harmonic still
//! lands under Nyquist ([`wavetable_level_for`]), which is what keeps a saw at
//! the top of the keyboard from spraying aliases back down the spectrum.
//!
//! Ten levels rather than the seven that reach 32 samples: seven stops at
//! sixteen harmonics, and sixteen harmonics of A8 is 112 kHz. The last three
//! levels cost 28 samples a frame between them.
//!
//! # Where the memory goes, and when
//!
//! A frame's pyramid is `2 × WAVETABLE_LEN` samples — 16 KB — so a 32-frame
//! table is half a megabyte and the whole bank a few megabytes. Tables are
//! built **lazily**, on first use, behind a lock in [`WavetableBank`], and
//! `Sampler::prepare` is the only thing allowed to ask: locking and allocating
//! are fine off the RT thread and forbidden in `render` (INVARIANT 1).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One frame of a table, at the finest level. A power of two, because the
/// pyramid is built by halving and the transform that band-limits it is
/// radix-2.
pub const WAVETABLE_LEN: usize = 2048;

/// How many band-limited copies of each frame there are: `2048, 1024, … 4`.
pub const WAVETABLE_LEVELS: usize = 10;

/// The finest level whose top harmonic still lands under Nyquist at `f0`.
///
/// The **smallest** such level, so a low note keeps every harmonic it can and
/// only a high one gives them up. `0.45` rather than `0.5` leaves the last
/// twentieth of the band clear, where the linear interpolation between table
/// samples has its own error to spend.
pub fn wavetable_level_for(f0_hz: f32, sample_rate: f32) -> usize {
    let ceiling = sample_rate * 0.45;
    let f0 = f0_hz.abs().max(1e-6);
    for level in 0..WAVETABLE_LEVELS {
        let top_harmonic = ((WAVETABLE_LEN >> level) / 2) as f32;
        if f0 * top_harmonic <= ceiling {
            return level;
        }
    }
    WAVETABLE_LEVELS - 1
}

/// One built table: its frames, each with its own mip pyramid.
#[derive(Debug)]
pub struct Wavetable {
    frames: usize,
    /// `levels[frame * WAVETABLE_LEVELS + level]`. One `Vec` per level rather
    /// than one flat buffer with offsets, because the pyramid is built once
    /// and read for the life of the process — the indirection costs a pointer
    /// chase per block, not per sample.
    levels: Vec<Vec<f32>>,
}

impl Wavetable {
    pub fn frame_count(&self) -> usize {
        self.frames
    }

    /// One band-limited copy of one frame.
    pub fn level(&self, frame: usize, level: usize) -> &[f32] {
        let frame = frame.min(self.frames - 1);
        let level = level.min(WAVETABLE_LEVELS - 1);
        &self.levels[frame * WAVETABLE_LEVELS + level]
    }

    /// How much of the memory budget this table spends.
    pub fn bytes(&self) -> usize {
        self.levels
            .iter()
            .map(|level| level.len() * std::mem::size_of::<f32>())
            .sum()
    }

    /// One sample, at `position` across the frames and `phase` through the
    /// cycle, read at mip `level`.
    ///
    /// Linear in both axes: between the two frames either side of the
    /// position, and between the two samples either side of the phase. Four
    /// table reads, which is the per-unison-voice cost §10 budgets.
    ///
    /// A single-frame table ignores `position` entirely rather than clamping
    /// it, so a position knob on a static table is a knob that does nothing
    /// visibly rather than a knob that does something at one end.
    pub fn read(&self, position: f32, phase: f32, level: usize) -> f32 {
        let level = level.min(WAVETABLE_LEVELS - 1);
        let phase = phase.rem_euclid(1.0);
        if self.frames == 1 {
            return read_level(self.level(0, level), phase);
        }
        let across = position.clamp(0.0, 1.0) * (self.frames - 1) as f32;
        let low = across.floor() as usize;
        let high = (low + 1).min(self.frames - 1);
        let blend = across - low as f32;
        // **One read when the position sits on a frame**, which is the
        // ordinary case: a preset that does not modulate position sits on one
        // for every sample it ever plays, and one that does sits on one
        // between its moves. The blend is exactly zero there, so the second
        // read contributes exactly nothing — this is a skip, not an
        // approximation (`docs/flopsynth-plan.md` §10's first named lever).
        //
        // Worth 21 reads a sample on a three-oscillator seven-voice unison,
        // which is what "Supersaw" is.
        if blend <= 0.0 {
            return read_level(self.level(low, level), phase);
        }
        if blend >= 1.0 {
            return read_level(self.level(high, level), phase);
        }
        let a = read_level(self.level(low, level), phase);
        let b = read_level(self.level(high, level), phase);
        a + (b - a) * blend
    }

    /// A table built from **a sound somebody dropped in**, rather than from a
    /// recipe (`docs/flopsynth-plan.md` §3.2's "no files" is about what the
    /// *bank* ships, not about what a patch may carry).
    ///
    /// > *"i want to be able to drag audio files into it to use those
    /// > waveforms in the synthesis."*
    ///
    /// The file is cut into `frames` equal parts and each part is read across
    /// one frame of [`WAVETABLE_LEN`] samples, so moving the position knob
    /// walks through the sound. A file that is already a whole number of
    /// frames long is taken **verbatim** at the finest level — resampling a
    /// 2048-sample cycle onto 2048 samples is a filter nobody asked for — and
    /// anything else is read with linear interpolation, wrapping within its
    /// own part so a frame is a cycle rather than a fragment with an edge.
    ///
    /// Every frame then gets the same pyramid and the same one-gain
    /// normalisation a generated table gets, which is what keeps a dropped
    /// sound from aliasing at the top of the keyboard and from being a
    /// different loudness from the rest of the bank.
    ///
    /// Nothing is refused: no samples is one silent frame, because a table
    /// with no frames divides by zero in every reader downstream.
    pub fn from_samples(samples: &[f32], frames: usize) -> Self {
        let frames = frames.clamp(1, MAX_USER_FRAMES);
        if samples.is_empty() {
            return Self {
                frames: 1,
                levels: pyramid(&vec![0.0f32; WAVETABLE_LEN]),
            };
        }
        let per_frame = samples.len() as f64 / frames as f64;
        let mut levels: Vec<Vec<f32>> = Vec::with_capacity(frames * WAVETABLE_LEVELS);
        let mut finest = vec![0.0f32; WAVETABLE_LEN];
        for frame in 0..frames {
            let from = (frame as f64 * per_frame).round() as usize;
            let to = (((frame + 1) as f64 * per_frame).round() as usize).min(samples.len());
            let part = &samples[from.min(samples.len())..to.max(from.min(samples.len()))];
            for (i, out) in finest.iter_mut().enumerate() {
                *out = if part.is_empty() {
                    0.0
                } else if part.len() == WAVETABLE_LEN {
                    part[i]
                } else {
                    // Linear, wrapping inside this part: the sample after the
                    // last is the first, because a frame is one cycle.
                    let x = i as f64 * part.len() as f64 / WAVETABLE_LEN as f64;
                    let a = part[(x as usize) % part.len()];
                    let b = part[(x as usize + 1) % part.len()];
                    let t = (x - x.floor()) as f32;
                    a + (b - a) * t
                };
            }
            levels.extend(pyramid(&finest));
        }
        normalise(&mut levels);
        Self { frames, levels }
    }

    /// [`read`](Self::read) with a **four-point** interpolation between the
    /// table's samples instead of the two-point one.
    ///
    /// For an oversampled oscillator (`docs/flopsynth-next.md` §4.1). A
    /// linear read's images sit at multiples of the table's own rate, and
    /// the level chosen for four times the session's rate has enough
    /// harmonics that the top one's first image lands past the oversampled
    /// Nyquist and folds into the band — a saw at C7 read linearly at 4×
    /// measured 44 dB of alias against signal, with the sub-sample rendering
    /// and the decimator both doing their jobs (2026-09-19). The cubic's
    /// images are twenty-odd decibels further down, which is what moves the
    /// floor out of the way. At `Off` the linear read stays: it is the sound
    /// every preset was voiced through, and a sample-exact one.
    pub fn read_smooth(&self, position: f32, phase: f32, level: usize) -> f32 {
        let level = level.min(WAVETABLE_LEVELS - 1);
        let phase = phase.rem_euclid(1.0);
        if self.frames == 1 {
            return read_level_smooth(self.level(0, level), phase);
        }
        let across = position.clamp(0.0, 1.0) * (self.frames - 1) as f32;
        let low = across.floor() as usize;
        let high = (low + 1).min(self.frames - 1);
        let blend = across - low as f32;
        if blend <= 0.0 {
            return read_level_smooth(self.level(low, level), phase);
        }
        if blend >= 1.0 {
            return read_level_smooth(self.level(high, level), phase);
        }
        let a = read_level_smooth(self.level(low, level), phase);
        let b = read_level_smooth(self.level(high, level), phase);
        a + (b - a) * blend
    }

    /// One named frame, without the blend — what a picture of "frame 3" is,
    /// and what `read` falls back to when the position sits on one.
    pub fn read_frame(&self, frame: usize, phase: f32, level: usize) -> f32 {
        let level = level.min(WAVETABLE_LEVELS - 1);
        read_level(
            self.level(frame.min(self.frames - 1), level),
            phase.rem_euclid(1.0),
        )
    }
}

/// Linear interpolation within one frame. The table wraps, so the sample after
/// the last is the first — a cycle has no end.
///
/// Every level is [`WAVETABLE_LEN`] shifted down, so its length is a power
/// of two and the wrap is a mask: a remainder by a length the compiler
/// cannot see is a division, and a supersaw is twenty-one of these a
/// sample, four times over when oversampled.
fn read_level(samples: &[f32], phase: f32) -> f32 {
    let n = samples.len();
    debug_assert!(n.is_power_of_two());
    let mask = n - 1;
    let x = phase * n as f32;
    let i = x as usize & mask;
    let frac = x - x.floor();
    let a = samples[i];
    let b = samples[(i + 1) & mask];
    a + (b - a) * frac
}

/// Four-point Hermite (Catmull-Rom) interpolation within one frame, wrapping
/// the same way — see [`Wavetable::read_smooth`]. The same polynomial
/// `crate::interpolate` uses at `Normal`.
fn read_level_smooth(samples: &[f32], phase: f32) -> f32 {
    let n = samples.len();
    debug_assert!(n.is_power_of_two());
    let mask = n - 1;
    let x = phase * n as f32;
    let i = x as usize & mask;
    let t = x - x.floor();
    let y0 = samples[(i + n - 1) & mask];
    let y1 = samples[i];
    let y2 = samples[(i + 1) & mask];
    let y3 = samples[(i + 2) & mask];
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// Which table an oscillator reads.
///
/// **A variant's name is INVARIANT 7's** the moment a patch is saved naming
/// it: serde writes these by name, so a table renamed in code would be a
/// preset that no longer opens. Adding one is a variant and a recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WavetableId {
    // Basic
    Sine,
    Triangle,
    Saw,
    Square,
    Pulse,
    // Analog
    AnalogMorph,
    PwmSweep,
    SyncSweep,
    SubSaw,
    // Harmonic
    Drawbar,
    Odd,
    Even,
    Sawstack,
    BrightStack,
    Hollow,
    Struck,
    // Vocal
    Vowel,
    Choir,
    FormantSweep,
    // Bell
    FmBell,
    Glass,
    Tine,
    Gong,
    // Digital
    Grit,
    Stairs,
    Crunch,
    Bitwave,
    // Modern
    Growl,
    Reese,
    Hoover,
    Wide,
    // Sub
    SubSine,
    SubTri,
    SubSquare,
    // Chip
    NesPulse125,
    NesPulse25,
    NesPulse50,
    NesTriangle,
    GameBoy,
    C64,
}

impl Default for WavetableId {
    /// The one a subtractive patch starts from, for the reason
    /// `patch_params::SHAPES` puts a saw first.
    fn default() -> Self {
        Self::Saw
    }
}

impl WavetableId {
    /// Every table, in the order the drop-down groups them.
    pub const ALL: [Self; 40] = [
        Self::Sine,
        Self::Triangle,
        Self::Saw,
        Self::Square,
        Self::Pulse,
        Self::AnalogMorph,
        Self::PwmSweep,
        Self::SyncSweep,
        Self::SubSaw,
        Self::Drawbar,
        Self::Odd,
        Self::Even,
        Self::Sawstack,
        Self::BrightStack,
        Self::Hollow,
        Self::Struck,
        Self::Vowel,
        Self::Choir,
        Self::FormantSweep,
        Self::FmBell,
        Self::Glass,
        Self::Tine,
        Self::Gong,
        Self::Grit,
        Self::Stairs,
        Self::Crunch,
        Self::Bitwave,
        Self::Growl,
        Self::Reese,
        Self::Hoover,
        Self::Wide,
        Self::SubSine,
        Self::SubTri,
        Self::SubSquare,
        Self::NesPulse125,
        Self::NesPulse25,
        Self::NesPulse50,
        Self::NesTriangle,
        Self::GameBoy,
        Self::C64,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::Saw => "Saw",
            Self::Square => "Square",
            Self::Pulse => "Pulse",
            Self::AnalogMorph => "Analog Morph",
            Self::PwmSweep => "PWM Sweep",
            Self::SyncSweep => "Sync Sweep",
            Self::SubSaw => "Sub Saw",
            Self::Drawbar => "Drawbar",
            Self::Odd => "Odd",
            Self::Even => "Even",
            Self::Sawstack => "Sawstack",
            Self::BrightStack => "Bright Stack",
            Self::Hollow => "Hollow",
            Self::Struck => "Struck",
            Self::Vowel => "Vowel",
            Self::Choir => "Choir",
            Self::FormantSweep => "Formant Sweep",
            Self::FmBell => "FM Bell",
            Self::Glass => "Glass",
            Self::Tine => "Tine",
            Self::Gong => "Gong",
            Self::Grit => "Grit",
            Self::Stairs => "Stairs",
            Self::Crunch => "Crunch",
            Self::Bitwave => "Bitwave",
            Self::Growl => "Growl",
            Self::Reese => "Reese",
            Self::Hoover => "Hoover",
            Self::Wide => "Wide",
            Self::SubSine => "Sub Sine",
            Self::SubTri => "Sub Tri",
            Self::SubSquare => "Sub Square",
            Self::NesPulse125 => "NES Pulse 12.5",
            Self::NesPulse25 => "NES Pulse 25",
            Self::NesPulse50 => "NES Pulse 50",
            Self::NesTriangle => "NES Triangle",
            Self::GameBoy => "Game Boy",
            Self::C64 => "C64",
        }
    }

    /// The heading this table sits under in the chooser. Grouped rather than
    /// listed flat, because forty rows with no headings is a list
    /// nobody reads to the bottom of.
    pub fn family(self) -> &'static str {
        match self {
            Self::Sine | Self::Triangle | Self::Saw | Self::Square | Self::Pulse => "Basic",
            Self::AnalogMorph | Self::PwmSweep | Self::SyncSweep | Self::SubSaw => "Analog",
            Self::Drawbar
            | Self::Odd
            | Self::Even
            | Self::Sawstack
            | Self::BrightStack
            | Self::Hollow
            | Self::Struck => "Harmonic",
            Self::Vowel | Self::Choir | Self::FormantSweep => "Vocal",
            Self::FmBell | Self::Glass | Self::Tine | Self::Gong => "Bell",
            Self::Grit | Self::Stairs | Self::Crunch | Self::Bitwave => "Digital",
            Self::Growl | Self::Reese | Self::Hoover | Self::Wide => "Modern",
            Self::SubSine | Self::SubTri | Self::SubSquare => "Sub",
            Self::NesPulse125
            | Self::NesPulse25
            | Self::NesPulse50
            | Self::NesTriangle
            | Self::GameBoy
            | Self::C64 => "Chip",
        }
    }
}

/// The process-wide table cache.
///
/// One `Mutex<HashMap<..>>` behind an `Arc`: a table is built on first use and
/// handed out as an `Arc` afterwards, so a voice holds a pointer and never the
/// lock. **Only `prepare` may call [`get`](WavetableBank::get)** — it locks and
/// it allocates, both of which are fine off the RT thread and forbidden on it.
#[derive(Debug, Default)]
pub struct WavetableBank {
    built: Mutex<HashMap<WavetableId, Arc<Wavetable>>>,
}

impl WavetableBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// The table, building it if this is the first time anybody asked.
    pub fn get(&self, id: WavetableId) -> Arc<Wavetable> {
        // A poisoned lock is a panic in another thread's `build`, which cannot
        // happen for pure arithmetic — but recovering is one line and losing
        // every table to it would be worse.
        let mut built = self.built.lock().unwrap_or_else(|e| e.into_inner());
        built
            .entry(id)
            .or_insert_with(|| Arc::new(build(id)))
            .clone()
    }
}

/// The one every `Sampler` shares. A table is a constant, so there is no
/// reason for two of them to exist and every reason for the memory not to be
/// spent twice.
pub fn wavetables() -> &'static WavetableBank {
    static BANK: std::sync::OnceLock<WavetableBank> = std::sync::OnceLock::new();
    BANK.get_or_init(WavetableBank::new)
}

// ------------------------------------------------------------- generation ---

/// How one table's frames are described.
///
/// Two kinds, because two kinds of table are *defined* differently. A saw is
/// its harmonic series and generating it any other way is an approximation; an
/// NES pulse is its exact duty cycle and its steps **are** the sound, so
/// band-limiting it into existence would be the approximation.
enum Shape {
    /// `(amplitude, phase)` of harmonic `h` (1-based) at frame position `t`.
    Harmonics(fn(usize, f32) -> (f32, f32)),
    /// The sample value at phase `p` of frame position `t`. Level 0 is this
    /// exactly; the coarser levels are band-limited copies of it.
    Exact(fn(f32, f32) -> f32),
}

struct Recipe {
    frames: usize,
    shape: Shape,
}

fn build(id: WavetableId) -> Wavetable {
    let recipe = recipe(id);
    let mut levels: Vec<Vec<f32>> = Vec::with_capacity(recipe.frames * WAVETABLE_LEVELS);
    for frame in 0..recipe.frames {
        let t = if recipe.frames <= 1 {
            0.0
        } else {
            frame as f32 / (recipe.frames - 1) as f32
        };
        levels.extend(pyramid(&finest_level(&recipe.shape, t)));
    }
    normalise(&mut levels);
    Wavetable {
        frames: recipe.frames,
        levels,
    }
}

/// The most frames a table built from a dropped sound, or drawn, may have.
///
/// Serum's own number (`docs/flopsynth-next.md` §4.3): a frame's pyramid is
/// sixteen kilobytes, so the table is four megabytes in memory and one in
/// the file at sixteen bits. Sixty-four until Phase 4, when the editor's
/// frames wanted the room.
pub const MAX_USER_FRAMES: usize = 256;

/// **One gain for the whole table, taken over every level**, so that sweeping
/// the position knob does not sweep the volume and switching mip level
/// mid-note does not step it either.
///
/// Over every level rather than over level 0 alone, which is what §3.2 first
/// said: band-limiting a shape with a step in it (`Shape::Exact` — the sync
/// sweep, the chip pulses) overshoots at the discontinuity, and the overshoot
/// is by definition invisible at the level that still has the step.
/// Normalising on level 0 shipped a table whose *coarser* levels clipped,
/// which is a fizz that only appears at the top of the keyboard.
fn normalise(levels: &mut [Vec<f32>]) {
    let peak = levels
        .iter()
        .flat_map(|level| level.iter())
        .fold(0.0f32, |a, s| a.max(s.abs()));
    // 0.9 rather than 1.0: the last tenth is the headroom the frame
    // interpolation and the unison sum need before anything downstream sees it.
    let gain = if peak > 1e-9 { 0.9 / peak } else { 0.0 };
    for level in levels.iter_mut() {
        for sample in level {
            *sample *= gain;
        }
    }
}

/// Level 0 of one frame.
fn finest_level(shape: &Shape, t: f32) -> Vec<f32> {
    match shape {
        Shape::Harmonics(amplitude) => {
            let top = WAVETABLE_LEN / 2;
            let mut re = vec![0.0f32; WAVETABLE_LEN];
            let mut im = vec![0.0f32; WAVETABLE_LEN];
            for h in 1..top {
                let (a, phase) = amplitude(h, t);
                if a.abs() < 1e-7 {
                    continue;
                }
                // A real signal's spectrum is conjugate-symmetric; filling both
                // halves is what makes the inverse transform come back real.
                //
                // **A quarter turn back**, because the inverse transform's
                // basis is a cosine and a recipe's "phase 0" has to mean a
                // *sine*: an oscillator starts at phase 0, and a table whose
                // first sample is full amplitude is a click on every note-on.
                // With this, `Sine`, `Saw`, `Square` and `Triangle` are all
                // their textbook selves and all start at a zero crossing.
                let angle = phase - std::f32::consts::FRAC_PI_2;
                let (c, s) = (angle.cos(), angle.sin());
                re[h] += a * c;
                im[h] += a * s;
                re[WAVETABLE_LEN - h] += a * c;
                im[WAVETABLE_LEN - h] -= a * s;
            }
            inverse(&mut re, &mut im);
            re
        }
        Shape::Exact(f) => (0..WAVETABLE_LEN)
            .map(|i| f(i as f32 / WAVETABLE_LEN as f32, t))
            .collect(),
    }
}

/// Every level of one frame, from its finest.
///
/// Band-limited rather than decimated: the spectrum is taken once and the
/// shorter levels are inverse-transformed from the bins that still fit. Taking
/// every other sample instead would fold everything above the new Nyquist
/// straight back down, which is the aliasing the pyramid exists to prevent.
fn pyramid(finest: &[f32]) -> Vec<Vec<f32>> {
    let mut re: Vec<f32> = finest.to_vec();
    let mut im = vec![0.0f32; finest.len()];
    crate::fft_in_place(&mut re, &mut im);

    let mut levels = Vec::with_capacity(WAVETABLE_LEVELS);
    levels.push(finest.to_vec());
    for level in 1..WAVETABLE_LEVELS {
        let len = WAVETABLE_LEN >> level;
        let top = len / 2;
        let mut lre = vec![0.0f32; len];
        let mut lim = vec![0.0f32; len];
        // The forward transform above was not normalised, and the inverse
        // below divides by *its* length, so the bins carry a factor of the
        // length ratio out of the round trip.
        let scale = len as f32 / WAVETABLE_LEN as f32;
        for h in 1..top {
            lre[h] = re[h] * scale;
            lim[h] = im[h] * scale;
            lre[len - h] = re[WAVETABLE_LEN - h] * scale;
            lim[len - h] = im[WAVETABLE_LEN - h] * scale;
        }
        lre[0] = re[0] * scale;
        lim[0] = im[0] * scale;
        inverse(&mut lre, &mut lim);
        levels.push(lre);
    }
    levels
}

/// An inverse FFT by the conjugate trick: conjugate, transform forward,
/// conjugate, divide by the length. One transform rather than a second
/// implementation to keep in step with [`crate::fft_in_place`].
fn inverse(re: &mut [f32], im: &mut [f32]) {
    for v in im.iter_mut() {
        *v = -*v;
    }
    crate::fft_in_place(re, im);
    let n = re.len() as f32;
    for v in re.iter_mut() {
        *v /= n;
    }
    for v in im.iter_mut() {
        *v /= -n;
    }
}

// ---------------------------------------------------------------- recipes ---

/// A deterministic scatter in `0..1` for harmonic `h`.
///
/// Deterministic because a table is a constant: two builds of the same binary
/// have to produce the same bytes, and a preset's sound cannot depend on the
/// order a `HashMap` happened to iterate in.
fn scatter(h: usize, salt: u32) -> f32 {
    let mut x = (h as u32).wrapping_mul(2_654_435_761).wrapping_add(salt);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x >> 8) as f32 / 16_777_216.0
}

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

/// The Bessel function of the first kind, by its own series.
///
/// What an FM spectrum *is*: two-operator FM at carrier `c` and modulator `m`
/// with index `I` puts `J_k(I)` at `c + k·m` for every `k`. A wavetable can
/// only hold harmonics, so the bell tables use integer ratios — which is the
/// honest approximation for a single-cycle table and is what makes their
/// partials sparse and metallic rather than a filtered saw.
fn bessel_j(k: i32, x: f32) -> f32 {
    let (k, sign) = if k < 0 {
        // J_-k(x) = (-1)^k J_k(x)
        (-k, if k % 2 == 0 { 1.0 } else { -1.0 })
    } else {
        (k, 1.0)
    };
    let mut term = 1.0f64;
    for i in 1..=k {
        term *= f64::from(x) / 2.0 / f64::from(i);
    }
    let half_x_squared = (f64::from(x) / 2.0).powi(2);
    let mut sum = term;
    for m in 1..40 {
        term *= -half_x_squared / (f64::from(m) * f64::from(m + k));
        sum += term;
        if term.abs() < 1e-12 {
            break;
        }
    }
    sign * sum as f32
}

/// The amplitude of harmonic `h` in a two-operator FM tone.
fn fm_harmonic(h: usize, carrier: i32, modulator: i32, index: f32) -> f32 {
    let mut total = 0.0f32;
    for k in -12..=12 {
        let partial = carrier + k * modulator;
        // A negative sideband folds back as a positive one with its phase
        // inverted, which is why through-zero FM has the spectrum it does.
        if partial.unsigned_abs() as usize == h && partial != 0 {
            let fold = if partial < 0 && k % 2 != 0 { -1.0 } else { 1.0 };
            total += fold * bessel_j(k, index);
        }
    }
    total
}

/// The three formants of the five cardinal vowels, in hertz, with the relative
/// level of each. Peterson & Barney's table, which is the one every vocoder,
/// formant filter and choir patch has used since 1952.
pub const VOWEL_FORMANTS: [[(f32, f32); 3]; 5] = [
    // /a/ as in "father"
    [(730.0, 1.0), (1090.0, 0.50), (2440.0, 0.25)],
    // /e/
    [(530.0, 1.0), (1840.0, 0.36), (2480.0, 0.25)],
    // /i/
    [(270.0, 1.0), (2290.0, 0.20), (3010.0, 0.16)],
    // /o/
    [(570.0, 1.0), (840.0, 0.50), (2410.0, 0.10)],
    // /u/
    [(300.0, 1.0), (870.0, 0.25), (2240.0, 0.05)],
];

/// The vowel's formant table at `t`, morphing A → E → I → O → U.
pub fn vowel_at(t: f32) -> [(f32, f32); 3] {
    let across = t.clamp(0.0, 1.0) * (VOWEL_FORMANTS.len() - 1) as f32;
    let low = across.floor() as usize;
    let high = (low + 1).min(VOWEL_FORMANTS.len() - 1);
    let blend = across - low as f32;
    std::array::from_fn(|i| {
        let (fa, ga) = VOWEL_FORMANTS[low][i];
        let (fb, gb) = VOWEL_FORMANTS[high][i];
        (fa + (fb - fa) * blend, ga + (gb - ga) * blend)
    })
}

/// A formant envelope over a harmonic series whose fundamental is `f0`.
fn formant_gain(h: usize, f0: f32, formants: &[(f32, f32); 3], bandwidth: f32) -> f32 {
    let hz = h as f32 * f0;
    let mut gain: f32 = 0.0;
    for (centre, level) in formants {
        // A resonance rather than a boxcar: a formant is a peak with skirts,
        // and a hard band would put a step in the spectrum where a voice has a
        // slope.
        let ratio = (hz - centre) / (bandwidth * centre / 700.0).max(30.0);
        gain = gain.max(level / (1.0 + ratio * ratio));
    }
    // The source is a glottal pulse, not a click: without the 1/h the formants
    // sit on white noise and the table sounds like a filter sweep on static.
    gain / (h as f32).sqrt()
}

fn recipe(id: WavetableId) -> Recipe {
    match id {
        // --- Basic ---
        WavetableId::Sine => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| if h == 1 { (1.0, 0.0) } else { (0.0, 0.0) }),
        },
        WavetableId::Triangle => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| {
                if h % 2 == 1 {
                    // 1/h², sign alternating: the triangle's own series.
                    let sign = if (h / 2) % 2 == 0 { 1.0 } else { -1.0 };
                    (sign / (h * h) as f32, 0.0)
                } else {
                    (0.0, 0.0)
                }
            }),
        },
        WavetableId::Saw => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| (1.0 / h as f32, 0.0)),
        },
        WavetableId::Square => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| {
                if h % 2 == 1 {
                    (1.0 / h as f32, 0.0)
                } else {
                    (0.0, 0.0)
                }
            }),
        },
        // Width 50 % at frame 0 down to 5 % at the last: a pulse train's
        // harmonic h is (2/πh)·sin(πhd), which is exactly the notch pattern
        // that makes a narrow pulse sound thin and nasal.
        WavetableId::Pulse => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                let duty = 0.5 - 0.45 * t;
                let h = h as f32;
                ((2.0 / (PI * h)) * (PI * h * duty).sin(), 0.0)
            }),
        },

        // --- Analog ---
        // Sine → triangle → saw → square across the frames: the four shapes an
        // analogue synth's waveform switch had, as a knob.
        WavetableId::AnalogMorph => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let stage = t * 3.0;
                let sine = if h == 1 { 1.0 } else { 0.0 };
                let triangle = if h % 2 == 1 {
                    let sign = if (h / 2) % 2 == 0 { 1.0 } else { -1.0 };
                    sign / (h * h) as f32
                } else {
                    0.0
                };
                let saw = 1.0 / h as f32;
                let square = if h % 2 == 1 { 1.0 / h as f32 } else { 0.0 };
                let table = [sine, triangle, saw, square];
                let low = (stage.floor() as usize).min(2);
                let blend = stage - low as f32;
                (table[low] + (table[low + 1] - table[low]) * blend, 0.0)
            }),
        },
        // The same pulse notches, but with the top rolled off and the Analog
        // family's alternating phases, so it reads as a circuit rather than as
        // arithmetic — and audibly softer than `Pulse` at the same duty.
        WavetableId::PwmSweep => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let duty = 0.5 - 0.4 * t;
                let hf = h as f32;
                let roll = 1.0 / (1.0 + (hf / 24.0).powi(2));
                let phase = if h % 2 == 0 { PI * 0.5 } else { 0.0 };
                ((2.0 / (PI * hf)) * (PI * hf * duty).sin() * roll, phase)
            }),
        },
        // A saw hard-synced at 1× to 6×, baked per frame — so a *static* sync
        // tone costs one table read instead of a second oscillator.
        WavetableId::SyncSweep => Recipe {
            frames: 32,
            shape: Shape::Exact(|p, t| {
                let ratio = 1.0 + 5.0 * t;
                2.0 * (p * ratio).fract() - 1.0
            }),
        },
        WavetableId::SubSaw => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| {
                // A saw with its top taken off: everything a sub needs and
                // none of the fizz that fights the layer above it.
                (1.0 / h as f32 * (-(h as f32) / 8.0).exp(), 0.0)
            }),
        },

        // --- Harmonic ---
        // The nine Hammond drawbars, faded in one at a time across the frames:
        // pulling the position knob up is pulling the bars out.
        WavetableId::Drawbar => Recipe {
            frames: 18,
            shape: Shape::Harmonics(|h, t| {
                const BARS: [usize; 9] = [1, 2, 3, 4, 6, 8, 10, 12, 16];
                let mut level = 0.0;
                for (index, bar) in BARS.iter().enumerate() {
                    if *bar != h {
                        continue;
                    }
                    // Each bar arrives over its own ninth of the sweep — and
                    // the **first is already out** at position 0, or the
                    // bottom of the knob would be silence rather than a
                    // fundamental, and the table's normalisation would then be
                    // set by its far end alone.
                    let arrival = index as f32 / BARS.len() as f32;
                    level += (1.0 + (t - arrival) * BARS.len() as f32).clamp(0.0, 1.0);
                }
                (level * 0.7, 0.0)
            }),
        },
        WavetableId::Odd => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                if h % 2 == 0 {
                    return (0.0, 0.0);
                }
                let hf = h as f32;
                (hf.powf(-(1.0 + 1.5 * t)), 0.0)
            }),
        },
        WavetableId::Even => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                if h != 1 && h % 2 == 1 {
                    return (0.0, 0.0);
                }
                let hf = h as f32;
                (hf.powf(-(1.0 + 1.5 * t)), 0.0)
            }),
        },
        // A saw whose harmonics are phase-scattered: the same spectrum, a
        // fatter waveform, and no two frames scattered the same way — which is
        // the ensemble a detuned stack has without the voices to pay for it.
        WavetableId::Sawstack => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                let spread = scatter(h, 0x51ed) * TAU * t;
                (1.0 / h as f32, spread)
            }),
        },
        WavetableId::BrightStack => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                let hf = h as f32;
                (hf.powf(-(0.55 + 0.6 * t)), scatter(h, 0x7c31) * TAU)
            }),
        },
        // Odd harmonics with a notch that moves: the woody, stopped-pipe tone,
        // and the one table here that sounds like an absence.
        WavetableId::Hollow => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                if h % 2 == 0 {
                    return (0.0, 0.0);
                }
                let hf = h as f32;
                let notch = (PI * hf / (4.0 + 8.0 * t)).cos().abs();
                (notch / hf, 0.0)
            }),
        },

        // A string **struck** an eighth of the way along, where a piano's
        // hammers land. The modes with a node under the hammer cannot be
        // excited, so the eighth partial and its multiples are silent and
        // the rest fall in lobes between them — a piano's comb, which a saw
        // does not have and which is why every synth piano built on a saw
        // sounds like a synth. The position is the **hammer's hardness**: a
        // felt hammer is soft, and how soft depends on how hard it was
        // thrown, so the partials above the lobe fall away at twenty-four
        // decibels an octave from the third partial at one end of the knob
        // and from the thirtieth at the other. Velocity is what moves it.
        WavetableId::Struck => Recipe {
            frames: 13,
            shape: Shape::Harmonics(|h, t| {
                let hf = h as f32;
                // **The roll-off is the knob**, and its exponent is what a
                // harder hammer changes. A struck string's displacement goes
                // as `sin(πhβ)/h²`; a hard hammer is in contact for less
                // time, puts more of its energy into the upper modes, and
                // tilts that exponent down. From `h^2.6` at one end to
                // `h^1.1` at the other, which is more than an octave of
                // centroid between a pianissimo and a fortissimo — and the
                // hard end is where a sampled grand's *bass* sits, its third
                // partial two decibels under its fundamental.
                //
                // It stays **above `h^0.9`** at every position, which is the
                // exponent at which the comb would put the second partial
                // level with the first: at `/h` this table was hollow — the
                // second partial measured three decibels *over* the
                // fundamental — and that spectrum is a clavinet's.
                let exponent = 2.6 - 1.5 * t;
                let comb = (PI * hf / 8.0).sin() / hf.powf(exponent);
                // And the felt itself, which cannot excite what it cannot
                // follow: a soft hammer is done by the sixth partial and a
                // hard one rings past the thirtieth.
                let corner = 3.0 * 2f32.powf(3.4 * t);
                let felt = 1.0 / (1.0 + (hf / corner).powi(4));
                (comb * felt, 0.0)
            }),
        },

        // --- Vocal ---
        WavetableId::Vowel => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| (formant_gain(h, 100.0, &vowel_at(t), 90.0), 0.0)),
        },
        // The same, with the partials scattered in phase and a breath band on
        // top: what turns one voice into several.
        WavetableId::Choir => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let voice = formant_gain(h, 110.0, &vowel_at(t * 0.6), 130.0);
                let breath = if h > 40 {
                    0.02 * scatter(h, 0x9a11) / (h as f32 / 40.0)
                } else {
                    0.0
                };
                (voice + breath, scatter(h, 0x2f7d) * TAU * t)
            }),
        },
        // One formant sweeping the whole band rather than five vowels: the
        // talk-box and the filter-sweep pad, without a filter.
        WavetableId::FormantSweep => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let centre = 200.0 * (30.0f32).powf(t);
                let formants = [(centre, 1.0), (centre * 2.2, 0.4), (centre * 3.6, 0.15)];
                (formant_gain(h, 100.0, &formants, 110.0), 0.0)
            }),
        },

        // --- Bell ---
        WavetableId::FmBell => Recipe {
            frames: 24,
            shape: Shape::Harmonics(|h, t| (fm_harmonic(h, 1, 7, t * 6.0), 0.0)),
        },
        WavetableId::Glass => Recipe {
            frames: 24,
            shape: Shape::Harmonics(|h, t| (fm_harmonic(h, 1, 11, t * 3.0), 0.0)),
        },
        // The DX e-piano: a 1:14 ratio at a low index, which is the tine and
        // nothing else. It is why "EP Tine" is a preset and not a sample.
        WavetableId::Tine => Recipe {
            frames: 24,
            shape: Shape::Harmonics(|h, t| (fm_harmonic(h, 1, 14, t * 1.6), 0.0)),
        },
        WavetableId::Gong => Recipe {
            frames: 24,
            shape: Shape::Harmonics(|h, t| {
                let index = 1.0 + t * 8.0;
                (
                    fm_harmonic(h, 1, 13, index) * 0.7 + fm_harmonic(h, 1, 5, index * 0.6) * 0.5,
                    scatter(h, 0x31c9) * TAU,
                )
            }),
        },

        // --- Digital ---
        // Quantised and folded shapes: harmonics that decay slower than 1/n,
        // which is what "digital" sounds like.
        WavetableId::Grit => Recipe {
            frames: 24,
            shape: Shape::Exact(|p, t| {
                let steps = (32.0 * (1.0f32 - t) + 3.0).round().max(2.0);
                let saw = 2.0 * p - 1.0;
                ((saw * 0.5 + 0.5) * steps).floor() / (steps - 1.0) * 2.0 - 1.0
            }),
        },
        WavetableId::Stairs => Recipe {
            frames: 24,
            shape: Shape::Exact(|p, t| {
                let steps = (24.0 * (1.0f32 - t) + 3.0).round().max(2.0);
                let sine = (TAU * p).sin();
                ((sine * 0.5 + 0.5) * steps).round() / steps * 2.0 - 1.0
            }),
        },
        // A wavefolder: past full scale the wave turns back on itself, adding
        // harmonics that no filter can take out again.
        WavetableId::Crunch => Recipe {
            frames: 24,
            shape: Shape::Exact(|p, t| {
                let drive = 1.0 + t * 6.0;
                let x = (TAU * p).sin() * drive;
                // Triangle folding: the closed form of "reflect at ±1".
                (x * PI * 0.5).sin()
            }),
        },
        WavetableId::Bitwave => Recipe {
            frames: 16,
            shape: Shape::Harmonics(|h, t| {
                let hf = h as f32;
                let sign = if scatter(h, 0x4e2b) > 0.5 { 1.0 } else { -1.0 };
                (sign * hf.powf(-(0.5 + 0.5 * t)) * 0.6, 0.0)
            }),
        },

        // --- Modern ---
        // A saw with a resonant growl that moves: the vowel table's mechanism
        // with none of its manners.
        WavetableId::Growl => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let centre = 150.0 + 1400.0 * t;
                let formants = [(centre, 1.0), (centre * 1.6, 0.7), (centre * 4.0, 0.35)];
                let voiced = formant_gain(h, 90.0, &formants, 60.0);
                let body = 0.35 / h as f32;
                (voiced + body, 0.0)
            }),
        },
        // Two saws beating against each other, the beat captured across the
        // frames: sweeping the position *is* the phasing, at no extra cost.
        WavetableId::Reese => Recipe {
            frames: 32,
            shape: Shape::Exact(|p, t| {
                let a = 2.0 * p - 1.0;
                let b = 2.0 * (p + t * 0.5).fract() - 1.0;
                (a + b) * 0.5
            }),
        },
        // A saw, a PWM'd square and an octave up, the three parts of the sound
        // every rave record of 1991 is made of.
        WavetableId::Hoover => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let hf = h as f32;
                let saw = 1.0 / hf;
                let duty = 0.5 - 0.35 * t;
                let pwm = (2.0 / (PI * hf)) * (PI * hf * duty).sin();
                let octave = if h % 2 == 0 { 0.5 / hf } else { 0.0 };
                (saw * 0.6 + pwm * 0.5 + octave, scatter(h, 0x6d13) * TAU * t)
            }),
        },
        WavetableId::Wide => Recipe {
            frames: 32,
            shape: Shape::Harmonics(|h, t| {
                let hf = h as f32;
                // Every harmonic somewhere else in the cycle: the widest a
                // single oscillator gets before unison is involved.
                (hf.powf(-0.8), scatter(h, 0xa731) * TAU * (0.2 + t))
            }),
        },

        // --- Sub ---
        // A sine with a whisper of second harmonic — the asymmetry a sub
        // oscillator picks up going through anything at all. It is a sine to
        // listen to and it is *not* `Sine`: a table that is bit-identical to
        // another table is one table with two names, which is the mistake the
        // drum kits taught (`drum-kit-axes`).
        WavetableId::SubSine => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| match h {
                1 => (1.0, 0.0),
                // −22 dB of second harmonic: the asymmetry a sub oscillator
                // picks up going through anything at all. Enough to be the
                // warmth it is for, and enough that this is measurably not
                // `Sine` — a table bit-identical to another table is one
                // table with two names.
                2 => (0.08, 0.0),
                _ => (0.0, 0.0),
            }),
        },
        // A triangle with the same warmth on it as `SubSine`, and for the same
        // reason: a triangle with its top nine harmonics kept is a `Triangle`
        // to within a fiftieth of full scale, and two names for one table is
        // the mistake the drum kits taught.
        WavetableId::SubTri => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| match h {
                2 => (0.06, 0.0),
                4 => (0.02, 0.0),
                h if h % 2 == 1 && h <= 9 => {
                    let sign = if (h / 2) % 2 == 0 { 1.0 } else { -1.0 };
                    (sign / (h * h) as f32, 0.0)
                }
                _ => (0.0, 0.0),
            }),
        },
        WavetableId::SubSquare => Recipe {
            frames: 1,
            shape: Shape::Harmonics(|h, _| {
                // Only the first few odd partials: a sub that keeps its edge
                // without putting anything in the way of the layer above it.
                if h % 2 == 1 && h <= 7 {
                    (1.0 / h as f32, 0.0)
                } else {
                    (0.0, 0.0)
                }
            }),
        },

        // --- Chip ---
        // The exact duty cycles, sampled rather than synthesised: these steps
        // *are* the sound, and band-limiting one into existence would be the
        // approximation. Level 0 is the hardware; the coarser levels are what
        // keeps it from aliasing at the top of the keyboard.
        WavetableId::NesPulse125 => Recipe {
            frames: 1,
            shape: Shape::Exact(|p, _| if p < 0.125 { 1.0 } else { -1.0 }),
        },
        WavetableId::NesPulse25 => Recipe {
            frames: 1,
            shape: Shape::Exact(|p, _| if p < 0.25 { 1.0 } else { -1.0 }),
        },
        WavetableId::NesPulse50 => Recipe {
            frames: 1,
            shape: Shape::Exact(|p, _| if p < 0.5 { 1.0 } else { -1.0 }),
        },
        // A four-bit DAC counting 0..15 and back down: thirty-two steps,
        // sixteen levels, and the reason an NES bass line sounds like one.
        WavetableId::NesTriangle => Recipe {
            frames: 1,
            shape: Shape::Exact(|p, _| {
                let step = (p * 32.0).floor() as i32 % 32;
                let level = if step < 16 { step } else { 31 - step };
                level as f32 / 15.0 * 2.0 - 1.0
            }),
        },
        // The Game Boy's wave channel: 32 nibbles of user-defined waveform.
        // This is one of the shapes its games actually shipped — a soft,
        // asymmetric pulse rather than another square.
        WavetableId::GameBoy => Recipe {
            frames: 8,
            shape: Shape::Exact(|p, t| {
                const WAVE: [i32; 32] = [
                    8, 11, 13, 15, 15, 14, 13, 11, 9, 7, 5, 3, 2, 1, 1, 2, 3, 4, 6, 8, 10, 12, 13,
                    14, 14, 13, 11, 9, 7, 5, 3, 1,
                ];
                let step = (p * 32.0).floor() as usize % 32;
                // The frames rotate the wave, which is what a game did to it
                // between one note and the next.
                let rotated = (step + (t * 31.0).round() as usize) % 32;
                WAVE[rotated] as f32 / 7.5 - 1.0
            }),
        },
        // The SID's pulse-and-saw mix, its two oscillator shapes combined the
        // way the chip's ring of registers let you.
        WavetableId::C64 => Recipe {
            frames: 16,
            shape: Shape::Exact(|p, t| {
                let saw = 2.0 * p - 1.0;
                let duty = 0.5 - 0.4 * t;
                let pulse = if p < duty { 1.0 } else { -1.0 };
                // The SID quantised to 12 bits at 8-bit-ish steps; the grain
                // is what makes it a C64 and not a modular.
                let mixed = saw * 0.5 + pulse * 0.5;
                (mixed * 16.0).round() / 16.0
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bessel_series_matches_its_known_values() {
        // Abramowitz & Stegun, table 9.1: J0(1) = 0.7651977, J1(1) = 0.4400506.
        assert!((bessel_j(0, 1.0) - 0.765_197_7).abs() < 1e-4);
        assert!((bessel_j(1, 1.0) - 0.440_050_6).abs() < 1e-4);
        assert!((bessel_j(2, 2.0) - 0.352_834).abs() < 1e-4);
        // J_-k(x) = (-1)^k J_k(x).
        assert!((bessel_j(-1, 1.0) + bessel_j(1, 1.0)).abs() < 1e-6);
        assert!((bessel_j(-2, 2.0) - bessel_j(2, 2.0)).abs() < 1e-6);
    }

    #[test]
    fn the_inverse_transform_undoes_the_forward_one() {
        let original: Vec<f32> = (0..64).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut re = original.clone();
        let mut im = vec![0.0f32; 64];
        crate::fft_in_place(&mut re, &mut im);
        inverse(&mut re, &mut im);
        for (a, b) in original.iter().zip(&re) {
            assert!((a - b).abs() < 1e-4, "round trip: {a} vs {b}");
        }
    }

    #[test]
    fn a_single_frame_table_ignores_its_position() {
        let bank = WavetableBank::new();
        let table = bank.get(WavetableId::Sine);
        assert_eq!(table.frame_count(), 1);
        for position in [0.0f32, 0.5, 1.0] {
            // A quarter of the way through a sine's cycle is its peak, and a
            // single-frame table has to give the same answer wherever the
            // position knob is.
            assert!(
                (table.read(position, 0.25, 0) - 0.9).abs() < 0.01,
                "at position {position}: {}",
                table.read(position, 0.25, 0)
            );
            // And it starts at a zero crossing, which is what stops a note-on
            // being a click.
            assert!(table.read(position, 0.0, 0).abs() < 0.01);
        }
    }
}
