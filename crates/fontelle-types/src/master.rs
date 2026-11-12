//! Master: the mastering chain as one insert (Ty, 2026-11-12: *"songs made
//! in fontelle feel quieter on average. make sure theres a plugin
//! specifically for mastering that goes through different tools you might
//! want when you master including things like leveling out audio cutting
//! things that are inaudible to people, optimizing for multiple speaker
//! types, etc. all with a lot of producer control but with several pre made
//! presets that make the master chain really easy for anyone to just add and
//! make their songs sound more complete with a few minor tweaks"*).
//!
//! In the order the signal meets them — the order a mastering engineer
//! works in:
//!
//! 1. **Clean**: a low cut under what anyone can hear (rumble, DC, the
//!    sub-sonic energy that eats a limiter's headroom for nothing) and a
//!    high cut over it.
//! 2. **Tone**: four broad moves — low end, mud, presence, air.
//! 3. **Glue**: one knob of gentle bus compression, with its timing.
//! 4. **Space**: width, and the bass kept in the middle below a corner, so a
//!    mono club system and a phone both get all of it.
//! 5. **Speakers**: *small speakers* adds the bass's harmonics, which a
//!    phone or a laptop can play where it cannot play the bass itself; and
//!    *check on* previews the master as a mono system, a phone or a laptop
//!    would play it — for listening, never for exporting.
//! 6. **Loudness**: rides the level to a target loudness (LUFS) the way a
//!    streaming service measures it, under a ceiling a look-ahead limiter
//!    holds. This is what makes a song stop sounding quieter than the next.
//!
//! The config is the shape `effect.rs` gives every kind; the DSP is
//! `fontelle_fx::Master`, built from the EQ, compressor, width and limiter
//! that already exist.

use crate::effect::{MIX, percent_param, switch_param, with_mix};

fn all_wet() -> f32 {
    1.0
}

/// Where the bass-mono corner is off: the bottom of its knob.
pub const MASTER_MONO_OFF_HZ: f32 = 20.0;

/// What the master is previewed as. Off is the only setting that is the
/// master itself; the others are for listening and say so in the window.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum MasterCheck {
    #[default]
    Off,
    /// Both sides summed: what a club, a mono speaker or a phone call does.
    Mono,
    /// One small speaker: mono, nothing under ~250 Hz, little over ~8 kHz.
    Phone,
    /// Two small speakers close together: narrow, nothing under ~150 Hz.
    Laptop,
}

impl MasterCheck {
    pub const ALL: [Self; 4] = [Self::Off, Self::Mono, Self::Phone, Self::Laptop];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Mono => "mono",
            Self::Phone => "phone",
            Self::Laptop => "laptop",
        }
    }
}

static CHECK_POSITIONS: [&str; 4] = ["off", "mono", "phone", "laptop"];

