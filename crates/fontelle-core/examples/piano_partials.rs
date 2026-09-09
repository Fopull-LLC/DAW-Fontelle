//! Where the Grand Piano's partials actually sit, against where a real one's do.
//!
//! Every test in `tests/grand_piano.rs` measures partial *levels* and decay
//! rates, and all ten pass. None measures partial **frequencies** — and that
//! is the one property a wavetable cannot get right by construction, because a
//! wavetable is periodic and therefore exactly harmonic.
//!
//! A real piano string is stiff, so its nth partial sits at
//! `n * f0 * sqrt(1 + B*n^2)` rather than at `n * f0`. At middle C, B is about
//! 4e-4: the 2nd partial is 2 cents sharp, the 4th about 10, the 8th about 40,
//! the 16th over a semitone. That stretch is why piano tuners stretch-tune,
//! and it is most of what separates "struck string" from "organ with a decay".
//!
//! ```text
//! cargo run --release -p fontelle-core --example piano_partials
//! ```
use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;
const N: usize = 65_536;

fn render(patch: Patch, key: u8, frames: usize) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 110));
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

/// The peak frequency within a window around `about`, by parabolic
/// interpolation on the magnitude spectrum — good to a fraction of a bin.
fn peak_near(mag: &[f32], about: f32, bin_hz: f32) -> Option<f32> {
    let centre = (about / bin_hz).round() as usize;
    let span = ((about * 0.03) / bin_hz).ceil() as usize + 2;
    let lo = centre.saturating_sub(span).max(1);
    let hi = (centre + span).min(mag.len() - 2);
    if lo >= hi {
        return None;
    }
    let mut best = lo;
    for b in lo..=hi {
        if mag[b] > mag[best] {
            best = b;
        }
    }
    if mag[best] <= 0.0 {
        return None;
    }
    let (a, b, c) = (mag[best - 1], mag[best], mag[best + 1]);
    let denom = a - 2.0 * b + c;
    let delta = if denom.abs() > 1e-20 {
        0.5 * (a - c) / denom
    } else {
        0.0
    };
    Some((best as f32 + delta) * bin_hz)
}

fn main() {
    let row = FACTORY
        .iter()
        .find(|r| r.name == "Grand Piano")
        .expect("a Grand Piano");
    // Middle C, where the reference in `grand_piano.rs` was read.
    let key = 60u8;
    let f0 = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
    let signal = render((row.build)(), key, N + 8_192);

    let mut re: Vec<f32> = signal[4_096..]
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
    // Reuse the transform from the other probe by inlining a small one.
    fft(&mut re, &mut im);
    let mag: Vec<f32> = (0..N / 2)
        .map(|b| (re[b] * re[b] + im[b] * im[b]).sqrt())
        .collect();
    let bin_hz = SR / N as f32;

    // B for a real middle-C string, from the reference in the header.
    let b_real = 4.0e-4f32;
    println!(
        "middle C, f0 {f0:.2} Hz\n{:>3}  {:>10}  {:>10}  {:>9}  {:>12}",
        "n", "measured", "harmonic", "cents off", "a real piano"
    );
    for n in 1..=12u32 {
        let harmonic = n as f32 * f0;
        let Some(found) = peak_near(&mag, harmonic, bin_hz) else {
            continue;
        };
        let cents = 1200.0 * (found / harmonic).log2();
        let stretched = n as f32 * f0 * (1.0 + b_real * (n * n) as f32).sqrt();
        let real_cents = 1200.0 * (stretched / harmonic).log2();
        println!("{n:>3}  {found:>10.2}  {harmonic:>10.2}  {cents:>+9.1}  {real_cents:>+12.1}");
    }
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
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((ang * k as f32).cos(), (ang * k as f32).sin());
                let (ur, ui) = (re[start + k], im[start + k]);
                let (vr, vi) = (
                    re[start + k + len / 2] * wr - im[start + k + len / 2] * wi,
                    re[start + k + len / 2] * wi + im[start + k + len / 2] * wr,
                );
                re[start + k] = ur + vr;
                im[start + k] = ui + vi;
                re[start + k + len / 2] = ur - vr;
                im[start + k + len / 2] = ui - vi;
            }
        }
        len <<= 1;
    }
}
