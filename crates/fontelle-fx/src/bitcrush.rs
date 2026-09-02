//! The bitcrusher's DSP. Its parameters are `fontelle_types::BitcrushConfig` —
//! the document owns those, this owns the held sample.
//!
//! Two independent destructions, and keeping them independent is the point:
//! **bit depth** quantises the amplitude and **rate** quantises time. They are
//! often shipped as one "crush" knob, and then neither can be had on its own —
//! a 12-bit sample at full rate and a 16-bit one held at 8 kHz are different
//! sounds with different uses.
//!
//! # The path
//!
//! ```text
//! in → × input → (anti-alias) → rails → dither → quantiser ─┐
//!                                                  held ←────┘
//!     out ← × output ← post filter ← hold / ramp / drop ← held
//! ```
//!
//! What makes one lo-fi machine sound unlike another is not the bit count.
//! It is **how** a value lands on the grid — to nearest, toward zero, or on a
//! logarithmic grid — **how** the output behaves between takes — held flat,
//! ramped, or dropped — and how loud the signal is when it gets there. Each
//! of those is a chooser or a knob here, and `tests/bitcrush.rs` measures
//! each by the thing that distinguishes it.

use fontelle_dsp::{SvfCoeffs, SvfFilter, SvfMode};
use fontelle_types::{BitcrushConfig, Decimation, Dither, MAX_BITS, Quantiser};

const MAX_CHANNELS: usize = 2;

/// µ-law's companding constant: the one telephony standardised on, and the
/// one that gives eight bits the dynamic range of about thirteen.
const MU: f32 = 255.0;

/// The top of the post-filter knob, which is where "off" lives.
const POST_OPEN_HZ: f32 = 19_999.0;

/// Bit-depth quantisation and sample-and-hold decimation (TDD §13.4,
/// `docs/effects-catalogue.md` §3.2).
pub struct Bitcrush {
    /// What the sample-and-hold is holding, per channel.
    held: [f32; MAX_CHANNELS],
    /// What it held before that — the start of the ramp, when the decimator
    /// is drawing lines between takes.
    previous: [f32; MAX_CHANNELS],
    /// How far through the current hold we are, in input samples. Fractional,
    /// because the rate is a frequency rather than a whole-number divisor —
    /// a hold of 3.7 samples is what 13 kHz asks for at 48 kHz, and rounding
    /// it to 4 would quantise the *rate knob* into audible steps.
    phase: f32,
    /// How long the current hold is, in samples — the period, or a jittered
    /// version of it — and how many samples of it have gone by. What the
    /// ramp's fraction is measured against.
    hold_length: f32,
    since_take: f32,
    /// The shaped dither's error, per channel: what the quantiser got wrong
    /// last time, fed back so it errs the other way this time.
    error: [f32; MAX_CHANNELS],
    /// The anti-alias filter, one per channel, two sections for a fourth-order
    /// slope.
    band_limit: [[SvfFilter; 2]; MAX_CHANNELS],
    /// The post filter, one per channel.
    post: [SvfFilter; MAX_CHANNELS],
    /// The dither generator's state. A plain xorshift: this is the audio
    /// thread, and dither wants uncorrelated numbers rather than good ones.
    noise: u32,
    sample_rate: f32,
}

impl Bitcrush {
    pub fn new() -> Self {
        Self {
            held: [0.0; MAX_CHANNELS],
            previous: [0.0; MAX_CHANNELS],
            phase: 0.0,
            hold_length: 1.0,
            since_take: 0.0,
            error: [0.0; MAX_CHANNELS],
            band_limit: Default::default(),
            post: Default::default(),
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
        self.held = [0.0; MAX_CHANNELS];
        self.previous = [0.0; MAX_CHANNELS];
        // Zero rather than one, so the first sample of the next note is taken
        // rather than the last one of the previous note being held into it.
        self.phase = 0.0;
        self.hold_length = 1.0;
        self.since_take = 0.0;
        self.error = [0.0; MAX_CHANNELS];
        for channel in &mut self.band_limit {
            for section in channel {
                section.reset();
            }
        }
        for filter in &mut self.post {
            filter.reset();
        }
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &BitcrushConfig) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);

        let input = 10f32.powf(config.input_db.clamp(-60.0, 60.0) / 20.0);
        let output = 10f32.powf(config.output_db.clamp(-60.0, 60.0) / 20.0);

        // How many input samples one output sample lasts. At or above the
        // device's rate this is 1.0 and nothing is held, which is what makes
        // the top of the knob transparent whatever the sound card is doing.
        let rate = config.rate_hz.clamp(1.0, self.sample_rate);
        let period = (self.sample_rate / rate).max(1.0);
        let jitter = config.jitter.clamp(0.0, 1.0);

        // `2^bits` levels over the -1..1 range. The knob is fractional, so
        // this is not a shift.
        let levels = 2f32.powf(config.bits.clamp(1.0, MAX_BITS)) - 1.0;
        let step = 2.0 / levels;

