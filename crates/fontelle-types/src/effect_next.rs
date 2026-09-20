//! The seven effect kinds Flopsynth II added (`docs/flopsynth-next.md`
//! §4.5): phaser, flanger, wavefolder, frequency shifter, hyper, multiband
//! distortion and width. Their configs, in the shape `effect.rs` gives every
//! other kind — a struct, `new()` as the effect at rest, `get`/`set` by id,
//! a spec table through `with_mix`, sections — kept in their own file so
//! that `effect.rs` does not grow by another thousand lines. The variants,
//! the menu row and the dispatch arms are in `effect.rs`; the DSP is in
//! `fontelle-fx`; the banks are in `effect_presets.rs`.
//!
//! All seven are zero-latency (§4.5's rule stands: look-ahead kinds stay
//! out), so none needs a word in the delay compensation.

use crate::effect::{
    DIVISIONS, MAX_LFO_RATE_HZ, MIN_LFO_RATE_HZ, MIX, NoteDivision, percent_param, switch_param,
    with_mix,
};

/// Where the three that put copies under the track open their mix: the
/// chorus's half, for the chorus's reason (`EffectKind::is_time_based`).
const HALF: f32 = 50.0;
const ALL_WET: f32 = 100.0;

fn half() -> f32 {
    0.5
}

fn all_wet() -> f32 {
    1.0
}

/// The rate an LFO is asking for at `bpm`: one cycle per `division` when
/// synced, the knob otherwise — `ChorusConfig::effective_rate_hz`'s seam,
/// shared by the two sweeps here.
fn effective_rate_hz(rate_hz: f32, sync: bool, division: NoteDivision, bpm: f32) -> f32 {
    let asked = if sync {
        let bpm = if bpm.is_finite() { bpm } else { 0.0 };
        let seconds = division.beats() * 60.0 / bpm.clamp(1.0, 1_000.0);
        1.0 / seconds.max(1e-4)
    } else {
        rate_hz
    };
    asked.clamp(MIN_LFO_RATE_HZ, MAX_LFO_RATE_HZ)
}

const fn rate_param(default: f32) -> crate::ParamSpec {
    crate::ParamSpec {
        id: "rate",
        name: "Rate",
        min: MIN_LFO_RATE_HZ,
        max: MAX_LFO_RATE_HZ,
        default,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    }
}

const fn division_param() -> crate::ParamSpec {
    crate::ParamSpec {
        id: "division",
        name: "Division",
        min: 0.0,
        max: (DIVISIONS.len() - 1) as f32,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(DIVISIONS.len() as u32),
        positions: &DIVISIONS,
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

/// A signed percentage, for the feedbacks that have a hollow half.
const fn signed_percent_param(
    id: &'static str,
    name: &'static str,
    reach: f32,
    default: f32,
) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min: -reach,
        max: reach,
        default,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    }
}

fn division_index(division: NoteDivision) -> Option<f32> {
    NoteDivision::ALL
        .iter()
        .position(|d| *d == division)
        .map(|i| i as f32)
}

fn division_at(value: f32) -> Option<NoteDivision> {
    NoteDivision::ALL
        .get(value.round().max(0.0) as usize)
        .copied()
}

// ------------------------------------------------------------------ phaser

/// The phaser's stage count, at both ends.
pub const MIN_PHASER_STAGES: u32 = 2;
pub const MAX_PHASER_STAGES: u32 = 12;

/// How far the sweep moves the centre at full depth, in octaves each way.
pub const PHASER_SWEEP_OCTAVES: f32 = 2.0;

