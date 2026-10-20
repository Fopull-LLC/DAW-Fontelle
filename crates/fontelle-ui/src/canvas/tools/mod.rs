//! The piano roll's tools: a menu of them, and a dialog for each.
//!
//! `docs/roll-tools-plan.md` is the design; Ty, 2026-10-20: *"spend some time
//! putting together a plan to make the tools section start feeling feature
//! complete"*, then *"great plan now follow through"*.
//!
//! # The shape
//!
//! - The chip opens a **menu** ([`TOOL_MENU`]) in groups under headings —
//!   timing, pitch, levels, length, shape, generate, select — and the file
//!   actions under their own heading at the end.
//! - A tool with settings is a **dialog**: its rows ([`Tools::rows`], which
//!   follow the mode it is in), then one line of Reset, *Hear it* and Apply.
//! - A tool's answer is a [`ToolResult`] from `fontelle_model`, and it is the
//!   **same** answer the roll ghosts, *Hear it* plays and Apply writes
//!   ([`Tools::result`]) — a preview computed a second way is a preview that
//!   is right until somebody edits one of the two.
//! - With nothing selected a tool acts on the whole clip, as Ctrl+Q and *Fit
//!   notes to key* always have (plan §3.2); the dialog's title says which.
//!
//! It began as one bench holding every tool's settings at once, and before
//! this round each dialog stepped its values on a click with nothing on screen
//! saying so. Every value row now has arrows either side of it, a drag across
//! it scrubs it, and the wheel still never changes anything (Ty's rule: the
//! wheel only scrolls).
//!
//! Everything here is pure: the dialog is geometry ([`layout`]), the settings
//! are numbers, and reading a file is not something this crate may do
//! (INVARIANT 2), so the file rows name an action the window carries out.

mod layout;

pub use layout::{
    ToolPress, ToolsDialog, pattern_cells, step_arrows, tools_dialog_hit, tools_dialog_layout,
    tools_dialog_place, tools_dialog_press, tools_dialog_size,
};

use fontelle_model::{
    ArpDirection, ArpSpec, ChopSpec, ChopVelocity, FlamPitch, FlamSpec, HumanizeSpec, LengthsMode,
    LengthsSpec, LevelsMode, LevelsSpec, MAX_ARP_OCTAVES, MAX_ARP_REPEATS, MAX_CHOP_PATTERN,
    MAX_FLAM_COUNT, Note, NoteProperty, QuantizeSpec, RandomMode, SelectHow, ShuffleHow,
    ShuffleSpec, ShuffleWhat, StrumDirection, StrumSpec, ThinSpec, ToolResult,
};
use fontelle_types::{NoteId, PPQN, Tick};

/// One tool with settings — one dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Quantize,
    Strum,
    Humanize,
    Speed,
    Transpose,
    Flip,
    Levels,
    Lengths,
    Chop,
    Flam,
    ThinOut,
    Shuffle,
    Arpeggiate,
}

impl ToolKind {
    /// Every one, in the menu's order.
    pub const ALL: [Self; 13] = [
        Self::Quantize,
        Self::Strum,
        Self::Humanize,
        Self::Speed,
        Self::Transpose,
        Self::Flip,
        Self::Levels,
        Self::Lengths,
        Self::Chop,
        Self::Flam,
        Self::ThinOut,
        Self::Shuffle,
        Self::Arpeggiate,
    ];

    /// The dialog's title, and the menu row's name.
    pub fn title(self) -> &'static str {
        match self {
            Self::Quantize => "Quantize",
            Self::Strum => "Strum",
            Self::Humanize => "Humanize",
            Self::Speed => "Speed",
            Self::Transpose => "Transpose",
            Self::Flip => "Flip",
            Self::Levels => "Levels",
            Self::Lengths => "Lengths",
            Self::Chop => "Chop",
            Self::Flam => "Flam",
            Self::ThinOut => "Thin out",
            Self::Shuffle => "Shuffle",
            Self::Arpeggiate => "Arpeggiate",
        }
    }

    /// What the status line says it did, before "N note(s)".
    pub fn done(self) -> &'static str {
        match self {
            Self::Quantize => "Quantized",
            Self::Strum => "Strummed",
            Self::Humanize => "Humanized",
            Self::Speed => "Changed the speed of",
            Self::Transpose => "Transposed",
            Self::Flip => "Flipped",
            Self::Levels => "Levelled",
            Self::Lengths => "Set the lengths of",
            Self::Chop => "Chopped",
            Self::Flam => "Flammed",
            Self::ThinOut => "Thinned out",
            Self::Shuffle => "Shuffled",
            Self::Arpeggiate => "Arpeggiated",
        }
    }

    /// The key that opens it, if one does.
    pub fn action(self) -> Option<super::keymap::Action> {
        use super::keymap::Action;
        Some(match self {
            Self::Quantize => Action::Quantize,
            Self::Arpeggiate => Action::OpenArpeggiate,
            Self::Chop => Action::OpenChop,
            Self::Lengths => Action::OpenLengths,
            Self::Flip => Action::OpenFlip,
            Self::Flam => Action::OpenFlam,
            Self::ThinOut => Action::OpenThinOut,
            Self::Levels => Action::OpenLevels,
            _ => return None,
        })
    }
}

/// One row of a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolRow {
    // Transpose
    Semitones,
    InKey,
    KeepRange,
    RangeLow,
    RangeHigh,
    // Levels
    Property,
    LevelsMode,
    LevelsAmount,
    LevelsPercent,
    LevelsLow,
    LevelsHigh,
    LevelsFrom,
    LevelsTo,
    // Humanize
    HumanProperty,
    RandomAmount,
    RandomMode,
    HumanTiming,
    HumanLength,
    // Arpeggiate
    ArpRate,
    ArpDirection,
    ArpOctaves,
    ArpGate,
    ArpRepeats,
    ArpSwing,
    ArpRamp,
    // Quantize
    QuantizeGrid,
    QuantizeStrength,
    QuantizeEnds,
    QuantizeSwing,
    QuantizeRange,
    // Strum
    StrumTime,
    StrumDirection,
    StrumTension,
    StrumVelocity,
    StrumKeepEnds,
    StrumLand,
    // Chop
    ChopStep,
    ChopPatternLength,
    ChopPattern,
    ChopGate,
    ChopVelocity,
    ChopPitch,
    ChopOnGrid,
    // Flip
    FlipWhich,
    FlipKeepGrid,
    FlipInKey,
    // Lengths
    LengthsMode,
    LengthsPercent,
    LengthsValue,
    LengthsGap,
    // Flam
    FlamTime,
    FlamWhere,
    FlamVelocity,
    FlamPitch,
    FlamCount,
    // Thin out
    ThinPeriod,
    ThinEvery,
    ThinStretch,
    // Shuffle
    ShuffleWhat,
    ShuffleHow,
    // Speed
    SpeedFactor,
    /// The random tools' seed: Humanize, Shuffle, and Strum when it is
    /// random. One number for all three, so "seed 4" is one answer to
    /// remember.
    Seed,
    // The bottom line of every dialog.
    Reset,
    Hear,
    Apply,
}

/// How a row is pressed, and so how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolControl {
    /// `‹ value ›`: the arrows step it, a click on the value steps it on (a
    /// Ctrl+click back), a drag across it scrubs.
    Steps,
    /// A row of cells, each on or off — Chop's pattern.
    Pattern,
    /// Pressed, it does something.
    Button,
}

