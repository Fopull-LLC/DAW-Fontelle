//! Curves at the models' frame rate, and the lanes' units in the models'.

use fontelle_sing::Curve;

pub fn midi_to_hz(midi: f32) -> f32 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

/// A curve's value at each of `n` frames from `start`, `fallback` when the
/// lane is not drawn.
pub fn sample(curve: Option<&Curve>, start: f64, frame: f64, n: usize, fallback: f32) -> Vec<f32> {
    (0..n)
        .map(|i| match curve {
            Some(c) => c.at(start + i as f64 * frame, fallback),
            None => fallback,
        })
        .collect()
}

/// `values` (frames of `from_frame` seconds from `from_start`) at `n`
/// frames of `to_frame` from `to_start`, linear, held past the ends.
pub fn resample(
    values: &[f32],
    from_start: f64,
    from_frame: f64,
    to_start: f64,
    to_frame: f64,
    n: usize,
) -> Vec<f32> {
    if values.is_empty() {
        return vec![0.0; n];
    }
    let last = values.len() - 1;
    (0..n)
        .map(|i| {
            let x = (to_start + i as f64 * to_frame - from_start) / from_frame;
            if x <= 0.0 {
                return values[0];
            }
            let k = x.floor() as usize;
            if k >= last {
                return values[last];
            }
            let f = (x - k as f64) as f32;
            values[k] + (values[k + 1] - values[k]) * f
        })
        .collect()
}

/// The Gender lane (formant shift in cents, positive brighter) as the
/// acoustic model's `gender` input: -1..1 over the key shift range it was
/// trained with.
pub fn gender_input(cents: f32, range: (f32, f32)) -> f32 {
    let semitones = cents / 100.0;
    let (low, high) = range;
    let g = if semitones >= 0.0 {
        if high > 0.0 { semitones / high } else { 0.0 }
    } else if low < 0.0 {
        semitones / -low
    } else {
        0.0
    };
    g.clamp(-1.0, 1.0)
}

/// The standard -1..1 Breathiness and Voicing lanes in the dB the models
/// predict, as a change to add.
pub fn db_delta(lane: f32) -> f32 {
    // The full lane is 12 dB either way, as OpenUtau's curves are.
    lane * 12.0
}

/// The -1..1 Tension lane as a change to the predicted tension.
pub fn tension_delta(lane: f32) -> f32 {
    lane * 5.0
}

/// Multiplies each sample by the Dynamics lane (dB) at its time.
pub fn apply_gain(samples: &mut [f32], sample_rate: u32, start: f64, dynamics: &Curve) {
    let rate = f64::from(sample_rate.max(1));
    for (i, s) in samples.iter_mut().enumerate() {
        let db = dynamics.at(start + i as f64 / rate, 0.0);
        *s *= 10f32.powf(db / 20.0);
    }
}
