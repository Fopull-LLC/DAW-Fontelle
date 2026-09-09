//! **A wind instrument's note has to be mostly its note.**
//!
//! > *"a lot of the presets sound very noisy especially the wind instruments."*
//!
//! Measured, and the report is exact. Breath is what makes a flute a flute, so
//! these presets carry a noise layer on purpose — but four of them carried
//! *more noise than tone*. Read as a harmonic-to-noise ratio over the sustain,
//! with the pitch found by autocorrelation rather than assumed:
//!
//! ```text
//! Shakuhachi  -3.6 dB      Oboe          50.2 dB
//! Pan Pipe    -1.5 dB      Bassoon       50.3 dB
//! Piccolo     -0.1 dB      Clarinet      44.7 dB
//! Flute        2.1 dB      Solo Trumpet  36.6 dB
//! ```
//!
//! The right-hand column is the same shelf's reed and brass presets, which
//! have no breath layer at all. Nothing here asks a flute to measure like an
//! oboe — it asks that the tone be the larger half of it.
//!
//! **Ten decibels**, which is the tone carrying ten times the energy of the
//! breath. The floor is read off the shelf rather than chosen: the lowest of
//! the presets nobody complained about is Brass Section at 13.0 dB, and this
//! sits under it with room for a preset that is *meant* to be airy.

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

/// The strongest periodicity, in Hz. Detected rather than assumed, because a
/// preset that transposes would otherwise be measured against a comb its
/// partials never touch.
fn detect_f0(signal: &[f32]) -> f32 {
    let lo = (SR / 1_200.0) as usize;
    let hi = (SR / 40.0) as usize;
    let n = signal.len().min(8_192);
    let mean = signal[..n].iter().sum::<f32>() / n as f32;
    let x: Vec<f32> = signal[..n].iter().map(|s| s - mean).collect();
    if x.iter().map(|s| s * s).sum::<f32>() <= 0.0 {
        return 0.0;
    }
    let (mut best_lag, mut best) = (0usize, 0.0f32);
    for lag in lo..hi.min(n / 2) {
        let mut sum = 0.0f32;
        for i in 0..n - lag {
            sum += x[i] * x[i + lag];
        }
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

/// Energy on the harmonics of `f0` against everything else, in dB.
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

/// See the header: read off the shelf's own quietest uncomplained-about
/// preset, not chosen.
const FLOOR_DB: f32 = 10.0;

/// Flopsynth's slot order is fixed — A, B, C, sub, **noise** — so this is the
/// breath layer of every patch the bank builds.
const NOISE_LAYER: usize = 4;

/// Whether this preset has a breath layer at all.
///
/// The claim is about *breath*, so it is asked only of presets that have some.
/// Without this the reading also catches unison detune, which spreads a
/// partial into sidebands that no comb can hold: `brass()` runs three voices
/// nine cents apart and no noise whatever, and it read as 9.7 dB — noisy by a
/// measure of hiss, and not remotely hissy.
fn has_breath(patch: &Patch) -> bool {
    // The same test the voice itself uses to decide a layer is audible — a
    // layer at `SILENT_DB` is skipped entirely.
    patch
        .layers
        .get(NOISE_LAYER)
        .is_some_and(|layer| layer.gain_db > fontelle_core::SILENT_DB)
}

#[test]
fn every_wind_preset_is_more_note_than_breath() {
    let mut bad = Vec::new();
    let mut checked = 0;
    for row in FACTORY {
        if row.category.label() != "Brass & Winds" {
            continue;
        }
        let patch = (row.build)();
        if !has_breath(&patch) {
            continue;
        }
        checked += 1;
        let signal = render(patch, 69, N + 4_096);
        // Past the attack: the chiff at the front is *meant* to be breath.
        let steady = &signal[2_048..];
        let hnr = hnr_db(steady, detect_f0(steady));
        if hnr < FLOOR_DB {
            bad.push(format!("  {:6.1} dB  {}", hnr, row.name));
        }
    }
    assert!(
        checked >= 6,
        "only {checked} winds with breath were checked"
    );
    bad.sort();
    assert!(
        bad.is_empty(),
        "winds whose sustain is more breath than note (floor {FLOOR_DB} dB):\n{}",
        bad.join("\n")
    );
}

/// The other half: the fix must not simply delete the breath.
#[test]
fn the_flutes_still_have_breath_in_them() {
    for name in ["Flute", "Piccolo", "Pan Pipe", "Shakuhachi"] {
        let row = FACTORY
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no {name}"));
        let signal = render((row.build)(), 69, N + 4_096);
        let steady = &signal[2_048..];
        let hnr = hnr_db(steady, detect_f0(steady));
        assert!(
            hnr < 34.0,
            "{name} has had its breath taken out entirely: {hnr:.1} dB"
        );
    }
}
