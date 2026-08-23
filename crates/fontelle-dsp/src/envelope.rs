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
    /// Captured on `note_off` so an early release ramps down from wherever the
    /// envelope actually was, not from full scale.
    release_start_level: f32,
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
        self.release_start_level = self.level;
        self.stage = Some(EnvelopeStage::Release);
        self.time_in_stage = 0.0;
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.stage, None | Some(EnvelopeStage::Idle))
    }

    /// Advances by one sample and returns the envelope's level in `[0, 1]`
    /// (`Release` may briefly exceed it if `note_off` interrupted a stage above
    /// 1.0, which never happens with well-formed configs). Stages with zero (or
    /// negative) duration are skipped within the same call, so a chain of
    /// zero-length stages — `hold_s == 0.0 && decay_s == 0.0`, the common case —
    /// resolves to sustain on the very next sample rather than stalling.
    pub fn advance(&mut self, config: &EnvelopeConfig, sample_rate: f32) -> f32 {
        let Some(mut stage) = self.stage else {
            self.level = 0.0;
            return 0.0;
        };

        let dt = 1.0 / sample_rate;

        let duration = loop {
            let duration = match stage {
                EnvelopeStage::Idle => {
                    self.stage = None;
                    self.level = 0.0;
                    return 0.0;
                }
                EnvelopeStage::Delay => config.delay_s,
                EnvelopeStage::Attack => config.attack_s,
                EnvelopeStage::Hold => config.hold_s,
                EnvelopeStage::Decay => config.decay_s,
                EnvelopeStage::Sustain => {
                    self.stage = Some(stage);
                    self.level = config.sustain_level;
                    return self.level;
                }
                EnvelopeStage::Release => config.release_s,
            };

            // Half-a-sample tolerance: accumulating `dt` in f32 drifts either
            // side of an exact target, and a strict `<` here would make the
            // stage-completion sample depend on which way it drifted.
            if self.time_in_stage < duration - dt * 0.5 {
                break duration;
            }

            self.time_in_stage -= duration.max(0.0);
            stage = match stage {
                EnvelopeStage::Delay => EnvelopeStage::Attack,
                EnvelopeStage::Attack => EnvelopeStage::Hold,
                EnvelopeStage::Hold => EnvelopeStage::Decay,
                EnvelopeStage::Decay => EnvelopeStage::Sustain,
                EnvelopeStage::Release => EnvelopeStage::Idle,
                other => other,
            };
        };

        let t = if duration > 0.0 {
            (self.time_in_stage / duration).clamp(0.0, 1.0)
        } else {
            1.0
        };

        self.level = match stage {
            EnvelopeStage::Delay => 0.0,
            EnvelopeStage::Attack => t,
            EnvelopeStage::Hold => 1.0,
            EnvelopeStage::Decay => 1.0 + (config.sustain_level - 1.0) * t,
            EnvelopeStage::Release => self.release_start_level * (1.0 - t),
            EnvelopeStage::Sustain | EnvelopeStage::Idle => unreachable!("handled above"),
        };

        self.stage = Some(stage);
        self.time_in_stage += dt;
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// delay=0, attack=0.1s, hold=0, decay=0, sustain=0.5, release=0.1s, at a
    /// sample rate chosen so attack is exactly 100 samples and release exactly
    /// 50 samples — every assertion below is an exact sample count, not a
    /// tolerance-based guess.
    fn config() -> EnvelopeConfig {
        EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.1,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 0.5,
            release_s: 0.05,
        }
    }
    const SR: f32 = 1000.0;

    #[test]
    fn silent_and_inactive_before_note_on() {
        let mut env = EnvelopeGenerator::new();
        assert!(!env.is_active());
        assert_eq!(env.advance(&config(), SR), 0.0);
    }

    #[test]
    fn attack_ramps_from_zero_to_one_linearly() {
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        assert!(env.is_active());

        let first = env.advance(&config(), SR);
        assert_eq!(first, 0.0, "attack must start at 0, not jump ahead");

        for _ in 0..49 {
            env.advance(&config(), SR);
        }
        let halfway = env.advance(&config(), SR);
        assert!(
            (halfway - 0.5).abs() < 0.02,
            "50/100 samples into a linear attack should be ~0.5, got {halfway}"
        );
    }

    #[test]
    fn zero_duration_hold_and_decay_land_exactly_on_sustain() {
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        // 100 samples completes the 0.1s attack; hold=0 and decay=0 must not
        // stall an extra sample each — the very next call should already read
        // sustain_level, not 1.0 (attack peak) or an intermediate value.
        for _ in 0..100 {
            env.advance(&config(), SR);
        }
        let after_attack = env.advance(&config(), SR);
        assert!(
            (after_attack - config().sustain_level).abs() < 1e-4,
            "expected sustain level {} immediately after attack completes, got {after_attack}",
            config().sustain_level
        );
    }

    #[test]
    fn sustain_holds_indefinitely_until_note_off() {
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        for _ in 0..10_000 {
            env.advance(&config(), SR);
        }
        assert_eq!(env.advance(&config(), SR), config().sustain_level);
        assert!(env.is_active());
    }

    #[test]
    fn release_ramps_to_zero_then_goes_inactive() {
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        for _ in 0..200 {
            env.advance(&config(), SR); // settle into sustain
        }
        env.note_off();

        // release_s = 0.05s @ 1000Hz = 50 samples of ramp-down; by the same
        // "stage completes on call `duration*rate + 1`" convention the
        // zero-duration test above already confirms, sample 51 is the first
        // one past the release.
        let mut previous = f32::INFINITY;
        for _ in 0..50 {
            let level = env.advance(&config(), SR);
            assert!(
                level <= previous,
                "release must be monotonically non-increasing"
            );
            assert!(
                level > 0.0,
                "should still be releasing within the 50-sample ramp"
            );
            previous = level;
        }
        let after_release = env.advance(&config(), SR);
        assert!(
            after_release.abs() < 1e-4,
            "release should reach ~0 by sample 51, got {after_release}"
        );
        assert!(
            !env.is_active(),
            "envelope must go inactive once release completes"
        );

        // Once idle, it must stay silent rather than resetting or looping.
        assert_eq!(env.advance(&config(), SR), 0.0);
    }

    #[test]
    fn early_release_ramps_down_from_current_level_not_from_full_scale() {
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        // 30 samples into a 100-sample attack: level should be ~0.3.
        for _ in 0..30 {
            env.advance(&config(), SR);
        }
        env.note_off();
        let just_after_release = env.advance(&config(), SR);
        assert!(
            just_after_release < 0.35,
            "an early release must ramp down from the level it interrupted (~0.3), \
             not snap up to 1.0 first — got {just_after_release}"
        );
    }

    #[test]
    fn cascading_zero_duration_stages_resolve_in_a_single_call() {
        let zero = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.0,
        };
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        let level = env.advance(&zero, SR);
        assert_eq!(
            level, 1.0,
            "delay/attack/hold/decay all being 0 duration should land on sustain in one call"
        );
    }
}
