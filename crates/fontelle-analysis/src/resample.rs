//! Sample-rate conversion for analysis (rubato's FFT resampler, offline).

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// `input` (mono) at `from` Hz, resampled to `to` Hz, the same duration.
/// Same rate, or nothing to resample: a copy.
pub fn resample_mono(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let resampled = (|| {
        let mut resampler =
            Fft::<f32>::new(from as usize, to as usize, 1024, 1, FixedSync::Input).ok()?;
        let adapter = InterleavedSlice::new(input, 1, input.len()).ok()?;
        let out = resampler.process_all(&adapter, input.len(), None).ok()?;
        Some(out.take_data())
    })();
    // rubato refuses only impossible configurations (a zero rate, above);
    // should it ever, linear interpolation is a better answer than none.
    resampled.unwrap_or_else(|| linear(input, from, to))
}

fn linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let len = (input.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    (0..len)
        .map(|i| {
            let x = i as f64 * step;
            let j = x as usize;
            let frac = (x - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}
