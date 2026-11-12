//! Master: the mastering chain in one insert. The config and why it is
//! shaped as it is are `fontelle_types::MasterConfig`'s; this is the sound.
//!
//! Built from the effects that already exist and are already measured — the
//! parametric EQ for the clean-up and the tone, the compressor for the glue,
//! Width for the space, the look-ahead limiter for the ceiling — with three
//! things of its own: the bass's harmonics for small speakers, the leveler
//! that rides the gain to a loudness, and the checks.
//!
//! **The leveler measures what comes out**, after the limiter: an integral
//! controller on the output's K-weighted loudness (ITU-R BS.1770). Measuring
//! the input would put a song squeezed by the limiter under its target and
//! never notice; measuring the output is what lands it there.

use fontelle_dsp::{SvfFilter, SvfMode};
use fontelle_types::{
    BandChannel, BandType, CompressorConfig, DetectionMode, EqConfig, MasterCheck, MasterConfig,
    WidthConfig,
};

use crate::{Compressor, Limiter, LimiterConfig, ParametricEq, Width};

const MAX_CHANNELS: usize = 2;

/// The leveler's reach either way, in dB: a song more than this far from its
/// target is somebody's choice, not a level to fix.
const MAX_LEVEL_DB: f32 = 24.0;

/// How fast the leveler closes on the target, per second, per dB away — and
/// the fastest it moves at all.
const LEVEL_RATE: f32 = 1.6;
const LEVEL_MAX_DB_PER_S: f32 = 10.0;

/// The output loudness's averaging, in seconds: longer than a beat so a kick
/// does not pump it, short enough to follow a song's sections.
const LOUDNESS_TAU_S: f32 = 0.6;

/// Under this the output is silence (a gap, a fade's end): the gain holds
/// rather than climbing to make nothing loud.
const SILENCE_LUFS: f32 = -55.0;

/// Where *small speakers* takes the bass from, and where it puts its
/// harmonics.
const BASS_CORNER_HZ: f32 = 130.0;
const HARMONICS_LOW_HZ: f32 = 140.0;
const HARMONICS_HIGH_HZ: f32 = 900.0;

/// A biquad, for the K-weighting's two stages (RBJ cookbook form).
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn high_shelf(sample_rate: f32, hz: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w = std::f32::consts::TAU * hz / sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / 2.0 * std::f32::consts::SQRT_2;
        let sq = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) - (a - 1.0) * cos + sq;
        Self::normalised(
            [
                a * ((a + 1.0) + (a - 1.0) * cos + sq),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sq),
            ],
            [
                a0,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sq,
            ],
        )
    }

    fn high_pass(sample_rate: f32, hz: f32, q: f32) -> Self {
        let w = std::f32::consts::TAU * hz / sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        Self::normalised(
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        )
    }

    fn normalised(b: [f32; 3], a: [f32; 3]) -> Self {
        Self {
            b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]],
            a: [a[1] / a[0], a[2] / a[0]],
            z: [0.0; 2],
        }
    }

    /// Transposed direct form II.
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The mastering chain. See the module.
pub struct Master {
    sample_rate: f32,
    eq: ParametricEq,
    eq_config: EqConfig,
    compressor: Compressor,
    width: Width,
    limiter: Limiter,
    limiter_config: LimiterConfig,
    // ---- small speakers
    bass: SvfFilter,
    harmonics_high: [SvfFilter; 2],
    harmonics_low: [SvfFilter; 2],
    // ---- the leveler
    k_shelf: [Biquad; MAX_CHANNELS],
    k_high: [Biquad; MAX_CHANNELS],
    /// The output's K-weighted power, averaged.
    power: f32,
    level_db: f32,
    // ---- the checks
    check_filters: [[SvfFilter; 4]; MAX_CHANNELS],
}

impl Default for Master {
    fn default() -> Self {
        Self::new()
    }
}

