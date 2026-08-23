#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeStage {
    Idle,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvelopeConfig {
    pub delay_s: f32,
    pub attack_s: f32,
    pub hold_s: f32,
    pub decay_s: f32,
    pub sustain_level: f32,
    pub release_s: f32,
}

/// A single multi-stage envelope generator. Every field of `EnvelopeConfig` is a
/// mod-matrix destination (TDD §7.5). Fixed-cost per voice (INVARIANT 6): no
/// allocation, no dynamic stage list.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvelopeGenerator {
    stage: Option<EnvelopeStage>,
    level: f32,
    time_in_stage: f32,
}

impl EnvelopeGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn note_on(&mut self) {
        self.stage = Some(EnvelopeStage::Delay);
        self.time_in_stage = 0.0;
    }

    pub fn note_off(&mut self) {
        self.stage = Some(EnvelopeStage::Release);
        self.time_in_stage = 0.0;
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.stage, None | Some(EnvelopeStage::Idle))
    }

    pub fn advance(&mut self, _config: &EnvelopeConfig, _sample_rate: f32) -> f32 {
        let _ = self.level;
        todo!("stage advance + level integration")
    }
}
