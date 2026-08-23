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

#[derive(Debug, Clone, Copy, Default)]
pub struct Oscillator {
    phase: f32,
}

impl Oscillator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Band-limited where it matters (`Saw`/`Square`); `Sine`/`Triangle`/`Noise` need
    /// no anti-aliasing treatment.
    pub fn next_sample(&mut self, _kind: OscKind, _freq_hz: f32, _sample_rate: f32) -> f32 {
        todo!("PolyBLEP or equivalent band-limited generation")
    }
}
