/// Topology-preserving-transform state-variable filter. Zero-delay feedback, so it
/// stays stable under per-sample modulation (TDD §13.4 — used by the parametric EQ
/// and anywhere a mod-matrix route targets cutoff/resonance).
#[derive(Debug, Clone, Copy, Default)]
pub struct SvfFilter {
    ic1eq: f32,
    ic2eq: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SvfMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Bell,
    LowShelf,
    HighShelf,
}

/// Cytomic-form SVF coefficients: `g`/`k` set the corner and damping, `a1..a3`
/// solve the zero-delay feedback loop, and `m0..m2` mix the filter's three
/// internal signals into the requested response.
///
/// Keeping the mode in the mix coefficients rather than in `process` means the
/// per-sample path has no branch on mode, and a mod-matrix route that moves
/// cutoff or resonance only has to rebuild this struct.
#[derive(Debug, Clone, Copy)]
pub struct SvfCoeffs {
    pub g: f32,
    pub k: f32,
    pub a1: f32,
    pub a2: f32,
    pub a3: f32,
    pub m0: f32,
    pub m1: f32,
    pub m2: f32,
}

/// Below this the `1/Q` damping term explodes; well under any musical setting,
/// and low enough that a caller asking for zero gets self-oscillation rather
/// than a division blow-up.
const MIN_Q: f32 = 0.025;

impl SvfFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Derives coefficients for one filter setting.
    ///
    /// `resonance` is **Q**, not a normalised dial: at the Butterworth value of
    /// `1/sqrt(2)` a lowpass is exactly -3 dB at its cutoff, and above that the
    /// magnitude at the corner *is* Q. That makes the parameter checkable, and
    /// it matches both `EqBand::q` and the SF2 spec, whose `initialFilterQ` is
    /// defined as the peak height above DC gain.
    ///
    /// `gain_db` applies only to `Bell`, `LowShelf`, and `HighShelf`; the other
    /// modes ignore it.
    ///
    /// RT-safe: arithmetic and one `tan`, no allocation. Cheap enough to call
    /// per sample when a mod route is moving the cutoff.
    pub fn coeffs(
        mode: SvfMode,
        cutoff_hz: f32,
        resonance: f32,
        gain_db: f32,
        sample_rate: f32,
    ) -> SvfCoeffs {
        // Keep the corner inside the representable band. `tan` goes to
        // infinity at Nyquist, and a cutoff at or past it is a coefficient
        // blow-up rather than a filter.
        let nyquist = sample_rate * 0.5;
        let cutoff = cutoff_hz.clamp(1.0, nyquist * 0.99);
        let q = resonance.max(MIN_Q);
        // Bilinear pre-warp, so the digital corner lands on the requested
        // analogue one instead of drifting as it approaches Nyquist.
        let g = (std::f32::consts::PI * cutoff / sample_rate).tan();
        let amplitude = 10f32.powf(gain_db / 40.0);

        // Bell and the shelves fold their gain into the damping and the corner
        // respectively, which is what keeps them a single 2-pole section.
        let (g, k) = match mode {
            SvfMode::Bell => (g, 1.0 / (q * amplitude)),
            SvfMode::LowShelf => (g / amplitude.sqrt(), 1.0 / q),
            SvfMode::HighShelf => (g * amplitude.sqrt(), 1.0 / q),
            _ => (g, 1.0 / q),
        };

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let (m0, m1, m2) = match mode {
            SvfMode::Lowpass => (0.0, 0.0, 1.0),
            SvfMode::Bandpass => (0.0, 1.0, 0.0),
            SvfMode::Highpass => (1.0, -k, -1.0),
            SvfMode::Notch => (1.0, -k, 0.0),
            SvfMode::Bell => (1.0, k * (amplitude * amplitude - 1.0), 0.0),
            SvfMode::LowShelf => (1.0, k * (amplitude - 1.0), amplitude * amplitude - 1.0),
            SvfMode::HighShelf => (
                amplitude * amplitude,
                k * (1.0 - amplitude) * amplitude,
                1.0 - amplitude * amplitude,
            ),
        };

        SvfCoeffs {
            g,
            k,
            a1,
            a2,
            a3,
            m0,
            m1,
            m2,
        }
    }

    /// One sample through the zero-delay-feedback loop.
    ///
    /// The feedback is solved algebraically rather than by inserting a unit
    /// delay, which is why the filter stays stable when the coefficients change
    /// between one sample and the next — the case a mod-matrix route on cutoff
    /// creates constantly, and the one a naive biquad handles badly.
    pub fn process(&mut self, input: f32, coeffs: &SvfCoeffs) -> f32 {
        let v3 = input - self.ic2eq;
        let v1 = coeffs.a1 * self.ic1eq + coeffs.a2 * v3;
        let v2 = self.ic2eq + coeffs.a2 * self.ic1eq + coeffs.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        coeffs.m0 * input + coeffs.m1 * v1 + coeffs.m2 * v2
    }
}

