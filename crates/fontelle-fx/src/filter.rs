//! The synthesiser filter as an insert (`docs/effects-catalogue.md` §2.2).
//! Its parameters are `fontelle_types::FilterConfig` — the document owns
//! those, this owns the two sections and the two things that move them.
//!
//! # The path
//!
//! ```text
//!                 ┌─ follower on the input ─┐
//!                 │                          ↓
//! in → drive → the filter, at cutoff × 2^(env + lfo) → × output → out
//!                                            ↑
//!                 └────────── LFO ───────────┘
//! ```
//!
//! # What makes this more than an EQ band
//!
//! The corner **moves**, and the two things that move it are the two things a
//! synthesiser has: a follower on what is arriving, which is an auto-wah, and
//! an LFO that follows the song, which is everything from a slow open to a
//! rhythmic gate. An EQ band with a bell at 800 Hz is a decision; this is a
//! decision that changes with the music, and the whole design is about
//! keeping that decision cheap enough to make per sample.
//!
//! The SVF is already zero-delay-feedback, which is exactly what a filter
//! being modulated per sample needs: its coefficients can change between one
//! sample and the next without the ringing a naive biquad produces. So the
//! coefficients are rebuilt per sample **when something is moving them** and
//! once per block when nothing is — a filter with its modulation at zero
//! costs what an EQ band costs, which is what makes it reasonable to reach
//! for one on every bus.
//!
//! # Where the drive is, and why
//!
//! Before the filter. That is where a synthesiser puts it and the only place
//! it sounds like one: the drive makes harmonics, and then the filter sweeps
//! through what the drive made. After the filter it would be a distortion
//! with a tone control in front of it — a different effect, and one this
//! program already has.

use fontelle_dsp::{SvfCoeffs, SvfFilter, SvfMode};
use fontelle_types::{
    FILTER_MOD_OCTAVES, FilterConfig, FilterShape, LfoWave, MAX_FILTER_HZ, MIN_FILTER_HZ,
};

const MAX_CHANNELS: usize = 2;

/// Two two-pole sections is the deepest shape here — 24 dB an octave.
const SECTIONS: usize = 2;

/// The Q of a section that is not carrying the resonance: Butterworth, so a
/// 24 dB slope with the knob at zero is flat to its corner rather than
/// sagging into it.
const FLAT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The Q the resonance knob reaches at the top. Twelve is a filter that
/// sings; past it the peak is louder than the music and the drive is what
/// the knob beside it is for.
const MAX_Q: f32 = 12.0;

/// How far up [`FilterShape::Peak`] lifts its corner at full resonance.
const MAX_PEAK_DB: f32 = 24.0;

/// A resonant multimode filter with an envelope follower and an LFO
/// (`docs/effects-catalogue.md` §2.2).
pub struct Filter {
    /// Two sections per channel; the second is unused by the 12 dB shapes.
    sections: [[SvfFilter; SECTIONS]; MAX_CHANNELS],
    /// The follower on the input. **Stereo-linked**, like every other
    /// detector in this crate: a corner that moved independently per side
    /// would make the image swim.
    envelope: f32,
    /// The LFO's phase, 0..1. `f64` for the reason the chorus's is.
    phase: f64,
    /// What sample-and-hold is holding, and whether it has been given
    /// anything yet.
    held: f32,
    /// The sample-and-hold generator's state: a plain xorshift, because this
    /// is the audio thread and it wants uncorrelated numbers rather than good
    /// ones.
    noise: u32,
    sample_rate: f32,
}

