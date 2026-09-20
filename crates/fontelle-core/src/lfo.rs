//! One voice's LFO (`docs/flopsynth-plan.md` §3.5).
//!
//! # Why this is in `fontelle-core` and not in `fontelle-dsp`
//!
//! Because its shape is [`fontelle_types::LfoWave`] — **the same six the
//! Filter insert offers**, so that the chooser names its positions once
//! (catalogue rule 3) and the shape the window draws is the shape the voice
//! plays. `fontelle-dsp` sees nothing but numbers (INVARIANT 4) and may not
//! depend on `fontelle-types`; `fontelle-core` depends on both. So the state
//! lives here, next to the [`crate::patch::Lfo`] it reads.
//!
//! # Why it needs state at all
//!
//! [`LfoWave::SampleHold`](fontelle_types::LfoWave::SampleHold) has no closed
//! form — its value is a *memory*, a number drawn once per cycle and held —
//! and [`LfoMode::Free`](crate::patch::LfoMode) needs none at all, because its
//! phase comes off the transport. Between the two of them, an `Oscillator`
//! was no longer enough.

use fontelle_types::LfoWave;

use crate::patch::{Lfo, LfoMode};

/// An LFO's depth and phase as the voice plays them this block: the
/// config's, moved by whatever the matrix routes to them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LfoLive {
    pub depth: f32,
    pub phase: f32,
}

/// One LFO's per-voice memory. `Copy`, fixed-size, allocation-free.
#[derive(Debug, Clone, Copy)]
pub struct LfoState {
    phase: f32,
    /// Sample & hold's current value, redrawn when the phase wraps.
    held: f32,
    rng: u32,
    /// The one-pole that `smooth` runs the output through.
    smoothed: f32,
    /// Whether a one-shot has finished its cycle and is holding.
    finished: bool,
    /// Whether anything has been drawn yet — so the first block of a sample &
    /// hold has a value rather than a zero.
    started: bool,
}

impl Default for LfoState {
    fn default() -> Self {
        Self {
            phase: 0.0,
            held: 0.0,
            // Any non-zero seed; xorshift is stuck at zero forever.
            rng: 0x2545_f491,
            smoothed: 0.0,
            finished: false,
            started: false,
        }
    }
}

