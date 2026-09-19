//! The distortion's DSP. Its parameters are `fontelle_types::DistortionConfig`
//! — the document owns those, this owns the filters and the sag envelope.
//!
//! # The path
//!
//! ```text
//! in → pre high-pass → pre mid bell → split ─┬─ low band, untouched ──────────────┐
//!                                            └─ + bias → × drive (less sag)      │
//!                                                → curve → DC removed → tone     │
//!                                                → auto-gain ─────────────────── + → × output
//! ```
//!
//! Everything before the curve exists because *what* goes into a clipper is
//! most of what comes out: a high-pass first is the difference between a bass
//! fuzz and a mess, a bell first is every pedal's voicing, and a low band
//! taken around the curve is bass distortion that keeps its bottom. Everything
//! after it exists because a curve's output is bounded but not tidy — it has a
//! DC offset if the curve was biased, a harmonic series that does not stop,
//! and a level that has nothing to do with the level that went in.
//!
//! # Why this one is oversampled and the others are not
//!
//! Every effect in this crate is nonlinear except the EQ, but a compressor's
//! nonlinearity is in its *gain*, which moves at the speed of its envelope —
//! milliseconds — and produces almost nothing above the band. A waveshaper's
//! is in the *signal*, per sample, and produces a harmonic series that does
//! not stop at Nyquist. Clipping a 7 kHz tone puts a seventh harmonic at
//! 49 kHz; at a 48 kHz sample rate that frequency does not exist, so it comes
//! back as a 1 kHz tone unrelated to anything played. That is the sound of a
//! cheap distortion, and it is not a subtle effect — see
//! `tests/distortion.rs`, which measures it.
//!
//! So the shaper runs at a multiple of the rate, between a pair of
//! eighth-order Butterworth low-passes: one to remove the images that
//! zero-stuffing creates, one to remove everything above the original Nyquist
//! before the rate comes back down. Four sections each, because the images
//! sit close above the band and a gentler slope leaves too much of them for
//! the curve to fold back.

use fontelle_dsp::{DcBlocker, SvfCoeffs, SvfFilter, SvfMode};
use fontelle_types::{DistortionConfig, DistortionCurve};

const MAX_CHANNELS: usize = 2;

/// The most the shaper's rate is multiplied by — the top of the chooser.
const MAX_FACTOR: usize = 8;

/// How many 2-pole sections the anti-imaging and anti-aliasing filters are.
/// Eight poles: 48 dB an octave, which puts the first image of a 7 kHz tone
/// — 41 kHz, just over an octave above the corner — about 50 dB down.
const OS_SECTIONS: usize = 4;

/// The Qs of an eighth-order Butterworth cascade — the same four the EQ's
/// 48 dB/oct bands use, and for the same reason: four identical sections are
/// not a Butterworth, they are a filter that sags into its corner.
const BUTTERWORTH_QS: [f32; OS_SECTIONS] = [0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_4];

/// The Q of one section of a fourth-order Linkwitz–Riley crossover: two
/// Butterworth sections in series, whose low and high halves sum to a flat
/// magnitude. What the clean-low split is built from.
const LR_SECTION_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Where the oversampling filters sit, in Hz.
///
/// Above the audible band so nothing wanted is lost, and far enough below the
/// original Nyquist that the slope has somewhere to work.
const OS_CUTOFF_HZ: f32 = 20_000.0;

/// The value of the tone control that means "not in the path".
///
/// A filter at the top of its range is not quite a wire — it still shifts
/// phase, and on a hard-clipped square that shows up as overshoot past the
/// ceiling the clipper just imposed. Taking it out of the path instead is both
/// cheaper and more honest about what the knob at its maximum means.
const TONE_OPEN_HZ: f32 = 20_000.0;

/// The bottom of the two pre-filter knobs, which is where "off" lives: a
/// high-pass at 20 Hz removes nothing anybody can hear, and a split there
/// sends nothing around the curve.
const FILTER_OFF_HZ: f32 = 20.5;

/// The Q of the pre-mid bell. Broad: a hump rather than a resonance, which
/// is what a pedal's voicing is.
const PRE_MID_Q: f32 = 1.0;

/// Where the DC blocker after the curve sits. Low enough to leave a 30 Hz
/// note alone; high enough that a biased curve's offset is gone in a few
/// cycles rather than a few seconds.
const DC_HZ: f32 = 10.0;

/// Where the output is held, well outside the rails. The curves are bounded
/// to ±1 and the filters after them ring by a sixth at most, so nothing
/// ordinary reaches this; see the note where it is applied.
const SAFETY_NET: f32 = 1.5;