/// A cascade of first-order all-passes swept by an LFO
/// (`docs/effects-catalogue.md` §2.4's row). What it writes is the
/// all-passed signal; the notches are the sum with the dry that
/// `EffectNode` makes, which is why it opens half wet.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhaserConfig {
    /// How many all-passes, [`MIN_PHASER_STAGES`]..=[`MAX_PHASER_STAGES`]:
    /// a notch for every two.
    pub stages: u32,
    /// Where the chain sits when the LFO is at its middle, in hertz.
    pub centre_hz: f32,
    /// How far the sweep goes, 0..=1 of [`PHASER_SWEEP_OCTAVES`] each way.
    pub depth: f32,
    pub rate_hz: f32,
    pub sync: bool,
    pub division: NoteDivision,
    /// Round the chain, −0.9..=0.9: **signed**, like the chorus's, and for
    /// its reason — negative feedback is the other comb.
    pub feedback: f32,
    /// How far apart the two sides are in the sweep, 0..=1 (half a cycle).
    pub spread: f32,
    #[serde(default = "half")]
    pub mix: f32,
}

impl PhaserConfig {
    /// Six stages at 800 Hz, a slow half-depth sweep, a touch of feedback:
    /// a phaser somebody would recognise as one, half wet. Slow — a fifth
    /// of a hertz — because a phaser's notches pass over whatever is
    /// playing, and a fresh one that happened to be sitting on the note
    /// when it was added would read as an effect that silences the track.
    pub fn new() -> Self {
        Self {
            stages: 6,
            centre_hz: 800.0,
            depth: 0.5,
            rate_hz: 0.2,
            sync: false,
            division: NoteDivision::Whole,
            feedback: 0.3,
            spread: 0.5,
            mix: HALF / 100.0,
        }
    }

    pub fn effective_rate_hz(&self, bpm: f32) -> f32 {
        effective_rate_hz(self.rate_hz, self.sync, self.division, bpm)
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "stages" => self.stages as f32,
            "centre" => self.centre_hz,
            "depth" => self.depth * 100.0,
            "rate" => self.rate_hz,
            "sync" => f32::from(u8::from(self.sync)),
            "division" => division_index(self.division)?,
            "feedback" => self.feedback * 100.0,
            "spread" => self.spread * 100.0,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "stages" => {
                self.stages =
                    (value.round().max(0.0) as u32).clamp(MIN_PHASER_STAGES, MAX_PHASER_STAGES);
            }
            "centre" => self.centre_hz = value,
            "depth" => self.depth = value / 100.0,
            "rate" => self.rate_hz = value,
            "sync" => self.sync = value >= 0.5,
            "division" => {
                if let Some(division) = division_at(value) {
                    self.division = division;
                }
            }
            "feedback" => self.feedback = value / 100.0,
            "spread" => self.spread = value / 100.0,
            _ => {}
        }
    }
}

impl Default for PhaserConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static PHASER_PARAMS: [crate::ParamSpec; 9] = with_mix(&PHASER_OWN_PARAMS, HALF);

pub(crate) static PHASER_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Sweep",
        count: 3,
    },
    crate::ParamSection {
        name: "Rate",
        count: 3,
    },
    crate::ParamSection {
        name: "Output",
        count: 3,
    },
];

static PHASER_OWN_PARAMS: [crate::ParamSpec; 8] = [
    crate::ParamSpec {
        id: "stages",
        name: "Stages",
        min: MIN_PHASER_STAGES as f32,
        max: MAX_PHASER_STAGES as f32,
        default: 6.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(MAX_PHASER_STAGES - MIN_PHASER_STAGES + 1),
        positions: &[],
    },
    hz_param("centre", "Centre", 100.0, 8_000.0, 800.0),
    percent_param("depth", "Depth", 50.0),
    rate_param(0.2),
    switch_param("sync", "Sync"),
    division_param(),
    signed_percent_param("feedback", "Feedback", 90.0, 30.0),
    percent_param("spread", "Spread", 50.0),
];

// ----------------------------------------------------------------- flanger

/// The ends of the flanger's delay, in milliseconds: a tenth is the
/// through-zero region, ten is where the chorus's knob begins.
pub const MIN_FLANGER_DELAY_MS: f32 = 0.1;
pub const MAX_FLANGER_DELAY_MS: f32 = 10.0;

