//! Measures Flopsynth's Grand Piano against a **sampled** grand, side by side.
//!
//! > *"the grand piano preset still doesnt sound much like a grand piano its
//! > sounding kind of like a mix between a clav and a electric piano."*
//!
//! `tests/grand_piano.rs` holds the claims a piano can be held to; this is
//! the tool that says what a real one measures on the same axes, so the row
//! can be voiced towards it rather than towards a guess. A soundfont grand is
//! the reference — the thing this synth piano is not pretending to be, and
//! exactly the right thing to be measured against.
//!
//! ```text
//! cargo run -p fontelle-app --example piano_probe --release -- <grand.sf2> [out-dir]
//! ```
//!
//! For each of a handful of keys and velocities it prints, for both: the
//! level of each partial against the fundamental at the strike and later on,
//! the brightness tilt over time, the time to fall 30 dB, and the RMS
//! envelope. With an out-dir it writes both notes to wav so an ear can judge
//! what the table cannot.

use std::path::Path;

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;

fn render(patch: Patch, store: &SampleStore, key: u8, velocity: u8, seconds: f32) -> Vec<f32> {
    let mut sampler = Sampler::new(patch);
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

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Seconds from the loudest ten milliseconds to the first one 30 dB under it.
fn t30(samples: &[f32]) -> f32 {
    let envelope: Vec<f32> = samples.chunks(480).map(rms).collect();
    let (loudest_at, loudest) = envelope
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    envelope[loudest_at..]
        .iter()
        .position(|level| *level < loudest * 0.0316)
        .unwrap_or(envelope.len() - loudest_at) as f32
        * 0.01
}

/// The level of partials 1..=n against full scale, in dB, over `seconds`
/// from `from`, searching a little either side of the exact harmonic so a
/// stretched (inharmonic) partial is still found.
fn partials(samples: &[f32], key: u8, from: f32, seconds: f32, n: usize) -> Vec<(f32, f32)> {
    let f0 = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
    let start = ((SR * from) as usize).min(samples.len());
    let samples = &samples[start..samples.len().min(start + (SR * seconds) as usize)];
    let len = samples.len() as f32;
    let power_at = |hz: f32| {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, sample) in samples.iter().enumerate() {
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / len).cos();
            let phase = std::f32::consts::TAU * hz * i as f32 / SR;
            re += sample * window * phase.cos();
            im -= sample * window * phase.sin();
        }
        (re * re + im * im).sqrt() / len
    };
    (1..=n)
        .map(|h| {
            let nominal = f0 * h as f32;
            // Up to 3 % sharp, which is where a piano's high partials sit.
            let mut best = (nominal, 0.0f32);
            let steps = 12;
            for s in 0..=steps {
                let hz = nominal * (1.0 + 0.03 * s as f32 / steps as f32);
                let p = power_at(hz);
                if p > best.1 {
                    best = (hz, p);
                }
            }
            (best.0 / nominal, db(best.1))
        })
        .collect()
}

fn describe(name: &str, out: &[f32], key: u8) {
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    let peak_at = out
        .iter()
        .position(|s| s.abs() >= peak * 0.999)
        .unwrap_or(0) as f32
        / SR;
    println!(
        "  {name:<10} peak {:5.1} dB at {:5.1} ms   t30 {:5.2} s",
        db(peak),
        peak_at * 1000.0,
        t30(out)
    );
    let env: Vec<String> = [0.01f32, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 3.0, 5.0]
        .iter()
        .map(|t| {
            let a = ((SR * t) as usize).min(out.len().saturating_sub(1));
            let b = out.len().min(a + 2400);
            format!("{:5.1}", db(rms(&out[a..b])))
        })
        .collect();
    println!(
        "             rms@ 10ms 50ms 100 200 500 1s 2s 3s 5s: {}",
        env.join(" ")
    );
    for (from, span) in [(0.0f32, 0.12f32), (0.3, 0.3), (1.0, 0.4), (2.5, 0.5)] {
        let table = partials(out, key, from, span, 12);
        let f = table[0].1;
        let rel: Vec<String> = table
            .iter()
            .map(|(ratio, level)| {
                format!(
                    "{:5.1}{}",
                    level - f,
                    if *ratio > 1.008 { "+" } else { " " }
                )
            })
            .collect();
        let low: f32 = table[..3].iter().map(|(_, l)| 10f32.powf(l / 10.0)).sum();
        let high: f32 = table[3..].iter().map(|(_, l)| 10f32.powf(l / 10.0)).sum();
        println!(
            "             {from:3.1}s partials 1..12 vs f0 ({:5.1} dB): {}  tilt {:5.1}",
            f,
            rel.join(""),
            10.0 * (high / low.max(1e-12)).log10()
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(sf2) = args.get(1) else {
        eprintln!("usage: piano_probe <grand.sf2> [out-dir]");
        std::process::exit(2);
    };
    let out_dir = args.get(2).map(Path::new);
    let mut store = SampleStore::new();
    let imported = fontelle_assets::import_sf2_preset(Path::new(sf2), 0, &mut store)
        .expect("the soundfont loads");
    let row = FACTORY
        .iter()
        .find(|row| row.name == "Grand Piano")
        .expect("a Grand Piano row");
    let empty = SampleStore::new();

    for key in [36u8, 48, 60, 72, 84, 96] {
        for velocity in [40u8, 100, 127] {
            println!("== key {key} velocity {velocity}");
            let sampled = render(imported.patch.clone(), &store, key, velocity, 6.0);
            let synth = render((row.build)(), &empty, key, velocity, 6.0);
            describe("sampled", &sampled, key);
            describe("flopsynth", &synth, key);
            if let Some(dir) = out_dir {
                std::fs::create_dir_all(dir).ok();
                for (name, data) in [("sampled", &sampled), ("flopsynth", &synth)] {
                    let path = dir.join(format!("{name}-{key}-{velocity}.wav"));
                    let mut w = fontelle_assets::WavWriter::create(&path, 48_000, 1).unwrap();
                    w.write(data).unwrap();
                    w.finish().unwrap();
                }
            }
        }
    }
}