/// The whole chain's settings. See the module.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MasterConfig {
    // ---- clean
    pub low_cut_hz: f32,
    pub high_cut_hz: f32,
    // ---- tone
    /// A low shelf at 90 Hz.
    pub low_db: f32,
    /// A wide bell at 300 Hz — where a crowded mix gets muddy.
    pub mud_db: f32,
    /// A wide bell at 3 kHz — where a voice and a snare speak.
    pub presence_db: f32,
    /// A high shelf at 12 kHz.
    pub air_db: f32,
    // ---- glue
    /// 0..=100: how hard the bus compressor holds the mix together.
    pub glue: f32,
    pub glue_attack_ms: f32,
    pub glue_release_ms: f32,
    // ---- space
    /// 0 (mono) ..= 2; 1 leaves it.
    pub width: f32,
    /// The side is high-passed here; [`MASTER_MONO_OFF_HZ`] is off.
    pub mono_below_hz: f32,
    // ---- speakers
    /// 0..=100: the bass's harmonics added for small speakers.
    pub small_speakers: f32,
    pub check: MasterCheck,
    // ---- loudness
    /// Ride the level to `target_lufs`.
    pub auto_level: bool,
    pub target_lufs: f32,
    pub ceiling_db: f32,
    /// The limiter's release.
    pub release_ms: f32,
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl MasterConfig {
    /// What a Master dropped on a strip does: cleans the extremes, keeps
    /// the bass in the middle and levels the song to streaming loudness
    /// under a −1 dB ceiling — and touches the tone not at all. Unlike most
    /// effects it is not a wire when it opens, because leveling is the
    /// reason to reach for it.
    pub fn new() -> Self {
        Self {
            low_cut_hz: 28.0,
            high_cut_hz: 20_000.0,
            low_db: 0.0,
            mud_db: 0.0,
            presence_db: 0.0,
            air_db: 0.0,
            glue: 20.0,
            glue_attack_ms: 30.0,
            glue_release_ms: 200.0,
            width: 1.0,
            mono_below_hz: 100.0,
            small_speakers: 0.0,
            check: MasterCheck::Off,
            auto_level: true,
            target_lufs: -14.0,
            ceiling_db: -1.0,
            release_ms: 80.0,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "low_cut" => self.low_cut_hz,
            "high_cut" => self.high_cut_hz,
            "low" => self.low_db,
            "mud" => self.mud_db,
            "presence" => self.presence_db,
            "air" => self.air_db,
            "glue" => self.glue,
            "glue_attack" => self.glue_attack_ms,
            "glue_release" => self.glue_release_ms,
            "width" => self.width * 100.0,
            "mono_below" => self.mono_below_hz,
            "small_speakers" => self.small_speakers,
            "check" => MasterCheck::ALL
                .iter()
                .position(|c| *c == self.check)
                .unwrap_or(0) as f32,
            "auto_level" => f32::from(u8::from(self.auto_level)),
            "target" => self.target_lufs,
            "ceiling" => self.ceiling_db,
            "release" => self.release_ms,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "low_cut" => self.low_cut_hz = value,
            "high_cut" => self.high_cut_hz = value,
            "low" => self.low_db = value,
            "mud" => self.mud_db = value,
            "presence" => self.presence_db = value,
            "air" => self.air_db = value,
            "glue" => self.glue = value,
            "glue_attack" => self.glue_attack_ms = value,
            "glue_release" => self.glue_release_ms = value,
            "width" => self.width = value / 100.0,
            "mono_below" => self.mono_below_hz = value,
            "small_speakers" => self.small_speakers = value,
            "check" => {
                if let Some(check) = MasterCheck::ALL.get(value.round().max(0.0) as usize) {
                    self.check = *check;
                }
            }
            "auto_level" => self.auto_level = value >= 0.5,
            "target" => self.target_lufs = value,
            "ceiling" => self.ceiling_db = value,
            "release" => self.release_ms = value,
            _ => {}
        }
    }
}

impl Default for MasterConfig {
    fn default() -> Self {
        Self::new()
    }
}

const fn db_param(
    id: &'static str,
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min,
        max,
        default,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    }
}

const fn hz_param(
    id: &'static str,
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min,
        max,
        default,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    }
}

const fn ms_param(
    id: &'static str,
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min,
        max,
        default,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    }
}

const fn auto_level_param() -> crate::ParamSpec {
    let mut spec = switch_param("auto_level", "Auto level");
    spec.default = 1.0;
    spec
}

pub(crate) static MASTER_PARAMS: [crate::ParamSpec; 18] = with_mix(&MASTER_OWN_PARAMS, 100.0);

pub(crate) static MASTER_SECTIONS: [crate::ParamSection; 6] = [
    crate::ParamSection {
        name: "Clean",
        count: 2,
    },
    crate::ParamSection {
        name: "Tone",
        count: 4,
    },
    crate::ParamSection {
        name: "Glue",
        count: 3,
    },
    crate::ParamSection {
        name: "Space",
        count: 2,
    },
    crate::ParamSection {
        name: "Speakers",
        count: 2,
    },
    // The mix rides with the loudness: the last card holds what comes out.
    crate::ParamSection {
        name: "Loudness",
        count: 5,
    },
];