/// One short modulated delay with signed feedback
/// (`docs/effects-catalogue.md` §2.4's row). Shares the chorus's line and
/// the chorus's rule: what it writes is the copy, the comb is the sum the
/// node makes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlangerConfig {
    /// The centre of the sweep, in milliseconds.
    pub delay_ms: f32,
    /// How far the sweep goes, 0..=1 of the room the centre has above
    /// nothing — at full depth the copy passes from almost no delay to
    /// twice the centre.
    pub depth: f32,
    /// A second, **fixed** copy at the centre beside the swept one, so the
    /// sweep passes through no difference at all — the tape flange. Set the
    /// mix to full for the pure thing; the node's dry is a third copy.
    pub through_zero: bool,
    pub rate_hz: f32,
    pub sync: bool,
    pub division: NoteDivision,
    /// Signed, −0.95..=0.95: negative is the hollow comb.
    pub feedback: f32,
    /// How far apart the two sides are in the sweep, 0..=1.
    pub spread: f32,
    #[serde(default = "half")]
    pub mix: f32,
}

impl FlangerConfig {
    /// A millisecond swept most of the way at a third of a hertz, with
    /// half of it fed back: the pedal.
    pub fn new() -> Self {
        Self {
            delay_ms: 1.0,
            depth: 0.7,
            through_zero: false,
            rate_hz: 0.3,
            sync: false,
            division: NoteDivision::Whole,
            feedback: 0.5,
            spread: 0.5,
            mix: HALF / 100.0,
        }
    }

    pub fn effective_rate_hz(&self, bpm: f32) -> f32 {
        effective_rate_hz(self.rate_hz, self.sync, self.division, bpm)
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "delay" => self.delay_ms,
            "depth" => self.depth * 100.0,
            "through_zero" => f32::from(u8::from(self.through_zero)),
            "rate" => self.rate_hz,
            "sync" => f32::from(u8::from(self.sync)),
            "division" => division_index(self.division)?,
            "feedback" => self.feedback * 100.0,
            "spread" => self.spread * 100.0,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "delay" => self.delay_ms = value,
            "depth" => self.depth = value / 100.0,
            "through_zero" => self.through_zero = value >= 0.5,
            "rate" => self.rate_hz = value,
            "sync" => self.sync = value >= 0.5,
            "division" => {
                if let Some(division) = division_at(value) {
                    self.division = division;
                }
            }
            "feedback" => self.feedback = value / 100.0,
            "spread" => self.spread = value / 100.0,
            _ => {}
        }
    }
}

impl Default for FlangerConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static FLANGER_PARAMS: [crate::ParamSpec; 9] = with_mix(&FLANGER_OWN_PARAMS, HALF);

pub(crate) static FLANGER_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Sweep",
        count: 3,
    },
    crate::ParamSection {
        name: "Rate",
        count: 3,
    },
    crate::ParamSection {
        name: "Output",
        count: 3,
    },
];

static FLANGER_OWN_PARAMS: [crate::ParamSpec; 8] = [
    crate::ParamSpec {
        id: "delay",
        name: "Delay",
        min: MIN_FLANGER_DELAY_MS,
        max: MAX_FLANGER_DELAY_MS,
        default: 1.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    percent_param("depth", "Depth", 70.0),
    switch_param("through_zero", "Through zero"),
    rate_param(0.3),
    switch_param("sync", "Sync"),
    division_param(),
    signed_percent_param("feedback", "Feedback", 95.0, 50.0),
    percent_param("spread", "Spread", 50.0),
];

// -------------------------------------------------------------------- fold

/// The wavefolder: a signal driven past full scale is folded back rather
/// than clipped, so the harmonics keep coming as the drive goes up instead
/// of flattening into a square.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FoldConfig {
    /// Into the fold, 0..=40 dB. Nought is a wire.
    pub drive_db: f32,
    /// A bias before the fold, −1..=1: the even harmonics.
    pub symmetry: f32,
    /// The corner of the fold, 0..=1: a crease (the triangle) at nought, a
    /// sine's turn at one.
    pub smooth: f32,
    /// After the fold, in decibels.
    pub output_db: f32,
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl FoldConfig {
    /// A wire: no drive, no bias, a soft corner ready for when there is.
    pub fn new() -> Self {
        Self {
            drive_db: 0.0,
            symmetry: 0.0,
            smooth: 0.5,
            output_db: 0.0,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "drive" => self.drive_db,
            "symmetry" => self.symmetry * 100.0,
            "smooth" => self.smooth * 100.0,
            "output" => self.output_db,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "drive" => self.drive_db = value,
            "symmetry" => self.symmetry = value / 100.0,
            "smooth" => self.smooth = value / 100.0,
            "output" => self.output_db = value,
            _ => {}
        }
    }
}

impl Default for FoldConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static FOLD_PARAMS: [crate::ParamSpec; 5] = with_mix(&FOLD_OWN_PARAMS, ALL_WET);

pub(crate) static FOLD_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Fold",
    count: FOLD_PARAMS.len(),
}];