/// The most sag can take off the drive, in dB, at full amount and full
/// scale. An amplifier's supply sags by a few; a knob that goes further is a
/// knob with more of a range.
const SAG_MAX_DB: f32 = 24.0;

/// The sag envelope's speeds. Fast enough to catch a loud passage's first
/// note, slow enough not to follow the waveform — a supply, not a detector.
const SAG_ATTACK_MS: f32 = 5.0;
const SAG_RELEASE_MS: f32 = 150.0;

/// The level auto-gain measures the curve at: -12 dBFS, which is where a
/// track sits. A reference that was much quieter would find the bottom of
/// every curve straight and compensate nothing; one at full scale would
/// compensate a square wave nobody is making.
const AUTO_GAIN_REF: f32 = 0.25;

/// How many points of one cycle the reference is measured over.
const AUTO_GAIN_POINTS: usize = 64;

/// Waveshaping distortion, oversampled, with the voicing around it
/// (TDD §13.4, `docs/effects-catalogue.md` §3.1).
pub struct Distortion {
    /// The two pre-filters, one each per channel.
    pre_hp: [SvfFilter; MAX_CHANNELS],
    pre_mid: [SvfFilter; MAX_CHANNELS],
    /// The clean-low split: a Linkwitz–Riley pair per channel, two sections
    /// each way.
    split_low: [[SvfFilter; 2]; MAX_CHANNELS],
    split_high: [[SvfFilter; 2]; MAX_CHANNELS],
    /// The tone control, one filter per channel.
    tone: [SvfFilter; MAX_CHANNELS],
    /// What takes a biased curve's offset back out.
    dc: [DcBlocker; MAX_CHANNELS],
    /// The anti-imaging cascade, per channel, running at the oversampled rate.
    up: [[SvfFilter; OS_SECTIONS]; MAX_CHANNELS],
    /// The anti-aliasing cascade, per channel, same rate.
    down: [[SvfFilter; OS_SECTIONS]; MAX_CHANNELS],
    /// The sag envelope. One for both channels, so the drive does not differ
    /// between the sides — the same reason the compressor is stereo-linked.
    sag: f32,
    sample_rate: f32,
}