static MASTER_OWN_PARAMS: [crate::ParamSpec; 17] = [
    hz_param("low_cut", "Low cut", 10.0, 150.0, 28.0),
    hz_param("high_cut", "High cut", 10_000.0, 22_000.0, 20_000.0),
    db_param("low", "Low end", -6.0, 6.0, 0.0),
    db_param("mud", "Mud", -6.0, 3.0, 0.0),
    db_param("presence", "Presence", -4.0, 4.0, 0.0),
    db_param("air", "Air", -6.0, 6.0, 0.0),
    percent_param("glue", "Glue", 20.0),
    ms_param("glue_attack", "Attack", 1.0, 100.0, 30.0),
    ms_param("glue_release", "Release", 30.0, 1_000.0, 200.0),
    crate::ParamSpec {
        id: "width",
        name: "Width",
        min: 0.0,
        max: 200.0,
        default: 100.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    hz_param("mono_below", "Mono bass", MASTER_MONO_OFF_HZ, 300.0, 100.0),
    percent_param("small_speakers", "Small speakers", 0.0),
    crate::ParamSpec {
        id: "check",
        name: "Check on",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(4),
        positions: &CHECK_POSITIONS,
    },
    auto_level_param(),
    crate::ParamSpec {
        id: "target",
        name: "Target LUFS",
        min: -20.0,
        max: -6.0,
        default: -14.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    db_param("ceiling", "Ceiling", -3.0, 0.0, -1.0),
    ms_param("release", "Limit release", 10.0, 500.0, 80.0),
];

/// Master's bank: a starting point for every kind of song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MasterPreset {
    /// Streaming loudness (−14 LUFS), light glue, nothing else: the safe one.
    Streaming,
    /// Club and EDM loud (−9), harder glue, bass firmly in the middle.
    Loud,
    /// Barely there: quieter (−16), the lightest glue.
    Gentle,
    /// Low end up, top end softened.
    Warm,
    /// Presence and air up, mud down.
    Bright,
    /// Faster, firmer glue for drums and hip-hop.
    Punchy,
    /// Loud, wide on top, mono below 150 Hz, the bass heard on phones.
    Club,
    /// Speech: the rumble cut higher, mono, presence up, −16 LUFS.
    Voice,
    /// Dark and narrow, the top rolled off.
    LoFi,
    /// Acoustic and orchestral: little glue, room to breathe (−18).
    Natural,
}

impl MasterPreset {
    pub const ALL: [Self; 10] = [
        Self::Streaming,
        Self::Loud,
        Self::Gentle,
        Self::Warm,
        Self::Bright,
        Self::Punchy,
        Self::Club,
        Self::Voice,
        Self::LoFi,
        Self::Natural,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Loud => "loud",
            Self::Gentle => "gentle polish",
            Self::Warm => "warm",
            Self::Bright => "bright and airy",
            Self::Punchy => "punchy",
            Self::Club => "club",
            Self::Voice => "voice and podcast",
            Self::LoFi => "lo-fi",
            Self::Natural => "natural",
        }
    }
}

impl MasterConfig {
    pub fn from_preset(preset: MasterPreset) -> Self {
        let base = Self::new();
        match preset {
            MasterPreset::Streaming => base,
            MasterPreset::Loud => Self {
                glue: 45.0,
                glue_attack_ms: 20.0,
                mono_below_hz: 130.0,
                target_lufs: -9.0,
                ceiling_db: -0.8,
                release_ms: 50.0,
                ..base
            },
            MasterPreset::Gentle => Self {
                glue: 10.0,
                target_lufs: -16.0,
                ..base
            },
            MasterPreset::Warm => Self {
                low_db: 2.0,
                mud_db: -1.0,
                air_db: -2.0,
                high_cut_hz: 17_000.0,
                glue: 25.0,
                ..base
            },
            MasterPreset::Bright => Self {
                mud_db: -2.0,
                presence_db: 1.5,
                air_db: 3.0,
                glue: 20.0,
                ..base
            },
            MasterPreset::Punchy => Self {
                low_db: 1.0,
                glue: 40.0,
                glue_attack_ms: 12.0,
                glue_release_ms: 120.0,
                target_lufs: -11.0,
                ..base
            },
            MasterPreset::Club => Self {
                low_db: 1.5,
                glue: 40.0,
                width: 1.15,
                mono_below_hz: 150.0,
                small_speakers: 35.0,
                target_lufs: -8.0,
                ceiling_db: -0.8,
                release_ms: 40.0,
                ..base
            },
            MasterPreset::Voice => Self {
                low_cut_hz: 80.0,
                mud_db: -2.0,
                presence_db: 2.0,
                glue: 35.0,
                width: 0.0,
                mono_below_hz: MASTER_MONO_OFF_HZ,
                target_lufs: -16.0,
                ..base
            },
            MasterPreset::LoFi => Self {
                low_cut_hz: 45.0,
                high_cut_hz: 11_000.0,
                low_db: 1.0,
                air_db: -5.0,
                glue: 50.0,
                width: 0.7,
                target_lufs: -13.0,
                ..base
            },
            MasterPreset::Natural => Self {
                glue: 5.0,
                mono_below_hz: 80.0,
                target_lufs: -18.0,
                release_ms: 150.0,
                ..base
            },
        }
    }
}