        // Band-limiting goes *before* the hold, which is the only place it can
        // do anything: after it, the aliases are already in the signal and
        // indistinguishable from what was meant to be there.
        let cutoff = (rate * 0.5).min(self.sample_rate * 0.49);
        let coeffs: [SvfCoeffs; 2] = [
            SvfFilter::coeffs(SvfMode::Lowpass, cutoff, 0.541_196_1, 0.0, self.sample_rate),
            SvfFilter::coeffs(SvfMode::Lowpass, cutoff, 1.306_562_9, 0.0, self.sample_rate),
        ];
        let posting = config.post_lp_hz < POST_OPEN_HZ;
        let post = SvfFilter::coeffs(
            SvfMode::Lowpass,
            config.post_lp_hz,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            self.sample_rate,
        );

        for frame in 0..frames {
            let take = self.phase <= 0.0;
            if take {
                // The next hold's length is decided when it starts, so a
                // jittered clock is uneven between holds and steady within
                // one — a clock, not noise on the read pointer.
                self.hold_length = if jitter > 0.0 {
                    (period * (1.0 + jitter * self.uniform() * 0.5)).max(1.0)
                } else {
                    period
                };
                self.since_take = 0.0;
            }
            for channel in 0..used {
                let mut sample = channels[channel][frame] * input;
                if config.anti_alias {
                    for section in 0..2 {
                        sample = self.band_limit[channel][section].process(sample, &coeffs[section]);
                    }
                }
                // Into the rails before the grid: a signal driven past full
                // scale sits on the top level rather than off the end of it.
                let sample = sample.clamp(-1.0, 1.0);
                if take {
                    self.previous[channel] = self.held[channel];
                    self.held[channel] =
                        self.quantise(channel, sample, step, config.quantiser, config.dither);
                }
                let out = match config.decimation {
                    Decimation::Hold => self.held[channel],
                    // A line from the last value toward this one, reaching
                    // it as the next is taken: a sampler with interpolation,
                    // one hold late.
                    Decimation::Linear => {
                        let fraction = (self.since_take / self.hold_length).clamp(0.0, 1.0);
                        self.previous[channel]
                            + (self.held[channel] - self.previous[channel]) * fraction
                    }
                    Decimation::Drop => {
                        if take {
                            self.held[channel]
                        } else {
                            0.0
                        }
                    }
                };
                let out = if posting {
                    self.post[channel].process(out, &post)
                } else {
                    out
                };
                channels[channel][frame] = out * output;
            }
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
            if take {
                self.phase += self.hold_length;
            }
            self.phase -= 1.0;
            self.since_take += 1.0;
        }
    }

    /// One value onto the grid, the way the chooser says.
    ///
    /// Always lands on one of the `2^bits` levels, and rounding at full scale
    /// rounds *up*, so the result is held to the rails: a bitcrusher that
    /// handed on 1.03 would clip whatever came after it.
    fn quantise(
        &mut self,
        channel: usize,
        sample: f32,
        step: f32,
        quantiser: Quantiser,
        dither: Dither,
    ) -> f32 {
        // µ-law is the linear grid applied to the companded signal, so the
        // grid itself is the same and only the domain changes.
        let value = match quantiser {
            Quantiser::MuLaw => compress(sample),
            _ => sample,
        };
        // Dither before the quantiser, which is the only order in which it
        // does anything: added after, it would be hiss on top of the steps
        // rather than a way of breaking them up.
        let noise = match dither {
            Dither::Off => 0.0,
            Dither::Rectangular => self.uniform() * step * 0.5,
            Dither::Triangular | Dither::Shaped => self.triangular() * step * 0.5,
        };
        // Shaped: last time's error is subtracted first, so the grid errs
        // the other way this time and the noise's spectrum gains a zero at
        // DC — it moves up, out of the way.
        let fed = if dither == Dither::Shaped {
            value - self.error[channel]
        } else {
            value
        };
        let scaled = (fed + noise) / step;
        let gridded = match quantiser {
            Quantiser::Round | Quantiser::MuLaw => scaled.round(),
            Quantiser::Truncate => scaled.trunc(),
        } * step;
        if dither == Dither::Shaped {
            self.error[channel] = gridded - fed;
        }
        let out = match quantiser {
            Quantiser::MuLaw => expand(gridded.clamp(-1.0, 1.0)),
            _ => gridded,
        };
        out.clamp(-1.0, 1.0)
    }

    /// Triangular dither in `-1..=1`: the sum of two independent uniforms,
    /// which is the standard choice — its noise floor is flat and, unlike
    /// rectangular dither, the *level* of the remaining error stops depending
    /// on the signal.
    fn triangular(&mut self) -> f32 {
        (self.uniform() + self.uniform()) * 0.5
    }

    fn uniform(&mut self) -> f32 {
        // xorshift32.
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

impl Default for Bitcrush {
    fn default() -> Self {
        Self::new()
    }
}

/// µ-law companding: `sign(x) · ln(1 + µ|x|) / ln(1 + µ)`. Fine near zero,
/// coarse near full scale, so a grid laid over it keeps quiet detail.
fn compress(sample: f32) -> f32 {
    ((1.0 + MU * sample.abs()).ln() / (1.0 + MU).ln()).copysign(sample)
}

/// And back.
fn expand(value: f32) -> f32 {
    (((1.0 + MU).powf(value.abs()) - 1.0) / MU).copysign(value)
}
