/// Bare oscillator sources, usable as a `Layer` on their own (TDD §7.2 —
/// `Source::Oscillator`) for reinforcing a sample's sub-bass or adding attack noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscKind {
    Sine,
    Saw,
    Square,
    Triangle,
    Noise,
}

#[derive(Debug, Clone, Copy)]
pub struct Oscillator {
    /// 0..1 through one cycle. Every shape reads the same phase, so swapping
    /// one for another moves the waveform without jumping its position.
    phase: f32,
    /// `Noise` has no phase to advance; it needs a source of numbers instead.
    /// An xorshift because it is a handful of instructions with no memory
    /// traffic, which is what a per-sample RT path can afford.
    rng: u32,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self {
            phase: 0.0,
            // Any non-zero seed; xorshift is stuck at zero forever.
            rng: 0x2545_f491,
        }
    }
}

/// The polynomial band-limited step: the correction that turns a naive
/// discontinuity into one that does not scatter aliases back down the
/// spectrum.
///
/// A hard jump in a sampled signal has energy at every frequency, and
/// everything above Nyquist folds down as inharmonic tones — the metallic
/// ringing that gives a naive digital saw away. `poly_blep` subtracts a
/// two-sample polynomial approximation of the band-limited step around each
/// discontinuity, which is cheap enough to run per sample and removes most of
/// it.
///
/// `t` is the phase (0..1) and `dt` the phase increment per sample.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        // Just after the step.
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        // Just before the next one.
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