impl Master {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            eq: ParametricEq::new(),
            eq_config: EqConfig::new(),
            compressor: Compressor::new(),
            width: Width::new(),
            limiter: Limiter::new(),
            limiter_config: LimiterConfig::default(),
            bass: SvfFilter::new(),
            harmonics_high: [SvfFilter::new(), SvfFilter::new()],
            harmonics_low: [SvfFilter::new(), SvfFilter::new()],
            k_shelf: [Biquad::default(); MAX_CHANNELS],
            k_high: [Biquad::default(); MAX_CHANNELS],
            power: 0.0,
            level_db: 0.0,
            check_filters: Default::default(),
        }
    }

    /// The only place that allocates (the limiter's line).
    pub fn prepare(&mut self, sample_rate: f32, config: &MasterConfig) {
        self.sample_rate = sample_rate.max(1.0);
        self.eq.prepare(self.sample_rate);
        self.compressor.prepare(self.sample_rate);
        self.width.prepare(self.sample_rate);
        self.limiter_config = limiter_config(config);
        self.limiter.prepare(self.sample_rate, &self.limiter_config);
        for c in 0..MAX_CHANNELS {
            self.k_shelf[c] = Biquad::high_shelf(self.sample_rate, 1_681.97, 3.999_84);
            self.k_high[c] = Biquad::high_pass(self.sample_rate, 38.135, 0.500_33);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        self.eq.reset();
        self.compressor.reset();
        self.width.reset();
        self.limiter.reset();
        self.bass.reset();
        for f in self
            .harmonics_high
            .iter_mut()
            .chain(self.harmonics_low.iter_mut())
        {
            f.reset();
        }
        for c in 0..MAX_CHANNELS {
            self.k_shelf[c].z = [0.0; 2];
            self.k_high[c].z = [0.0; 2];
            for f in &mut self.check_filters[c] {
                f.reset();
            }
        }
        self.power = 0.0;
        self.level_db = 0.0;
    }

    /// The limiter's look-ahead: the whole chain's latency.
    pub fn latency_samples(&self) -> u32 {
        self.limiter.latency_samples()
    }

    /// The leveler's gain now, in dB — for a meter.
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &MasterConfig) {
        if channels.is_empty() {
            return;
        }
        let frames = channels[0].len();
        let sr = self.sample_rate;

        // 1 and 2: clean and tone, as eight EQ bands.
        write_eq(&mut self.eq_config, config);
        self.eq.process(channels, &self.eq_config);

        // 3: glue.
        if config.glue > 0.0 {
            self.compressor
                .process(channels, None, &compressor_config(config));
        }

        // 4: space.
        self.width.process(channels, &width_config(config));

        // 5: the bass's harmonics, for speakers that cannot play the bass.
        if config.small_speakers > 0.0 {
            let amount = config.small_speakers / 100.0;
            let low = SvfFilter::coeffs(SvfMode::Lowpass, BASS_CORNER_HZ, 0.707, 0.0, sr);
            let hp = SvfFilter::coeffs(SvfMode::Highpass, HARMONICS_LOW_HZ, 0.707, 0.0, sr);
            let lp = SvfFilter::coeffs(SvfMode::Lowpass, HARMONICS_HIGH_HZ, 0.707, 0.0, sr);
            for i in 0..frames {
                let mid = if channels.len() > 1 {
                    (channels[0][i] + channels[1][i]) * 0.5
                } else {
                    channels[0][i]
                };
                let bass = self.bass.process(mid, &low);
                // A rectifier and a soft clip: the second harmonic and the
                // odd ones above it, which is what the ear rebuilds a
                // missing fundamental from.
                let driven = (bass * 6.0).tanh() * 0.5 + bass.abs() * 1.5;
                let mut h = driven;
                for f in &mut self.harmonics_high {
                    h = f.process(h, &hp);
                }
                for f in &mut self.harmonics_low {
                    h = f.process(h, &lp);
                }
                let add = h * amount * 1.6;
                for channel in channels.iter_mut() {
                    channel[i] += add;
                }
            }
        }

        // 6: the leveler's gain, then the ceiling.
        let gain = 10f32.powf(self.level_db / 20.0);
        if (gain - 1.0).abs() > 1e-6 {
            for channel in channels.iter_mut() {
                for v in channel.iter_mut() {
                    *v *= gain;
                }
            }
        }
        self.limiter_config = limiter_config(config);
        self.limiter.process(channels, None, &self.limiter_config);

        // The output's loudness, and the leveler's next step toward the
        // target.
        let mut sum = 0.0f32;
        for (c, channel) in channels.iter().enumerate().take(MAX_CHANNELS) {
            for v in channel.iter() {
                let k = self.k_high[c].process(self.k_shelf[c].process(*v));
                sum += k * k;
            }
        }
        // A mono insert measures its one side as both, as BS.1770 sums them.
        let sides = if channels.len() > 1 { 1.0 } else { 2.0 };
        let block_power = sides * sum / frames.max(1) as f32;
        let dt = frames as f32 / sr;
        let blend = 1.0 - (-dt / LOUDNESS_TAU_S).exp();
        self.power += (block_power - self.power) * blend;
        if config.auto_level {
            let loudness = -0.691 + 10.0 * self.power.max(1e-12).log10();
            if loudness > SILENCE_LUFS {
                let step = ((config.target_lufs - loudness) * LEVEL_RATE)
                    .clamp(-LEVEL_MAX_DB_PER_S, LEVEL_MAX_DB_PER_S)
                    * dt;
                self.level_db = (self.level_db + step).clamp(-MAX_LEVEL_DB, MAX_LEVEL_DB);
            }
        } else {
            // Off: back to unity, without a jump.
            self.level_db *= (-dt / 0.05).exp();
            if self.level_db.abs() < 0.01 {
                self.level_db = 0.0;
            }
        }

        // The checks, last: what a mono system, a phone or a laptop makes of
        // the master. For listening — Off is the only setting worth exporting.
        self.check(channels, config.check);
    }

    fn check(&mut self, channels: &mut [&mut [f32]], check: MasterCheck) {
        if check == MasterCheck::Off || channels.len() < 2 {
            return;
        }
        let sr = self.sample_rate;
        let (narrow, high_pass, low_pass) = match check {
            MasterCheck::Off => return,
            MasterCheck::Mono => (0.0, None, None),
            MasterCheck::Phone => (0.0, Some(260.0), Some(7_500.0)),
            MasterCheck::Laptop => (0.3, Some(160.0), Some(15_000.0)),
        };
        let hp = high_pass.map(|hz| SvfFilter::coeffs(SvfMode::Highpass, hz, 0.707, 0.0, sr));
        let lp = low_pass.map(|hz| SvfFilter::coeffs(SvfMode::Lowpass, hz, 0.707, 0.0, sr));
        let (left, right) = channels.split_at_mut(1);
        for (l, r) in left[0].iter_mut().zip(right[0].iter_mut()) {
            let mid = (*l + *r) * 0.5;
            let side = (*l - *r) * 0.5 * narrow;
            let mut out = [mid + side, mid - side];
            for (c, v) in out.iter_mut().enumerate() {
                if let Some(hp) = &hp {
                    *v = self.check_filters[c][0].process(*v, hp);
                    *v = self.check_filters[c][1].process(*v, hp);
                }
                if let Some(lp) = &lp {
                    *v = self.check_filters[c][2].process(*v, lp);
                    *v = self.check_filters[c][3].process(*v, lp);
                }
            }
            *l = out[0];
            *r = out[1];
        }
    }
}