impl Filter {
    pub fn new() -> Self {
        Self {
            sections: Default::default(),
            envelope: 0.0,
            phase: 0.0,
            held: 0.0,
            // Any non-zero seed; xorshift is stuck at zero.
            noise: 0x9E37_79B9,
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        for channel in &mut self.sections {
            for section in channel {
                section.reset();
            }
        }
        self.envelope = 0.0;
        self.phase = 0.0;
        self.held = 0.0;
    }

    /// Runs `channels` through the filter in place.
    ///
    /// `bpm` is the tempo where this block sits, for an LFO set in note
    /// values — the same arrangement the delay and the chorus have.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &FilterConfig, bpm: f32) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let rate = self.sample_rate;

        let shape = config.shape;
        let sections = shape.sections();
        let resonance = config.resonance.clamp(0.0, 1.0);
        let drive = config.drive.clamp(0.0, 1.0);
        let output = 10f32.powf(config.output_db.clamp(-60.0, 24.0) / 20.0);
        let cutoff = config.cutoff_hz.clamp(MIN_FILTER_HZ, MAX_FILTER_HZ);

        // Q rises geometrically with the knob, because Q is a ratio: the step
        // from 1 to 2 is the same audible move as the one from 6 to 12.
        let q = FLAT_Q * (MAX_Q / FLAT_Q).powf(resonance);
        let peak_db = resonance * MAX_PEAK_DB;

        let env_amount = config.env_amount.clamp(-1.0, 1.0);
        let env_attack = coefficient(config.env_attack_ms, rate);
        let env_release = coefficient(config.env_release_ms, rate);
        let lfo_amount = config.lfo_amount.clamp(0.0, 1.0);
        let lfo_step = f64::from(config.effective_lfo_hz(bpm)) / f64::from(rate);
        let wave = config.lfo_wave;

        // Whether anything is moving the corner. When nothing is, the
        // coefficients are built once for the whole block and this costs what
        // an EQ band costs — see the module doc.
        let modulating = env_amount != 0.0 || lfo_amount != 0.0;
        let mut coeffs = self.coeffs(shape, cutoff, q, peak_db);

        for frame in 0..frames {
            if modulating {
                // The follower, on the loudest side — stereo-linked, so the
                // two channels stay in the same place.
                let mut peak = 0.0f32;
                for channel in 0..used {
                    peak = peak.max(channels[channel][frame].abs());
                }
                let coeff = if peak > self.envelope {
                    env_attack
                } else {
                    env_release
                };
                self.envelope += coeff * (peak - self.envelope);

                let lfo = match wave {
                    LfoWave::SampleHold => self.held,
                    _ => wave.value(self.phase as f32),
                };
                // Both amounts are in octaves, and they add: the LFO sweeps
                // around wherever the envelope has left the corner, which is
                // what a synthesiser does and what makes the two knobs worth
                // having together.
                let octaves = FILTER_MOD_OCTAVES
                    * (env_amount * self.envelope.clamp(0.0, 1.0) + lfo_amount * lfo);
                let moved = (cutoff * 2f32.powf(octaves)).clamp(MIN_FILTER_HZ, MAX_FILTER_HZ);
                coeffs = self.coeffs(shape, moved, q, peak_db);
            }

            for channel in 0..used {
                // Into the drive first: the filter then sweeps through the
                // harmonics the drive made, which is the order a synthesiser
                // has and the reason this is not a distortion with a tone
                // control.
                let mut sample = saturate(channels[channel][frame], drive);
                for section in 0..sections {
                    sample = self.sections[channel][section].process(sample, &coeffs[section]);
                }
                channels[channel][frame] = sample * output;
            }
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }

            if modulating {
                let before = self.phase;
                self.phase = (self.phase + lfo_step).fract();
                // A new random value at the top of each cycle, held until the
                // next one. The wrap is the edge.
                if wave == LfoWave::SampleHold && self.phase < before {
                    self.held = self.uniform();
                }
            }
        }
    }

    /// The one or two sections this shape is built from, at this corner.
    ///
    /// The resonance goes on the **last** section and the first is left flat,
    /// which is what puts one peak at the corner rather than two stacked
    /// peaks either side of it — the difference between a filter that sings
    /// and one that honks.
    fn coeffs(
        &self,
        shape: FilterShape,
        cutoff: f32,
        q: f32,
        peak_db: f32,
    ) -> [SvfCoeffs; SECTIONS] {
        let rate = self.sample_rate;
        let mode = match shape {
            FilterShape::LowPass12 | FilterShape::LowPass24 => SvfMode::Lowpass,
            FilterShape::HighPass12 | FilterShape::HighPass24 => SvfMode::Highpass,
            FilterShape::BandPass12 | FilterShape::BandPass24 => SvfMode::Bandpass,
            FilterShape::Notch => SvfMode::Notch,
            FilterShape::Peak => SvfMode::Bell,
        };
        let (first_q, last_q, gain) = match shape {
            // A bell's height *is* its gain, so the resonance knob becomes
            // the lift rather than the Q — see `FilterShape::Peak`.
            FilterShape::Peak => (FLAT_Q, FLAT_Q, peak_db),
            _ => (FLAT_Q, q, 0.0),
        };
        if shape.sections() == 1 {
            let one = normalise(SvfFilter::coeffs(mode, cutoff, last_q, gain, rate), mode);
            [one, one]
        } else {
            [
                normalise(SvfFilter::coeffs(mode, cutoff, first_q, gain, rate), mode),
                normalise(SvfFilter::coeffs(mode, cutoff, last_q, gain, rate), mode),
            ]
        }
    }

    fn uniform(&mut self) -> f32 {
        // xorshift32.
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self::new()
    }
}

/// A band-pass at unity in its own centre, and everything else untouched.
///
/// The SVF's plain band-pass output peaks at Q, which `eq.rs` calls "the right
/// one for a voice's resonant filter, where the resonance is meant to be
/// loud". This *is* that filter — and it is also an insert on a mix bus, where
/// a resonance knob that put twenty-one decibels of gain on a sweep would be a
/// knob nobody could turn past a third. So the same scaling the EQ uses, at
/// the same cost of one multiply when the coefficients are built: the
/// resonance still narrows the band, which is the wah, and it no longer also
/// raises it.
fn normalise(mut coeffs: SvfCoeffs, mode: SvfMode) -> SvfCoeffs {
    if mode == SvfMode::Bandpass {
        coeffs.m1 *= coeffs.k;
    }
    coeffs
}

/// A one-pole smoother's coefficient for a time constant in milliseconds.
fn coefficient(ms: f32, sample_rate: f32) -> f32 {
    let samples = (ms.max(0.001) / 1000.0) * sample_rate;
    if samples <= 1.0 {
        1.0
    } else {
        1.0 - (-1.0 / samples).exp()
    }
}

/// Soft saturation, blended in by `drive` so that zero drive is **exactly** a
/// wire rather than nearly one — the delay's, and for the same reason.
fn saturate(sample: f32, drive: f32) -> f32 {
    if drive <= 0.0 {
        return sample;
    }
    let k = 1.0 + drive * 8.0;
    let shaped = (k * sample).tanh() / k.tanh();
    sample + (shaped - sample) * drive
}
