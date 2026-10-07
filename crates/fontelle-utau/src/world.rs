//! A WORLD-style vocoder: F0, spectral envelope (CheapTrick) and band
//! aperiodicity analysis, and pulse-by-pulse synthesis from them. After
//! Morise's WORLD (modified BSD), written here from its published
//! algorithms; the F0 estimator is YIN's (de Cheveigné & Kawahara) and the
//! aperiodicity a period-to-period comparison in bands, simpler than D4C.
//!
//! A sample analysed once can be sung at any pitch and length with its
//! formants where they were: the envelope is a smooth function of
//! frequency, not a set of harmonics, so new harmonics are drawn from it.
//!
//! Levels: an envelope frame is a power spectrum whose mean over the bins
//! is the signal's variance per sample there; synthesis keeps that (noise
//! of unit variance per sample, pulses scaled by the square root of their
//! interval), so loudness survives analysis and resynthesis.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Seconds between frames.
pub const FRAME_PERIOD: f64 = 0.005;

/// The pulse rate used to time noise where nothing is voiced.
const UNVOICED_RATE: f64 = 500.0;

/// What a sample is, frame by frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub fs: u32,
    pub fft_size: usize,
    /// Hz, 0 where unvoiced.
    pub f0: Vec<f32>,
    /// Power spectra, `fft_size / 2 + 1` per frame, flat.
    pub sp: Vec<f32>,
    /// Aperiodicity (amplitude ratio, 0..1) at [`ap_band_hz`] frequencies,
    /// [`AP_BANDS`] per frame, flat.
    pub ap: Vec<f32>,
}

/// The frequencies coarse aperiodicity is kept at.
pub const AP_BANDS: usize = 16;

pub fn ap_band_hz(i: usize, fs: u32) -> f64 {
    (fs as f64 / 2.0) * i as f64 / (AP_BANDS - 1) as f64
}

impl Analysis {
    pub fn frames(&self) -> usize {
        self.f0.len()
    }

    pub fn bins(&self) -> usize {
        self.fft_size / 2 + 1
    }

    pub fn sp_frame(&self, i: usize) -> &[f32] {
        let b = self.bins();
        &self.sp[i * b..(i + 1) * b]
    }

    pub fn ap_frame(&self, i: usize) -> &[f32] {
        &self.ap[i * AP_BANDS..(i + 1) * AP_BANDS]
    }

    /// Bytes it holds, for cache budgets.
    pub fn bytes(&self) -> usize {
        (self.f0.len() + self.sp.len() + self.ap.len()) * 4
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysisOptions {
    pub f0_floor: f32,
    pub f0_ceil: f32,
    /// The sample's expected F0 (a `.frq`'s average, or the note it was
    /// recorded on), to keep the estimate off octave errors.
    pub prior: Option<f32>,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            f0_floor: 71.0,
            f0_ceil: 1100.0,
            prior: None,
        }
    }
}

/// CheapTrick's FFT size: room for three periods of the lowest F0.
pub fn fft_size_for(fs: u32, f0_floor: f32) -> usize {
    let n = 3.0 * fs as f64 / f64::from(f0_floor.max(20.0));
    1usize << (1 + n.log2().floor() as u32)
}

/// Real FFTs of one size with their scratch space.
struct Fft {
    n: usize,
    fwd: Arc<dyn RealToComplex<f64>>,
    inv: Arc<dyn ComplexToReal<f64>>,
    time: Vec<f64>,
    freq: Vec<Complex<f64>>,
    scratch_f: Vec<Complex<f64>>,
    scratch_i: Vec<Complex<f64>>,
}

impl Fft {
    fn new(n: usize) -> Self {
        // Plans are made once per size and shared: planning is far slower
        // than the buffers each user needs of its own.
        type Plans = (Arc<dyn RealToComplex<f64>>, Arc<dyn ComplexToReal<f64>>);
        static PLANS: OnceLock<Mutex<HashMap<usize, Plans>>> = OnceLock::new();
        let (fwd, inv) = {
            let mut plans = PLANS
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            plans
                .entry(n)
                .or_insert_with(|| {
                    let mut planner = RealFftPlanner::<f64>::new();
                    (planner.plan_fft_forward(n), planner.plan_fft_inverse(n))
                })
                .clone()
        };
        let scratch_f = fwd.make_scratch_vec();
        let scratch_i = inv.make_scratch_vec();
        Fft {
            n,
            time: fwd.make_input_vec(),
            freq: fwd.make_output_vec(),
            fwd,
            inv,
            scratch_f,
            scratch_i,
        }
    }