impl LfoState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where in its cycle this LFO is, 0..1.
    ///
    /// For the **dot on the picture** (`docs/flopsynth-plan.md` §11, phase 6):
    /// a shape with nothing moving on it says what the LFO is, and a dot going
    /// round says what it is doing.
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Starts a note.
    ///
    /// `seed` is the voice's own age counter, so two notes draw different
    /// sample & hold sequences and the same note draws the same one twice —
    /// which is what lets a test measure it.
    pub fn reset(&mut self, lfo: &Lfo, seed: u32) {
        self.phase = lfo.phase.rem_euclid(1.0);
        self.rng = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x85eb_ca6b) | 1;
        self.held = 0.0;
        self.smoothed = 0.0;
        self.finished = false;
        self.started = false;
    }

    /// How fast this LFO runs, in cycles per second.
    ///
    /// A synced LFO's rate is a fact about the **tempo**, so it is computed
    /// here from the clock rather than stored: a project whose tempo changes
    /// has an LFO that changes with it, without anything having to rewrite the
    /// patch.
    pub fn rate_hz(lfo: &Lfo, bpm: f32) -> f32 {
        if lfo.sync {
            let beats = lfo.division.beats().max(1e-4);
            (bpm.max(1.0) / 60.0) / beats
        } else {
            lfo.rate_hz
        }
    }

    /// The value at the **start** of this block, then advances by the whole
    /// block.
    ///
    /// Block rate, like every other modulation source: at the engine's
    /// 128-frame blocks that is 375 Hz, which is the same order as every
    /// hardware sampler ever shipped.
    ///
    /// `age_samples` is how long the note has been sounding, for the delay and
    /// the fade. `clock_phase` is where a [`LfoMode::Free`] LFO is — computed
    /// from the transport by the caller, because the voice does not have one
    /// and the sampler does.
    ///
    /// `live` is the depth and the phase **after the matrix**: the two of
    /// the LFO's own settings a route can move, handed over beside the
    /// config rather than written into a copy of it — an `Lfo` carries a
    /// drawn shape now, and a copy of one on the audio thread would be an
    /// allocation (INVARIANT 1).
    // Eight: the config, the two live values, and five facts about the
    // block. The first two are one thing to the caller and two here.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_block(
        &mut self,
        lfo: &Lfo,
        live: LfoLive,
        rate_hz: f32,
        sample_rate: f32,
        frames: usize,
        age_samples: u64,
        clock_phase: Option<f32>,
    ) -> f32 {
        if sample_rate <= 0.0 {
            return 0.0;
        }
        // Free runs off the clock, so every voice reads the same value and the
        // same bar sounds the same every time it plays — at no cost in shared
        // state, because the clock *is* the state.
        if lfo.mode == LfoMode::Free
            && let Some(clock) = clock_phase
        {
            self.phase = (clock + live.phase).rem_euclid(1.0);
        }

        // A drawn shape plays in place of the wave when there is one
        // (`docs/flopsynth-next.md` §3.4, read since phase 3): the same
        // `LfoShape::value` the editor draws, so the picture and the voice
        // cannot disagree. Sixty-four points at most, walked once a step,
        // which is nothing beside the table read the step pays for.
        let raw = match &lfo.shape {
            Some(shape) => shape.value(self.phase),
            None => self.value_at_phase(lfo.wave),
        };

        // The delay holds the LFO at rest; the fade brings the depth in after
        // it. Both are why a vibrato on an acoustic imitation arrives late
        // rather than on the first sample, which is the tell §7.3 names.
        let delay_samples = (lfo.delay_s.max(0.0) * sample_rate) as u64;
        let level = if age_samples < delay_samples {
            0.0
        } else if lfo.fade_s > 0.0 {
            let since = (age_samples - delay_samples) as f32 / sample_rate;
            (since / lfo.fade_s).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // The smoothing is applied to the *shape* and not to the depth, so
        // rounding off a square's corners does not also round off the fade.
        let smooth = lfo.smooth.clamp(0.0, 1.0);
        let value = if smooth > 0.0 {
            // A one-pole whose time constant grows with the knob, worked out
            // per call because that is the rate this runs at. The exponent
            // is `frames / 512`, which is what the knob sounded like at the
            // engine's 128-frame blocks (`(smooth·0.995)^(1/4)` a block)
            // from the day it existed: the voice calls this every eight
            // samples now (`voice::MOD_STEP`), and the old `32 / frames`
            // made the time constant a property of the block size — the
            // bank's presets were voiced at 128 and the gate rendered at
            // 512, and neither heard the same knob.
            let coefficient = (smooth * 0.995).powf(frames.max(1) as f32 / 512.0);
            self.smoothed += (raw - self.smoothed) * (1.0 - coefficient);
            self.smoothed
        } else {
            self.smoothed = raw;
            raw
        };

        // Advance, unless the clock is driving it or a one-shot has finished.
        if lfo.mode != LfoMode::Free && !self.finished {
            let advanced = self.phase + rate_hz / sample_rate * frames as f32;
            if advanced >= 1.0 {
                if lfo.mode == LfoMode::OneShot {
                    // Holds its last value at the end of the first cycle: an
                    // envelope with a shape chooser on it.
                    self.finished = true;
                    self.phase = 1.0 - f32::EPSILON;
                } else {
                    self.phase = advanced.rem_euclid(1.0);
                    // A new random number per cycle is what sample & hold is.
                    self.held = self.next_random();
                }
            } else {
                self.phase = advanced;
            }
        }

        value * live.depth * level
    }

    fn value_at_phase(&mut self, wave: LfoWave) -> f32 {
        match wave {
            // `LfoWave::value` returns zero for sample & hold, because its
            // value is a memory and a closed form cannot have one. This is
            // that memory.
            LfoWave::SampleHold => {
                if !self.started {
                    self.started = true;
                    self.held = self.next_random();
                }
                self.held
            }
            other => other.value(self.phase),
        }
    }

    fn next_random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / 8_388_608.0 - 1.0
    }
}

/// Where a [`LfoMode::Free`] LFO is, from the transport.
///
/// `(position / sample_rate · rate) mod 1`. A pure function of the clock, so
/// every voice on the channel reads the same number and none of them has to
/// remember anything.
pub fn free_phase(position_sample: u64, sample_rate: f32, rate_hz: f32) -> f32 {
    if sample_rate <= 0.0 {
        return 0.0;
    }
    // In `f64` before the modulo: at 48 kHz a `f32` position loses whole
    // samples after about three minutes, and an LFO that drifted out of step
    // with the transport after the third chorus would be a bug nobody could
    // reproduce on a short loop.
    let seconds = position_sample as f64 / f64::from(sample_rate);
    (seconds * f64::from(rate_hz)).rem_euclid(1.0) as f32
}