/// One-pole high-pass, used for DC offset removal at sample import (TDD §7.8.4).
#[derive(Debug, Clone, Copy, Default)]
pub struct DcBlocker {
    prev_in: f32,
    prev_out: f32,
}

impl DcBlocker {
    /// `y[n] = x[n] - x[n-1] + r*y[n-1]`, the standard one-pole/one-zero DC
    /// blocker. `r` is placed from `cutoff_hz` so the -3 dB corner lands where
    /// asked; the differencing zero sits exactly on DC, so a constant offset
    /// decays to nothing rather than merely being attenuated.
    pub fn process(&mut self, input: f32, cutoff_hz: f32, sample_rate: f32) -> f32 {
        let r = 1.0 - std::f32::consts::TAU * (cutoff_hz.max(0.0) / sample_rate.max(1.0));
        let r = r.clamp(0.0, 0.999_999);
        let out = input - self.prev_in + r * self.prev_out;
        self.prev_in = input;
        self.prev_out = out;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    /// Butterworth: the SF2 "no resonance" case, and the point where a 2-pole
    /// lowpass is exactly -3 dB at its cutoff.
    const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

    /// Peak amplitude of the filter's steady-state response to a sine at
    /// `freq_hz`. The first half of the run is discarded so the transient from
    /// zero initial state doesn't count.
    fn response(mode: SvfMode, cutoff_hz: f32, q: f32, freq_hz: f32) -> f32 {
        let coeffs = SvfFilter::coeffs(mode, cutoff_hz, q, 0.0, SR);
        let mut filter = SvfFilter::new();
        let total = 8192;
        let mut peak = 0.0f32;
        for i in 0..total {
            let phase = std::f32::consts::TAU * freq_hz * i as f32 / SR;
            let out = filter.process(phase.sin(), &coeffs);
            if i > total / 2 {
                peak = peak.max(out.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_passes_dc_untouched() {
        let coeffs = SvfFilter::coeffs(SvfMode::Lowpass, 1_000.0, BUTTERWORTH_Q, 0.0, SR);
        let mut filter = SvfFilter::new();
        let mut out = 0.0;
        for _ in 0..4096 {
            out = filter.process(1.0, &coeffs);
        }
        assert!(
            (out - 1.0).abs() < 1e-3,
            "a lowpass must have unity DC gain, settled at {out}"
        );
    }

    #[test]
    fn lowpass_is_three_db_down_at_its_cutoff() {
        // The defining property, and the one that catches a missing bilinear
        // pre-warp: without `tan()` the actual corner drifts from the
        // requested one, increasingly so as the cutoff approaches Nyquist.
        for cutoff in [100.0, 1_000.0, 10_000.0] {
            let got = response(SvfMode::Lowpass, cutoff, BUTTERWORTH_Q, cutoff);
            assert!(
                (got - BUTTERWORTH_Q).abs() < 0.02,
                "cutoff {cutoff} Hz: expected ~{BUTTERWORTH_Q} at the corner, got {got}"
            );
        }
    }

    #[test]
    fn lowpass_rejects_content_far_above_the_cutoff() {
        let got = response(SvfMode::Lowpass, 500.0, BUTTERWORTH_Q, 12_000.0);
        assert!(
            got < 0.01,
            "24x above a 2-pole corner should be far down, got {got}"
        );
    }

    #[test]
    fn resonance_peaks_at_the_cutoff() {
        // `resonance` is Q: the lowpass magnitude at the corner is Q itself, so
        // the parameter means something checkable rather than being a dial.
        for q in [2.0f32, 4.0, 8.0] {
            let got = response(SvfMode::Lowpass, 1_000.0, q, 1_000.0);
            assert!(
                (got - q).abs() < q * 0.05,
                "Q {q} should peak at ~{q} at the corner, got {got}"
            );
        }
    }

    #[test]
    fn highpass_blocks_dc_and_passes_the_top() {
        let coeffs = SvfFilter::coeffs(SvfMode::Highpass, 1_000.0, BUTTERWORTH_Q, 0.0, SR);
        let mut filter = SvfFilter::new();
        let mut out = 0.0;
        for _ in 0..4096 {
            out = filter.process(1.0, &coeffs);
        }
        assert!(
            out.abs() < 1e-3,
            "a highpass must reject DC, settled at {out}"
        );
        let top = response(SvfMode::Highpass, 100.0, BUTTERWORTH_Q, 12_000.0);
        assert!((top - 1.0).abs() < 0.02, "and pass the top, got {top}");
    }

    #[test]
    fn bandpass_peaks_at_the_cutoff_and_rejects_both_ends() {
        let at_centre = response(SvfMode::Bandpass, 1_000.0, BUTTERWORTH_Q, 1_000.0);
        let below = response(SvfMode::Bandpass, 1_000.0, BUTTERWORTH_Q, 30.0);
        let above = response(SvfMode::Bandpass, 1_000.0, BUTTERWORTH_Q, 20_000.0);
        assert!(at_centre > below * 10.0 && at_centre > above * 10.0);
    }

    #[test]
    fn notch_rejects_the_cutoff_and_passes_both_ends() {
        let at_centre = response(SvfMode::Notch, 1_000.0, BUTTERWORTH_Q, 1_000.0);
        let below = response(SvfMode::Notch, 1_000.0, BUTTERWORTH_Q, 30.0);
        let above = response(SvfMode::Notch, 1_000.0, BUTTERWORTH_Q, 20_000.0);
        assert!(
            at_centre < 0.1,
            "notch centre should be rejected, got {at_centre}"
        );
        assert!((below - 1.0).abs() < 0.05 && (above - 1.0).abs() < 0.05);
    }

    #[test]
    fn bell_lifts_only_its_own_band() {
        let coeffs = SvfFilter::coeffs(SvfMode::Bell, 1_000.0, 2.0, 12.0, SR);
        let mut at_centre = SvfFilter::new();
        let mut peak = 0.0f32;
        for i in 0..8192 {
            let phase = std::f32::consts::TAU * 1_000.0 * i as f32 / SR;
            let out = at_centre.process(phase.sin(), &coeffs);
            if i > 4096 {
                peak = peak.max(out.abs());
            }
        }
        let expected = 10f32.powf(12.0 / 20.0);
        assert!(
            (peak - expected).abs() < expected * 0.05,
            "+12 dB bell should reach ~{expected} at its centre, got {peak}"
        );
    }

    #[test]
    fn stays_stable_when_the_cutoff_moves_every_sample() {
        // The reason for the zero-delay-feedback topology at all (TDD §13.4):
        // a mod-matrix route on cutoff moves the coefficients per sample, and a
        // naive biquad blows up or zipper-noises when it does.
        let mut filter = SvfFilter::new();
        let mut peak = 0.0f32;
        for i in 0..48_000 {
            let sweep = 200.0 + 15_000.0 * (i as f32 / 300.0).sin().abs();
            let coeffs = SvfFilter::coeffs(SvfMode::Lowpass, sweep, 6.0, 0.0, SR);
            let phase = std::f32::consts::TAU * 440.0 * i as f32 / SR;
            let out = filter.process(phase.sin(), &coeffs);
            assert!(out.is_finite(), "sample {i} produced {out}");
            peak = peak.max(out.abs());
        }
        assert!(
            peak < 20.0,
            "a swept resonant lowpass should stay bounded, peaked at {peak}"
        );
    }

    #[test]
    fn reset_clears_the_state() {
        let coeffs = SvfFilter::coeffs(SvfMode::Lowpass, 1_000.0, BUTTERWORTH_Q, 0.0, SR);
        let mut filter = SvfFilter::new();
        for _ in 0..1000 {
            filter.process(1.0, &coeffs);
        }
        filter.reset();
        let fresh = SvfFilter::new();
        assert_eq!(
            filter.process(0.5, &coeffs),
            {
                let mut f = fresh;
                f.process(0.5, &coeffs)
            },
            "a reset filter must behave like a new one"
        );
    }

    #[test]
    fn dc_blocker_removes_a_constant_offset() {
        let mut blocker = DcBlocker::default();
        let mut out = 1.0;
        for _ in 0..48_000 {
            out = blocker.process(1.0, 20.0, SR);
        }
        assert!(
            out.abs() < 1e-2,
            "a steady offset should be removed, left {out}"
        );
    }

    #[test]
    fn dc_blocker_passes_audio_above_its_corner() {
        let mut blocker = DcBlocker::default();
        let mut peak = 0.0f32;
        for i in 0..48_000 {
            let phase = std::f32::consts::TAU * 1_000.0 * i as f32 / SR;
            let out = blocker.process(phase.sin(), 20.0, SR);
            if i > 24_000 {
                peak = peak.max(out.abs());
            }
        }
        assert!(
            (peak - 1.0).abs() < 0.01,
            "1 kHz is far above a 20 Hz corner and should pass, got {peak}"
        );
    }
}