impl Oscillator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// One sample, then advance. Band-limited where it matters (`Saw` and
    /// `Square`, whose discontinuities alias); `Sine` has no harmonics to
    /// alias, `Triangle`'s fall off as 1/n² and are already near the noise
    /// floor by Nyquist, and `Noise` is broadband on purpose.
    pub fn next_sample(&mut self, kind: OscKind, freq_hz: f32, sample_rate: f32) -> f32 {
        let dt = if sample_rate > 0.0 {
            freq_hz / sample_rate
        } else {
            0.0
        };
        let value = self.shape(kind, dt);
        self.advance_phase(dt);
        value
    }

    /// The value at the start of this block, then advances the phase by the
    /// whole block.
    ///
    /// This is how an LFO is read: a modulation source is sampled once per
    /// block and held, so a 1 Hz LFO takes a second whatever the block size
    /// is. It deliberately skips the band-limiting `next_sample` applies —
    /// aliasing is an audio-rate concern, and a control signal read at block
    /// rate has no spectrum worth protecting.
    pub fn advance_block(
        &mut self,
        kind: OscKind,
        freq_hz: f32,
        sample_rate: f32,
        frames: usize,
    ) -> f32 {
        let dt = if sample_rate > 0.0 {
            freq_hz / sample_rate
        } else {
            0.0
        };
        // `dt` rather than 0: a `Saw` or `Square` still wants its step
        // correction to be a no-op away from the discontinuity, and passing
        // the real increment is what makes `shape` agree with `next_sample`.
        let value = self.shape(kind, dt);
        self.advance_phase(dt * frames as f32);
        value
    }

    fn advance_phase(&mut self, by: f32) {
        self.phase += by;
        // `fract` rather than a `while`: a control-rate advance can cross many
        // cycles in one step, and a loop would spin proportionally.
        if self.phase >= 1.0 || self.phase < 0.0 {
            self.phase = self.phase.rem_euclid(1.0);
        }
    }

    fn shape(&mut self, kind: OscKind, dt: f32) -> f32 {
        let phase = self.phase;
        match kind {
            OscKind::Sine => (std::f32::consts::TAU * phase).sin(),
            OscKind::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
            OscKind::Square => {
                let naive = if phase < 0.5 { 1.0 } else { -1.0 };
                // Two discontinuities per cycle, half a cycle apart.
                naive + poly_blep(phase, dt) - poly_blep((phase + 0.5).fract(), dt)
            }
            // Phase-aligned with the sine: zero rising, peak at a quarter,
            // zero falling at a half, trough at three quarters.
            OscKind::Triangle => {
                if phase < 0.25 {
                    4.0 * phase
                } else if phase < 0.75 {
                    2.0 - 4.0 * phase
                } else {
                    4.0 * phase - 4.0
                }
            }
            OscKind::Noise => {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 17;
                self.rng ^= self.rng << 5;
                // 0..1 from the top 24 bits, then to -1..1.
                (self.rng >> 8) as f32 / 8_388_608.0 - 1.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn render(kind: OscKind, freq_hz: f32, samples: usize) -> Vec<f32> {
        let mut osc = Oscillator::new();
        (0..samples)
            .map(|_| osc.next_sample(kind, freq_hz, SR))
            .collect()
    }

    /// Every shape starts at the same place in its cycle and runs the same
    /// direction. An LFO whose shapes disagree about phase makes swapping one
    /// for another jump the modulation, which is not what "change the shape"
    /// should do.
    #[test]
    fn sine_and_triangle_start_at_zero_and_rise() {
        for kind in [OscKind::Sine, OscKind::Triangle] {
            let out = render(kind, 100.0, 8);
            assert!(
                out[0].abs() < 1e-6,
                "{kind:?} starts at zero, got {}",
                out[0]
            );
            assert!(out[1] > out[0], "{kind:?} must rise out of zero");
        }
    }

    #[test]
    fn every_shape_stays_inside_full_scale() {
        for kind in [
            OscKind::Sine,
            OscKind::Saw,
            OscKind::Square,
            OscKind::Triangle,
            OscKind::Noise,
        ] {
            let out = render(kind, 997.0, 4096);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(
                peak <= 1.0001,
                "{kind:?} reached {peak}, which would clip anything downstream"
            );
        }
    }

    /// The period has to be the period asked for, or every tempo-synced LFO
    /// and every oscillator layer is detuned.
    #[test]
    fn a_sine_completes_exactly_one_cycle_per_period() {
        let freq = 480.0; // 100 samples at 48 kHz, exactly.
        let out = render(OscKind::Sine, freq, 301);
        for cycle in 0..3 {
            let at = cycle * 100;
            assert!(
                out[at].abs() < 1e-5,
                "sample {at} should be a zero crossing, got {}",
                out[at]
            );
        }
        assert!(
            (out[25] - 1.0).abs() < 1e-5,
            "a quarter cycle in is the peak, got {}",
            out[25]
        );
    }

    #[test]
    fn a_triangle_peaks_a_quarter_cycle_in_and_troughs_three_quarters() {
        let out = render(OscKind::Triangle, 480.0, 101);
        assert!((out[25] - 1.0).abs() < 1e-5, "peak {}", out[25]);
        assert!((out[50]).abs() < 1e-5, "zero crossing {}", out[50]);
        assert!((out[75] + 1.0).abs() < 1e-5, "trough {}", out[75]);
    }

    /// PolyBLEP is the whole reason `next_sample` isn't three lines, and the
    /// test that matters is not "the shape looks like a saw" but "the energy
    /// that belongs above Nyquist has not folded back down below it".
    ///
    /// Measured as the energy *below the fundamental*, where a correct saw has
    /// none at all: every partial of an ideal saw sits at a multiple of the
    /// fundamental, so anything down there arrived by aliasing. At 5 kHz
    /// against a 48 kHz rate the 9th, 10th and 19th harmonics all fold into
    /// that region, which is exactly the metallic ringing a naive digital
    /// oscillator is recognisable by.
    #[test]
    fn a_saw_is_band_limited_rather_than_aliasing_below_its_own_fundamental() {
        let freq = 5_000.0;
        let samples = 4096;
        let out = render(OscKind::Saw, freq, samples);
        let naive: Vec<f32> = (0..samples)
            .map(|i| {
                let phase = (i as f32 * freq / SR).fract();
                2.0 * phase - 1.0
            })
            .collect();

        let (limited, aliased) = (
            energy_below(&out, freq * 0.8),
            energy_below(&naive, freq * 0.8),
        );
        assert!(
            limited < aliased * 0.5,
            "band-limiting must halve the aliased energy at least: {limited} \
             against {aliased}"
        );
    }

    #[test]
    fn a_square_is_band_limited_too() {
        let freq = 5_000.0;
        let samples = 4096;
        let out = render(OscKind::Square, freq, samples);
        let naive: Vec<f32> = (0..samples)
            .map(|i| {
                let phase = (i as f32 * freq / SR).fract();
                if phase < 0.5 { 1.0 } else { -1.0 }
            })
            .collect();
        let (limited, aliased) = (
            energy_below(&out, freq * 0.8),
            energy_below(&naive, freq * 0.8),
        );
        assert!(limited < aliased * 0.5, "{limited} against {aliased}");
    }

    /// Total spectral energy below `upper_hz`, excluding DC.
    ///
    /// Hann-windowed, because the point is to measure faint aliases and an
    /// unwindowed transform smears the fundamental across the whole spectrum
    /// far above them. A naive DFT rather than an FFT: this runs twice, in a
    /// test, and pulling in an FFT crate to check an oscillator would be a
    /// dependency for nothing.
    fn energy_below(signal: &[f32], upper_hz: f32) -> f32 {
        let n = signal.len();
        let windowed: Vec<f32> = signal
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / n as f32).cos());
                s * w
            })
            .collect();
        let bin_hz = SR / n as f32;
        let mut total = 0.0;
        for bin in 1..((upper_hz / bin_hz) as usize).min(n / 2) {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in windowed.iter().enumerate() {
                let angle = -std::f32::consts::TAU * bin as f32 * i as f32 / n as f32;
                re += s * angle.cos();
                im += s * angle.sin();
            }
            total += re * re + im * im;
        }
        total / (n * n) as f32
    }

    #[test]
    fn noise_is_broadband_and_centred_rather_than_a_repeating_pattern() {
        let out = render(OscKind::Noise, 1.0, 8192);
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        assert!(
            mean.abs() < 0.05,
            "white noise has no DC offset, got {mean}"
        );
        // A short cycle would show up as the first half equalling the second.
        let half = out.len() / 2;
        let matches = out[..half]
            .iter()
            .zip(&out[half..])
            .filter(|(a, b)| (*a - *b).abs() < 1e-9)
            .count();
        assert!(
            matches < half / 100,
            "noise must not repeat: {matches} matches"
        );
    }

    #[test]
    fn reset_returns_the_oscillator_to_the_start_of_its_cycle() {
        let mut osc = Oscillator::new();
        for _ in 0..37 {
            osc.next_sample(OscKind::Sine, 1_000.0, SR);
        }
        osc.reset();
        assert!(osc.next_sample(OscKind::Sine, 1_000.0, SR).abs() < 1e-6);
    }

    /// Control-rate use: an LFO's value is read once per block and its phase
    /// advanced by the whole block, so a 1 Hz LFO takes a second whether the
    /// blocks are 64 frames or 512.
    #[test]
    fn advancing_a_whole_block_lands_where_sample_by_sample_would() {
        let freq = 5.0;
        let frames = 128;
        let mut block = Oscillator::new();
        let mut sample = Oscillator::new();

        let at_block_start = block.advance_block(OscKind::Sine, freq, SR, frames);
        assert!(at_block_start.abs() < 1e-6, "reads before it advances");
        for _ in 0..frames {
            sample.next_sample(OscKind::Sine, freq, SR);
        }
        let after_block = block.advance_block(OscKind::Sine, freq, SR, frames);
        let after_samples = sample.next_sample(OscKind::Sine, freq, SR);
        assert!(
            (after_block - after_samples).abs() < 1e-4,
            "block-rate and sample-rate phase must agree: {after_block} against \
             {after_samples}"
        );
    }
}
