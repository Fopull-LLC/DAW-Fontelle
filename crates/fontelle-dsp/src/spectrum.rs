//! A real-signal spectrum: the analyser behind a parametric EQ's curve.
//!
//! *"currently theres no eq monitor graph drawn to view the frequency spectrum
//! and make edits based off it and see in realtime."* Every modern parametric
//! EQ draws one, and a curve with nothing behind it is a curve you are setting
//! by ear alone — which is fine, and is not what the window promised by drawing
//! a frequency axis.
//!
//! # Why the transform is written out rather than pulled in
//!
//! It is forty lines. An iterative radix-2 Cooley-Tukey over a power-of-two
//! block is the whole of what an analyser needs, a crate for it would be the
//! first dependency in `fontelle-dsp` (which has exactly one, and that is
//! `serde`), and the correctness of it is checkable by arithmetic — a sine at a
//! bin's own frequency puts all its energy in that bin, which is what
//! `tests/spectrum.rs` asserts.
//!
//! # Where it runs
//!
//! **Not on the audio thread.** The RT side does one thing — copy samples into
//! a ring — and the window does the transform, once a frame, while the EQ's
//! window is open. A transform is O(n log n) over a few thousand points, which
//! is a few tens of microseconds; doing it per block per EQ, whether or not
//! anybody is looking, would be a cost the mix pays for a picture nobody is
//! watching.

use std::f32::consts::PI;

/// How many points the analyser transforms at once.
///
/// 2048 at 48 kHz is 23 Hz per bin and a 43 ms window. The trade is the usual
/// one and both ends of it are audible in the picture: fewer points and the
/// bottom two octaves smear into one bar; more, and the display lags what you
/// just played.
pub const SPECTRUM_SIZE: usize = 2048;

/// A reusable scratch space for the transform.
///
/// Holds its own buffers so a window that draws an analyser every frame
/// allocates nothing after the first — the same reason every other DSP object
/// in this crate is a struct rather than a function.
#[derive(Debug, Clone)]
pub struct SpectrumAnalyser {
    /// The Hann window, precomputed: it is the same every time.
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    /// Magnitudes in decibels, one per bin up to Nyquist.
    magnitudes: Vec<f32>,
}

impl Default for SpectrumAnalyser {
    fn default() -> Self {
        Self::new()
    }
}

/// The floor of the display, in decibels. Below this a bin is silence.
///
/// Ninety, because that is about where a mix's noise floor sits and where the
/// eye stops being able to tell one low bar from another.
pub const SPECTRUM_FLOOR_DB: f32 = -90.0;

impl SpectrumAnalyser {
    pub fn new() -> Self {
        let n = SPECTRUM_SIZE;
        Self {
            // Hann: the sidelobes of a rectangular window put a pure tone's
            // energy into every bin, which reads as a noise floor that is not
            // there.
            window: (0..n)
                .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
                .collect(),
            re: vec![0.0; n],
            im: vec![0.0; n],
            magnitudes: vec![SPECTRUM_FLOOR_DB; n / 2],
        }
    }

    /// Transforms `samples` — the most recent [`SPECTRUM_SIZE`] of them, oldest
    /// first — and returns one magnitude in decibels per bin, bin 0 at DC and
    /// the last just under Nyquist.
    ///
    /// A short slice is zero-padded rather than refused: a graph that vanishes
    /// for the first fifty milliseconds after a window opens is a graph that
    /// looks broken.
    pub fn analyse(&mut self, samples: &[f32]) -> &[f32] {
        let n = SPECTRUM_SIZE;
        let offset = n.saturating_sub(samples.len());
        for i in 0..n {
            let sample = if i >= offset {
                samples[samples.len() - (n - i)]
            } else {
                0.0
            };
            self.re[i] = sample * self.window[i];
            self.im[i] = 0.0;
        }
        fft_in_place(&mut self.re, &mut self.im);

        // Normalised so that a full-scale sine reads 0 dB: the Hann window
        // halves the coherent gain, and a real signal's energy is split between
        // the positive and negative frequency halves.
        let scale = 4.0 / n as f32;
        for (bin, out) in self.magnitudes.iter_mut().enumerate() {
            let power = self.re[bin] * self.re[bin] + self.im[bin] * self.im[bin];
            let amplitude = power.sqrt() * scale;
            *out = if amplitude > 0.0 {
                (20.0 * amplitude.log10()).max(SPECTRUM_FLOOR_DB)
            } else {
                SPECTRUM_FLOOR_DB
            };
        }
        &self.magnitudes
    }