/// What a menu row or a key does without a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAction {
    /// The open dialog's tool, at its settings.
    Apply(ToolKind),
    Hear,
    Reset,
    /// Quick legato, Ctrl+L.
    Legato,
    /// Touching same-key notes joined.
    Glue,
    /// Chopped by the roll's snap.
    QuickChop,
    /// Starts onto the snap at full strength, ends alone.
    QuantizeStarts,
    RemoveDoubles,
    TrimOverlaps,
    MakeMono,
    /// Muted if any of them plays, unmuted if all are muted.
    ToggleMute,
    Select(SelectHow),
    ImportMidi,
    ImportScore,
    ExportMidi,
}

impl ToolAction {
    /// What the status line says it did, before "N note(s)".
    pub fn done(self) -> &'static str {
        match self {
            Self::Apply(kind) => kind.done(),
            Self::Legato => "Joined up",
            Self::Glue => "Glued",
            Self::QuickChop => "Chopped",
            Self::QuantizeStarts => "Quantized the starts of",
            Self::RemoveDoubles => "Cleaned the doubles out of",
            Self::TrimOverlaps => "Trimmed the overlaps of",
            Self::MakeMono => "Made mono",
            Self::ToggleMute => "Muted or unmuted",
            _ => "",
        }
    }
}

/// One row of the menu the Tools chip opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMenuItem {
    /// A group's name: drawn greyed, pressing it does nothing.
    Heading(&'static str),
    /// Opens a dialog. Its name ends in an ellipsis, which is what every menu
    /// uses to say "this asks you something first".
    Open(ToolKind),
    /// Does it now.
    Run(ToolAction),
}

impl ToolMenuItem {
    /// The key that does the same, if one does — shown on the row, read off
    /// the keymap since the shortcuts page can change it.
    pub fn action(self) -> Option<super::keymap::Action> {
        use super::keymap::Action;
        match self {
            Self::Open(kind) => kind.action(),
            Self::Run(ToolAction::Legato) => Some(Action::LegatoOrPlayMode),
            Self::Run(ToolAction::ExportMidi) => Some(Action::ExportMidi),
            Self::Run(ToolAction::Glue) => Some(Action::Glue),
            Self::Run(ToolAction::QuickChop) => Some(Action::QuickChop),
            Self::Run(ToolAction::QuantizeStarts) => Some(Action::QuantizeStarts),
            // The arrangement's mute key, meaning the notes while the roll
            // has the keyboard.
            Self::Run(ToolAction::ToggleMute) => Some(Action::MuteClips),
            _ => None,
        }
    }

    pub fn label_in(self, keymap: &super::keymap::Keymap) -> String {
        match self.action() {
            Some(action) if !keymap.chords(action).is_empty() => {
                format!("{} \u{2014} {}", self.label(), keymap.label(action))
            }
            _ => self.label(),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Heading(name) => name.to_string(),
            Self::Open(kind) => format!("{}\u{2026}", kind.title()),
            Self::Run(action) => match action {
                ToolAction::Legato => "Legato",
                ToolAction::Glue => "Glue",
                ToolAction::QuickChop => "Quick chop",
                ToolAction::QuantizeStarts => "Quantize starts",
                ToolAction::RemoveDoubles => "Remove doubles",
                ToolAction::TrimOverlaps => "Trim overlaps",
                ToolAction::MakeMono => "Make mono",
                ToolAction::ToggleMute => "Mute or unmute",
                ToolAction::Select(how) => select_label(how),
                ToolAction::ImportMidi => "Import MIDI file\u{2026}",
                ToolAction::ImportScore => "Import FL score\u{2026}",
                ToolAction::ExportMidi => "Export MIDI file\u{2026}",
                ToolAction::Apply(_) | ToolAction::Hear | ToolAction::Reset => "",
            }
            .to_string(),
        }
    }
}

fn select_label(how: SelectHow) -> &'static str {
    match how {
        SelectHow::SameKey => "Same key",
        SelectHow::Overlapping => "Overlapping",
        SelectHow::OffBeat => "Off the beat",
        SelectHow::Random => "Some at random",
        SelectHow::Invert => "Invert selection",
        SelectHow::Short => "Shorter than the snap",
        SelectHow::Long => "Longer than the snap",
    }
}

/// The menu the Tools chip opens (plan §3.7), in groups. Bringing a file in
/// is not something done to the notes, so the files are their own group,
/// last — they stay here because this is where people look for them.
pub static TOOL_MENU: &[ToolMenuItem] = &[
    ToolMenuItem::Heading("Timing"),
    ToolMenuItem::Open(ToolKind::Quantize),
    ToolMenuItem::Run(ToolAction::QuantizeStarts),
    ToolMenuItem::Open(ToolKind::Strum),
    ToolMenuItem::Open(ToolKind::Humanize),
    ToolMenuItem::Open(ToolKind::Speed),
    ToolMenuItem::Heading("Pitch"),
    ToolMenuItem::Open(ToolKind::Transpose),
    ToolMenuItem::Open(ToolKind::Flip),
    ToolMenuItem::Heading("Levels"),
    ToolMenuItem::Open(ToolKind::Levels),
    ToolMenuItem::Heading("Length"),
    ToolMenuItem::Run(ToolAction::Legato),
    ToolMenuItem::Open(ToolKind::Lengths),
    ToolMenuItem::Run(ToolAction::RemoveDoubles),
    ToolMenuItem::Run(ToolAction::TrimOverlaps),
    ToolMenuItem::Run(ToolAction::MakeMono),
    ToolMenuItem::Heading("Shape"),
    ToolMenuItem::Open(ToolKind::Chop),
    ToolMenuItem::Run(ToolAction::QuickChop),
    ToolMenuItem::Run(ToolAction::Glue),
    ToolMenuItem::Run(ToolAction::ToggleMute),
    ToolMenuItem::Open(ToolKind::Flam),
    ToolMenuItem::Open(ToolKind::ThinOut),
    ToolMenuItem::Open(ToolKind::Shuffle),
    ToolMenuItem::Heading("Generate"),
    ToolMenuItem::Open(ToolKind::Arpeggiate),
    ToolMenuItem::Heading("Select"),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::SameKey)),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::Overlapping)),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::OffBeat)),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::Random)),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::Short)),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::Long)),
    ToolMenuItem::Run(ToolAction::Select(SelectHow::Invert)),
    ToolMenuItem::Heading("Files"),
    ToolMenuItem::Run(ToolAction::ImportMidi),
    ToolMenuItem::Run(ToolAction::ImportScore),
    ToolMenuItem::Run(ToolAction::ExportMidi),
];

// ---------------------------------------------------------------- ladders ---

/// The divisions a step, a grid or a slice walks through, in ticks — the
/// ones a part is actually written on, straight and triplet.
const DIVISIONS: [Tick; 9] = [
    PPQN,
    PPQN * 2 / 3,
    PPQN / 2,
    PPQN / 3,
    PPQN / 4,
    PPQN / 6,
    PPQN / 8,
    PPQN / 12,
    PPQN / 16,
];

/// The arpeggiator's steps (it never went to 1/64).
const ARP_STEPS: [i32; 8] = [
    PPQN as i32,
    (PPQN * 2 / 3) as i32,
    (PPQN / 2) as i32,
    (PPQN / 3) as i32,
    (PPQN / 4) as i32,
    (PPQN / 6) as i32,
    (PPQN / 8) as i32,
    (PPQN / 12) as i32,
];