impl Distortion {
    pub fn new() -> Self {
        Self {
            pre_hp: Default::default(),
            pre_mid: Default::default(),
            split_low: Default::default(),
            split_high: Default::default(),
            tone: Default::default(),
            dc: Default::default(),
            up: Default::default(),
            down: Default::default(),
            sag: 0.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        for filter in self
            .pre_hp
            .iter_mut()
            .chain(self.pre_mid.iter_mut())
            .chain(self.tone.iter_mut())
        {
            filter.reset();
        }
        for channel in self.split_low.iter_mut().chain(self.split_high.iter_mut()) {
            for filter in channel {
                filter.reset();
            }
        }
        for channel in self.up.iter_mut().chain(self.down.iter_mut()) {
            for filter in channel {
                filter.reset();
            }
        }
        self.dc = Default::default();
        self.sag = 0.0;
    }

    /// Runs `channels` through the shaper in place.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &DistortionConfig) {
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let rate = self.sample_rate;

        let drive = 10f32.powf(config.drive_db.clamp(0.0, 60.0) / 20.0);
        let output = 10f32.powf(config.output_db.clamp(-60.0, 24.0) / 20.0);
        let shape = config.shape.clamp(0.0, 1.0);
        let bias = config.bias.clamp(-1.0, 1.0);
        let sag_amount = config.sag.clamp(0.0, 1.0);
        let curve = config.curve;

        // The stages that are in the path this block, and their filters.
        // Built once here rather than per sample: none of these move inside
        // a block.
        let pre_hp_on = config.pre_hp_hz > FILTER_OFF_HZ;
        let pre_hp =
            SvfFilter::coeffs(SvfMode::Highpass, config.pre_hp_hz, LR_SECTION_Q, 0.0, rate);
        let pre_mid_on = config.pre_mid_db.abs() > 0.01;
        let pre_mid = SvfFilter::coeffs(
            SvfMode::Bell,
            config.pre_mid_hz,
            PRE_MID_Q,
            config.pre_mid_db,
            rate,
        );
        let split_on = config.clean_low_hz > FILTER_OFF_HZ;
        let split_low = SvfFilter::coeffs(
            SvfMode::Lowpass,
            config.clean_low_hz,
            LR_SECTION_Q,
            0.0,
            rate,
        );
        let split_high = SvfFilter::coeffs(
            SvfMode::Highpass,
            config.clean_low_hz,
            LR_SECTION_Q,
            0.0,
            rate,
        );
        let toning = config.tone_hz < TONE_OPEN_HZ;
        let tone = SvfFilter::coeffs(SvfMode::Lowpass, config.tone_hz, LR_SECTION_Q, 0.0, rate);
        let factor = config.oversample.factor().clamp(1, MAX_FACTOR);
        let os_coeffs = os_coeffs(rate, factor);

        let sag_attack = one_pole(SAG_ATTACK_MS, rate);
        let sag_release = one_pole(SAG_RELEASE_MS, rate);

        // Auto-gain: what this curve, at this drive and bias, does to the
        // level of a reference sine — measured rather than estimated, so it
        // is right for a folder as well as a clipper. Sag is left out: it is
        // the drive *moving*, and compensating it would undo it.
        let auto_gain = if config.auto_gain {
            reference_gain(curve, shape, bias, drive)
        } else {
            1.0
        };

        for frame in 0..frames {
            // The signal on its way to the curve, per channel, and the low
            // band that goes around it.
            let mut hot = [0.0f32; MAX_CHANNELS];
            let mut low = [0.0f32; MAX_CHANNELS];
            let mut peak = 0.0f32;
            for channel in 0..used {
                let mut sample = channels[channel][frame];
                if pre_hp_on {
                    sample = self.pre_hp[channel].process(sample, &pre_hp);
                }
                if pre_mid_on {
                    sample = self.pre_mid[channel].process(sample, &pre_mid);
                }
                if split_on {
                    // Both halves of the crossover see the same signal; the
                    // low half is kept back and added after the curve.
                    let mut below = sample;
                    let mut above = sample;
                    for section in 0..2 {
                        below = self.split_low[channel][section].process(below, &split_low);
                        above = self.split_high[channel][section].process(above, &split_high);
                    }
                    low[channel] = below;
                    sample = above;
                }
                hot[channel] = sample;
                peak = peak.max(sample.abs());
            }

            // The supply: a follower on the level going in, pulling the drive
            // down by up to `SAG_MAX_DB` at full scale. Squared, so a quiet
            // passage is left alone and a loud one is what sags it.
            let coeff = if peak > self.sag {
                sag_attack
            } else {
                sag_release
            };
            self.sag += (peak - self.sag) * coeff;
            let sagged = self.sag.clamp(0.0, 1.0);
            let sag_db = -SAG_MAX_DB * sag_amount * sagged * sagged;
            let drive_now = drive * 10f32.powf(sag_db / 20.0);

            for channel in 0..used {
                let driven = hot[channel] * drive_now;
                let shaped = if factor > 1 {
                    self.oversampled(channel, driven, factor, &os_coeffs, curve, shape, bias)
                } else {
                    curve_at(driven, curve, shape, bias)
                };
                // The offset a biased curve leaves, taken back out — and then
                // held to a *wide* net, not the rails. A curve is bounded, but
                // what leaves the oversampling filters is the band-limited
                // version of it, and a band-limited square overshoots its
                // corners by a sixth; clamping that at the base rate would
                // re-clip the ringing at 48 kHz, which is exactly the
                // aliasing the oversampling exists to prevent (measured: a
                // 13 kHz tone at -25 dB from a clamp at ±1). The net is for
                // the pathological case only — a biased curve with a lopsided
                // duty cycle — so the mixer is never handed a number it
                // cannot hold.
                let centred = self.dc[channel]
                    .process(shaped, DC_HZ, rate)
                    .clamp(-SAFETY_NET, SAFETY_NET);
                // The tone control after the shaper: it is there to take the
                // top off what the shaper *made*, and before it there would be
                // nothing for it to remove.
                let toned = if toning {
                    self.tone[channel].process(centred, &tone)
                } else {
                    centred
                };
                // The clean band comes back after the auto-gain, not inside
                // it: it was never driven, so there is nothing to compensate.
                channels[channel][frame] = (toned * auto_gain + low[channel]) * output;
            }
        }
    }

