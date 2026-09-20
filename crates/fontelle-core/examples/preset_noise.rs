//! **How much of each preset is noise rather than tone?**
//!
//! > *"a lot of the presets sound very noisy especially the wind instruments."*
//!
//! A held note is rendered and its spectrum split in two: energy that sits on
//! a harmonic of the note, and energy that does not. The second over the total
//! is the reading. A flute has breath in it on purpose and a saw lead has
//! none, so the number is not a score — but a preset whose *tone* is buried
//! under its noise is one nobody would call by its name, and that is what the
//! report describes.
//!
//! ```text
//! cargo run --release -p fontelle-core --example preset_noise
//! ```
use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

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
        // The transport moves, as the gate's does: a free-running LFO
        // reads its phase off the clock.
        sampler.set_clock(fontelle_core::RenderClock {
            bpm: 120.0,
            position_sample: out.len() as u64,
        });
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

/// The strongest periodicity in `signal`, in Hz, by autocorrelation.
///
/// **Detected rather than assumed**, which the first two versions of this got
/// wrong in opposite ways. A comb built on the key you pressed calls an 808
/// kick and every sub-octave preset pure noise; spectral flatness over the
/// whole band calls every low-passed preset pure tone, because the empty
/// bands above the cutoff sink the geometric mean. Finding the period first
/// asks the only question that matters — *how much of this is the pitch and
/// how much is hiss* — wherever the pitch turns out to be.
fn detect_f0(signal: &[f32]) -> f32 {
    let lo = (SR / 1_200.0) as usize;
    let hi = (SR / 40.0) as usize;
    let n = signal.len().min(8_192);
    let mean = signal[..n].iter().sum::<f32>() / n as f32;
    let x: Vec<f32> = signal[..n].iter().map(|s| s - mean).collect();
    let energy: f32 = x.iter().map(|s| s * s).sum();
    if energy <= 0.0 {
        return 0.0;
    }
    let (mut best_lag, mut best) = (0usize, 0.0f32);
    for lag in lo..hi.min(n / 2) {
        let mut sum = 0.0f32;
        for i in 0..n - lag {
            sum += x[i] * x[i + lag];
        }
        // Normalised, so a long lag is not favoured simply for overlapping
        // less signal.
        let r = sum / (n - lag) as f32;
        if r > best {
            best = r;
            best_lag = lag;
        }
    }
    if best_lag == 0 {
        0.0
    } else {
        SR / best_lag as f32
    }
}

/// Harmonic-to-noise ratio in dB: energy on the harmonics of `f0` against
/// everything else. A pure tone is large, a hiss is negative.
fn hnr_db(signal: &[f32], f0: f32) -> f32 {
    if f0 <= 0.0 {
        return f32::NEG_INFINITY;
    }
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
    // Three bins either side of each harmonic: a Hann window spreads a line
    // about that far, and unison detune moves it a little more.
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

fn main() {
    let key = 69u8;
    let mut rows: Vec<(f32, &str, &str, f32)> = Vec::new();
    for row in FACTORY {
        let signal = render((row.build)(), key, N + 4_096);
        let steady = &signal[2_048..];
        let f0 = detect_f0(steady);
        rows.push((hnr_db(steady, f0), row.category.label(), row.name, f0));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    // `-- <category>` prints just that shelf, whole.
    if let Some(want) = std::env::args().nth(1) {
        println!("== {want} ==");
        for (hnr, category, name, f0) in rows.iter() {
            if category.to_lowercase().contains(&want.to_lowercase()) {
                println!("  {hnr:6.1} dB  {name:<16} f0 {f0:6.1} Hz");
            }
        }
        return;
    }
    println!("== the twenty-five noisiest (harmonic-to-noise, dB) ==");
    for (hnr, category, name, f0) in rows.iter().take(25) {
        println!("  {hnr:6.1} dB  {category:<18} {name:<16} f0 {f0:6.1} Hz");
    }
    let mut by_category: std::collections::BTreeMap<&str, Vec<f32>> = Default::default();
    for (hnr, category, _, _) in &rows {
        by_category.entry(category).or_default().push(*hnr);
    }
    println!("\n== by category (median HNR) ==");
    let mut cats: Vec<(f32, &str, usize)> = by_category
        .iter()
        .map(|(c, v)| {
            let mut v = v.clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (v[v.len() / 2], *c, v.len())
        })
        .collect();
    cats.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (median, category, n) in cats {
        println!("  {median:6.1} dB  {category:<18} ({n})");
    }
}
