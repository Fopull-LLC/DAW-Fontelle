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

/// How an envelope's falling stages are shaped.
///
/// This is not cosmetic. A note held on a linear-in-amplitude decay sits near
/// full level for most of the decay and then drops off a cliff; a real
/// instrument — and every SF2 file, which is written expecting the shape below
/// — loses a constant number of decibels per second, so it falls fast at first
/// and then tails away. Getting this wrong makes every sustained soundfont
/// patch sound wrong in a way that is easy to hear and hard to attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvelopeCurve {
    /// Every stage is a straight line in amplitude. Correct for a modulation
    /// envelope, whose output is a control value rather than a gain — SF2
    /// defines its modulation envelope this way — and the right default for a
    /// general-purpose envelope with no particular domain in mind.
    #[default]
    Linear,
    /// Decay and release fall at a constant rate in decibels; attack stays a
    /// linear amplitude ramp. This is the SF2 2.04 volume envelope.
    ///
    /// On this curve a stage *time* is the time to travel [`DECIBEL_SPAN_DB`],
    /// not the duration of the stage: SF2 defines `decayVolEnv` and
    /// `releaseVolEnv` as "the time for a 100% change in the Volume Envelope
    /// value", with full attenuation being 1000 centibels. A stage with less
    /// than that to cover finishes proportionally sooner — a decay to a -6 dB
    /// sustain takes 6% of `decay_s`. Reading those times as stage durations
    /// instead stretches every decay by more than an order of magnitude.
    Decibel,
}

/// The reference span for [`EnvelopeCurve::Decibel`] stage times, and the level
/// below which a decibel-curve release is considered silent (-100 dB).
pub const DECIBEL_SPAN_DB: f32 = 100.0;

/// -100 dB in linear amplitude. Also exactly what `sustainVolEnv`'s
/// full-attenuation value (1000 cB) imports to, so a "sustain at zero" patch
/// and the floor agree.
const MIN_LEVEL: f32 = 1e-5;

fn level_to_db(level: f32) -> f32 {
    20.0 * level.max(MIN_LEVEL).log10()
}

fn db_to_level(db: f32) -> f32 {
    // 10^(db/20), via exp2 because it is the cheaper of the two and this runs
    // per sample per voice.
    (db * (std::f32::consts::LOG2_10 / 20.0)).exp2()
}

