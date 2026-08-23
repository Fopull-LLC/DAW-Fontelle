#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSource {
    Envelope(u8),
    Lfo(u8),
    Velocity,
    Key,
    Aftertouch,
    ModWheel,
    PitchBend,
    Random,
    NoteOnCounter,
}

/// Any continuous patch parameter, addressed by stable ID (TDD §7.5). Minimum set:
/// layer pitch/gain/pan, sample start offset, loop start/length, filter cutoff and
/// resonance, every envelope stage time/level, every LFO rate/depth, unison detune.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModDest {
    LayerPitch(u8),
    LayerGain(u8),
    LayerPan(u8),
    SampleStartOffset(u8),
    LoopStart(u8),
    LoopLength(u8),
    FilterCutoff(u8),
    FilterResonance(u8),
    EnvelopeStageTime(u8, u8),
    EnvelopeStageLevel(u8, u8),
    LfoRate(u8),
    LfoDepth(u8),
    UnisonDetune,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Exponential,
    Logarithmic,
    SCurve,
    Quantised,
}

#[derive(Debug, Clone, Copy)]
pub struct ModRoute {
    pub source: ModSource,
    pub destination: ModDest,
    /// Bipolar.
    pub depth: f32,
    pub curve: Curve,
    /// A secondary modulator scaling this route's depth — e.g. "LFO depth
    /// controlled by mod wheel" (TDD §7.5).
    pub via: Option<ModSource>,
}

#[derive(Debug, Clone, Default)]
pub struct ModMatrix {
    pub routes: Vec<ModRoute>,
}

impl ModMatrix {
    /// Sums every route targeting `dest` into a single modulation value for this
    /// voice's current source values. Called once per block per destination that
    /// has at least one route — never allocates (INVARIANT 1).
    pub fn evaluate(&self, _dest: ModDest, _source_values: &dyn Fn(ModSource) -> f32) -> f32 {
        todo!("sum routed sources through curve + via-scaling")
    }
}