static FOLD_OWN_PARAMS: [crate::ParamSpec; 4] = [
    db_param("drive", "Drive", 0.0, 40.0, 0.0),
    signed_percent_param("symmetry", "Symmetry", 100.0, 0.0),
    percent_param("smooth", "Smooth", 50.0),
    db_param("output", "Output", -24.0, 24.0, 0.0),
];

// ----------------------------------------------------------------- shifter

/// Which way the frequency shifter moves things.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ShiftDirection {
    #[default]
    Up,
    Down,
    /// Both sidebands, which is a ring modulator with the carrier taken out.
    Both,
}

impl ShiftDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Both => "both",
        }
    }

    pub const ALL: [Self; 3] = [Self::Up, Self::Down, Self::Both];
}

static SHIFT_DIRECTIONS: [&str; 3] = ["up", "down", "both"];

/// How far the shift knob reaches, in hertz each way.
pub const MAX_SHIFT_HZ: f32 = 5_000.0;

/// The Bode frequency shifter (`docs/effects-catalogue.md` §2.3's row):
/// every partial moved by the same number of hertz, so the harmonics stop
/// being harmonics. A Hilbert pair and a quadrature oscillator; feedback
/// is the barber-pole.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShifterConfig {
    /// The shift, −[`MAX_SHIFT_HZ`]..=[`MAX_SHIFT_HZ`]. The sign is the
    /// direction too: a negative shift *up* is a shift down.
    pub shift_hz: f32,
    /// Added to the shift, ±20 Hz, for the slow barber-poles a coarse knob
    /// cannot land.
    pub fine_hz: f32,
    pub direction: ShiftDirection,
    /// The shifted output fed back into the input, 0..=0.9: each pass
    /// shifts again, and the lines climb.
    pub feedback: f32,
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl ShifterConfig {
    /// A hundred hertz up, no feedback: audibly a shifter, not yet a
    /// barber-pole.
    pub fn new() -> Self {
        Self {
            shift_hz: 100.0,
            fine_hz: 0.0,
            direction: ShiftDirection::Up,
            feedback: 0.0,
            mix: 1.0,
        }
    }

    /// The shift the two knobs add up to.
    pub fn total_hz(&self) -> f32 {
        self.shift_hz + self.fine_hz
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "shift" => self.shift_hz,
            "fine" => self.fine_hz,
            "direction" => ShiftDirection::ALL
                .iter()
                .position(|d| *d == self.direction)? as f32,
            "feedback" => self.feedback * 100.0,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "shift" => self.shift_hz = value,
            "fine" => self.fine_hz = value,
            "direction" => {
                if let Some(direction) = ShiftDirection::ALL.get(value.round().max(0.0) as usize) {
                    self.direction = *direction;
                }
            }
            "feedback" => self.feedback = value / 100.0,
            _ => {}
        }
    }
}

impl Default for ShifterConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static SHIFTER_PARAMS: [crate::ParamSpec; 5] = with_mix(&SHIFTER_OWN_PARAMS, ALL_WET);

pub(crate) static SHIFTER_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Shift",
    count: SHIFTER_PARAMS.len(),
}];

