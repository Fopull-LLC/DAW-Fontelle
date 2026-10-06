//! Noise reduction for the Clean page (plan §2.7, P3).
//!
//! Capture a profile of the noise from a span that holds nothing else, then
//! take it out of the whole clip: a decision-directed Wiener filter
//! (Ephraim–Malah's a-priori SNR) over an STFT of 2048 points every 512.
//!
//! Offline and allocating: this runs on a worker, never the RT thread.

/// Points in one transform. 43 ms at 48 kHz: fine enough in frequency to
/// leave a voice's harmonics standing between the noise bins.
pub const FFT_SIZE: usize = 2048;
/// A quarter of the window, so Hann² overlaps to a constant.
pub const HOP: usize = 512;
/// Bins up to Nyquist.
pub const BINS: usize = FFT_SIZE / 2 + 1;

/// What the noise sounds like: its mean power per bin, as a magnitude.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NoiseProfile {
    /// [`BINS`] values: the root of the noise's mean power in each bin, in
    /// the units of a Hann-windowed [`FFT_SIZE`]-point transform.
    pub magnitudes: Vec<f32>,
    pub sample_rate: u32,
}

/// What [`denoise`] hands back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DenoiseOutput {
    /// The clip with the noise taken out.
    #[default]
    Cleaned,
    /// Only what was taken out — "Listen to what's removed".
    Removed,
}

/// The Clean page's knobs.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DenoiseSettings {
    /// "Reduce by": how far down the noise goes at most, in dB. The
    /// spectral floor β is this as a gain, so noise is turned down rather
    /// than carved out — what keeps the residue from warbling.
    pub reduce_db: f32,
    /// 0..1: how hard it subtracts, as an over-subtraction of 1 to 4 times
    /// the profile.
    pub amount: f32,
    /// 0..1: how loud something has to be, against the profile, to count as
    /// signal. 0.5 takes the profile as it was captured; each end moves it
    /// by 6 dB.
    pub sensitivity: f32,
    pub output: DenoiseOutput,
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self {
            reduce_db: 12.0,
            amount: 0.5,
            sensitivity: 0.5,
            output: DenoiseOutput::Cleaned,
        }
    }
}

impl NoiseProfile {
    /// The profile of `noise`: a span the user chose because it holds
    /// nothing but the noise. `None` when it is shorter than one window.
    ///
    /// Per bin, the **median** magnitude across the span's frames, turned
    /// into a mean power by the Rayleigh law (noise's magnitude in a bin is
    /// Rayleigh-distributed, whose mean power is median² / ln 2). A median
    /// rather than a mean so a cough in the selection does not set the
    /// profile; then smoothed over a sixth of an octave either side, which
    /// steadies the estimate without smearing a hum's line (at 60 Hz that
    /// window is narrower than a bin).
    pub fn capture(noise: &[f32], sample_rate: u32) -> Option<Self> {
        if noise.len() < FFT_SIZE {
            return None;
        }
        let mut stft = Stft::new();
        let mut columns: Vec<Vec<f32>> = vec![Vec::new(); BINS];
        let mut at = 0;
        while at + FFT_SIZE <= noise.len() {
            let spectrum = stft.forward(&noise[at..at + FFT_SIZE]);
            for (column, value) in columns.iter_mut().zip(spectrum) {
                column.push(value.norm());
            }
            at += HOP;
        }
        let raw: Vec<f32> = columns
            .into_iter()
            .map(|mut column| {
                column.sort_by(f32::total_cmp);
                let median = column[column.len() / 2];
                median / std::f32::consts::LN_2.sqrt()
            })
            .collect();
        Some(Self {
            magnitudes: smooth_sixth_octave(&raw),
            sample_rate,
        })
    }
}

/// Power-averages each bin with its neighbours within a sixth of an octave.
fn smooth_sixth_octave(raw: &[f32]) -> Vec<f32> {
    let ratio = 2f32.powf(1.0 / 6.0);
    (0..raw.len())
        .map(|bin| {
            let lo = ((bin as f32 / ratio).floor() as usize).min(bin);
            let hi = ((bin as f32 * ratio).ceil() as usize).clamp(bin, raw.len() - 1);
            let power: f32 = raw[lo..=hi].iter().map(|m| m * m).sum::<f32>() / (hi - lo + 1) as f32;
            power.sqrt()
        })
        .collect()
}

/// A Hann-windowed real transform and its inverse, with the scratch reused.
struct Stft {
    forward: std::sync::Arc<dyn realfft::RealToComplex<f32>>,
    inverse: std::sync::Arc<dyn realfft::ComplexToReal<f32>>,
    window: Vec<f32>,
    time: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
}

impl Stft {
    fn new() -> Self {
        let mut planner = realfft::RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(FFT_SIZE);
        let inverse = planner.plan_fft_inverse(FFT_SIZE);
        Self {
            spectrum: forward.make_output_vec(),
            time: forward.make_input_vec(),
            forward,
            inverse,
            window: hann(FFT_SIZE),
        }
    }

    /// The windowed spectrum of one frame of [`FFT_SIZE`] samples.
    fn forward(&mut self, frame: &[f32]) -> &mut [realfft::num_complex::Complex<f32>] {
        for ((t, x), w) in self.time.iter_mut().zip(frame).zip(&self.window) {
            *t = x * w;
        }
        // Lengths are the plan's own, so this cannot fail.
        let _ = self.forward.process(&mut self.time, &mut self.spectrum);
        &mut self.spectrum
    }