    /// The bins of the last [`analyse`](Self::analyse).
    pub fn magnitudes(&self) -> &[f32] {
        &self.magnitudes
    }
}

/// How wide one bin is, in hertz, at `sample_rate`.
pub fn bin_width_hz(sample_rate: f32) -> f32 {
    sample_rate / SPECTRUM_SIZE as f32
}

/// An in-place radix-2 Cooley-Tukey FFT.
///
/// `re` and `im` must be the same length and a power of two; anything else
/// leaves them alone rather than panicking, because the one caller controls the
/// size and a panic in a drawing path is worse than a flat graph.
pub fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n != im.len() || n < 2 || !n.is_power_of_two() {
        return;
    }

    // Bit-reversal permutation, done by walking a reversed counter rather than
    // reversing each index: the same order, without a loop per element.
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
        let angle = -2.0 * PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        let mut start = 0;
        while start < n {
            // The twiddle is stepped rather than called per butterfly: a `cos`
            // per point per stage is most of the cost of a small transform.
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let (xr, xi) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
                let next = (cr * wr - ci * wi, cr * wi + ci * wr);
                cr = next.0;
                ci = next.1;
            }
            start += len;
        }
        len <<= 1;
    }
}

// --- The spectral source's analysis (`docs/flopsynth-next.md` §4.3) -------

/// One frame of a spectral analysis: the partials a recording had at one
/// moment, each as a ratio to the recording's fundamental and a level
/// (a full-scale sine reads about 1.0). Sixty-four, like the string's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralFrame {
    pub ratio: [f32; crate::MAX_PARTIALS],
    pub amp: [f32; crate::MAX_PARTIALS],
}

impl Default for SpectralFrame {
    fn default() -> Self {
        Self {
            ratio: std::array::from_fn(|n| (n + 1) as f32),
            amp: [0.0; crate::MAX_PARTIALS],
        }
    }
}

/// A recording as frames of partials, `hop_s` apart — what
/// [`SynthSource::Spectral`](crate::SynthSource::Spectral) plays through
/// the string's bank of phasors.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectralFrames {
    pub hop_s: f32,
    /// How many partials sat under Nyquist for the recording's fundamental.
    pub count: usize,
    pub frames: Vec<SpectralFrame>,
}

impl SpectralFrames {
    /// The frame at `position` of the way through, the two nearest mixed
    /// by where between them it falls — so a scan across the frames is a
    /// crossfade rather than a stair.
    pub fn at(&self, position: f32) -> SpectralFrame {
        let Some(last) = self.frames.len().checked_sub(1) else {
            return SpectralFrame::default();
        };
        let at = position.clamp(0.0, 1.0) * last as f32;
        let index = (at.floor() as usize).min(last);
        let next = (index + 1).min(last);
        let t = at - index as f32;
        let (a, b) = (&self.frames[index], &self.frames[next]);
        let mut out = *a;
        for n in 0..crate::MAX_PARTIALS {
            out.ratio[n] = a.ratio[n] + (b.ratio[n] - a.ratio[n]) * t;
            out.amp[n] = a.amp[n] + (b.amp[n] - a.amp[n]) * t;
        }
        out
    }
}

