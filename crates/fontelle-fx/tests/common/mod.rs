//! What the seven of `docs/flopsynth-next.md` §4.5 measure with: sines,
//! impulses, a windowed DFT at one frequency, and the wet-only rule every
//! effect here follows (`EffectNode` owns the blend).

#![allow(dead_code)]

pub const SR: f32 = 48_000.0;
pub const BPM: f32 = 120.0;

pub fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

pub fn impulse(frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames];
    buffer[0] = 1.0;
    buffer
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Energy at `hz`, a Hann-windowed DFT — amplitude, so a unit sine reads
/// about 0.25 (a half for the sine's two halves, a half for the window).
pub fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

pub fn db(ratio: f32) -> f32 {
    20.0 * ratio.max(1e-9).log10()
}

/// The steady level of a sine at `hz` through `run`, measured on the last
/// half: the effect's gain at that frequency, wet only. RMS × √2 rather
/// than the peak, because a sine near the top of the band is sampled at
/// too few points a cycle for its peak to be reliably hit.
pub fn level_at(
    hz: f32,
    frames: usize,
    run: impl FnOnce(Vec<f32>, Vec<f32>) -> (Vec<f32>, Vec<f32>),
) -> f32 {
    let (out, _) = run(sine(hz, frames), sine(hz, frames));
    rms(&out[frames / 2..]) * std::f32::consts::SQRT_2
}
