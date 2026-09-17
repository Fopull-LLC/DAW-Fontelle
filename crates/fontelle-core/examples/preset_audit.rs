//! Audits the factory bank for the faults a preset can have that the bank's
//! gates do not catch: a tone that is mostly noise, a tone that vanishes
//! down the keyboard, and the Init patch's serial filter route running a
//! tone through a filter meant for the breath.
//!
//! > *"pan flute sounds very noisy right now it just sounds like noise and
//! > air and a faint wave in the background. make sure all the presets are
//! > up to par"* — Ty, 2026-09-17
//!
//! The Pan Pipe's sine was on the Init patch's **Serial** route, which runs
//! through Filter 2 — and Filter 2 was the breath's band-pass at 1.8 kHz,
//! resonance 1.2. At A4 (where `tests/wind_breath.rs` measures) enough of
//! the tone survives to pass; at C4 and below the tone is gone and the
//! noise rings in the resonance. The organ shelf had the same fault on
//! 2026-09-13. So this reads every preset at **three keys** — C3, C4, C5 —
//! and prints, per preset:
//!
//! - `hnr` — energy on the note's harmonics against everything else, in dB,
//!   at each key, for presets with a noise layer (the reading also catches
//!   unison detune, so it is only asked where there is noise to catch);
//! - `range` — how far the C3 and C5 levels sit from the C4 level, in dB,
//!   for every preset: a tone through a fixed band-pass falls off a cliff;
//! - `serial→F2` — which tone layers are on the serial route while Filter 2
//!   is enabled, and what Filter 2 is, so a band-pass or high-pass on the
//!   tone can be seen for what it is.
//!
//! ```text
//! cargo run -p fontelle-core --example preset_audit --release [category]
//! ```

use std::env;

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source};
use fontelle_dsp::{FilterRoute, SvfMode};

const SR: f32 = 48_000.0;
const N: usize = 16_384;

fn render(patch: Patch, key: u8, frames: usize) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        let n = 512.min(frames - out.len());
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut l[..], &mut r[..]]);
        for (a, b) in l.iter().zip(&r) {
            out.push((a + b) * 0.5);
        }
    }
    out
}

fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ar, ai) = (re[start + k], im[start + k]);
                let (br, bi) = (re[start + k + len / 2], im[start + k + len / 2]);
                let (tr, ti) = (br * cr - bi * ci, br * ci + bi * cr);
                re[start + k] = ar + tr;
                im[start + k] = ai + ti;
                re[start + k + len / 2] = ar - tr;
                im[start + k + len / 2] = ai - ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Energy on the harmonics of the key's own pitch against everything else,
/// in dB. The pitch is the key's rather than detected: a preset whose tone
/// is gone has no pitch to detect, and that is the case being looked for.
fn hnr_db(signal: &[f32], f0: f32) -> f32 {
    let mut re: Vec<f32> = signal
        .iter()
        .take(N)
        .enumerate()
        .map(|(i, x)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
            x * w
        })
        .collect();
    re.resize(N, 0.0);
    let mut im = vec![0.0f32; N];
    fft(&mut re, &mut im);
    let bin_hz = SR / N as f32;
    let slack = 4i64;
    let mut harmonic = vec![false; N / 2];
    let mut h = 1;
    while (h as f32) * f0 < SR * 0.45 {
        let centre = ((h as f32 * f0) / bin_hz).round() as i64;
        for d in -slack..=slack {
            let b = centre + d;
            if b > 0 && (b as usize) < N / 2 {
                harmonic[b as usize] = true;
            }
        }
        h += 1;
    }
    let (mut tone, mut noise) = (0.0f64, 0.0f64);
    for bin in (50.0 / bin_hz) as usize..N / 2 {
        let p = (re[bin] * re[bin] + im[bin] * im[bin]) as f64;
        if harmonic[bin] {
            tone += p;
        } else {
            noise += p;
        }
    }
    if noise <= 0.0 {
        return 99.0;
    }
    (10.0 * (tone / noise).log10()) as f32
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn hz(key: u8) -> f32 {
    440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0)
}

fn main() {
    let only: Option<String> = env::args().nth(1);
    println!(
        "{:<20} {:<16} {:>6} {:>6} {:>6} | {:>6} {:>6} | serial→F2",
        "preset", "category", "hnr48", "hnr60", "hnr72", "C3", "C5"
    );
    for row in FACTORY {
        if only
            .as_ref()
            .is_some_and(|want| !row.category.label().eq_ignore_ascii_case(want))
        {
            continue;
        }
        let patch = (row.build)();
        let has_noise = patch
            .layers
            .get(4)
            .is_some_and(|l| l.gain_db > fontelle_core::SILENT_DB);
        let mut hnr = [f32::NAN; 3];
        let mut level = [0.0f32; 3];
        for (i, key) in [48u8, 60, 72].into_iter().enumerate() {
            let signal = render(patch.clone(), key, N + 4_096);
            let steady = &signal[2_048..];
            level[i] = db(rms(&signal[..N]));
            if has_noise {
                hnr[i] = hnr_db(steady, hz(key));
            }
        }
        let serial: Vec<String> = patch
            .layers
            .iter()
            .enumerate()
            .take(4)
            .filter_map(|(index, layer)| match &layer.source {
                Source::Synth(osc)
                    if layer.gain_db > fontelle_core::SILENT_DB
                        && osc.filter_route == FilterRoute::Serial
                        && patch.filters[1].enabled =>
                {
                    Some(["A", "B", "C", "SUB"][index].to_string())
                }
                _ => None,
            })
            .collect();
        let f2 = &patch.filters[1];
        let f2_text = if serial.is_empty() {
            String::new()
        } else {
            let mode = match f2.mode {
                SvfMode::Lowpass => "LP",
                SvfMode::Highpass => "HP",
                SvfMode::Bandpass => "BP",
                _ => "??",
            };
            format!(
                "{} through {mode} {:.0} Hz q{:.2}{}",
                serial.join(""),
                f2.cutoff_hz,
                f2.resonance,
                if matches!(f2.mode, SvfMode::Bandpass | SvfMode::Highpass) {
                    "  <- tone in the breath's filter"
                } else {
                    ""
                }
            )
        };
        let fmt = |x: f32| {
            if x.is_nan() {
                "     -".to_string()
            } else {
                format!("{x:6.1}")
            }
        };
        let flag = |i: usize| {
            let d = level[i] - level[1];
            if d < -12.0 { " <-cliff" } else { "" }
        };
        println!(
            "{:<20} {:<16} {} {} {} | {:>+6.1}{} {:>+6.1}{} | {}",
            row.name,
            row.category.label(),
            fmt(hnr[0]),
            fmt(hnr[1]),
            fmt(hnr[2]),
            level[0] - level[1],
            flag(0),
            level[2] - level[1],
            flag(2),
            f2_text
        );
    }
}
