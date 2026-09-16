//! Cuts the factory Grand Piano's recordings out of a sampled grand.
//!
//! > *"the grand piano still sounds just a lot like a basic synth wave and
//! > not a actual grand piano ... maybe you could use the osc sampling
//! > feature to make the piano sound more realistic if you can find a grand
//! > piano one shot to use."* — Ty, 2026-09-16
//!
//! The recordings are the **Salamander Grand Piano** (Alexander Holm, a
//! Yamaha C5, CC BY 3.0 — `assets/flopsynth/samples/grand/README.md` carries
//! the credit), in the SF2 build FreePats assembled. This tool plays that
//! soundfont through Fontelle's own sampler — so what is cut is a note as
//! the soundfont's zones and envelopes shape it — and writes one short mono
//! file per key and layer:
//!
//! ```text
//! cargo run -p fontelle-app --example grand_samples --release -- \
//!     <SalamanderGrandPiano.sf2> assets/flopsynth/samples/grand
//! ```
//!
//! Two layers, **soft** and **hard**, because the piano crossfades between
//! them by velocity: a soft strike is not a quiet hard one, it has a
//! different spectrum, and a knob on the level cannot make one from the
//! other. A key every major third from C1 (and the bottom A), which with the
//! nearest-zone rule is never more than two semitones of transposition.
//! The bass is cut longer than the top because it rings longer, and at a
//! lower rate because there is nothing up there to keep; together that is
//! what keeps the set to a few megabytes in the binary
//! (`fontelle_core::factory_samples`).

use std::path::Path;

use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;

/// The keys: the bottom A, then every four semitones from C1 to C8 — on
/// the C's, so middle C is a recording and not a transposition.
fn keys() -> Vec<u8> {
    let mut keys = vec![21u8];
    keys.extend((24..=108).step_by(4));
    keys
}

/// How long a key's recording is kept, and at what rate. The C5's bass
/// rings for ten seconds; what is kept is where the note has fallen far
/// enough that the amp envelope's own fade takes it the rest of the way.
fn cut(key: u8) -> (f32, u32) {
    match key {
        0..=36 => (4.0, 24_000),
        37..=48 => (3.2, 24_000),
        49..=60 => (3.0, 32_000),
        61..=84 => (2.2, 32_000),
        _ => (1.3, 32_000),
    }
}

fn note_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "Cs", "D", "Ds", "E", "F", "Fs", "G", "Gs", "A", "As", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(key % 12)],
        i32::from(key / 12) - 1
    )
}

fn render(
    patch: &fontelle_core::Patch,
    store: &SampleStore,
    key: u8,
    velocity: u8,
    seconds: f32,
) -> Vec<f32> {
    let mut sampler = Sampler::new(patch.clone());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, velocity));
    let total = (SR * seconds) as usize;
    let mut out = Vec::with_capacity(total);
    let mut done = 0usize;
    while done < total {
        let frames = 512.min(total - done);
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        sampler.render(store, &mut [&mut left[..], &mut right[..]]);
        for (a, b) in left.iter().zip(&right) {
            out.push((a + b) * 0.5);
        }
        done += frames;
    }
    out
}