static SHIFTER_OWN_PARAMS: [crate::ParamSpec; 4] = [
    crate::ParamSpec {
        id: "shift",
        name: "Shift",
        min: -MAX_SHIFT_HZ,
        max: MAX_SHIFT_HZ,
        default: 100.0,
        unit: crate::Unit::Hertz,
        // Linear, because it crosses zero and a log taper cannot; the fine
        // knob is the small end.
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "fine",
        name: "Fine",
        min: -20.0,
        max: 20.0,
        default: 0.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "direction",
        name: "Direction",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(3),
        positions: &SHIFT_DIRECTIONS,
    },
    percent_param("feedback", "Feedback", 0.0),
];

// ------------------------------------------------------------------- hyper

/// The most copies hyper makes.
pub const MAX_HYPER_VOICES: u32 = 4;

/// The ends of the crossfade window, in milliseconds.
pub const MIN_HYPER_WINDOW_MS: f32 = 5.0;
pub const MAX_HYPER_WINDOW_MS: f32 = 50.0;

/// Unison as an effect: up to four copies each detuned by a fixed number
/// of cents and spread across the image. Wet only, like the chorus, and
/// half wet for its reason — the copies under the signal that made them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HyperConfig {
    /// How many copies, 1..=[`MAX_HYPER_VOICES`], spread evenly across
    /// ±[`detune_cents`](Self::detune_cents).
    pub voices: u32,
    /// The outermost copies' detune, in cents.
    pub detune_cents: f32,
    /// How far across the image the copies are panned, 0..=1.
    pub spread: f32,
    /// The crossfade window a copy is read through, in milliseconds:
    /// shorter keeps a transient tighter, longer is smoother on a held
    /// note.
    pub window_ms: f32,
    #[serde(default = "half")]
    pub mix: f32,
}

impl HyperConfig {
    /// Two copies fifteen cents apart, spread most of the way.
    pub fn new() -> Self {
        Self {
            voices: 2,
            detune_cents: 15.0,
            spread: 0.7,
            window_ms: 20.0,
            mix: HALF / 100.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "voices" => self.voices as f32,
            "detune" => self.detune_cents,
            "spread" => self.spread * 100.0,
            "window" => self.window_ms,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "voices" => self.voices = (value.round().max(1.0) as u32).min(MAX_HYPER_VOICES),
            "detune" => self.detune_cents = value,
            "spread" => self.spread = value / 100.0,
            "window" => self.window_ms = value,
            _ => {}
        }
    }
}

impl Default for HyperConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static HYPER_PARAMS: [crate::ParamSpec; 5] = with_mix(&HYPER_OWN_PARAMS, HALF);

pub(crate) static HYPER_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Hyper",
    count: HYPER_PARAMS.len(),
}];