    /// `self.time` → `self.freq`.
    fn forward(&mut self) {
        let _ = self
            .fwd
            .process_with_scratch(&mut self.time, &mut self.freq, &mut self.scratch_f);
    }

    /// `self.freq` → `self.time`, unnormalised (scaled by n).
    fn inverse(&mut self) {
        self.freq[0].im = 0.0;
        let last = self.freq.len() - 1;
        self.freq[last].im = 0.0;
        let _ = self
            .inv
            .process_with_scratch(&mut self.freq, &mut self.time, &mut self.scratch_i);
    }
}

fn frame_count(len: usize, fs: u32) -> usize {
    (len as f64 / fs as f64 / FRAME_PERIOD) as usize + 1
}

/// F0 every [`FRAME_PERIOD`], 0 where unvoiced.
pub fn estimate_f0(x: &[f32], fs: u32, opts: &AnalysisOptions) -> Vec<f32> {
    let frames = frame_count(x.len(), fs);
    let x: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let peak_rms = loudest_rms(&x, fs);
    let mut yin = Yin::new(fs, opts.f0_floor);
    // Without a prior, a coarse pass over every fourth frame finds the
    // voice's register, and the real pass searches an octave either side.
    let prior = opts.prior.or_else(|| {
        let mut found: Vec<f64> = (0..frames)
            .step_by(4)
            .filter_map(|i| {
                yin.at(
                    &x,
                    i as f64 * FRAME_PERIOD,
                    opts.f0_floor.into(),
                    opts.f0_ceil.into(),
                    peak_rms,
                )
            })
            .map(|(f, _)| f)
            .collect();
        if found.len() < 3 {
            return None;
        }
        found.sort_by(f64::total_cmp);
        Some(found[found.len() / 2] as f32)
    });
    let (lo, hi) = match prior {
        Some(p) => (
            (f64::from(p) / 1.9).max(f64::from(opts.f0_floor)),
            (f64::from(p) * 1.9).min(f64::from(opts.f0_ceil)),
        ),
        None => (f64::from(opts.f0_floor), f64::from(opts.f0_ceil)),
    };
    let mut f0: Vec<f32> = (0..frames)
        .into_par_iter()
        .map_init(
            || Yin::new(fs, opts.f0_floor),
            |yin, i| {
                yin.at(&x, i as f64 * FRAME_PERIOD, lo, hi, peak_rms)
                    .map(|(f, _)| f as f32)
                    .unwrap_or(0.0)
            },
        )
        .collect();
    clean_f0(&mut f0);
    f0
}

/// The RMS of the loudest 20 ms, for a relative silence threshold.
fn loudest_rms(x: &[f64], fs: u32) -> f64 {
    let w = (0.02 * fs as f64) as usize;
    let mut best: f64 = 0.0;
    let mut i = 0;
    while i + w <= x.len() {
        let e = x[i..i + w].iter().map(|v| v * v).sum::<f64>() / w as f64;
        best = best.max(e);
        i += w / 2;
    }
    best.sqrt()
}

