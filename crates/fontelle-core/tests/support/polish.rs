//! The readings behind `tests/preset_polish.rs`, shared with
//! `examples/preset_polish.rs` so the gate and the table that tunes a row
//! against it measure one thing, the same way.

use std::sync::OnceLock;

use fontelle_core::flopsynth::presets::{FACTORY, FactoryPreset};
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

pub const SR: f32 = 48_000.0;
const MS: usize = 48;

pub fn render(
    patch: Patch,
    key: u8,
    velocity: u8,
    hold: f32,
    seconds: f32,
) -> (Vec<f32>, Vec<f32>) {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, velocity));
    let total = (SR * seconds) as usize;
    let release_at = (SR * hold) as usize;
    let (mut left, mut right) = (Vec::with_capacity(total), Vec::with_capacity(total));
    let mut released = false;
    while left.len() < total {
        let done = left.len();
        if !released && done >= release_at {
            sampler.release_all();
            released = true;
        }
        let mut n = 512.min(total - done);
        if !released {
            n = n.min(release_at - done);
        }
        // The transport moves, as in the bank's gate: a free-running LFO
        // reads its phase off the clock.
        sampler.set_clock(fontelle_core::RenderClock {
            bpm: 120.0,
            position_sample: done as u64,
        });
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut l[..], &mut r[..]]);
        left.extend_from_slice(&l);
        right.extend_from_slice(&r);
    }
    (left, right)
}

pub fn mono(l: &[f32], r: &[f32]) -> Vec<f32> {
    l.iter().zip(r).map(|(a, b)| (a + b) * 0.5).collect()
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn loudest50(s: &[f32]) -> f32 {
    s.chunks((SR * 0.05) as usize).map(rms).fold(0.0, f32::max)
}

/// Two RBJ high-passes at 6 kHz: what is left of a sound that a click is
/// made of.
fn highpass(s: &[f32]) -> Vec<f32> {
    let w = std::f32::consts::TAU * 6_000.0 / SR;
    let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
    let cw = w.cos();
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = (
        (1.0 + cw) / 2.0 / a0,
        -(1.0 + cw) / a0,
        (1.0 + cw) / 2.0 / a0,
    );
    let (a1, a2) = (-2.0 * cw / a0, (1.0 - alpha) / a0);
    let mut x = s.to_vec();
    for _ in 0..2 {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0, 0.0, 0.0);
        for v in x.iter_mut() {
            let y = b0 * *v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, y);
            *v = y;
        }
    }
    x
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

const N: usize = 16_384;

fn power_spectrum(s: &[f32]) -> Vec<f32> {
    let mut re: Vec<f32> = s
        .iter()
        .take(N)
        .enumerate()
        .map(|(i, x)| x * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()))
        .collect();
    re.resize(N, 0.0);
    let mut im = vec![0.0f32; N];
    fft(&mut re, &mut im);
    (0..N / 2).map(|b| re[b] * re[b] + im[b] * im[b]).collect()
}

/// Energy **off** the note's harmonic grid (and its sub-octaves' — a layer
/// an octave or two down is on its own grid) against energy on it, in dB,
/// 50 Hz..20 kHz.
fn off_grid_db(s: &[f32], f0: f32) -> f32 {
    let p = power_spectrum(s);
    let bin = SR / N as f32;
    let grid = f0 / 4.0;
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (b, v) in p.iter().enumerate().skip((50.0 / bin) as usize) {
        let hz = b as f32 * bin;
        if hz > 20_000.0 {
            break;
        }
        if (hz - (hz / grid).round() * grid).abs() < 6.0 * bin {
            on += *v as f64;
        } else {
            off += *v as f64;
        }
    }
    (10.0 * (off.max(1e-30) / on.max(1e-30)).log10()) as f32
}

/// Energy under 30 Hz against the whole, in dB.
fn sub30_db(s: &[f32]) -> f32 {
    let p = power_spectrum(s);
    let bin = SR / N as f32;
    let total: f64 = p.iter().skip(1).map(|v| *v as f64).sum();
    let low: f64 = p
        .iter()
        .enumerate()
        .skip(1)
        .take_while(|(b, _)| (*b as f32) * bin < 30.0)
        .map(|(_, v)| *v as f64)
        .sum();
    (10.0 * (low.max(1e-30) / total.max(1e-30)).log10()) as f32
}

fn hz(key: u8) -> f32 {
    440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0)
}