/// Analyses `samples` at `sample_rate`, a recording of a note at
/// `root_hz`, into [`SpectralFrames`]: a Hann-windowed transform every ten
/// milliseconds, and in each the strongest line within half a harmonic of
/// each multiple of the root — its frequency from the phase the bin turned
/// through since the last frame (the vocoder's estimate, exact for a
/// steady line; a parabola over the three bins round the peak for the
/// first frame, which has no last), its level from the peak's magnitude.
///
/// Harmonic buckets rather than free peak-picking, because the source
/// plays a *note*: partial `n` is whatever the recording had near `n`
/// times its root, which keeps a stiff string's stretch (the bucket is
/// wide enough) and puts every frame's partials in the same slots, so a
/// scan across frames moves each phasor a little rather than reassigning
/// them. The window is 4096 samples for a root under 200 Hz (the bass's
/// buckets are narrow) and 2048 above.
pub fn analyse_spectral(samples: &[f32], sample_rate: f32, root_hz: f32) -> SpectralFrames {
    let hop_s = 0.01;
    let root = root_hz.max(20.0);
    let window = if root < 200.0 { 4096 } else { 2048 };
    let hop = ((hop_s * sample_rate) as usize).max(1);
    let count = ((sample_rate * 0.5 / root).floor() as usize).clamp(1, crate::MAX_PARTIALS);
    let hann: Vec<f32> = (0..window)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / window as f32).cos())
        .collect();
    let window_sum: f32 = hann.iter().sum();
    let bin_hz = sample_rate / window as f32;
    let mut frames = Vec::new();
    let mut re = vec![0.0f32; window];
    let mut im = vec![0.0f32; window];
    // The last frame's phase per bin, for the vocoder's estimate.
    let mut last_phase: Vec<f32> = Vec::new();
    let mut phase = vec![0.0f32; window / 2];
    let mut start = 0usize;
    // The last frame is the one that starts inside the recording, so a
    // short recording still has at least one.
    while start < samples.len().max(1) {
        for (i, (r, w)) in re.iter_mut().zip(&hann).enumerate() {
            *r = samples.get(start + i).copied().unwrap_or(0.0) * w;
        }
        im.fill(0.0);
        fft_in_place(&mut re, &mut im);
        let magnitude = |k: usize| (re[k] * re[k] + im[k] * im[k]).sqrt();
        for (k, p) in phase.iter_mut().enumerate() {
            *p = im[k].atan2(re[k]);
        }
        let mut frame = SpectralFrame::default();
        for n in 1..=count {
            let centre = n as f32 * root;
            let low = ((centre - 0.5 * root) / bin_hz).ceil().max(1.0) as usize;
            let high = (((centre + 0.5 * root) / bin_hz).floor() as usize).min(window / 2 - 2);
            if low > high {
                continue;
            }
            let mut best = low;
            let mut best_magnitude = 0.0;
            for k in low..=high {
                let m = magnitude(k);
                if m > best_magnitude {
                    best_magnitude = m;
                    best = k;
                }
            }
            let (a, b, c) = (
                magnitude(best - 1).max(1e-12).ln(),
                best_magnitude.max(1e-12).ln(),
                magnitude(best + 1).max(1e-12).ln(),
            );
            let denominator = a - 2.0 * b + c;
            let parabola = if denominator.abs() > 1e-9 {
                (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            // The bin's phase advanced `2π k hop / window` if the line sat
            // exactly on it; what it advanced beyond that, over the hop,
            // is how far off the bin the line really is.
            let offset = match last_phase.get(best) {
                Some(previous) => {
                    let expected = std::f32::consts::TAU * best as f32 * hop as f32 / window as f32;
                    let turned = phase[best] - previous - expected;
                    let wrapped =
                        turned - std::f32::consts::TAU * (turned / std::f32::consts::TAU).round();
                    let bins = wrapped * window as f32 / (std::f32::consts::TAU * hop as f32);
                    if bins.abs() <= 0.6 { bins } else { parabola }
                }
                None => parabola,
            };
            let hz = (best as f32 + offset) * bin_hz;
            frame.ratio[n - 1] = hz / root;
            // A sine at amplitude A under a window of sum S reads S·A/2.
            frame.amp[n - 1] = best_magnitude * 2.0 / window_sum;
        }
        frames.push(frame);
        last_phase.clear();
        last_phase.extend_from_slice(&phase);
        start += hop;
    }
    SpectralFrames {
        hop_s,
        count,
        frames,
    }
}