    /// One sample through the shaper at `factor` times the rate.
    ///
    /// Zero-stuffing divides the signal's amplitude by the factor across the
    /// sub-samples, so the input is multiplied by it going in — the standard
    /// bookkeeping, and the thing that makes an oversampled path the same
    /// level as a plain one.
    #[allow(clippy::too_many_arguments)]
    fn oversampled(
        &mut self,
        channel: usize,
        sample: f32,
        factor: usize,
        coeffs: &[SvfCoeffs; OS_SECTIONS],
        curve: DistortionCurve,
        shape: f32,
        bias: f32,
    ) -> f32 {
        let mut result = 0.0;
        for phase in 0..factor {
            let stuffed = if phase == 0 {
                sample * factor as f32
            } else {
                0.0
            };
            let mut value = stuffed;
            for section in 0..OS_SECTIONS {
                value = self.up[channel][section].process(value, &coeffs[section]);
            }
            value = curve_at(value, curve, shape, bias);
            for section in 0..OS_SECTIONS {
                value = self.down[channel][section].process(value, &coeffs[section]);
            }
            // Decimation is keeping one of the sub-samples, and it must be
            // the same one every sample or the output is amplitude-modulated
            // at a fraction of the sample rate.
            if phase == 0 {
                result = value;
            }
        }
        result
    }
}

impl Default for Distortion {
    fn default() -> Self {
        Self::new()
    }
}

/// The anti-imaging and anti-aliasing cascade's coefficients at `factor`
/// times `sample_rate`.
fn os_coeffs(sample_rate: f32, factor: usize) -> [SvfCoeffs; OS_SECTIONS] {
    let oversampled = sample_rate.max(1.0) * factor as f32;
    std::array::from_fn(|section| {
        SvfFilter::coeffs(
            SvfMode::Lowpass,
            OS_CUTOFF_HZ,
            BUTTERWORTH_QS[section],
            0.0,
            oversampled,
        )
    })
}

/// A one-pole smoother's coefficient for a time constant in milliseconds.
fn one_pole(ms: f32, sample_rate: f32) -> f32 {
    let samples = (ms.max(0.001) / 1000.0) * sample_rate;
    if samples <= 1.0 {
        1.0
    } else {
        1.0 - (-1.0 / samples).exp()
    }
}

/// The gain that puts a reference sine back at its own level after this
/// curve at this drive: the RMS it went in at over the RMS it came out at.
///
/// Sixty-four points of one cycle, evaluated once per block — a few hundred
/// operations, against the tens of thousands the block itself is.
fn reference_gain(curve: DistortionCurve, shape: f32, bias: f32, drive: f32) -> f32 {
    let mut before = 0.0f32;
    let mut after = 0.0f32;
    for point in 0..AUTO_GAIN_POINTS {
        let phase = std::f32::consts::TAU * point as f32 / AUTO_GAIN_POINTS as f32;
        let input = AUTO_GAIN_REF * phase.sin();
        let output = curve_at(input * drive, curve, shape, bias);
        before += input * input;
        after += output * output;
    }
    // The curve's output has a mean if it is biased or rectified, and the DC
    // blocker takes that out; the level that matters is what is left.
    let mean = (0..AUTO_GAIN_POINTS)
        .map(|point| {
            let phase = std::f32::consts::TAU * point as f32 / AUTO_GAIN_POINTS as f32;
            curve_at(AUTO_GAIN_REF * phase.sin() * drive, curve, shape, bias)
        })
        .sum::<f32>()
        / AUTO_GAIN_POINTS as f32;
    let after = (after / AUTO_GAIN_POINTS as f32 - mean * mean)
        .max(1e-12)
        .sqrt();
    let before = (before / AUTO_GAIN_POINTS as f32).sqrt();
    (before / after).clamp(1e-3, 1.0e3)
}

/// The curve with the operating point shifted by `bias`, and the shift's own
/// output taken back out so that zero still goes through as zero and a reset
/// effect is still silent. The DC blocker takes out the rest — the offset
/// the *signal's* asymmetry leaves.
///
/// Public for the picture on the slot's card (`docs/flopsynth-next.md`
/// §3.6): the transfer curve drawn is the one the effect plays, sampled
/// through this one function.
pub fn curve_at(sample: f32, curve: DistortionCurve, shape: f32, bias: f32) -> f32 {
    if bias == 0.0 {
        shape_of(sample, curve, shape)
    } else {
        shape_of(sample + bias, curve, shape) - shape_of(bias, curve, shape)
    }
}