/// Quantize's grids after "the snap".
const QUANTIZE_GRIDS: [Tick; 7] = [
    PPQN,
    PPQN / 2,
    PPQN / 3,
    PPQN / 4,
    PPQN / 6,
    PPQN / 8,
    PPQN / 12,
];

/// Short times — a strum's gap, a flam's grace — in ticks. Shown in
/// milliseconds at the song's tempo, which is how anybody hears them.
const SHORT_TIMES: [i32; 9] = [10, 20, 30, 45, 60, 90, 120, 180, 240];

/// How far Humanize and Quantize's "leave near misses" reach, in ticks.
const WOBBLES: [i32; 8] = [0, 10, 20, 30, 45, 60, 90, 120];

const PERCENTS: [i32; 10] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
const LEVEL_PERCENTS: [i32; 10] = [0, 25, 50, 75, 80, 90, 110, 125, 150, 200];
const LENGTH_PERCENTS: [i32; 8] = [10, 25, 33, 50, 66, 75, 90, 100];
const HUMAN_LENGTHS: [i32; 7] = [0, 5, 10, 15, 20, 30, 50];
const RANDOM_LADDER: [i32; 11] = [0, 5, 10, 15, 20, 25, 30, 40, 50, 75, 100];
const GAPS: [i32; 7] = [0, 10, 20, 30, 60, 120, 240];
const SPEEDS: [f32; 6] = [0.25, 0.5, 0.75, 1.5, 2.0, 4.0];
const ADD_LADDER: [i32; 24] = [
    -64, -48, -32, -25, -20, -15, -10, -8, -5, -3, -2, -1, 1, 2, 3, 5, 8, 10, 15, 20, 25, 32, 48,
    64,
];

/// How far a transpose goes either way.
const MAX_TRANSPOSE: i32 = 24;

/// The six properties, in the property lane's order.
const PROPERTIES: [NoteProperty; 6] = [
    NoteProperty::Velocity,
    NoteProperty::Pan,
    NoteProperty::FinePitch,
    NoteProperty::Release,
    NoteProperty::ModX,
    NoteProperty::ModY,
];

fn property_name(p: NoteProperty) -> &'static str {
    match p {
        NoteProperty::Velocity => "Velocity",
        NoteProperty::Pan => "Pan",
        NoteProperty::FinePitch => "Fine pitch",
        NoteProperty::Release => "Release",
        NoteProperty::ModX => "Mod X",
        NoteProperty::ModY => "Mod Y",
    }
}

/// A division named the way the snap chip names one.
fn step_label(step: Tick) -> String {
    let ratio = PPQN as f32 / (step.max(1) as f32);
    for (divisor, name) in [
        (0.25, "1/1"),
        (0.5, "1/2"),
        (1.0, "1/4"),
        (1.5, "1/4T"),
        (2.0, "1/8"),
        (3.0, "1/8T"),
        (4.0, "1/16"),
        (6.0, "1/16T"),
        (8.0, "1/32"),
        (12.0, "1/32T"),
        (16.0, "1/64"),
    ] {
        if (ratio - divisor).abs() < 0.05 {
            return name.to_string();
        }
    }
    format!("{step} ticks")
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

/// `value` moved one rung along `ladder`, stopping at both ends. A value off
/// the ladder (a settings file somebody edited) lands on the nearest rung the
/// way it was going.
fn ladder_step(ladder: &[i32], value: i32, step: i32) -> i32 {
    if step > 0 {
        ladder
            .iter()
            .copied()
            .find(|rung| *rung > value)
            .unwrap_or_else(|| ladder.last().copied().unwrap_or(value))
    } else {
        ladder
            .iter()
            .copied()
            .rev()
            .find(|rung| *rung < value)
            .unwrap_or_else(|| ladder.first().copied().unwrap_or(value))
    }
}

fn tick_ladder_step(ladder: &[Tick], value: Tick, step: i32) -> Tick {
    // Divisions run longest first; "on" is shorter, which reads as finer.
    let wide: Vec<i32> = ladder.iter().rev().map(|t| *t as i32).collect();
    ladder_step(&wide, value as i32, -step) as Tick
}

/// A choice wraps: `values` is a set with no ends.
fn cycle<T: Copy + PartialEq>(values: &[T], now: T, step: i32) -> T {
    let at = values.iter().position(|v| *v == now).unwrap_or(0) as i32;
    values[(at + step).rem_euclid(values.len() as i32) as usize]
}

// --------------------------------------------------------------- settings ---

/// What the tools are set to.
///
/// Window state, not document state, and kept in the user's settings
/// (plan §3.6) — a tool set up the way you like it should still be that way
/// tomorrow. The fields the window fills in before each use are not kept.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Tools {
    pub semitones: i32,
    /// Transpose counts scale steps (only while the song has a key).
    pub in_key: bool,
    pub keep_range: bool,
    pub range_low: u8,
    pub range_high: u8,
    pub levels: LevelsSpec,
    pub humanize: HumanizeSpec,
    pub arp: ArpSpec,
    pub quantize: QuantizeSpec,
    /// Quantize's grid is the roll's snap.
    pub quantize_snap: bool,
    pub strum: StrumSpec,
    pub chop: ChopSpec,
    /// Flip turns it upside down rather than back to front.
    pub flip_pitch: bool,
    pub flip_keep_grid: bool,
    pub flip_in_key: bool,
    pub lengths: LengthsSpec,
    pub flam: FlamSpec,
    pub thin: ThinSpec,
    pub shuffle: ShuffleSpec,
    /// Lengths multiplied by this: 0.5 is twice as fast.
    pub speed: f32,

    /// The roll's snap, in ticks. Set by the window before every use.
    #[serde(skip)]
    pub roll_grid: Tick,
    /// Where the open clip ends, which legato carries the last notes to.
    #[serde(skip)]
    pub clip_end: Option<Tick>,
    /// The song's key, as a mask of pitch classes (bit 0 = C), if it has one.
    #[serde(skip)]
    pub song_key: Option<u16>,
    /// The song's tempo, for saying short times in milliseconds.
    #[serde(skip)]
    pub tempo: f64,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            // An octave: the transpose everybody reaches for first.
            semitones: 12,
            in_key: false,
            keep_range: false,
            range_low: 36,
            range_high: 84,
            levels: LevelsSpec::default(),
            humanize: HumanizeSpec::default(),
            arp: ArpSpec::default(),
            quantize: QuantizeSpec::default(),
            quantize_snap: true,
            strum: StrumSpec::default(),
            chop: ChopSpec::default(),
            flip_pitch: false,
            flip_keep_grid: false,
            flip_in_key: true,
            lengths: LengthsSpec::default(),
            flam: FlamSpec::default(),
            thin: ThinSpec::default(),
            shuffle: ShuffleSpec::default(),
            speed: 0.5,
            roll_grid: PPQN / 4,
            clip_end: None,
            song_key: None,
            tempo: 120.0,
        }
    }
}

impl Tools {
    /// For the settings file.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Back from the settings file; anything missing is its default, and a
    /// file that is not this at all is `None`.
    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    fn has_key(&self) -> bool {
        self.song_key.is_some_and(|m| m & 0xFFF != 0)
    }