/// The ceiling and release as the limiter's own, with the fixed look-ahead.
fn limiter_config(config: &MasterConfig) -> LimiterConfig {
    LimiterConfig {
        ceiling: 10f32.powf(config.ceiling_db / 20.0),
        lookahead_ms: fontelle_types::LIMITER_LOOKAHEAD_MS,
        release_ms: config.release_ms,
    }
}

/// The glue as a compressor: one knob, from a whisper to a firm hold.
fn compressor_config(config: &MasterConfig) -> CompressorConfig {
    let g = (config.glue / 100.0).clamp(0.0, 1.0);
    CompressorConfig {
        threshold_db: -8.0 - 16.0 * g,
        ratio: 1.3 + 2.7 * g,
        attack_ms: config.glue_attack_ms,
        release_ms: config.glue_release_ms,
        knee_db: 8.0,
        makeup_db: 0.0,
        auto_makeup: false,
        detection: DetectionMode::Rms,
        mix: 1.0,
    }
}

fn width_config(config: &MasterConfig) -> WidthConfig {
    WidthConfig {
        width: config.width,
        mono_below_hz: config.mono_below_hz.max(fontelle_types::MASTER_MONO_OFF_HZ),
        mid_db: 0.0,
        side_db: 0.0,
        mix: 1.0,
    }
}

/// Clean and tone as the EQ's eight bands: a steep low cut, a gentle high
/// cut (off at the top of its knob), and the four broad moves.
fn write_eq(eq: &mut EqConfig, config: &MasterConfig) {
    let mut band = |i: usize, band_type: BandType, hz: f32, gain: f32, q: f32, on: bool| {
        let b = &mut eq.bands[i];
        b.band_type = band_type;
        b.freq_hz = hz;
        b.gain_db = gain;
        b.q = q;
        b.enabled = on;
        b.solo = false;
        b.channel = BandChannel::Stereo;
    };
    band(0, BandType::HighPass24, config.low_cut_hz, 0.0, 0.707, true);
    band(
        1,
        BandType::LowPass12,
        config.high_cut_hz,
        0.0,
        0.707,
        config.high_cut_hz < 21_500.0,
    );
    band(
        2,
        BandType::LowShelf,
        90.0,
        config.low_db,
        0.7,
        config.low_db != 0.0,
    );
    band(
        3,
        BandType::Bell,
        300.0,
        config.mud_db,
        0.8,
        config.mud_db != 0.0,
    );
    band(
        4,
        BandType::Bell,
        3_000.0,
        config.presence_db,
        0.8,
        config.presence_db != 0.0,
    );
    band(
        5,
        BandType::HighShelf,
        12_000.0,
        config.air_db,
        0.7,
        config.air_db != 0.0,
    );
    band(6, BandType::Bell, 1_000.0, 0.0, 1.0, false);
    band(7, BandType::Bell, 1_000.0, 0.0, 1.0, false);
    eq.mix = 1.0;
}
