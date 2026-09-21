//! A preset's preview (`docs/flopsynth-next.md` §5.2): the shape of one
//! note through the patch — its envelope to draw, and a vector to compare
//! by — and the nearest presets by that vector, which is what *sounds
//! like* is.
//!
//! The vector is the one `tests/flopsynth_presets.rs` holds the bank
//! apart with, lifted here so the browser and the gate agree about what
//! "alike" means: four scalar axes in log units (how long it rings, where
//! its energy sits, how peaky it is, what its attack does) and a ten-band
//! spectral shape whose distance is a total variation — the axis that
//! tells two vowels apart when their centroids coincide.
//!
//! **Off the audio thread only.** A preview is a render: 1.5 seconds of
//! C3 at velocity 100, held for six tenths and let go, at 48 kHz.

use crate::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;
const SECONDS: f32 = 1.5;
const HOLD: f32 = 0.6;
const KEY: u8 = 60;
const VELOCITY: u8 = 100;

/// Where a sound sits, on the axes the bank is held apart by.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SoundVector {
    /// `ln t30`, the log-frequency centroid, `ln crest`, `ln (flux + 1)`.
    pub scalar: [f32; 4],
    /// Ten log-spaced bands, summing to one.
    pub shape: [f32; 10],
}

/// One preset, previewed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preview {
    /// The note's envelope across the preview, [`Preview::COLUMNS`] columns
    /// in 0..1 of its own peak.
    pub peaks: Vec<f32>,
    pub vector: SoundVector,
}

impl Preview {
    /// How many columns the envelope is drawn in: one per 25 ms.
    pub const COLUMNS: usize = 60;
}

/// Renders the preview note through `patch` and describes it.
pub fn preset_preview(patch: &Patch) -> Preview {
    let samples = render(patch);
    Preview {
        peaks: envelope(&samples),
        vector: sound_vector(&samples),
    }
}

/// The distance between two sounds: the larger of the furthest scalar
/// axis and the shape's total variation — the same rule the bank's gate
/// uses, so what the gate calls alike the browser calls near.
pub fn sound_distance(a: &SoundVector, b: &SoundVector) -> f32 {
    let scalar = a
        .scalar
        .iter()
        .zip(&b.scalar)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    let shape: f32 = a
        .shape
        .iter()
        .zip(&b.shape)
        .map(|(x, y)| (x - y).abs())
        .sum();
    scalar.max(shape)
}

/// The `count` nearest to `of` among `vectors`, nearest first, itself left
/// out.
pub fn nearest(vectors: &[SoundVector], of: usize, count: usize) -> Vec<usize> {
    let Some(from) = vectors.get(of) else {
        return Vec::new();
    };
    let mut others: Vec<(usize, f32)> = vectors
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != of)
        .map(|(index, vector)| (index, sound_distance(from, vector)))
        .collect();
    others.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    others
        .into_iter()
        .take(count)
        .map(|(index, _)| index)
        .collect()
}

/// The sound's place on the axes.
pub fn sound_vector(samples: &[f32]) -> SoundVector {
    let peak_level = peak(samples).max(1e-9);
    // t30: how long after its **loudest moment** the envelope takes to
    // fall 30 dB. From the loudest moment and not from the start, because
    // half the bank has a slow attack.
    let envelope: Vec<f32> = samples.chunks(480).map(rms).collect();
    let loudest_at = envelope
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let floor = peak_level * 0.0316;
    let t30 = envelope[loudest_at..]
        .iter()
        .position(|level| *level < floor)
        .unwrap_or(envelope.len() - loudest_at) as f32
        * 0.01;

    let mut weighted = 0.0f32;
    let mut total = 0.0f32;
    let mut hz = 60.0f32;
    while hz < 14_000.0 {
        let e = energy_at(samples, hz);
        weighted += hz.ln() * e;
        total += e;
        hz *= 1.2;
    }
    let centroid = if total > 1e-9 { weighted / total } else { 0.0 };

    let level = rms(samples).max(1e-9);
    let crest = (peak_level / level).ln();

    // Spectral flux over the first 300 ms — the attack's character.
    let attack = &samples[..samples.len().min((SR * 0.3) as usize)];
    let flux = attack
        .chunks(480)
        .map(rms)
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .sum::<f32>()
        / level;

    let mut shape = [0.0f32; 10];
    for (index, band) in shape.iter_mut().enumerate() {
        let lo = 60.0 * (14_000.0f32 / 60.0).powf(index as f32 / 10.0);
        let hi = 60.0 * (14_000.0f32 / 60.0).powf((index + 1) as f32 / 10.0);
        let mut hz = lo;
        while hz < hi {
            *band += energy_at(samples, hz);
            hz *= 1.08;
        }
    }
    let total: f32 = shape.iter().sum::<f32>().max(1e-9);
    for band in &mut shape {
        *band /= total;
    }

    SoundVector {
        scalar: [t30.max(1e-3).ln(), centroid, crest, (flux + 1.0).ln()],
        shape,
    }
}

fn render(patch: &Patch) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch.clone());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(KEY, VELOCITY));
    let total = (SR * SECONDS) as usize;
    let release_at = (SR * HOLD) as usize;
    let mut out = Vec::with_capacity(total);
    let mut done = 0usize;
    let mut released = false;
    while done < total {
        if !released && done >= release_at {
            sampler.release_all();
            released = true;
        }
        let n = 512.min(total - done);
        sampler.set_clock(crate::RenderClock {
            bpm: 120.0,
            position_sample: done as u64,
        });
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut l, &mut r]);
        for (a, b) in l.iter().zip(&r) {
            out.push((a + b) * 0.5);
        }
        done += n;
    }
    out
}

/// The envelope in [`Preview::COLUMNS`] columns, 0..1 of its own peak.
fn envelope(samples: &[f32]) -> Vec<f32> {
    let per = (samples.len() / Preview::COLUMNS).max(1);
    let columns: Vec<f32> = (0..Preview::COLUMNS)
        .map(|i| {
            let from = (i * per).min(samples.len());
            let to = ((i + 1) * per).min(samples.len());
            peak(&samples[from..to])
        })
        .collect();
    let top = columns.iter().cloned().fold(0.0f32, f32::max).max(1e-9);
    columns.into_iter().map(|c| c / top).collect()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

/// A Hann-windowed DFT at `hz`.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
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