    fn key(&self, wanted: bool) -> Option<u16> {
        if wanted && self.has_key() {
            self.song_key
        } else {
            None
        }
    }

    /// `ticks` said in milliseconds at the song's tempo.
    fn ms(&self, ticks: Tick) -> String {
        let tempo = if self.tempo > 0.0 { self.tempo } else { 120.0 };
        let ms = ticks as f64 * 60_000.0 / (tempo * PPQN as f64);
        format!("{} ms", ms.round() as i64)
    }

    /// `kind`'s setting rows, as its mode has them.
    pub fn rows(&self, kind: ToolKind) -> Vec<ToolRow> {
        use ToolRow as R;
        let mut rows = Vec::new();
        match kind {
            ToolKind::Transpose => {
                rows.push(R::Semitones);
                if self.has_key() {
                    rows.push(R::InKey);
                }
                rows.push(R::KeepRange);
                if self.keep_range {
                    rows.extend([R::RangeLow, R::RangeHigh]);
                }
            }
            ToolKind::Levels => {
                rows.extend([R::Property, R::LevelsMode]);
                rows.extend(match self.levels.mode {
                    LevelsMode::Add | LevelsMode::Set => vec![R::LevelsAmount],
                    LevelsMode::Scale | LevelsMode::Compress => vec![R::LevelsPercent],
                    LevelsMode::Limit => vec![R::LevelsLow, R::LevelsHigh],
                    LevelsMode::Ramp => vec![R::LevelsFrom, R::LevelsTo],
                });
            }
            ToolKind::Humanize => rows.extend([
                R::HumanProperty,
                R::RandomAmount,
                R::RandomMode,
                R::HumanTiming,
                R::HumanLength,
                R::Seed,
            ]),
            // The five FL has, then the two it does not.
            ToolKind::Arpeggiate => rows.extend([
                R::ArpRate,
                R::ArpDirection,
                R::ArpOctaves,
                R::ArpGate,
                R::ArpRepeats,
                R::ArpSwing,
                R::ArpRamp,
            ]),
            ToolKind::Quantize => rows.extend([
                R::QuantizeGrid,
                R::QuantizeStrength,
                R::QuantizeEnds,
                R::QuantizeSwing,
                R::QuantizeRange,
            ]),
            ToolKind::Strum => {
                rows.extend([
                    R::StrumTime,
                    R::StrumDirection,
                    R::StrumTension,
                    R::StrumVelocity,
                    R::StrumKeepEnds,
                    R::StrumLand,
                ]);
                if self.strum.direction == StrumDirection::Random {
                    rows.push(R::Seed);
                }
            }
            ToolKind::Chop => rows.extend([
                R::ChopStep,
                R::ChopPatternLength,
                R::ChopPattern,
                R::ChopGate,
                R::ChopVelocity,
                R::ChopPitch,
                R::ChopOnGrid,
            ]),
            ToolKind::Flip => {
                rows.push(R::FlipWhich);
                if !self.flip_pitch {
                    rows.push(R::FlipKeepGrid);
                } else if self.has_key() {
                    rows.push(R::FlipInKey);
                }
            }
            ToolKind::Lengths => {
                rows.push(R::LengthsMode);
                rows.push(match self.lengths.mode {
                    LengthsMode::Legato | LengthsMode::Gap => R::LengthsGap,
                    LengthsMode::Staccato => R::LengthsPercent,
                    LengthsMode::Fixed | LengthsMode::AtLeast | LengthsMode::AtMost => {
                        R::LengthsValue
                    }
                });
            }
            ToolKind::Flam => rows.extend([
                R::FlamTime,
                R::FlamWhere,
                R::FlamVelocity,
                R::FlamPitch,
                R::FlamCount,
            ]),
            ToolKind::ThinOut => rows.extend([R::ThinPeriod, R::ThinEvery, R::ThinStretch]),
            ToolKind::Shuffle => rows.extend([R::ShuffleWhat, R::ShuffleHow, R::Seed]),
            ToolKind::Speed => rows.push(R::SpeedFactor),
        }
        rows
    }

    /// How `row` is pressed.
    pub fn control(&self, row: ToolRow) -> ToolControl {
        match row {
            ToolRow::Reset | ToolRow::Hear | ToolRow::Apply => ToolControl::Button,
            ToolRow::ChopPattern => ToolControl::Pattern,
            _ => ToolControl::Steps,
        }
    }

    /// The name in the row's left-hand column.
    pub fn label(&self, row: ToolRow) -> String {
        use ToolRow as R;
        match row {
            R::Semitones if self.in_key && self.has_key() => "Scale steps",
            R::Semitones => "Semitones",
            R::InKey => "Move by",
            R::KeepRange => "Keep in range",
            R::RangeLow | R::LevelsLow => "Lowest",
            R::RangeHigh | R::LevelsHigh => "Highest",
            R::Property | R::HumanProperty => "Property",
            R::LevelsMode => "Do",
            R::LevelsAmount if self.levels.mode == LevelsMode::Set => "Value",
            R::LevelsAmount => "Amount",
            R::LevelsPercent if self.levels.mode == LevelsMode::Compress => "Spread",
            R::LevelsPercent => "By",
            R::LevelsFrom => "From",
            R::LevelsTo => "To",
            R::RandomAmount => "Strength",
            R::RandomMode => "Kind",
            R::HumanTiming => "Timing",
            R::HumanLength => "Length",
            R::ArpRate => "Step",
            R::ArpDirection | R::StrumDirection => "Direction",
            R::ArpOctaves => "Range",
            R::ArpGate | R::ChopGate => "Gate",
            R::ArpRepeats => "Repeat",
            R::ArpSwing | R::QuantizeSwing => "Swing",
            R::ArpRamp => "Ramp",
            R::QuantizeGrid => "Grid",
            R::QuantizeStrength => "Strength",
            R::QuantizeEnds => "Ends",
            R::QuantizeRange => "Leave near misses",
            R::StrumTime => "Gap",
            R::StrumTension => "Curve",
            R::StrumVelocity | R::ChopVelocity | R::FlamVelocity => "Velocity",
            R::StrumKeepEnds => "Keep the ends",
            R::StrumLand => "Lead into the beat",
            R::ChopStep => "Piece",
            R::ChopPatternLength => "Pattern steps",
            R::ChopPattern => "Pattern",
            R::ChopPitch | R::FlamPitch => "Pitch",
            R::ChopOnGrid => "Cut on the grid",
            R::FlipWhich => "Flip",
            R::FlipKeepGrid => "Keep the rhythm",
            R::FlipInKey => "In key",
            R::LengthsMode => "Make",
            R::LengthsPercent => "Keep",
            R::LengthsValue | R::FlamTime => "Length",
            R::LengthsGap => "Gap",
            R::FlamWhere => "Where",
            R::FlamCount => "Notes",
            R::ThinPeriod => "Slice",
            R::ThinEvery => "Remove",
            R::ThinStretch => "Close the gaps",
            R::ShuffleWhat => "Rearrange",
            R::ShuffleHow => "How",
            R::SpeedFactor => "Speed",
            R::Seed => "Seed",
            R::Reset => "Reset",
            R::Hear => "Hear it",
            R::Apply => "Apply",
        }
        .to_string()
    }