/// Voiced runs shorter than 3 frames go; single unvoiced frames between
/// voiced ones are filled.
fn clean_f0(f0: &mut [f32]) {
    let n = f0.len();
    for i in 1..n.saturating_sub(1) {
        if f0[i] == 0.0 && f0[i - 1] > 0.0 && f0[i + 1] > 0.0 {
            f0[i] = 0.5 * (f0[i - 1] + f0[i + 1]);
        }
    }
    let mut i = 0;
    while i < n {
        if f0[i] > 0.0 {
            let mut j = i;
            while j < n && f0[j] > 0.0 {
                j += 1;
            }
            if j - i < 3 {
                for v in &mut f0[i..j] {
                    *v = 0.0;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

/// YIN's cumulative mean normalised difference, by FFT.
struct Yin {
    fs: f64,
    w: usize,
    max_lag: usize,
    fft: Fft,
    a: Vec<Complex<f64>>,
}

impl Yin {
    fn new(fs: u32, f0_floor: f32) -> Self {
        let max_lag = (fs as f64 / f64::from(f0_floor)).ceil() as usize + 2;
        let w = max_lag;
        let n = (2 * w + max_lag).next_power_of_two();
        let fft = Fft::new(n);
        let a = vec![Complex::new(0.0, 0.0); n / 2 + 1];
        Yin {
            fs: fs as f64,
            w,
            max_lag,
            fft,
            a,
        }
    }

    /// (F0, aperiodicity measure) at `t` seconds, `None` if unvoiced.
    fn at(&mut self, x: &[f64], t: f64, lo: f64, hi: f64, peak_rms: f64) -> Option<(f64, f64)> {
        let (w, max_lag) = (self.w, self.max_lag);
        let start = (t * self.fs).round() as isize - (w / 2) as isize;
        let seg: Vec<f64> = (0..w + max_lag)
            .map(|i| {
                let k = start + i as isize;
                if k >= 0 && (k as usize) < x.len() {
                    x[k as usize]
                } else {
                    0.0
                }
            })
            .collect();
        let rms = (seg[..w].iter().map(|v| v * v).sum::<f64>() / w as f64).sqrt();
        if rms < peak_rms * 0.01 || rms < 1e-5 {
            return None;
        }
        // r(τ) = Σ_j s[j] s[j+τ], j < w.
        let n = self.fft.n;
        self.fft.time.iter_mut().for_each(|v| *v = 0.0);
        self.fft.time[..w].copy_from_slice(&seg[..w]);
        self.fft.forward();
        self.a.copy_from_slice(&self.fft.freq);
        self.fft.time.iter_mut().for_each(|v| *v = 0.0);
        self.fft.time[..w + max_lag].copy_from_slice(&seg);
        self.fft.forward();
        for (b, a) in self.fft.freq.iter_mut().zip(&self.a) {
            *b *= a.conj();
        }
        self.fft.inverse();
        let r: Vec<f64> = self.fft.time[..=max_lag]
            .iter()
            .map(|v| v / n as f64)
            .collect();
        // Energies of the shifted windows.
        let mut cum = vec![0.0; seg.len() + 1];
        for (i, v) in seg.iter().enumerate() {
            cum[i + 1] = cum[i] + v * v;
        }
        let e = |tau: usize| cum[tau + w] - cum[tau];
        let d: Vec<f64> = (0..=max_lag)
            .map(|tau| (e(0) + e(tau) - 2.0 * r[tau]).max(0.0))
            .collect();
        let mut cmnd = vec![1.0; max_lag + 1];
        let mut acc = 0.0;
        for tau in 1..=max_lag {
            acc += d[tau];
            cmnd[tau] = if acc > 0.0 {
                d[tau] * tau as f64 / acc
            } else {
                1.0
            };
        }
        let tau_min = ((self.fs / hi).floor() as usize).max(2);
        let tau_max = ((self.fs / lo).ceil() as usize).min(max_lag - 1);
        if tau_min >= tau_max {
            return None;
        }
        // The first dip under the threshold, to its bottom; else the
        // lowest point.
        let mut pick = None;
        let mut tau = tau_min;
        while tau < tau_max {
            if cmnd[tau] < 0.12 {
                while tau + 1 < tau_max && cmnd[tau + 1] < cmnd[tau] {
                    tau += 1;
                }
                pick = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = pick.unwrap_or_else(|| {
            (tau_min..tau_max)
                .min_by(|&a, &b| cmnd[a].total_cmp(&cmnd[b]))
                .unwrap_or(tau_min)
        });
        if cmnd[tau] > 0.35 {
            return None;
        }
        // Parabolic refinement on the difference function.
        let (y0, y1, y2) = (d[tau - 1], d[tau], d[tau + 1]);
        let den = y0 - 2.0 * y1 + y2;
        let shift = if den.abs() > 1e-12 {
            (0.5 * (y0 - y2) / den).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let f = self.fs / (tau as f64 + shift);
        if f < lo * 0.97 || f > hi * 1.03 {
            return None;
        }
        Some((f, cmnd[tau]))
    }
}

/// F0, envelope and aperiodicity of `x`.
pub fn analyze(x: &[f32], fs: u32, opts: &AnalysisOptions) -> Analysis {
    let f0 = estimate_f0(x, fs, opts);
    let fft_size = fft_size_for(fs, opts.f0_floor);
    let xd: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let sp = cheaptrick(&xd, fs, &f0, fft_size, opts.f0_floor);
    let ap = aperiodicity(&xd, fs, &f0, fft_size);
    Analysis {
        fs,
        fft_size,
        f0,
        sp,
        ap,
    }
}

fn sample_at(x: &[f64], k: isize) -> f64 {
    if k >= 0 && (k as usize) < x.len() {
        x[k as usize]
    } else {
        0.0
    }
}

/// CheapTrick: a pitch-adaptive window three periods long, the power
/// spectrum smoothed over 2/3 of F0, then liftered so the envelope passes
/// through the harmonics rather than between them.
fn cheaptrick(x: &[f64], fs: u32, f0: &[f32], fft_size: usize, f0_floor: f32) -> Vec<f32> {
    let bins = fft_size / 2 + 1;
    let mut out = vec![0.0f32; f0.len() * bins];
    let fsd = fs as f64;
    // Frames are independent: analysed in parallel, each thread with its
    // own FFT and buffers.
    let init = || (Fft::new(fft_size), vec![0.0; bins], vec![0.0; bins]);
    out.par_chunks_mut(bins)
        .zip(f0.par_iter())
        .enumerate()
        .for_each_init(init, |(fft, power, smooth), (i, (o, &f))| {
            let f0v = if f > 0.0 {
                f64::from(f.max(f0_floor))
            } else {
                UNVOICED_RATE
            };
            let t = i as f64 * FRAME_PERIOD;
            let half = (1.5 * fsd / f0v).round() as isize;
            let centre = (t * fsd).round() as isize;
            let mut win = Vec::with_capacity((2 * half + 1) as usize);
            let mut seg = Vec::with_capacity(win.capacity());
            for n in -half..=half {
                let k = centre + n;
                let bt = k as f64 / fsd - t;
                let w = 0.5 * (PI * bt * f0v / 1.5).cos() + 0.5;
                win.push(w);
                seg.push(sample_at(x, k));
            }
            let norm = win.iter().map(|w| w * w).sum::<f64>().sqrt().max(1e-12);
            win.iter_mut().for_each(|w| *w /= norm);
            let mut wave: Vec<f64> = seg.iter().zip(&win).map(|(s, w)| s * w).collect();
            let mean = wave.iter().sum::<f64>() / win.iter().sum::<f64>().max(1e-12);
            for (v, w) in wave.iter_mut().zip(&win) {
                *v -= w * mean;
            }
            fft.time.iter_mut().for_each(|v| *v = 0.0);
            let len = wave.len().min(fft_size);
            fft.time[..len].copy_from_slice(&wave[..len]);
            fft.forward();
            for (p, c) in power.iter_mut().zip(&fft.freq) {
                *p = c.norm_sqr();
            }
            dc_correction(power, f0v, fsd, fft_size);
            linear_smoothing(power, f0v * 2.0 / 3.0, fsd, fft_size, smooth);
            let floor = smooth.iter().cloned().fold(0.0, f64::max) * 1e-12 + 1e-24;
            smooth.iter_mut().for_each(|p| *p += floor);
            // Lifter: smoothing and the recovery of the harmonic peaks.
            for (k, p) in smooth.iter().enumerate() {
                fft.freq[k] = Complex::new(p.ln(), 0.0);
            }
            fft.inverse();
            let q1 = -0.15;
            for n in 0..fft_size {
                let q = n.min(fft_size - n) as f64 / fsd;
                let a = PI * f0v * q;
                let sm = if q == 0.0 { 1.0 } else { a.sin() / a };
                let comp = (1.0 - 2.0 * q1) + 2.0 * q1 * (2.0 * PI * q * f0v).cos();
                fft.time[n] *= sm * comp / fft_size as f64;
            }
            fft.forward();
            for (k, v) in o.iter_mut().enumerate() {
                *v = (fft.freq[k].re.exp() * LEVEL) as f32;
            }
        });
    out
}

/// The recovery lifter sharpens the envelope towards the harmonic peaks,
/// which keeps its mean in log but raises its mean in power: resynthesised
/// recordings came out 1.2 to 2.1 dB louder (Kasane Teto's CV and VCV
/// samples, Canary's ARPAsing ones). This takes the middle of that back.
const LEVEL: f64 = 0.692; // -1.6 dB

/// Folds the spectrum below F0 back in, as CheapTrick does, so the
/// lowest harmonic is not under-weighted by the window's own DC dip.
fn dc_correction(power: &mut [f64], f0: f64, fs: f64, n: usize) {
    let df = fs / n as f64;
    let upper = ((f0 / df) as usize + 1).min(power.len());
    let orig: Vec<f64> = power[..(upper * 2 + 2).min(power.len())].to_vec();
    for (k, p) in power.iter_mut().enumerate().take(upper) {
        let f = f0 - k as f64 * df;
        if f < 0.0 {
            break;
        }
        let x = f / df;
        let i = x.floor() as usize;
        let fr = x - i as f64;
        let a = orig.get(i).copied().unwrap_or(0.0);
        let b = orig.get(i + 1).copied().unwrap_or(a);
        *p += a + (b - a) * fr;
    }
}

/// A rectangular smoothing of `width` Hz, by differences of the running
/// integral, the spectrum mirrored at both ends.
fn linear_smoothing(power: &[f64], width: f64, fs: f64, n: usize, out: &mut [f64]) {
    let df = fs / n as f64;
    let bins = power.len();
    let pad = (width / df).ceil() as usize + 2;
    let total = bins + 2 * pad;
    let ext = |j: isize| -> f64 {
        let mut k = j - pad as isize;
        if k < 0 {
            k = -k;
        }
        let last = bins as isize - 1;
        if k > last {
            k = 2 * last - k;
        }
        power[k.clamp(0, last) as usize]
    };
    // cum[j] = integral of the step function up to the left edge of bin j.
    let mut cum = vec![0.0; total + 1];
    for j in 0..total {
        cum[j + 1] = cum[j] + ext(j as isize) * df;
    }
    let integral = |f: f64| -> f64 {
        // Bin j covers [(j - pad - 0.5) df, (j - pad + 0.5) df).
        let x = f / df + pad as f64 + 0.5;
        let i = (x.floor().max(0.0) as usize).min(total - 1);
        let fr = (x - i as f64).clamp(0.0, 1.0);
        cum[i] + (cum[i + 1] - cum[i]) * fr
    };
    for (k, o) in out.iter_mut().enumerate() {
        let f = k as f64 * df;
        *o = ((integral(f + width / 2.0) - integral(f - width / 2.0)) / width).max(0.0);
    }
}

/// Band aperiodicity: each frame against the same window one period later.
/// Periodic sound repeats exactly; what does not repeat is noise.
fn aperiodicity(x: &[f64], fs: u32, f0: &[f32], fft_size: usize) -> Vec<f32> {
    let mut out = vec![1.0f32; f0.len() * AP_BANDS];
    let fsd = fs as f64;
    let bins = fft_size / 2 + 1;
    let df = fsd / fft_size as f64;
    let band_w = fsd / 2.0 / (AP_BANDS - 1) as f64;
    out.par_chunks_mut(AP_BANDS)
        .zip(f0.par_iter())
        .enumerate()
        .for_each_init(
            || Fft::new(fft_size),
            |fft, (i, (o, &f))| {
                if f <= 0.0 {
                    return;
                }
                let t0 = fsd / f64::from(f);
                let len = ((3.0 * t0).round() as usize).min(fft_size / 2).max(16);
                let t = i as f64 * FRAME_PERIOD * fsd;
                let a = (t - t0 / 2.0 - len as f64 / 2.0).round() as isize;
                let win: Vec<f64> = (0..len)
                    .map(|n| 0.5 - 0.5 * (2.0 * PI * (n as f64 + 0.5) / len as f64).cos())
                    .collect();
                let spectrum = |fft: &mut Fft, start: isize| -> Vec<Complex<f64>> {
                    fft.time.iter_mut().for_each(|v| *v = 0.0);
                    for (n, w) in win.iter().enumerate() {
                        fft.time[n] = sample_at(x, start + n as isize) * w;
                    }
                    fft.forward();
                    fft.freq.clone()
                };
                let x1 = spectrum(fft, a);
                let mut shifted = vec![Complex::new(0.0, 0.0); bins];
                // The period, searched a little either side of the F0 estimate,
                // each band taking the period it repeats best at: the voice drifts
                // within a window, and the drift tells most at high frequencies.
                // Each band's comparison allows a gain (a crescendo is not noise).
                o.iter_mut().for_each(|v| *v = 1.0);
                for scale in [0.99, 0.995, 0.9975, 1.0, 1.0025, 1.005, 1.01] {
                    let p = t0 * scale;
                    let whole = p.floor();
                    let frac = p - whole;
                    let x2 = spectrum(fft, a + whole as isize);
                    let rot = Complex::new(
                        (2.0 * PI * frac / fft_size as f64).cos(),
                        (2.0 * PI * frac / fft_size as f64).sin(),
                    );
                    let mut z = Complex::new(1.0, 0.0);
                    for (k, y) in shifted.iter_mut().enumerate().take(bins) {
                        *y = x2[k] * z;
                        z *= rot;
                    }
                    for (b, v) in o.iter_mut().enumerate() {
                        let fc = ap_band_hz(b, fs);
                        let lo = (((fc - band_w) / df).ceil().max(0.0)) as usize;
                        let hi = (((fc + band_w) / df).floor() as usize).min(bins - 1);
                        let (mut s1, mut s2, mut c) = (0.0, 0.0, 0.0);
                        for k in lo..=hi {
                            let w = 1.0 - ((k as f64 * df - fc).abs() / band_w);
                            if w <= 0.0 {
                                continue;
                            }
                            s1 += w * x1[k].norm_sqr();
                            s2 += w * shifted[k].norm_sqr();
                            c += w * (x1[k] * shifted[k].conj()).re;
                        }
                        if s1 <= 0.0 || s2 <= 0.0 {
                            continue;
                        }
                        let residual = if c > 0.0 { s1 - c * c / s2 } else { s1 };
                        let r = (residual / s1).clamp(1e-6, 1.0).sqrt() as f32;
                        if r < *v {
                            *v = r;
                        }
                    }
                }
            },
        );
    out
}

/// Frames to synthesise: F0 (0 unvoiced), power spectrum and full-band
/// aperiodicity per frame, every [`FRAME_PERIOD`].
#[derive(Debug, Clone, Default)]
pub struct Frames {
    pub fft_size: usize,
    pub f0: Vec<f32>,
    pub sp: Vec<f32>,
    pub ap: Vec<f32>,
}

/// A small, fixed-seed generator for the noise: the same take every time.
struct Noise(u64, Option<f64>);

impl Noise {
    fn new(seed: u64) -> Self {
        Noise(seed ^ 0x9E37_79B9_7F4A_7C15 | 1, None)
    }

    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((v >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn gauss(&mut self) -> f64 {
        if let Some(v) = self.1.take() {
            return v;
        }
        let (u, v) = (self.uniform(), self.uniform());
        let r = (-2.0 * u.ln()).sqrt();
        self.1 = Some(r * (2.0 * PI * v).sin());
        r * (2.0 * PI * v).cos()
    }
}

/// Synthesises `frames` at `fs`; `seed` fixes the noise. `None` when
/// `cancelled` was raised.
pub fn synthesize(
    frames: &Frames,
    fs: u32,
    seed: u64,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<f32>> {
    let nf = frames.f0.len();
    if nf == 0 {
        return Some(Vec::new());
    }
    let n = frames.fft_size;
    let fsd = fs as f64;
    let len = ((nf - 1) as f64 * FRAME_PERIOD * fsd) as usize + 1;
    let mut out = vec![0.0f64; len + n];
    // Pulse times by integrating F0 sample by sample.
    let f0_at = |s: f64| -> (f64, bool) {
        let x = s / fsd / FRAME_PERIOD;
        let i = (x.floor() as usize).min(nf - 1);
        let j = (i + 1).min(nf - 1);
        let fr = x - i as f64;
        let (a, b) = (f64::from(frames.f0[i]), f64::from(frames.f0[j]));
        let voiced = if fr < 0.5 { a > 0.0 } else { b > 0.0 };
        let a = if a > 0.0 { a } else { UNVOICED_RATE };
        let b = if b > 0.0 { b } else { UNVOICED_RATE };
        (a + (b - a) * fr, voiced)
    };
    let mut pulses: Vec<(f64, bool)> = Vec::new();
    let mut phase = 0.0;
    for s in 0..len {
        let (f, _) = f0_at(s as f64);
        let step = f / fsd;
        let next = phase + step;
        if next >= 1.0 || s == 0 {
            let back = if s == 0 { 0.0 } else { (next - 1.0) / step };
            let t = s as f64 - back;
            pulses.push((t, f0_at(t.max(0.0)).1));
        }
        phase = next.fract();
    }

    // Pulses are sung in chunks, in parallel, each into its own stretch of
    // output summed in order afterwards, with noise seeded per pulse: the
    // take does not depend on how many threads there were. The cancel
    // flag is read between rounds of chunks.
    const CHUNK: usize = 96;
    let chunks: Vec<std::ops::Range<usize>> = (0..pulses.len())
        .step_by(CHUNK)
        .map(|c| c..(c + CHUNK).min(pulses.len()))
        .collect();
    let round = rayon::current_num_threads().max(1) * 2;
    for group in chunks.chunks(round) {
        if cancelled() {
            return None;
        }
        let parts: Vec<(usize, Vec<f64>)> = group
            .par_iter()
            .map(|r| sing_pulses(frames, fs, seed, &pulses, r.clone(), len))
            .collect();
        for (start, buf) in parts {
            for (o, v) in out[start..].iter_mut().zip(&buf) {
                *o += v;
            }
        }
    }
    out.truncate(len);
    Some(out.into_iter().map(|v| v as f32).collect())
}

/// The pulses in `range`, as (first output sample, samples).
fn sing_pulses(
    frames: &Frames,
    fs: u32,
    seed: u64,
    pulses: &[(f64, bool)],
    range: std::ops::Range<usize>,
    len: usize,
) -> (usize, Vec<f64>) {
    let n = frames.fft_size;
    let bins = n / 2 + 1;
    let nf = frames.f0.len();
    let fsd = fs as f64;
    let first = pulses[range.start].0.floor().max(0.0) as usize;
    let last = pulses[range.end - 1].0.floor().max(0.0) as usize;
    let mut buf = vec![0.0f64; last - first + n];
    let mut fft = Fft::new(n);
    // Each frame's two filters (periodic, aperiodic) as minimum-phase
    // spectra, made once and kept while pulses pass between them.
    let mut kept: Vec<(usize, Arc<FrameFilters>)> = Vec::new();
    let mut amp = vec![0.0f64; bins];
    let mut filter = |i: usize, fft: &mut Fft| -> Arc<FrameFilters> {
        if let Some((_, f)) = kept.iter().find(|(k, _)| *k == i) {
            return f.clone();
        }
        let sp = &frames.sp[i * bins..(i + 1) * bins];
        let ap = &frames.ap[i * bins..(i + 1) * bins];
        let voiced = frames.f0[i] > 0.0 && ap[0] < 0.999;
        let periodic = if voiced {
            for k in 0..bins {
                let a = f64::from(ap[k]).clamp(0.0, 1.0);
                amp[k] = (f64::from(sp[k]) * (1.0 - a * a)).max(0.0).sqrt();
            }
            amp[0] = 0.0;
            min_phase(fft, &amp);
            Some(fft.freq.clone())
        } else {
            None
        };
        for k in 0..bins {
            let a = if voiced {
                f64::from(ap[k]).clamp(0.0, 1.0)
            } else {
                1.0
            };
            amp[k] = (f64::from(sp[k]).max(0.0)).sqrt() * a;
        }
        min_phase(fft, &amp);
        let f = Arc::new(FrameFilters {
            periodic,
            aperiodic: fft.freq.clone(),
        });
        // Frames behind the pulses are no longer needed.
        kept.retain(|(k, _)| *k + 2 > i);
        kept.push((i, f.clone()));
        f
    };
    let mut spectrum = vec![Complex::new(0.0, 0.0); bins];
    for pi in range {
        let (t, voiced) = pulses[pi];
        let mut noise = Noise::new(seed ^ (pi as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
        let next_t = pulses.get(pi + 1).map(|p| p.0).unwrap_or(len as f64);
        let size = ((next_t - t).round() as usize).clamp(1, n);
        // The filters at this pulse, between its frames.
        let x = t / fsd / FRAME_PERIOD;
        let i = (x.floor().max(0.0) as usize).min(nf - 1);
        let j = (i + 1).min(nf - 1);
        let fr = (x - i as f64).clamp(0.0, 1.0);
        let (fi, fj) = (filter(i, &mut fft), filter(j, &mut fft));
        let start = t.floor().max(0.0);
        let frac = t - start;
        let start = start as usize;
        // This pulse's share of noise, through the aperiodic filter.
        fft.time.iter_mut().for_each(|v| *v = 0.0);
        for v in fft.time.iter_mut().take(size) {
            *v = noise.gauss();
        }
        fft.forward();
        for (k, s) in spectrum.iter_mut().enumerate() {
            let h = fi.aperiodic[k] + (fj.aperiodic[k] - fi.aperiodic[k]) * fr;
            *s = fft.freq[k] * h;
        }
        // The pulse itself, delayed by its fraction of a sample, scaled by
        // the square root of its interval.
        if voiced {
            let g = (size as f64).sqrt();
            let rot = Complex::new(
                (2.0 * PI * frac / n as f64).cos(),
                -(2.0 * PI * frac / n as f64).sin(),
            );
            let mut z = Complex::new(g, 0.0);
            let zero = Complex::new(0.0, 0.0);
            for (k, s) in spectrum.iter_mut().enumerate() {
                let a = fi.periodic.as_ref().map_or(zero, |p| p[k]);
                let b = fj.periodic.as_ref().map_or(zero, |p| p[k]);
                *s += (a + (b - a) * fr) * z;
                z *= rot;
            }
        }
        fft.freq.copy_from_slice(&spectrum);
        fft.inverse();
        for (k, r) in fft.time.iter().enumerate() {
            if let Some(o) = buf.get_mut(start - first + k) {
                *o += r / n as f64;
            }
        }
    }
    (first, buf)
}

struct FrameFilters {
    periodic: Option<Vec<Complex<f64>>>,
    aperiodic: Vec<Complex<f64>>,
}

/// The minimum-phase spectrum with magnitude `amp`, into `fft.freq`.
fn min_phase(fft: &mut Fft, amp: &[f64]) {
    let n = fft.n;
    for (k, a) in amp.iter().enumerate() {
        fft.freq[k] = Complex::new(a.max(1e-12).ln(), 0.0);
    }
    fft.inverse();
    // Fold the real cepstrum onto positive quefrencies.
    let c = &mut fft.time;
    for v in c.iter_mut() {
        *v /= n as f64;
    }
    for v in c.iter_mut().take(n / 2).skip(1) {
        *v *= 2.0;
    }
    for v in c.iter_mut().skip(n / 2 + 1) {
        *v = 0.0;
    }
    fft.forward();
    for z in fft.freq.iter_mut() {
        let m = z.re.exp();
        *z = Complex::new(m * z.im.cos(), m * z.im.sin());
    }
}

/// Coarse aperiodicity to one value per bin, interpolated linearly.
pub fn expand_ap(coarse: &[f32], fs: u32, fft_size: usize, out: &mut [f32]) {
    let df = fs as f32 / fft_size as f32;
    let band = fs as f32 / 2.0 / (AP_BANDS - 1) as f32;
    for (k, o) in out.iter_mut().enumerate() {
        let x = k as f32 * df / band;
        let i = (x as usize).min(AP_BANDS - 1);
        let j = (i + 1).min(AP_BANDS - 1);
        let fr = (x - i as f32).clamp(0.0, 1.0);
        *o = (coarse[i] + (coarse[j] - coarse[i]) * fr).clamp(0.0, 1.0);
    }
}
