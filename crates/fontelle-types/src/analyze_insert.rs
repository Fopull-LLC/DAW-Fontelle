//! Analyze Musically as a **mixer insert** (`docs/analyze-musically-plan.md`
//! §6.1): an effect that passes audio through untouched and, when armed,
//! records what plays through the track into a study — Edison's place in
//! FL Studio.
//!
//! > *"you should be able to add it to a mixer track as a plugin like you
//! > can with edison in fl to record into it like that and then send
//! > something into the playlist"* — Ty
//!
//! These are the insert's **saved settings**: how it arms, its threshold,
//! where on the strip it listens, and which study it records into. Whether
//! it is armed right now is not saved — an insert that started recording
//! the moment a song was opened would be a surprise — and lives on the
//! engine's capture tap (`fontelle_engine::AnalyzeCapture`).

use crate::effect::{MIX, with_mix};

/// When an armed insert records (§6.1, Edison's three).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ArmMode {
    /// While the transport plays: a take is exactly the span that played.
    #[default]
    OnPlay,
    /// From the first sample over the threshold until the signal has stayed
    /// under it for the release time — catches the next take hands-free.
    OnInput,
    /// From the moment it is armed until it is disarmed.
    Now,
}

impl ArmMode {
    pub const ALL: [Self; 3] = [Self::OnPlay, Self::OnInput, Self::Now];

    pub fn label(self) -> &'static str {
        match self {
            Self::OnPlay => "on play",
            Self::OnInput => "on input",
            Self::Now => "now",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }
}

/// What an Analyze Musically insert is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnalyzeConfig {
    pub arm: ArmMode,
    /// [`ArmMode::OnInput`]'s threshold, in dBFS on the louder side.
    pub threshold_db: f32,
    /// [`ArmMode::OnInput`]'s release: how long under the threshold ends a
    /// take, in milliseconds.
    pub release_ms: f32,
    /// Records after the track's fader and pan rather than at the insert's
    /// place in the chain (pre-fader, what Edison does).
    #[serde(default)]
    pub post_fader: bool,
    /// The study this insert records into: its persistent id, `None` until
    /// the window binds one. Opaque here — the study model lives in the
    /// document (`Project::studies`), not in the effect.
    #[serde(default)]
    pub study: Option<crate::PersistentId>,
    /// The control every effect carries, and one this effect ignores: it is
    /// here because an automation lane naming `mix` must find one whatever
    /// the slot holds (`tests/effect_mix.rs`), and the engine never blends
    /// this insert — a wire with itself is itself, and `x·m + x·(1 − m)` is
    /// not `x` to the bit, which the pass-through promises.
    #[serde(default = "all_wet")]
    pub mix: f32,
}

fn all_wet() -> f32 {
    1.0
}

impl Default for AnalyzeConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzeConfig {
    pub fn new() -> Self {
        Self {
            arm: ArmMode::OnPlay,
            threshold_db: -40.0,
            release_ms: 1_000.0,
            post_fader: false,
            study: None,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "arm" => self.arm.index() as f32,
            "threshold" => self.threshold_db,
            "release" => self.release_ms,
            "post_fader" => {
                if self.post_fader {
                    1.0
                } else {
                    0.0
                }
            }
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "arm" => {
                if let Some(mode) = ArmMode::ALL.get(value.round().max(0.0) as usize) {
                    self.arm = *mode;
                }
            }
            "threshold" => self.threshold_db = value,
            "release" => self.release_ms = value,
            "post_fader" => self.post_fader = value >= 0.5,
            _ => {}
        }
    }
}

/// Fully wet, like every processor (see `mix`).
const ALL_WET: f32 = 100.0;

pub(crate) static ANALYZE_PARAMS: [crate::ParamSpec; 5] = with_mix(&ANALYZE_OWN_PARAMS, ALL_WET);

pub(crate) static ANALYZE_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Record",
    count: ANALYZE_PARAMS.len(),
}];

static ARM_MODES: [&str; 3] = ["on play", "on input", "now"];

static ANALYZE_OWN_PARAMS: [crate::ParamSpec; 4] = [
    crate::ParamSpec {
        id: "arm",
        name: "Arm",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(3),
        positions: &ARM_MODES,
    },
    crate::ParamSpec {
        id: "threshold",
        name: "Threshold",
        min: -80.0,
        max: 0.0,
        default: -40.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "release",
        name: "Release",
        min: 50.0,
        max: 10_000.0,
        default: 1_000.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "post_fader",
        name: "Post fader",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &[],
    },
];

/// The insert's bank: the ways to arm it that come up, by what they are for.
/// Every built-in ships one (`fontelle-app/tests/effect_editor.rs`), and for
/// a recorder the useful presets are its arming setups — Edison's "on
/// input" thresholds for a quiet booth or a loud stage are what people dial
/// in over and over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AnalyzePreset {
    /// On input at -40 dB, a second and a half of release: a sung or spoken
    /// phrase in a quiet room, breaths and all.
    CatchAVocal,
    /// On input at -24 dB, half a second: a loud source over a noisy room.
    CatchALoudSource,
    /// On input at -45 dB, three seconds: long phrases with gaps in them,
    /// kept as one take.
    CatchLongPhrases,
    OnPlay,
    OnPlayPostFader,
    Now,
    NowPostFader,
}

impl AnalyzePreset {
    pub const ALL: [Self; 7] = [
        Self::CatchAVocal,
        Self::CatchALoudSource,
        Self::CatchLongPhrases,
        Self::OnPlay,
        Self::OnPlayPostFader,
        Self::Now,
        Self::NowPostFader,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::CatchAVocal => "catch a vocal",
            Self::CatchALoudSource => "catch a loud source",
            Self::CatchLongPhrases => "catch long phrases",
            Self::OnPlay => "on play",
            Self::OnPlayPostFader => "on play, post fader",
            Self::Now => "now",
            Self::NowPostFader => "now, post fader",
        }
    }
}

impl AnalyzeConfig {
    /// The settings `preset` names. The study it records into is not a
    /// setting: a preset leaves it unbound.
    pub fn from_preset(preset: AnalyzePreset) -> Self {
        let on_input = |threshold_db: f32, release_ms: f32| Self {
            arm: ArmMode::OnInput,
            threshold_db,
            release_ms,
            ..Self::new()
        };
        match preset {
            AnalyzePreset::CatchAVocal => on_input(-40.0, 1_500.0),
            AnalyzePreset::CatchALoudSource => on_input(-24.0, 500.0),
            AnalyzePreset::CatchLongPhrases => on_input(-45.0, 3_000.0),
            AnalyzePreset::OnPlay => Self::new(),
            AnalyzePreset::OnPlayPostFader => Self {
                post_fader: true,
                ..Self::new()
            },
            AnalyzePreset::Now => Self {
                arm: ArmMode::Now,
                ..Self::new()
            },
            AnalyzePreset::NowPostFader => Self {
                arm: ArmMode::Now,
                post_fader: true,
                ..Self::new()
            },
        }
    }
}