/// The curve itself: one sample in, one out, no state.
///
/// Every one of these is **bounded** to ±1 — what leaves the effect is that,
/// plus whatever the band-limiting filters ring by — and every one passes
/// zero through as zero. `shape` is 0..=1 and means
/// what `DistortionCurve::shape_meaning` says it means for that curve.
fn shape_of(sample: f32, curve: DistortionCurve, shape: f32) -> f32 {
    let bounded = match curve {
        DistortionCurve::SoftClip => {
            // `x / (1 + |x|^p)^(1/p)`: p = 2 is the classic round-over and
            // p = 16 is a step short of a hard clip. The magnitude is held
            // to 64 first because 1000^16 is not a number an f32 can hold,
            // and 64 is already 1.0 to five figures on the softest setting.
            soft(sample, 2.0 + 14.0 * shape)
        }
        DistortionCurve::HardClip => {
            // A ceiling, with a knee `k` wide either side of it that meets
            // the flat part with matching slope — so at zero it is exactly a
            // clamp and at one the corner is a curve.
            let k = 0.5 * shape;
            let magnitude = sample.abs();
            let out = if k <= 0.0 || magnitude <= 1.0 - k {
                magnitude
            } else if magnitude >= 1.0 + k {
                1.0
            } else {
                let over = magnitude - (1.0 - k);
                magnitude - over * over / (4.0 * k)
            };
            out.min(1.0).copysign(sample)
        }
        DistortionCurve::Tube => {
            // A shifted operating point rather than two different slopes
            // either side of zero: the shift makes the curve asymmetric —
            // which is what puts **even** harmonics in — while staying smooth
            // through zero, where a kink would be crossover distortion rather
            // than warmth. Shape is how far it is shifted.
            let b = 0.25 + 0.75 * shape;
            (sample + b).tanh() - b.tanh()
        }
        DistortionCurve::Diode => {
            // Two ceilings, the negative one lower — a germanium pair's
            // forward drops are not the same — with a knee that shape
            // sharpens.
            const LOWER: f32 = 0.6;
            let k = 4.0 + 12.0 * shape;
            if sample >= 0.0 {
                soft(sample, k)
            } else {
                -LOWER * soft(-sample / LOWER, k)
            }
        }
        DistortionCurve::Fold => {
            // Past ±1 the sine turns back down, which is the fold. Bounded by
            // construction, and shape is how many more times it turns.
            (sample * std::f32::consts::FRAC_PI_2 * (1.0 + 3.0 * shape)).sin()
        }
        DistortionCurve::TriangleFold => {
            // The same reversal with straight edges: the value is reflected
            // back into ±t as many times as it takes. At t = 1 this is a
            // wire below full scale, which is what a folder at rest should
            // be; shape pulls t down so it folds sooner and more often.
            let t = 1.0 - 0.75 * shape;
            let y = (sample + t).rem_euclid(4.0 * t);
            let folded = if y < 2.0 * t { y - t } else { 3.0 * t - y };
            folded / t
        }
        DistortionCurve::WaveShape => {
            // `1.5x - 0.5x³` is the cubic whose turning points sit exactly at
            // ±1, and `x(1.875 - 1.25x² + 0.375x⁴)` is the quintic with the
            // same property and more of the higher harmonics; shape blends
            // them. Clamping at ±1 continues either flat rather than letting
            // it come back down.
            let c = sample.clamp(-1.0, 1.0);
            let c2 = c * c;
            let cubic = c * (1.5 - 0.5 * c2);
            let quintic = c * (1.875 - 1.25 * c2 + 0.375 * c2 * c2);
            cubic + (quintic - cubic) * shape
        }
        DistortionCurve::Rectify => {
            // The negative half flipped up by `shape`: none of it at zero
            // (half-wave), all of it at one (full-wave). Held to the rail
            // rather than rounded, so the fold-up is the whole effect.
            let flipped = if sample >= 0.0 {
                sample
            } else {
                -sample * shape
            };
            flipped.min(1.0)
        }
        DistortionCurve::Crossover => {
            // Nothing inside ±w gets out; what does is scaled so full scale
            // is still full scale.
            let w = 0.05 + 0.45 * shape;
            let magnitude = ((sample.abs() - w).max(0.0) / (1.0 - w)).min(1.0);
            magnitude.copysign(sample)
        }
        DistortionCurve::Wrap => {
            // Past ±t the value comes back in from the other side, as an
            // integer overflow does. At t = 1 it is a wire below full scale.
            let t = 1.0 - 0.75 * shape;
            ((sample + t).rem_euclid(2.0 * t) - t) / t
        }
    };
    bounded.clamp(-1.0, 1.0)
}

/// The variable-hardness saturator `x / (1 + |x|^p)^(1/p)`.
fn soft(sample: f32, p: f32) -> f32 {
    let magnitude = sample.abs().min(64.0);
    (magnitude / (1.0 + magnitude.powf(p)).powf(1.0 / p)).copysign(sample)
}