    /// The inverse of the spectrum in `self.spectrum`, windowed again and
    /// scaled for overlap-add at [`HOP`]: Hann² at a quarter-window hop sums
    /// to 1.5, and realfft's inverse is unnormalised by `FFT_SIZE`.
    fn inverse_into(&mut self, out: &mut [f32]) {
        // The DC and Nyquist bins of a real signal's spectrum are real.
        self.spectrum[0].im = 0.0;
        self.spectrum[BINS - 1].im = 0.0;
        let _ = self.inverse.process(&mut self.spectrum, &mut self.time);
        let scale = 1.0 / (FFT_SIZE as f32 * 1.5);
        for ((o, t), w) in out.iter_mut().zip(&self.time).zip(&self.window) {
            *o += t * w * scale;
        }
    }
}

/// A periodic Hann window: the one whose squares overlap-add flat.
fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
        .collect()
}

/// How much the a-priori SNR leans on the last frame's cleaned estimate
/// (Ephraim–Malah's α). 0.98 is the classic value: high enough that the
/// estimate does not flutter frame to frame, which is musical noise's
/// cause.
const DECISION_DIRECTED: f32 = 0.98;

/// `samples` with the noise in `profile` taken out (or, with
/// [`DenoiseOutput::Removed`], only what was taken out). Same length as
/// `samples`; `Cleaned + Removed` is the input.
///
/// Per frame and bin: the posterior SNR γ = |Y|² / N, the a-priori SNR
/// ξ = α·(G·|Y|)²₋₁ / N + (1 − α)·max(γ − 1, 0), the gain ξ / (ξ + over),
/// with over the over-subtraction from Amount, held above the floor β from
/// "Reduce by", then averaged with the last frame's gain (two-frame temporal
/// smoothing, the plan's guard against the residue warbling).
pub fn denoise(samples: &[f32], profile: &NoiseProfile, settings: &DenoiseSettings) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let floor = 10f32.powf(-settings.reduce_db.max(0.0) / 20.0);
    let over = 1.0 + 3.0 * settings.amount.clamp(0.0, 1.0);
    // Sensitivity moves the profile ±6 dB: more sensitive, more is noise.
    let sensitivity = 10f32.powf((settings.sensitivity.clamp(0.0, 1.0) - 0.5) * 12.0 / 20.0);
    let noise_power: Vec<f32> = (0..BINS)
        .map(|bin| {
            let m = profile.magnitudes.get(bin).copied().unwrap_or(0.0) * sensitivity;
            (m * m).max(1e-20)
        })
        .collect();

    // Padded a window either side so every input sample is under four
    // frames' worth of window, as the overlap-add's constant assumes.
    let pad = FFT_SIZE;
    let mut padded = vec![0.0f32; samples.len() + 2 * pad];
    padded[pad..pad + samples.len()].copy_from_slice(samples);
    let mut out = vec![0.0f32; padded.len()];

    let mut stft = Stft::new();
    let mut last_clean = vec![0.0f32; BINS];
    let mut last_gain = vec![1.0f32; BINS];
    let mut gains = vec![0.0f32; BINS];
    let mut at = 0;
    while at + FFT_SIZE <= padded.len() {
        let spectrum = stft.forward(&padded[at..at + FFT_SIZE]);
        for (bin, value) in spectrum.iter().enumerate() {
            let power = value.norm_sqr();
            let n = noise_power[bin];
            let gamma = power / n;
            let xi = DECISION_DIRECTED * last_clean[bin] / n
                + (1.0 - DECISION_DIRECTED) * (gamma - 1.0).max(0.0);
            let gain = (xi / (xi + over)).max(floor);
            let smoothed = 0.5 * (gain + last_gain[bin]);
            gains[bin] = smoothed;
            last_gain[bin] = gain;
            last_clean[bin] = gain * gain * power;
        }
        for (value, gain) in spectrum.iter_mut().zip(&gains) {
            *value *= *gain;
        }
        stft.inverse_into(&mut out[at..at + FFT_SIZE]);
        at += HOP;
    }

    let cleaned = &out[pad..pad + samples.len()];
    match settings.output {
        DenoiseOutput::Cleaned => cleaned.to_vec(),
        DenoiseOutput::Removed => samples.iter().zip(cleaned).map(|(x, c)| x - c).collect(),
    }
}

/// The **voice** denoiser: RNNoise's network (through `nnnoiseless`), which
/// needs no profile and knows what a voice is. 48 kHz mono only — the
/// caller resamples — and the feature `voice-denoise`, off by default.
///
/// Same length as `samples`: RNNoise delays by one 10 ms frame, and that
/// frame is taken back off the front.
#[cfg(feature = "voice-denoise")]
pub fn denoise_voice(samples: &[f32]) -> Vec<f32> {
    use nnnoiseless::DenoiseState;
    const FRAME: usize = DenoiseState::FRAME_SIZE;
    // RNNoise works in 16-bit units.
    const SCALE: f32 = 32_767.0;
    let mut state = DenoiseState::new();
    let mut input = [0.0f32; FRAME];
    let mut output = [0.0f32; FRAME];
    let mut out = Vec::with_capacity(samples.len() + 2 * FRAME);
    // One frame past the end, to flush the frame RNNoise holds back.
    let frames = samples.len().div_ceil(FRAME) + 1;
    for frame in 0..frames {
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = samples.get(frame * FRAME + i).map_or(0.0, |s| s * SCALE);
        }
        state.process_frame(&mut output, &input);
        // Its first frame is the fade-in of its own window, not audio.
        if frame > 0 {
            out.extend(output.iter().map(|s| s / SCALE));
        }
    }
    out.truncate(samples.len());
    out
}