    /// What the row is set to, in words. Empty for a button and a pattern.
    pub fn value(&self, row: ToolRow) -> String {
        use ToolRow as R;
        let pct = |f: f32| format!("{}%", (f * 100.0).round() as i32);
        match row {
            R::Semitones => format!("{:+}", self.semitones),
            R::InKey if self.in_key => "scale steps".to_string(),
            R::InKey => "semitones".to_string(),
            R::KeepRange => on_off(self.keep_range),
            R::RangeLow => super::key_name(self.range_low),
            R::RangeHigh => super::key_name(self.range_high),
            R::Property => property_name(self.levels.property).to_string(),
            R::HumanProperty => property_name(self.humanize.property).to_string(),
            R::LevelsMode => match self.levels.mode {
                LevelsMode::Add => "add",
                LevelsMode::Set => "set",
                LevelsMode::Scale => "scale",
                LevelsMode::Compress => "compress",
                LevelsMode::Limit => "limit",
                LevelsMode::Ramp => "ramp",
            }
            .to_string(),
            R::LevelsAmount if self.levels.mode == LevelsMode::Add => {
                format!("{:+}", self.levels.amount)
            }
            R::LevelsAmount => self.levels.amount.to_string(),
            R::LevelsPercent => format!("{}%", self.levels.percent),
            R::LevelsLow => self.levels.low.to_string(),
            R::LevelsHigh => self.levels.high.to_string(),
            R::LevelsFrom => self.levels.from.to_string(),
            R::LevelsTo => self.levels.to.to_string(),
            R::RandomAmount => format!("{}%", self.humanize.amount),
            R::RandomMode => match self.humanize.mode {
                RandomMode::Around => "Wobble",
                RandomMode::Anywhere => "Replace",
            }
            .to_string(),
            R::HumanTiming if self.humanize.timing == 0 => "off".to_string(),
            R::HumanTiming => format!("\u{b1}{}", self.ms(self.humanize.timing)),
            R::HumanLength if self.humanize.length == 0 => "off".to_string(),
            R::HumanLength => format!("\u{b1}{}%", self.humanize.length),
            R::ArpRate => step_label(self.arp.step),
            R::ArpDirection => self.arp.direction.label().to_string(),
            R::ArpOctaves => format!("{} oct", self.arp.octaves),
            R::ArpGate => pct(self.arp.gate),
            R::ArpRepeats => format!("\u{d7}{}", self.arp.repeats),
            R::ArpSwing => pct(self.arp.swing),
            R::ArpRamp => format!("{:+}%", (self.arp.velocity_ramp * 100.0).round() as i32),
            R::QuantizeGrid if self.quantize_snap => {
                format!("snap ({})", step_label(self.roll_grid))
            }
            R::QuantizeGrid => step_label(self.quantize.grid),
            R::QuantizeStrength => pct(self.quantize.strength),
            R::QuantizeEnds if self.quantize.ends => "starts and ends".to_string(),
            R::QuantizeEnds => "starts only".to_string(),
            R::QuantizeSwing => pct(self.quantize.swing),
            R::QuantizeRange if self.quantize.range == 0 => "off".to_string(),
            R::QuantizeRange => format!("within {}", self.ms(self.quantize.range)),
            R::StrumTime => self.ms(self.strum.time),
            R::StrumDirection => match self.strum.direction {
                StrumDirection::Up => "up",
                StrumDirection::Down => "down",
                StrumDirection::Alternate => "alternate",
                StrumDirection::Random => "random",
            }
            .to_string(),
            R::StrumTension => {
                let t = (self.strum.tension * 100.0).round() as i32;
                match t {
                    0 => "even".to_string(),
                    t if t > 0 => format!("speeds up {t}%"),
                    t => format!("slows down {}%", -t),
                }
            }
            R::StrumVelocity if self.strum.velocity == 0 => "flat".to_string(),
            R::StrumVelocity => format!("{:+} each", self.strum.velocity),
            R::StrumKeepEnds => on_off(self.strum.keep_ends),
            R::StrumLand => on_off(self.strum.land),
            R::ChopStep => step_label(self.chop.step),
            R::ChopPatternLength => self.chop.pattern_len.to_string(),
            R::ChopGate => pct(self.chop.gate),
            R::ChopVelocity => match self.chop.velocity {
                ChopVelocity::Flat => "flat",
                ChopVelocity::Fall => "falling",
                ChopVelocity::Rise => "rising",
                ChopVelocity::Accent => "accent",
            }
            .to_string(),
            R::ChopPitch if self.chop.pitch_step == 0 => "same".to_string(),
            R::ChopPitch => format!("{:+} each", self.chop.pitch_step),
            R::ChopOnGrid => on_off(self.chop.on_grid),
            R::FlipWhich if self.flip_pitch => "upside down".to_string(),
            R::FlipWhich => "back to front".to_string(),
            R::FlipKeepGrid => on_off(self.flip_keep_grid),
            R::FlipInKey => on_off(self.flip_in_key),
            R::LengthsMode => match self.lengths.mode {
                LengthsMode::Legato => "legato",
                LengthsMode::Staccato => "staccato",
                LengthsMode::Fixed => "fixed",
                LengthsMode::AtLeast => "at least",
                LengthsMode::AtMost => "at most",
                LengthsMode::Gap => "trim to the next",
            }
            .to_string(),
            R::LengthsPercent => format!("{}%", self.lengths.percent),
            R::LengthsValue => step_label(self.lengths.length),
            R::LengthsGap if self.lengths.gap == 0 => "none".to_string(),
            R::LengthsGap => self.ms(self.lengths.gap),
            R::FlamTime => self.ms(self.flam.time),
            R::FlamWhere if self.flam.before => "before".to_string(),
            R::FlamWhere => "on the beat".to_string(),
            R::FlamVelocity => format!("{}%", self.flam.velocity),
            R::FlamPitch => match self.flam.pitch {
                FlamPitch::Same => "same key",
                FlamPitch::Above => "a step above",
                FlamPitch::Below => "a step below",
            }
            .to_string(),
            R::FlamCount => self.flam.count.to_string(),
            R::ThinPeriod => step_label(self.thin.period),
            R::ThinEvery => match self.thin.every {
                2 => "every 2nd".to_string(),
                3 => "every 3rd".to_string(),
                n => format!("every {n}th"),
            },
            R::ThinStretch => on_off(self.thin.stretch),
            R::ShuffleWhat => match self.shuffle.what {
                ShuffleWhat::Pitch => "pitches",
                ShuffleWhat::Velocity => "velocities",
                ShuffleWhat::Length => "lengths",
            }
            .to_string(),
            R::ShuffleHow => match self.shuffle.how {
                ShuffleHow::Shuffle => "shuffle",
                ShuffleHow::Rotate => "rotate",
                ShuffleHow::Mirror => "mirror",
            }
            .to_string(),
            R::SpeedFactor if self.speed < 1.0 => {
                format!("{}\u{d7} faster", trim(1.0 / self.speed))
            }
            R::SpeedFactor => format!("{}\u{d7} slower", trim(self.speed)),
            R::Seed => format!("#{}", self.humanize.seed),
            R::ChopPattern | R::Reset | R::Hear | R::Apply => String::new(),
        }
    }

