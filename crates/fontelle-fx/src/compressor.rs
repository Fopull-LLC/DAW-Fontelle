//! The compressor's DSP. Its parameters are `fontelle_types::CompressorConfig`
//! — the document owns those, this owns the envelope.

use fontelle_types::{CompressorConfig, DetectionMode};

/// Left and right, as everywhere else in this crate.
const MAX_CHANNELS: usize = 2;

/// How long the RMS detector averages over.
///
/// Short enough to follow a phrase and long enough to ignore a waveform: at
/// 10 ms a 100 Hz note is one cycle, which is the floor for "how loud is this"
/// meaning anything.
const RMS_WINDOW_MS: f32 = 10.0;

/// The floor the detector reports for silence. Not negative infinity, because
/// every number downstream of it is arithmetic.
const SILENCE_DB: f32 = -120.0;

/// A feed-forward compressor: measure, decide a gain, smooth it, apply it
/// (TDD §13.4).
///
/// Feed-forward rather than feed-back because the decision is then a pure
/// function of the input, which is what makes the transfer curve a thing that
/// can be drawn and a thing this file's tests can assert.
///
/// **Stereo-linked**, like the limiter and for the same reason: one gain from
/// whichever channel is louder. Independent per-channel gains pull a mix
/// toward the quieter side every time the other one peaks, which is an image
/// that moves with the material.
pub struct Compressor {
    /// The gain reduction currently applied, in dB, **negative**. This is what
    /// attack and release smooth — a feed-forward design smooths the decision
    /// rather than the detector, so "attack" means what the word says.
    reduction_db: f32,
    /// The RMS detector's running mean square.
    mean_square: f32,
    sample_rate: f32,
}

impl Compressor {
    pub fn new() -> Self {
        Self {
            reduction_db: 0.0,
            mean_square: 0.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.reduction_db = 0.0;
        self.mean_square = 0.0;
    }

    /// How much it is pulling down right now, in dB. Negative, and zero when
    /// nothing is over the threshold — what a gain-reduction meter draws.
    pub fn gain_reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// Runs `main` through the compressor in place.
    ///
    /// `sidechain`, when given, is what the detector listens to instead of the
    /// signal itself (§13.4's "sidechain input from any mixer track"). Routing
    /// one track's audio here is the graph's job; this is the half that has to
    /// be ready for it.
    pub fn process(
        &mut self,
        main: &mut [&mut [f32]],
        sidechain: Option<&[f32]>,
        config: &CompressorConfig,
    ) {
        if main.is_empty() {
            return;
        }
        let used = main.len().min(MAX_CHANNELS);
        let frames = main.iter().take(used).map(|c| c.len()).min().unwrap_or(0);
        if frames == 0 {
            return;
        }

        let ratio = config.ratio.max(1.0);
        let knee = config.knee_db.max(0.0);
        let attack = coefficient(config.attack_ms, self.sample_rate);
        let release = coefficient(config.release_ms, self.sample_rate);
        let rms_coeff = coefficient(RMS_WINDOW_MS, self.sample_rate);
        // Auto makeup puts back what the threshold takes away at full scale,
        // which is what the knob would be doing by hand — so moving the
        // threshold stops also moving the level.
        let makeup = config.makeup_db
            + if config.auto_makeup {
                -config.threshold_db * (1.0 - 1.0 / ratio)
            } else {
                0.0
            };
        let makeup_gain = 10f32.powf(makeup / 20.0);

        for frame in 0..frames {
            // What the detector is looking at: the key if there is one, and
            // otherwise the loudest channel of the signal itself.
            let input = match sidechain {
                Some(key) => key.get(frame).copied().unwrap_or(0.0).abs(),
                None => main
                    .iter()
                    .take(used)
                    .fold(0.0f32, |m, c| m.max(c[frame].abs())),
            };

            let level_db = match config.detection {
                DetectionMode::Peak => to_db(input),
                DetectionMode::Rms => {
                    self.mean_square += rms_coeff * (input * input - self.mean_square);
                    to_db(self.mean_square.max(0.0).sqrt())
                }
            };

            // The transfer curve, in dB. Above the knee it is the plain
            // division; inside it, a quadratic that meets the flat part with
            // matching slope, which is what makes material sitting around the
            // threshold stop switching in and out audibly.
            let over = level_db - config.threshold_db;
            let target = if knee > 0.0 && over > -knee / 2.0 && over < knee / 2.0 {
                let x = over + knee / 2.0;
                -(1.0 / ratio - 1.0).abs() * x * x / (2.0 * knee)
            } else if over > 0.0 {
                -over * (1.0 - 1.0 / ratio)
            } else {
                0.0
            };

            // Attack when the gain is going *down*, release when it is coming
            // back — which is what those two words mean, and gets them the
            // wrong way round if the comparison is written on the level rather
            // than on the reduction.
            let coeff = if target < self.reduction_db {
                attack
            } else {
                release
            };
            self.reduction_db += coeff * (target - self.reduction_db);

            let gain = 10f32.powf(self.reduction_db / 20.0) * makeup_gain;
            for channel in main.iter_mut().take(used) {
                channel[frame] *= gain;
            }
        }
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

/// A one-pole smoother's coefficient for a time constant in milliseconds.
///
/// The time is what it takes to cover about 63% of the distance, which is the
/// convention every compressor's attack knob is calibrated in.
fn coefficient(ms: f32, sample_rate: f32) -> f32 {
    let samples = (ms.max(0.001) / 1000.0) * sample_rate;
    if samples <= 1.0 {
        1.0
    } else {
        1.0 - (-1.0 / samples).exp()
    }
}

fn to_db(level: f32) -> f32 {
    if level <= 1e-6 {
        SILENCE_DB
    } else {
        20.0 * level.log10()
    }
}
