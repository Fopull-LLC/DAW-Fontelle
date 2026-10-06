//! Trim, fades and gain as pure functions over samples (plan §2.7, P3).
//!
//! The study stores these as numbers and the render applies them; nothing
//! here cuts a file.

/// The curve a fade follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FadeShape {
    Linear,
    /// Equal power: sin/cos. What a crossfade between two takes wants.
    #[default]
    EqualPower,
    /// Slow then fast (in) — a fade-out that sounds natural.
    Exponential,
}

impl FadeShape {
    /// The gain at `t` (0..1) through a fade **in**. A fade out is
    /// `gain(1 - t)`. 0 at 0 and 1 at 1 for every shape.
    pub fn gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
            Self::Exponential => t * t,
        }
    }
}

/// `samples[start..end]`, clamped to the buffer.
pub fn trim(samples: &[f32], start: usize, end: usize) -> &[f32] {
    let end = end.min(samples.len());
    let start = start.min(end);
    &samples[start..end]
}

/// Fades the first `fade_in` and last `fade_out` samples, in place.
///
/// Sample `i` of a fade of `n` gets `gain(i / n)`, so the first sample is
/// silent and the fade reaches whole on the sample after it ends. Fades
/// longer than the buffer overlap, each multiplying.
pub fn apply_fades(samples: &mut [f32], fade_in: usize, fade_out: usize, shape: FadeShape) {
    let len = samples.len();
    for (i, sample) in samples.iter_mut().take(fade_in).enumerate() {
        *sample *= shape.gain(i as f32 / fade_in as f32);
    }
    for i in 0..fade_out.min(len) {
        samples[len - 1 - i] *= shape.gain(i as f32 / fade_out as f32);
    }
}

/// Multiplies by `db` as a gain, in place.
pub fn apply_gain_db(samples: &mut [f32], db: f32) {
    let gain = 10f32.powf(db / 20.0);
    for sample in samples {
        *sample *= gain;
    }
}

/// Where the sound in `samples` begins and ends: the first and one past the
/// last sample above `threshold_db` (dBFS). `None` for silence.
pub fn sound_bounds(samples: &[f32], threshold_db: f32) -> Option<(usize, usize)> {
    let threshold = 10f32.powf(threshold_db / 20.0);
    let first = samples.iter().position(|s| s.abs() > threshold)?;
    let last = samples.iter().rposition(|s| s.abs() > threshold)?;
    Some((first, last + 1))
}

/// Samples in `ms` milliseconds at `sample_rate`, rounded.
pub fn ms_to_samples(ms: f32, sample_rate: u32) -> usize {
    (f64::from(ms.max(0.0)) * f64::from(sample_rate) / 1000.0).round() as usize
}
