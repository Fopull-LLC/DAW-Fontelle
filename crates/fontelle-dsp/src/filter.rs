/// Topology-preserving-transform state-variable filter. Zero-delay feedback, so it
/// stays stable under per-sample modulation (TDD §13.4 — used by the parametric EQ
/// and anywhere a mod-matrix route targets cutoff/resonance).
#[derive(Debug, Clone, Copy, Default)]
pub struct SvfFilter {
    ic1eq: f32,
    ic2eq: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum SvfMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Bell,
    LowShelf,
    HighShelf,
}

#[derive(Debug, Clone, Copy)]
pub struct SvfCoeffs {
    pub g: f32,
    pub k: f32,
    pub a1: f32,
    pub a2: f32,
    pub a3: f32,
}

impl SvfFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    pub fn coeffs(
        _mode: SvfMode,
        _cutoff_hz: f32,
        _resonance: f32,
        _sample_rate: f32,
    ) -> SvfCoeffs {
        todo!("TPT/SVF coefficient derivation — FONTELLE_TDD.md §13.4")
    }

    pub fn process(&mut self, _input: f32, _coeffs: &SvfCoeffs) -> f32 {
        todo!("zero-delay-feedback SVF process step")
    }
}

/// One-pole high-pass, used for DC offset removal at sample import (TDD §7.8.4).
#[derive(Debug, Clone, Copy, Default)]
pub struct DcBlocker {
    prev_in: f32,
    prev_out: f32,
}

impl DcBlocker {
    pub fn process(&mut self, input: f32, _cutoff_hz: f32, _sample_rate: f32) -> f32 {
        let _ = (self.prev_in, self.prev_out, input);
        todo!("one-pole DC blocker")
    }
}