    /// What a hover tip says about the row.
    pub fn tip(&self, row: ToolRow) -> &'static str {
        use ToolRow as R;
        match row {
            R::Semitones => "How far to move the notes",
            R::InKey => "Count in semitones, or in steps of the song's scale so it stays in key",
            R::KeepRange => "Fold any note that leaves the range back in by octaves",
            R::RangeLow => "The lowest key a note may land on",
            R::RangeHigh => "The highest key a note may land on",
            R::Property => "Which of the notes' values to shape",
            R::HumanProperty => "Which of the notes' values to wobble",
            R::LevelsMode => {
                "Add to them, set them, scale them, pull them together, keep them in a range, or ramp them"
            }
            R::LevelsAmount => "How much to add (or take off), or the value to set",
            R::LevelsPercent => {
                "Scale: a share of each value. Spread: under 100% evens them out, over spreads them"
            }
            R::LevelsLow => "Nothing goes below this",
            R::LevelsHigh => "Nothing goes above this",
            R::LevelsFrom => "The value at the first note",
            R::LevelsTo => "The value at the last note",
            R::RandomAmount => "How far each value may stray. 0% leaves it alone",
            R::RandomMode => "Wobble: around what is there. Replace: a new value from anywhere",
            R::HumanTiming => {
                "How far each start may move, early or late. Never past its neighbours"
            }
            R::HumanLength => "How much each length may change",
            R::ArpRate => "How long one step of the run is",
            R::ArpDirection => "Which way the run walks the chord",
            R::ArpOctaves => "How many octaves it climbs before starting again",
            R::ArpGate => "How much of each step the note sounds for",
            R::ArpRepeats => "How many steps each note holds before the next",
            R::ArpSwing => "How late the off-beats land. 0% is a straight grid",
            R::ArpRamp => "How the weight moves across the run \u{2014} falling, flat, climbing",
            R::QuantizeGrid => "The lines notes go to: the roll's snap, or one chosen here",
            R::QuantizeStrength => "How much of the way to the line each note goes",
            R::QuantizeEnds => "Move the starts only, or put the ends on the grid too",
            R::QuantizeSwing => "Every second line later, for a shuffled feel",
            R::QuantizeRange => "Notes already this close to their line are left as played",
            R::StrumTime => "The gap from one string to the next",
            R::StrumDirection => {
                "Up starts on the lowest note; alternate goes up then down chord by chord"
            }
            R::StrumTension => "Even gaps, or a strum that speeds up or slows down as it goes",
            R::StrumVelocity => "Each string a little harder, or softer, than the one before",
            R::StrumKeepEnds => "The chord still lets go together",
            R::StrumLand => "The last string lands on the beat instead of the first",
            R::ChopStep => "How long each piece is",
            R::ChopPatternLength => "How many steps the pattern has before it repeats",
            R::ChopPattern => "Click a step to leave it silent \u{2014} or bring it back",
            R::ChopGate => "How much of its step each piece sounds for",
            R::ChopVelocity => "Flat, falling, rising, or the first of each pattern accented",
            R::ChopPitch => "Semitones added per piece, for a stair instead of a stutter",
            R::ChopOnGrid => "Cut on the song's grid, or every piece from each note's own start",
            R::FlipWhich => "Back to front in time, or upside down in pitch",
            R::FlipKeepGrid => "Only reverse the order, keeping every start on a start it had",
            R::FlipInKey => "Mirror in steps of the song's scale, so it stays in key",
            R::LengthsMode => {
                "Legato to the next note, staccato, one length, a least or most, or just no overlaps"
            }
            R::LengthsPercent => "How much of each note to keep",
            R::LengthsValue => "The length to use",
            R::LengthsGap => "Room to leave before the next note",
            R::FlamTime => "How long each grace note is",
            R::FlamWhere => "Before the note, or on its start with the note moved over",
            R::FlamVelocity => "The grace notes' weight, against their note's",
            R::FlamPitch => "The grace note's key: the same, or a step of the scale above or below",
            R::FlamCount => "How many grace notes lead in",
            R::ThinPeriod => "How long a slice of time is",
            R::ThinEvery => "Which slices to empty",
            R::ThinStretch => "Spread what is left to fill the time it had",
            R::ShuffleWhat => "What to rearrange; everything else stays where it is",
            R::ShuffleHow => "A random order, each moved along one, or reversed",
            R::SpeedFactor => "Faster or slower, from the first note",
            R::Seed => "Which roll of the dice \u{2014} the same number gives the same answer",
            R::Reset => "Put this tool's settings back as they came",
            R::Hear => {
                "Play what it would make, once \u{2014} it is ghosted in the roll as you change it"
            }
            R::Apply => "Write it (Enter). One undo takes it back",
        }
    }

    /// Steps `row` one place: a number stops at its ends, a choice goes
    /// round.
    pub fn nudge(&mut self, row: ToolRow, delta: i32) {
        use ToolRow as R;
        if delta == 0 {
            return;
        }
        let step = delta.signum();
        let property_step = |p: NoteProperty| {
            let (min, max) = p.range();
            ((max - min) / 32).max(1) * step
        };
        let clamp_to = |p: NoteProperty, v: i32| {
            let (min, max) = p.range();
            v.clamp(min, max)
        };
        match row {
            R::Semitones => {
                let reach = if self.in_key && self.has_key() {
                    14
                } else {
                    MAX_TRANSPOSE
                };
                self.semitones = (self.semitones + step).clamp(-reach, reach);
            }
            R::InKey => self.in_key = !self.in_key,
            R::KeepRange => self.keep_range = !self.keep_range,
            R::RangeLow => {
                self.range_low =
                    (i32::from(self.range_low) + step).clamp(0, i32::from(self.range_high)) as u8;
            }
            R::RangeHigh => {
                self.range_high =
                    (i32::from(self.range_high) + step).clamp(i32::from(self.range_low), 127) as u8;
            }
            R::Property => self.levels.property = cycle(&PROPERTIES, self.levels.property, step),
            R::HumanProperty => {
                self.humanize.property = cycle(&PROPERTIES, self.humanize.property, step);
            }
            R::LevelsMode => {
                self.levels.mode = cycle(
                    &[
                        LevelsMode::Add,
                        LevelsMode::Set,
                        LevelsMode::Scale,
                        LevelsMode::Compress,
                        LevelsMode::Limit,
                        LevelsMode::Ramp,
                    ],
                    self.levels.mode,
                    step,
                );
            }
            R::LevelsAmount if self.levels.mode == LevelsMode::Add => {
                self.levels.amount = ladder_step(&ADD_LADDER, self.levels.amount, step);
            }
            R::LevelsAmount => {
                let p = self.levels.property;
                self.levels.amount = clamp_to(p, self.levels.amount + property_step(p));
            }
            R::LevelsPercent => {
                self.levels.percent = ladder_step(&LEVEL_PERCENTS, self.levels.percent, step);
            }
            R::LevelsLow => {
                let p = self.levels.property;
                self.levels.low =
                    clamp_to(p, self.levels.low + property_step(p)).min(self.levels.high);
            }
            R::LevelsHigh => {
                let p = self.levels.property;
                self.levels.high =
                    clamp_to(p, self.levels.high + property_step(p)).max(self.levels.low);
            }
            R::LevelsFrom => {
                let p = self.levels.property;
                self.levels.from = clamp_to(p, self.levels.from + property_step(p));
            }
            R::LevelsTo => {
                let p = self.levels.property;
                self.levels.to = clamp_to(p, self.levels.to + property_step(p));
            }
            R::RandomAmount => {
                self.humanize.amount = ladder_step(&RANDOM_LADDER, self.humanize.amount, step);
            }
            R::RandomMode => self.humanize.mode = self.humanize.mode.next(),
            R::HumanTiming => {
                self.humanize.timing =
                    ladder_step(&WOBBLES, self.humanize.timing as i32, step) as Tick;
            }
            R::HumanLength => {
                self.humanize.length = ladder_step(&HUMAN_LENGTHS, self.humanize.length, step);
            }
            R::ArpRate => {
                self.arp.step = ladder_step(
                    &ARP_STEPS.iter().rev().copied().collect::<Vec<_>>(),
                    self.arp.step as i32,
                    -step,
                ) as Tick;
            }
            R::ArpDirection => {
                self.arp.direction = cycle(&ArpDirection::ALL, self.arp.direction, step)
            }
            R::ArpOctaves => {
                self.arp.octaves =
                    (self.arp.octaves as i32 + step).clamp(1, MAX_ARP_OCTAVES as i32) as u8;
            }
            R::ArpGate => self.arp.gate = (self.arp.gate + step as f32 * 0.05).clamp(0.05, 1.0),
            R::ArpRepeats => {
                self.arp.repeats =
                    (self.arp.repeats as i32 + step).clamp(1, MAX_ARP_REPEATS as i32) as u8;
            }
            R::ArpSwing => self.arp.swing = (self.arp.swing + step as f32 * 0.05).clamp(0.0, 1.0),
            R::ArpRamp => {
                self.arp.velocity_ramp =
                    (self.arp.velocity_ramp + step as f32 * 0.1).clamp(-1.0, 1.0);
            }
            // The snap, then each division, round and round.
            R::QuantizeGrid => {
                let at = if self.quantize_snap {
                    0
                } else {
                    QUANTIZE_GRIDS
                        .iter()
                        .position(|g| *g == self.quantize.grid)
                        .map_or(0, |i| i + 1)
                } as i32;
                let next = (at + step).rem_euclid(QUANTIZE_GRIDS.len() as i32 + 1) as usize;
                self.quantize_snap = next == 0;
                if next > 0 {
                    self.quantize.grid = QUANTIZE_GRIDS[next - 1];
                }
            }
            R::QuantizeStrength => {
                let now = (self.quantize.strength * 100.0).round() as i32;
                self.quantize.strength = ladder_step(&PERCENTS, now, step) as f32 / 100.0;
            }
            R::QuantizeEnds => self.quantize.ends = !self.quantize.ends,
            R::QuantizeSwing => {
                self.quantize.swing =
                    ((self.quantize.swing * 10.0).round() + step as f32).clamp(0.0, 10.0) / 10.0;
            }
            R::QuantizeRange => {
                self.quantize.range =
                    ladder_step(&WOBBLES, self.quantize.range as i32, step) as Tick;
            }
            R::StrumTime => {
                self.strum.time = ladder_step(&SHORT_TIMES, self.strum.time as i32, step) as Tick
            }
            R::StrumDirection => {
                self.strum.direction = cycle(
                    &[
                        StrumDirection::Up,
                        StrumDirection::Down,
                        StrumDirection::Alternate,
                        StrumDirection::Random,
                    ],
                    self.strum.direction,
                    step,
                );
            }
            R::StrumTension => {
                self.strum.tension =
                    ((self.strum.tension * 10.0).round() + step as f32).clamp(-10.0, 10.0) / 10.0;
            }
            R::StrumVelocity => self.strum.velocity = (self.strum.velocity + step).clamp(-20, 20),
            R::StrumKeepEnds => self.strum.keep_ends = !self.strum.keep_ends,
            R::StrumLand => self.strum.land = !self.strum.land,
            R::ChopStep => self.chop.step = tick_ladder_step(&DIVISIONS, self.chop.step, step),
            R::ChopPatternLength => {
                self.chop.pattern_len = (i32::from(self.chop.pattern_len) + step)
                    .clamp(1, i32::from(MAX_CHOP_PATTERN))
                    as u8;
            }
            R::ChopGate => {
                self.chop.gate =
                    ((self.chop.gate * 20.0).round() + step as f32).clamp(1.0, 20.0) / 20.0
            }
            R::ChopVelocity => {
                self.chop.velocity = cycle(
                    &[
                        ChopVelocity::Flat,
                        ChopVelocity::Fall,
                        ChopVelocity::Rise,
                        ChopVelocity::Accent,
                    ],
                    self.chop.velocity,
                    step,
                );
            }
            R::ChopPitch => {
                self.chop.pitch_step = (i32::from(self.chop.pitch_step) + step).clamp(-12, 12) as i8
            }
            R::ChopOnGrid => self.chop.on_grid = !self.chop.on_grid,
            R::FlipWhich => self.flip_pitch = !self.flip_pitch,
            R::FlipKeepGrid => self.flip_keep_grid = !self.flip_keep_grid,
            R::FlipInKey => self.flip_in_key = !self.flip_in_key,
            R::LengthsMode => {
                self.lengths.mode = cycle(
                    &[
                        LengthsMode::Legato,
                        LengthsMode::Staccato,
                        LengthsMode::Fixed,
                        LengthsMode::AtLeast,
                        LengthsMode::AtMost,
                        LengthsMode::Gap,
                    ],
                    self.lengths.mode,
                    step,
                );
            }
            R::LengthsPercent => {
                self.lengths.percent = ladder_step(&LENGTH_PERCENTS, self.lengths.percent, step);
            }
            R::LengthsValue => {
                self.lengths.length = tick_ladder_step(&DIVISIONS, self.lengths.length, step)
            }
            R::LengthsGap => {
                self.lengths.gap = ladder_step(&GAPS, self.lengths.gap as i32, step) as Tick
            }
            R::FlamTime => {
                self.flam.time = ladder_step(&SHORT_TIMES, self.flam.time as i32, step) as Tick
            }
            R::FlamWhere => self.flam.before = !self.flam.before,
            R::FlamVelocity => {
                self.flam.velocity = ladder_step(&PERCENTS, self.flam.velocity, step)
            }
            R::FlamPitch => {
                self.flam.pitch = cycle(
                    &[FlamPitch::Same, FlamPitch::Above, FlamPitch::Below],
                    self.flam.pitch,
                    step,
                );
            }
            R::FlamCount => {
                self.flam.count =
                    (i32::from(self.flam.count) + step).clamp(1, i32::from(MAX_FLAM_COUNT)) as u8;
            }
            R::ThinPeriod => {
                self.thin.period = tick_ladder_step(&DIVISIONS, self.thin.period, step)
            }
            R::ThinEvery => self.thin.every = (i32::from(self.thin.every) + step).clamp(2, 8) as u8,
            R::ThinStretch => self.thin.stretch = !self.thin.stretch,
            R::ShuffleWhat => {
                self.shuffle.what = cycle(
                    &[
                        ShuffleWhat::Pitch,
                        ShuffleWhat::Velocity,
                        ShuffleWhat::Length,
                    ],
                    self.shuffle.what,
                    step,
                );
            }
            R::ShuffleHow => {
                self.shuffle.how = cycle(
                    &[ShuffleHow::Shuffle, ShuffleHow::Rotate, ShuffleHow::Mirror],
                    self.shuffle.how,
                    step,
                );
            }
            R::SpeedFactor => {
                let at = SPEEDS
                    .iter()
                    .position(|s| (*s - self.speed).abs() < 1e-3)
                    .unwrap_or(1) as i32;
                self.speed = SPEEDS[(at + step).clamp(0, SPEEDS.len() as i32 - 1) as usize];
            }
            R::Seed => self.set_seed((self.humanize.seed as i64 + i64::from(step)).max(1) as u64),
            // A button holds nothing to step, and the pattern is cells.
            R::ChopPattern | R::Reset | R::Hear | R::Apply => {}
        }
    }

    /// One seed for every random tool.
    fn set_seed(&mut self, seed: u64) {
        self.humanize.seed = seed;
        self.shuffle.seed = seed;
        self.strum.seed = seed;
    }

    /// Cell `index` of a pattern row, on or off.
    pub fn toggle_cell(&mut self, row: ToolRow, index: usize) {
        if row == ToolRow::ChopPattern && index < usize::from(MAX_CHOP_PATTERN) {
            self.chop.pattern ^= 1 << index;
        }
    }

    /// `kind`'s settings as they came; every other tool's as they are.
    pub fn reset(&mut self, kind: ToolKind) {
        let fresh = Tools::default();
        match kind {
            ToolKind::Transpose => {
                self.semitones = fresh.semitones;
                self.in_key = fresh.in_key;
                self.keep_range = fresh.keep_range;
                self.range_low = fresh.range_low;
                self.range_high = fresh.range_high;
            }
            ToolKind::Levels => self.levels = fresh.levels,
            ToolKind::Humanize => self.humanize = fresh.humanize,
            ToolKind::Arpeggiate => self.arp = fresh.arp,
            ToolKind::Quantize => {
                self.quantize = fresh.quantize;
                self.quantize_snap = fresh.quantize_snap;
            }
            ToolKind::Strum => self.strum = fresh.strum,
            ToolKind::Chop => self.chop = fresh.chop,
            ToolKind::Flip => {
                self.flip_pitch = fresh.flip_pitch;
                self.flip_keep_grid = fresh.flip_keep_grid;
                self.flip_in_key = fresh.flip_in_key;
            }
            ToolKind::Lengths => self.lengths = fresh.lengths,
            ToolKind::Flam => self.flam = fresh.flam,
            ToolKind::ThinOut => self.thin = fresh.thin,
            ToolKind::Shuffle => self.shuffle = fresh.shuffle,
            ToolKind::Speed => self.speed = fresh.speed,
        }
    }

    /// What Quantize uses: its own settings, on the roll's snap while it
    /// follows it.
    pub fn quantize_spec(&self) -> QuantizeSpec {
        QuantizeSpec {
            grid: if self.quantize_snap {
                self.roll_grid
            } else {
                self.quantize.grid
            },
            ..self.quantize
        }
    }

    /// `kind`'s answer for `scope` — what the roll ghosts, *Hear it* plays
    /// and Apply writes.
    pub fn result(&self, kind: ToolKind, scope: &[(NoteId, Note)]) -> ToolResult {
        use fontelle_model as m;
        if scope.is_empty() {
            return ToolResult::default();
        }
        match kind {
            ToolKind::Transpose => m::transposed(
                scope,
                self.semitones,
                self.key(self.in_key),
                self.keep_range.then_some((self.range_low, self.range_high)),
            ),
            ToolKind::Levels => m::levelled(scope, self.levels),
            ToolKind::Humanize => m::humanized(scope, self.humanize),
            ToolKind::Arpeggiate => {
                let notes: Vec<Note> = scope.iter().map(|(_, n)| n.clone()).collect();
                let made = m::arpeggiated(&notes, self.arp);
                if made.is_empty() {
                    ToolResult::default()
                } else {
                    ToolResult::replacing(scope, made)
                }
            }
            ToolKind::Quantize => {
                let spans: Vec<(Tick, Tick)> =
                    scope.iter().map(|(_, n)| (n.start, n.length)).collect();
                let after = m::quantized(&spans, self.quantize_spec())
                    .into_iter()
                    .zip(scope)
                    .map(|((start, length), (_, n))| Note {
                        start,
                        length,
                        ..n.clone()
                    })
                    .collect();
                ToolResult::rewriting(scope, after)
            }
            ToolKind::Strum => m::strummed(scope, self.strum),
            ToolKind::Chop => m::chopped(scope, self.chop),
            ToolKind::Flip if self.flip_pitch => {
                m::flipped_in_pitch(scope, self.key(self.flip_in_key))
            }
            ToolKind::Flip => m::flipped_in_time(scope, self.flip_keep_grid),
            ToolKind::Lengths => m::lengthened(scope, self.lengths, self.clip_end),
            ToolKind::Flam => m::flammed(scope, self.flam, self.song_key),
            ToolKind::ThinOut => m::thinned(scope, self.thin),
            ToolKind::Shuffle => m::shuffled(scope, self.shuffle),
            ToolKind::Speed => m::stretched(scope, self.speed),
        }
    }

    /// The answer of a tool that has no dialog, or `None` for an action that
    /// is not one.
    pub fn instant(&self, action: ToolAction, scope: &[(NoteId, Note)]) -> Option<ToolResult> {
        use fontelle_model as m;
        Some(match action {
            ToolAction::Glue => m::glued(scope),
            ToolAction::QuickChop => m::chopped(
                scope,
                ChopSpec {
                    step: self.roll_grid.max(1),
                    ..ChopSpec::default()
                },
            ),
            ToolAction::QuantizeStarts => {
                let spans: Vec<(Tick, Tick)> =
                    scope.iter().map(|(_, n)| (n.start, n.length)).collect();
                let spec = QuantizeSpec {
                    grid: self.roll_grid.max(1),
                    ..QuantizeSpec::default()
                };
                let after = m::quantized(&spans, spec)
                    .into_iter()
                    .zip(scope)
                    .map(|((start, length), (_, n))| Note {
                        start,
                        length,
                        ..n.clone()
                    })
                    .collect();
                ToolResult::rewriting(scope, after)
            }
            ToolAction::RemoveDoubles => m::without_doubles(scope),
            ToolAction::TrimOverlaps => m::trimmed_overlaps(scope),
            ToolAction::MakeMono => m::made_mono(scope),
            ToolAction::ToggleMute => {
                let mute = scope.iter().any(|(_, n)| !n.muted);
                let after = scope
                    .iter()
                    .map(|(_, n)| Note {
                        muted: mute,
                        ..n.clone()
                    })
                    .collect();
                ToolResult::rewriting(scope, after)
            }
            _ => return None,
        })
    }

    /// After `kind` was applied: a random tool rolls its next answer, so
    /// pressing Apply again gives another one.
    pub fn applied(&mut self, kind: ToolKind) {
        let random = match kind {
            ToolKind::Humanize | ToolKind::Shuffle => true,
            ToolKind::Strum => self.strum.direction == StrumDirection::Random,
            _ => false,
        };
        if random {
            self.set_seed(self.humanize.seed.wrapping_add(1).max(1));
        }
    }

    /// The notes of `scope` that `how` picks.
    pub fn select(
        &self,
        how: SelectHow,
        scope: &[(NoteId, Note)],
        chosen: &[NoteId],
    ) -> Vec<NoteId> {
        fontelle_model::selected_where(
            scope,
            chosen,
            how,
            self.roll_grid.max(1),
            self.humanize.seed,
        )
    }
}

/// A factor without a trailing ".0".
fn trim(f: f32) -> String {
    let s = format!("{f:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
