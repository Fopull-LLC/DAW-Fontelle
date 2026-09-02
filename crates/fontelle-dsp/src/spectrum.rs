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