static HYPER_OWN_PARAMS: [crate::ParamSpec; 4] = [
    crate::ParamSpec {
        id: "voices",
        name: "Voices",
        min: 1.0,
        max: MAX_HYPER_VOICES as f32,
        default: 2.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(MAX_HYPER_VOICES),
        positions: &[],
    },
    crate::ParamSpec {
        id: "detune",
        name: "Detune",
        min: 0.0,
        max: 100.0,
        default: 15.0,
        // Cents, and there is no unit for them — the same call the
        // corrector's vibrato depth makes.
        unit: crate::Unit::None,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    percent_param("spread", "Spread", 70.0),
    crate::ParamSpec {
        id: "window",
        name: "Window",
        min: MIN_HYPER_WINDOW_MS,
        max: MAX_HYPER_WINDOW_MS,
        default: 20.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
];

// --------------------------------------------------------------- multiband

/// Three bands on two crossovers, each driven on its own.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MultibandConfig {
    /// The low crossover, in hertz.
    pub low_hz: f32,
    /// The high one.
    pub high_hz: f32,
    /// Each band's drive, 0..=40 dB; nought is that band untouched.
    pub low_drive_db: f32,
    pub mid_drive_db: f32,
    pub high_drive_db: f32,
    pub output_db: f32,
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl MultibandConfig {
    /// A wire: the crossovers at 200 Hz and 3 kHz, nothing driven.
    pub fn new() -> Self {
        Self {
            low_hz: 200.0,
            high_hz: 3_000.0,
            low_drive_db: 0.0,
            mid_drive_db: 0.0,
            high_drive_db: 0.0,
            output_db: 0.0,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "low" => self.low_hz,
            "high" => self.high_hz,
            "low_drive" => self.low_drive_db,
            "mid_drive" => self.mid_drive_db,
            "high_drive" => self.high_drive_db,
            "output" => self.output_db,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "low" => self.low_hz = value,
            "high" => self.high_hz = value,
            "low_drive" => self.low_drive_db = value,
            "mid_drive" => self.mid_drive_db = value,
            "high_drive" => self.high_drive_db = value,
            "output" => self.output_db = value,
            _ => {}
        }
    }
}

impl Default for MultibandConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static MULTIBAND_PARAMS: [crate::ParamSpec; 7] =
    with_mix(&MULTIBAND_OWN_PARAMS, ALL_WET);

pub(crate) static MULTIBAND_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Bands",
        count: 2,
    },
    crate::ParamSection {
        name: "Drive",
        count: 3,
    },
    crate::ParamSection {
        name: "Output",
        count: 2,
    },
];

static MULTIBAND_OWN_PARAMS: [crate::ParamSpec; 6] = [
    hz_param("low", "Low", 40.0, 1_000.0, 200.0),
    hz_param("high", "High", 1_000.0, 12_000.0, 3_000.0),
    db_param("low_drive", "Low drive", 0.0, 40.0, 0.0),
    db_param("mid_drive", "Mid drive", 0.0, 40.0, 0.0),
    db_param("high_drive", "High drive", 0.0, 40.0, 0.0),
    db_param("output", "Output", -24.0, 24.0, 0.0),
];

// ------------------------------------------------------------------- width

/// Where the bass-mono corner is off: the bottom of its knob.
pub const WIDTH_MONO_OFF_HZ: f32 = 20.0;

/// Mid/side width with the bass kept in the middle, and a gain on each
/// half. The utility has a width and a mono-maker too; this is the one
/// reached for as an *effect*, with the two gains the utility does not
/// have, and it is what a patch's chain gets (§4.5).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidthConfig {
    /// 0 (mono) ..= 2 (the side doubled); 1 is a wire.
    pub width: f32,
    /// The side signal is high-passed here, so the bass stays in the
    /// middle; [`WIDTH_MONO_OFF_HZ`] is off.
    pub mono_below_hz: f32,
    pub mid_db: f32,
    pub side_db: f32,
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl WidthConfig {
    /// A wire.
    pub fn new() -> Self {
        Self {
            width: 1.0,
            mono_below_hz: WIDTH_MONO_OFF_HZ,
            mid_db: 0.0,
            side_db: 0.0,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "width" => self.width * 100.0,
            "mono_below" => self.mono_below_hz,
            "mid" => self.mid_db,
            "side" => self.side_db,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "width" => self.width = value / 100.0,
            "mono_below" => self.mono_below_hz = value,
            "mid" => self.mid_db = value,
            "side" => self.side_db = value,
            _ => {}
        }
    }
}

impl Default for WidthConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static WIDTH_PARAMS: [crate::ParamSpec; 5] = with_mix(&WIDTH_OWN_PARAMS, ALL_WET);

pub(crate) static WIDTH_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Width",
    count: WIDTH_PARAMS.len(),
}];

static WIDTH_OWN_PARAMS: [crate::ParamSpec; 4] = [
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
    hz_param(
        "mono_below",
        "Mono below",
        WIDTH_MONO_OFF_HZ,
        500.0,
        WIDTH_MONO_OFF_HZ,
    ),
    db_param("mid", "Mid", -12.0, 12.0, 0.0),
    db_param("side", "Side", -12.0, 12.0, 0.0),
];