/// One preset's readings. The note is middle C at velocity 100, held for
/// two seconds, long enough for the slowest pad to arrive, and rendered
/// three seconds past the release.
pub struct Reading {
    pub name: &'static str,
    /// The worst click: how far a 2 ms burst of high-passed energy stands
    /// above the loudest such window in the 30 ms either side of it, in
    /// dB; how loud it is against the note's loudest 50 ms; and when.
    pub click: (f32, f32, f32),
    /// The same, inside the first 5 ms.
    pub onset: (f32, f32),
    /// The mean of the note's body against its RMS, in dB.
    pub dc: f32,
    /// Energy under 30 Hz in the note's body, in dB of the whole.
    pub sub: f32,
    /// Off-grid energy at C6 as written, and what 4× oversampling takes
    /// off it, in dB.
    pub alias: (f32, f32),
    /// When the loudest 10 ms of the held note arrives, in seconds.
    pub peak_at: f32,
}

const HOLD: f32 = 2.0;

pub fn read(row: &FactoryPreset) -> Reading {
    let patch = (row.build)();
    let (l, r) = render(patch.clone(), 60, 100, HOLD, HOLD + 3.0);
    let m = mono(&l, &r);
    let loud = db(loudest50(&m));

    // A click stands above what comes before it **and** after it: a bright
    // sound starting is not one, because what follows is as bright; a gate
    // slamming shut on a dark tone, or a release that steps, is.
    let hf = highpass(&m);
    let step = MS / 2;
    let windows: Vec<f32> = (0..hf.len().saturating_sub(2 * MS))
        .step_by(step)
        .map(|i| db(rms(&hf[i..i + 2 * MS])))
        .collect();
    let per_ms = MS / step;
    let mut click = (-99.0f32, -99.0f32, 0.0f32);
    let mut onset = (-99.0f32, -99.0f32);
    for (k, level) in windows.iter().enumerate() {
        if level - loud < -45.0 {
            continue;
        }
        let before_from = k.saturating_sub(30 * per_ms);
        let before_to = k.saturating_sub(2 * per_ms).max(before_from);
        let before = windows[before_from..before_to]
            .iter()
            .fold(-200.0f32, |a, b| a.max(*b));
        let after_from = (k + 3 * per_ms).min(windows.len());
        let after_to = (k + 33 * per_ms).min(windows.len());
        let after = windows[after_from..after_to]
            .iter()
            .fold(-200.0f32, |a, b| a.max(*b));
        let stands = level - before.max(after);
        if k < 5 * per_ms {
            if stands > onset.0 {
                onset = (stands, level - loud);
            }
        } else if stands > click.0 {
            click = (stands, level - loud, (k * step) as f32 / SR);
        }
    }

    // The note's body: a third of a second centred on its loudest moment,
    // so a slow attack is read where it has arrived and a pluck where it
    // still rings — not in the silence before one or after the other.
    let release = (HOLD * SR) as usize;
    let peak_at = m[..release]
        .chunks(480)
        .enumerate()
        .fold((0usize, 0.0f32), |best, (i, w)| {
            let v = rms(w);
            if v > best.1 { (i, v) } else { best }
        })
        .0 as f32
        * 0.01;
    let start = ((peak_at * SR) as usize)
        .saturating_sub(N / 2)
        .max((0.02 * SR) as usize)
        .min(m.len() - N);
    let body = &m[start..start + N];
    let (dc, sub) = if rms(body) < 1e-5 {
        (-99.0, -99.0)
    } else {
        let mean = body.iter().sum::<f32>() / N as f32;
        (db(mean.abs()) - db(rms(body)), sub30_db(body))
    };

    let alias_key = 84u8;
    let off_grid = |p: Patch| {
        let (l, r) = render(p, alias_key, 100, 1.0, 0.75);
        off_grid_db(&mono(&l, &r)[(0.35 * SR) as usize..], hz(alias_key))
    };
    let as_written = off_grid(patch.clone());
    let mut x4 = patch.clone();
    x4.oversampling = fontelle_dsp::Oversampling::X4;
    let alias = (as_written, as_written - off_grid(x4));

    Reading {
        name: row.name,
        click,
        onset,
        dc,
        sub,
        alias,
        peak_at,
    }
}

/// The bank, read once and shared by every test here — in parallel, since
/// it is four hundred notes of five seconds each.
pub fn bank() -> &'static [Reading] {
    static BANK: OnceLock<Vec<Reading>> = OnceLock::new();
    BANK.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 8);
        let chunk = FACTORY.len().div_ceil(threads);
        std::thread::scope(|scope| {
            let handles: Vec<_> = FACTORY
                .chunks(chunk)
                .map(|rows| scope.spawn(move || rows.iter().map(read).collect::<Vec<_>>()))
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("a reading panicked"))
                .collect()
        })
    })
}
