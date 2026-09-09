//! Modal resonance: what a struck object actually does.
//!
//! A drum is not a sine with an envelope on it. Strike a membrane and it rings
//! at a set of frequencies decided by its shape — for an ideal circular head,
//! ratios of 1.000, 1.593, 2.135, 2.295, 2.917 and up — each with its own
//! decay, the higher ones dying first because the air and the head take the
//! high modes away fastest. Those ratios are **inharmonic**: they are not
//! multiples of the lowest, so the ear never fuses them into a pitch, and that
//! is precisely the difference between "a drum" and "a beep at 120 Hz".
//!
//! > *"the sounds in it still sound way too synthesized and not realistic"*
//!
//! One oscillator through one envelope cannot get there from any setting,
//! because what is missing is not a number but the **modes**. This is them: a
//! small bank of two-pole resonators, struck by whatever excitation the voice
//! feeds in — an impulse for a stick, a burst of noise for a beater, the noise
//! source itself for a snare's wires.
//!
//! Cheap on purpose. Each mode is two multiplies and two adds a sample, the
//! whole bank is [`MAX_MODES`] of them, and the struct is `Copy` so a drum
//! voice can hold one and still be handed back to a pool by being overwritten.

/// How many modes a bank holds.
///
/// Six: enough for a membrane's first five plus a shell ring, and few enough
/// that a kit playing eight voices at once costs forty-eight resonators, which
/// is nothing beside the filters already in the chain. Past six the ear stops
/// hearing individual modes and starts hearing noise, and noise is what the
/// noise source is for.
pub const MAX_MODES: usize = 6;

/// One mode: where it sits, how long it rings, how loud it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModalMode {
    /// Where it sits, as a multiple of the bank's base frequency.
    ///
    /// A **ratio** rather than a frequency so that a tom tuned down keeps its
    /// character: the shape of a drum is the set of ratios, and the tuning is
    /// one number in front of them.
    pub ratio: f32,
    /// How long it rings, as a multiple of the bank's decay.
    ///
    /// Under one for the high modes, which is what makes a struck object grow
    /// duller as it rings rather than fading evenly.
    pub decay: f32,
    /// How hard it was struck, against the others.
    pub gain: f32,
}

/// A bank of struck resonators.
#[derive(Debug, Clone, Copy)]
pub struct ModalBank {
    /// Per mode: the two feedback coefficients, the input gain, and the two
    /// samples of state.
    a1: [f32; MAX_MODES],
    a2: [f32; MAX_MODES],
    gain: [f32; MAX_MODES],
    z1: [f32; MAX_MODES],
    z2: [f32; MAX_MODES],
    used: usize,
}

impl Default for ModalBank {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalBank {
    /// A bank with nothing in it. **Silent** until [`set`](Self::set) is
    /// called, because a voice comes out of a pool and one that rang before
    /// anybody struck it would be a drum machine that plays itself.
    pub fn new() -> Self {
        Self {
            a1: [0.0; MAX_MODES],
            a2: [0.0; MAX_MODES],
            gain: [0.0; MAX_MODES],
            z1: [0.0; MAX_MODES],
            z2: [0.0; MAX_MODES],
            used: 0,
        }
    }

    /// Tunes the bank: these modes, over this base frequency, ringing for this
    /// long, at this sample rate.
    ///
    /// Everything expensive — two transcendentals a mode — happens here rather
    /// than per sample, which is the same bargain every envelope in this crate
    /// makes.
    ///
    /// **Every input is clamped**, because a drum voice comes off a project
    /// file somebody may have edited by hand and a pole outside the unit
    /// circle does not fade: it grows, through the layer, the track and the
    /// master. The clamps are the reason
    /// `every_mode_is_stable_however_it_is_asked_for` can be a test rather
    /// than a hope.
    pub fn set(&mut self, modes: &[ModalMode], base_hz: f32, decay_s: f32, sample_rate: f32) {
        *self = Self::new();
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            return;
        };
        let base = if base_hz.is_finite() {
            base_hz.clamp(10.0, 18_000.0)
        } else {
            return;
        };
        let decay = if decay_s.is_finite() {
            decay_s.clamp(0.002, 20.0)
        } else {
            0.2
        };
        // Nyquist with a margin: a resonator asked to sit exactly at half the
        // rate is a resonator whose cosine is −1 and whose ring is a sample
        // alternating in sign, which is not a mode of anything.
        let ceiling = sr * 0.45;
        for (at, mode) in modes.iter().take(MAX_MODES).enumerate() {
            let ratio = if mode.ratio.is_finite() {
                mode.ratio.clamp(0.05, 200.0)
            } else {
                1.0
            };
            let hz = base * ratio;
            if hz > ceiling {
                // Above the band, and left out rather than folded down: a mode
                // that aliased would be a frequency the drum does not have.
                continue;
            }
            let scale = if mode.decay.is_finite() {
                mode.decay.clamp(0.02, 4.0)
            } else {
                1.0
            };
            let seconds = (decay * scale).clamp(0.002, 20.0);
            // The pole radius that takes the ring down sixty decibels over
            // `seconds`, which is the same reading of a decay time the rest of
            // the drum synth uses.
            let samples = (seconds * sr).max(1.0);
            let r = 10f32.powf(-3.0 / samples).clamp(0.0, 0.999_995);
            let w = std::f32::consts::TAU * hz / sr;
            self.a1[at] = 2.0 * r * w.cos();
            self.a2[at] = -(r * r);
            // Scaled by `(1 − r)` so a long mode and a short one are struck to
            // the same height: without it a mode that rings for four seconds
            // arrives a hundred times louder than one that rings for forty
            // milliseconds, and the bank's balance would be a function of its
            // decay rather than of its gains.
            let strength = if mode.gain.is_finite() {
                mode.gain.clamp(0.0, 4.0)
            } else {
                1.0
            };
            self.gain[at] = strength * (1.0 - r);
            self.used = at + 1;
        }
    }

    /// One sample of excitation in, one sample of ring out.
    #[inline]
    pub fn next_sample(&mut self, excite: f32) -> f32 {
        if self.used == 0 || !excite.is_finite() {
            return 0.0;
        }
        let mut sum = 0.0;
        for at in 0..self.used {
            let y = self.gain[at] * excite + self.a1[at] * self.z1[at] + self.a2[at] * self.z2[at];
            // The state is what feeds back, so it is what has to be caught:
            // one non-finite sample here would poison every sample after it.
            let y = if y.is_finite() {
                y.clamp(-8.0, 8.0)
            } else {
                0.0
            };
            self.z2[at] = self.z1[at];
            self.z1[at] = y;
            sum += y;
        }
        sum
    }
}