#[derive(Debug, Clone, Copy)]
pub struct EnvelopeConfig {
    pub delay_s: f32,
    pub attack_s: f32,
    pub hold_s: f32,
    pub decay_s: f32,
    pub sustain_level: f32,
    pub release_s: f32,
    pub curve: EnvelopeCurve,
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
        // An envelope starts at zero, and [`EnvelopeGenerator::level`] can be
        // read before the first `advance`. Left alone, the last note's final
        // level would be what a modulation destination saw for one block.
        self.level = 0.0;
    }

    pub fn note_off(&mut self) {
        self.release_start_level = self.level;
        self.stage = Some(EnvelopeStage::Release);
        self.time_in_stage = 0.0;
    }

    /// The level the last `advance` produced, without advancing.
    ///
    /// What a modulation destination reads: an envelope used as a source is
    /// sampled once per block and held, so the destination needs the current
    /// value rather than the next one.
    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.stage, None | Some(EnvelopeStage::Idle))
    }

    /// How long `stage` actually lasts, which on [`EnvelopeCurve::Decibel`] is
    /// not the same as the configured stage time — see that variant's docs.
    /// `Idle` and `Sustain` never reach here; `advance` returns out of them
    /// before asking.
    fn stage_duration(&self, stage: EnvelopeStage, config: &EnvelopeConfig) -> f32 {
        let db_share = |span_db: f32| span_db.clamp(0.0, DECIBEL_SPAN_DB) / DECIBEL_SPAN_DB;
        match stage {
            EnvelopeStage::Delay => config.delay_s,
            EnvelopeStage::Attack => config.attack_s,
            EnvelopeStage::Hold => config.hold_s,
            EnvelopeStage::Decay => match config.curve {
                EnvelopeCurve::Linear => config.decay_s,
                // From 0 dB down to the sustain level.
                EnvelopeCurve::Decibel => {
                    config.decay_s * db_share(-level_to_db(config.sustain_level))
                }
            },
            EnvelopeStage::Release => match config.curve {
                EnvelopeCurve::Linear => config.release_s,
                // From wherever `note_off` interrupted, down to the floor.
                EnvelopeCurve::Decibel => {
                    config.release_s
                        * db_share(level_to_db(self.release_start_level) + DECIBEL_SPAN_DB)
                }
            },
            EnvelopeStage::Idle | EnvelopeStage::Sustain => 0.0,
        }
    }

    /// Advances by one sample and returns the envelope's level in `[0, 1]`
    /// (`Release` may briefly exceed it if `note_off` interrupted a stage above
    /// 1.0, which never happens with well-formed configs). Stages with zero (or
    /// negative) duration are skipped within the same call, so a chain of
    /// zero-length stages — `hold_s == 0.0 && decay_s == 0.0`, the common case —
    /// resolves to sustain on the very next sample rather than stalling.
    ///
    /// Each stage's level is computed from the time elapsed within it rather
    /// than accumulated sample by sample. That costs an `exp2` per sample on
    /// the decibel curve, but it is the only form that stays correct when the
    /// mod matrix moves a stage time or level *during* the stage (TDD §7.5); a
    /// one-multiply recursive coefficient would have to be rederived on every
    /// change and would drift in between.
    pub fn advance(&mut self, config: &EnvelopeConfig, sample_rate: f32) -> f32 {
        let Some(mut stage) = self.stage else {
            self.level = 0.0;
            return 0.0;
        };

        let dt = 1.0 / sample_rate;

        let duration = loop {
            match stage {
                EnvelopeStage::Idle => {
                    self.stage = None;
                    self.level = 0.0;
                    return 0.0;
                }
                EnvelopeStage::Sustain => {
                    self.stage = Some(stage);
                    self.level = config.sustain_level;
                    return self.level;
                }
                _ => {}
            }

            let duration = self.stage_duration(stage, config);

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
            EnvelopeStage::Decay => match config.curve {
                EnvelopeCurve::Linear => 1.0 + (config.sustain_level - 1.0) * t,
                // Linear in dB from 0 dB to the sustain level is exactly
                // `sustain^t` — the floor keeps a sustain of zero from
                // collapsing the whole stage to silence on its first sample.
                EnvelopeCurve::Decibel => config.sustain_level.clamp(MIN_LEVEL, 1.0).powf(t),
            },
            EnvelopeStage::Release => match config.curve {
                EnvelopeCurve::Linear => self.release_start_level * (1.0 - t),
                EnvelopeCurve::Decibel => {
                    let span_db = (level_to_db(self.release_start_level) + DECIBEL_SPAN_DB)
                        .clamp(0.0, DECIBEL_SPAN_DB);
                    self.release_start_level * db_to_level(-span_db * t)
                }
            },
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
            curve: EnvelopeCurve::Linear,
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
            curve: EnvelopeCurve::Linear,
        };
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        let level = env.advance(&zero, SR);
        assert_eq!(
            level, 1.0,
            "delay/attack/hold/decay all being 0 duration should land on sustain in one call"
        );
    }

    /// The same shape as `config()` but on the decibel curve, so the two can be
    /// compared directly. Sustain 0.5 is -6.02 dB, which matters below: on the
    /// decibel curve a stage time is the time to travel `DECIBEL_SPAN_DB`, so a
    /// stage with only 6 dB to cover takes 6% of it.
    fn db_config() -> EnvelopeConfig {
        EnvelopeConfig {
            curve: EnvelopeCurve::Decibel,
            ..config()
        }
    }

    #[test]
    fn decibel_decay_falls_at_a_constant_db_rate_not_a_constant_amplitude_rate() {
        // Sustain at the -100 dB floor, so the decay runs the full span and
        // `decay_s` is exactly the time spent in decay (SF2 2.04: "if the
        // sustain level were zero, the decay time would be the time spent in
        // the decay phase").
        let config = EnvelopeConfig {
            attack_s: 0.0, // straight into decay; the attack has its own test
            decay_s: 1.0,
            sustain_level: 0.0,
            ..db_config()
        };
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        let levels: Vec<f32> = (0..1001).map(|_| env.advance(&config, SR)).collect();

        // 1000 samples for 100 dB is 10 dB per 100 samples: each 100-sample
        // step multiplies the level by 10^(-0.5). A linear-in-amplitude decay
        // would read 0.9 / 0.8 / 0.5 at these points instead.
        assert!((levels[0] - 1.0).abs() < 1e-4, "decay starts at unity");
        assert!(
            (levels[100] - 0.316_23).abs() < 1e-3,
            "10 dB down after a tenth of the decay, got {}",
            levels[100]
        );
        assert!(
            (levels[200] - 0.1).abs() < 1e-3,
            "20 dB down after a fifth, got {}",
            levels[200]
        );
        assert!(
            (levels[500] - 0.003_162_3).abs() < 1e-4,
            "50 dB down at the halfway point, got {}",
            levels[500]
        );
    }

    #[test]
    fn decibel_decay_reaches_sustain_after_its_share_of_the_decay_time() {
        // sustain 0.5 = -6.02 dB out of a 100 dB span, so the decay phase lasts
        // 6.02% of decay_s: 60 samples of the nominal 1000, not all 1000.
        let config = EnvelopeConfig {
            attack_s: 0.0,
            decay_s: 1.0,
            sustain_level: 0.5,
            ..db_config()
        };
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        let levels: Vec<f32> = (0..200).map(|_| env.advance(&config, SR)).collect();

        assert!(
            (levels[30] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "half the dB span at half the decay is -3 dB, got {}",
            levels[30]
        );
        assert!(
            (levels[61] - 0.5).abs() < 1e-3,
            "6 dB at 100 dB per decay_s takes ~60 of 1000 samples; a decay that \
             stretches the 6 dB across the whole decay_s would still read ~0.97 here, got {}",
            levels[61]
        );
        assert!(
            (levels[199] - 0.5).abs() < 1e-6,
            "and then holds at sustain"
        );
    }

    #[test]
    fn decibel_release_falls_to_the_floor_at_the_same_rate() {
        // Releasing from sustain 0.5 (-6.02 dB) leaves 93.98 dB to the floor,
        // so the release lasts 93.98% of release_s: ~94 of 100 samples.
        let config = EnvelopeConfig {
            release_s: 0.1,
            sustain_level: 0.5,
            ..db_config()
        };
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        for _ in 0..300 {
            env.advance(&config, SR); // settle into sustain
        }
        env.note_off();
        let levels: Vec<f32> = (0..100).map(|_| env.advance(&config, SR)).collect();

        assert!(
            (levels[0] - 0.5).abs() < 1e-4,
            "release starts where it left off"
        );
        let half_span = 0.5 * 10f32.powf(-93.979 / 2.0 / 20.0);
        assert!(
            (levels[47] - half_span).abs() < 1e-4,
            "half the remaining dB span after half the release, expected {half_span}, got {}",
            levels[47]
        );
        assert!(
            levels[92] > 0.0,
            "still audible just before the release ends"
        );
        assert_eq!(levels[96], 0.0, "and silent just after");
        assert!(!env.is_active());
    }

    #[test]
    fn decibel_curve_leaves_the_attack_linear_in_amplitude() {
        // SF2 puts only decay and release in the dB domain; the volume
        // envelope's attack is a linear amplitude ramp, and an exponential one
        // would make every note's onset audibly soft.
        let mut env = EnvelopeGenerator::new();
        env.note_on();
        for _ in 0..50 {
            env.advance(&db_config(), SR);
        }
        let halfway = env.advance(&db_config(), SR);
        assert!(
            (halfway - 0.5).abs() < 0.02,
            "50/100 samples into the attack should still be ~0.5, got {halfway}"
        );
    }
}