/// Windowed-sinc resampling from `SR` to `rate`, the low-pass a little
/// under the new Nyquist so nothing folds.
fn resample(input: &[f32], rate: u32) -> Vec<f32> {
    let ratio = SR / rate as f32;
    let cutoff = 0.45 / ratio; // cycles per input sample
    let half = 48i64;
    let out_len = (input.len() as f32 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let centre = i as f32 * ratio;
            let base = centre.floor() as i64;
            let mut sum = 0.0f32;
            for k in (base - half)..=(base + half) {
                if k < 0 || k >= input.len() as i64 {
                    continue;
                }
                let x = k as f32 - centre;
                let sinc = if x.abs() < 1e-6 {
                    2.0 * cutoff
                } else {
                    (std::f32::consts::TAU * cutoff * x).sin() / (std::f32::consts::PI * x)
                };
                // Blackman window over the taps.
                let w = x / half as f32;
                let window = 0.42
                    + 0.5 * (std::f32::consts::PI * w).cos()
                    + 0.08 * (std::f32::consts::TAU * w).cos();
                sum += input[k as usize] * sinc * window;
            }
            sum
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(sf2), Some(out)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: grand_samples <grand.sf2> <out-dir>");
        std::process::exit(2);
    };
    let out = Path::new(out);
    let mut store = SampleStore::new();
    let imported = fontelle_assets::import_sf2_preset(Path::new(sf2), 0, &mut store)
        .expect("the soundfont loads");
    println!(
        "{} layers in the soundfont's preset",
        imported.patch.layers.len()
    );
    // The velocity layers under middle C, so the two velocities below can
    // be read against the soundfont's own split.
    for layer in &imported.patch.layers {
        if layer.key_range.0 <= 60 && 60 <= layer.key_range.1 {
            println!(
                "  middle C: velocities {:?} root {} gain {:.1} dB",
                layer.vel_range, layer.root_key, layer.gain_db
            );
        }
    }

    for (layer, velocity) in [("soft", 40u8), ("hard", 120u8)] {
        let dir = out.join(layer);
        std::fs::create_dir_all(&dir).unwrap();
        // Cut every key first, then scale the whole layer by one gain: the
        // balance across the keyboard is the piano's own.
        let mut cuts: Vec<(u8, u32, Vec<f32>)> = Vec::new();
        for key in keys() {
            let (seconds, rate) = cut(key);
            let raw = render(&imported.patch, &store, key, velocity, seconds + 0.3);
            // The onset: two milliseconds before the first sample within
            // 66 dB of the note's own peak — its own, because the soft layer
            // renders thirty decibels under the hard one and is scaled up
            // below, and a threshold against full scale would keep the room
            // before a soft note and cut into a hard one.
            let peak = raw.iter().fold(0f32, |m, s| m.max(s.abs()));
            let onset = raw
                .iter()
                .position(|s| s.abs() > peak * 5e-4)
                .unwrap_or(0)
                .saturating_sub(96);
            let mut note: Vec<f32> = raw[onset..]
                .iter()
                .copied()
                .take((SR * seconds) as usize)
                .collect();
            // A one-millisecond fade in, so the first sample is rest whatever
            // the room was doing.
            for (i, s) in note.iter_mut().take(48).enumerate() {
                *s *= i as f32 / 48.0;
            }
            // A raised-cosine fade over the last 120 ms, so the cut is not a
            // click when a note outlives its recording.
            let fade = (SR * 0.12) as usize;
            let len = note.len();
            for (i, s) in note[len - fade..].iter_mut().enumerate() {
                let t = i as f32 / fade as f32;
                *s *= 0.5 + 0.5 * (std::f32::consts::PI * t).cos();
            }
            let note = resample(&note, rate);
            let peak = note.iter().fold(0f32, |m, s| m.max(s.abs()));
            println!(
                "  {layer} {:<4} v{velocity}: {:.1}s at {rate} Hz, peak {:.1} dB",
                note_name(key),
                seconds,
                20.0 * peak.max(1e-9).log10()
            );
            cuts.push((key, rate, note));
        }
        let loudest = cuts
            .iter()
            .flat_map(|(_, _, note)| note.iter().map(|s| s.abs()))
            .fold(0f32, f32::max);
        let gain = 0.95 / loudest.max(1e-9);
        for (key, rate, note) in cuts {
            let path = dir.join(format!("{}.wav", note_name(key)));
            let scaled: Vec<f32> = note.iter().map(|s| s * gain).collect();
            let mut w = fontelle_assets::WavWriter::create(&path, rate, 1).unwrap();
            w.write(&scaled).unwrap();
            w.finish().unwrap();
        }
        println!("{layer}: gain {:+.1} dB", 20.0 * gain.log10());
    }
}
