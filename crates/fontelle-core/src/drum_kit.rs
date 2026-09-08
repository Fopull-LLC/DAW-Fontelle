//! The built-in drum machine: kits, and the styles they come in.
//!
//! > *"i want you to create a new instrument, a built in general purpose drum
//! > machine that can just make a variety of drum styles and sounds and you can
//! > play them all in the piano roll all labeled and stuff should have lots of
//! > presets for different styles and genres of kits. should be encorperated
//! > like any other vst would be."*
//!
//! # It is not a special case
//!
//! *"Like any other vst"* is the whole design. A kit is an ordinary [`Patch`]
//! whose layers carry [`Source::Drum`], one layer per key, so **nothing else in
//! this program had to be told it exists**: it is saved by `to_data`, loaded by
//! `from_data`, labelled by `fontelle_app::key_map` because its layers are
//! one-key zones, filtered by the patch's two filters, routed by the channel's
//! mixer track, and automated through §8.2's addresses. The only new thing
//! below the panel is the `Source` variant and the twenty lines in `Voice` that
//! render it.
//!
//! # The key map is General MIDI's
//!
//! [`GM_DRUM_MAP`] is the standard layout — kick on 36, snare on 38, closed hat
//! on 42 — and choosing it rather than a run of keys from zero is what makes a
//! drum MIDI file dropped on the arrangement land on the right hits, and a part
//! written here play on anything else.
//!
//! # Why the styles are a table and not sixty hand-written kits
//!
//! A kit is thirty-six hits and each hit is ten numbers. Twenty kits written
//! out longhand is seven thousand numbers nobody can review, and the twentieth
//! would be a copy of the fourth with two edits.
//!
//! So there is **one** kit — the slots in [`GM_DRUM_MAP`], each with the
//! settings that make a plain studio version of that hit — and a
//! [`KitCharacter`] per style: eleven numbers saying how this genre's drums
//! differ from a plain one. An 808's kick rings and its hats are short; a rock
//! kit is loud and bright with long toms; a chiptune kit is square waves. Each
//! row reads as a recipe, and a kit built from one is a perfectly ordinary
//! concrete kit afterwards — every hit is still individually editable, because
//! what lands in the patch is thirty-six independent [`DrumVoice`]s.

use fontelle_dsp::{DrumBody, DrumModel, DrumVoice};

use crate::patch::{Layer, Patch, Source};

/// One hit of a kit: where it sits, what it is called, and how it sounds.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumSlot {
    /// Its MIDI key — General MIDI's, see [`GM_DRUM_MAP`].
    pub key: u8,
    /// What the piano roll writes on the row. *"you can play them all in the
    /// piano roll all labeled."*
    pub name: &'static str,
    pub voice: DrumVoice,
}

/// Which family a hit belongs to, so a style can say "shorter hats" without
/// naming all six of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Kick,
    Snare,
    Hat,
    Tom,
    Cymbal,
    Perc,
}

/// One row of the kit: everything about a hit that does **not** change between
/// styles.
///
/// Public because [`GM_DRUM_MAP`] is — the map is a fact about General MIDI
/// rather than about this build, and a caller that wants to know which key the
/// ride is on should be able to look rather than guess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GmSlot {
    pub key: u8,
    pub name: &'static str,
    pub model: DrumModel,
    family: Family,
    /// Where the pitched half sits, in hertz, in a plain studio kit.
    tune_hz: f32,
    /// How far the pitch falls, in semitones, and over how long.
    bend_semitones: f32,
    bend_s: f32,
    decay_s: f32,
    tone_hz: f32,
    noise: f32,
    snap: f32,
    gain_db: f32,
}

/// The General MIDI percussion map, as far as this kit fills it.
///
/// Keys 35 to 70, which is the block every drum part in every MIDI file uses.
/// The names are GM's own, shortened where GM's are unwieldy ("Acoustic Bass
/// Drum" is "Kick 2") because they are going on a 78-pixel keyboard strip.
///
/// **The order is the key order** and every key appears once: two hits on one
/// key is one hit that can never be played and a label on the wrong row.
pub const GM_DRUM_MAP: [GmSlot; 36] = [
    GmSlot {
        key: 35,
        name: "Kick 2",
        model: DrumModel::Kick,
        family: Family::Kick,
        tune_hz: 44.0,
        bend_semitones: 26.0,
        bend_s: 0.055,
        decay_s: 0.42,
        tone_hz: 320.0,
        noise: 0.05,
        snap: 0.30,
        gain_db: 0.0,
    },
    GmSlot {
        key: 36,
        name: "Kick",
        model: DrumModel::Kick,
        family: Family::Kick,
        tune_hz: 52.0,
        bend_semitones: 24.0,
        bend_s: 0.040,
        decay_s: 0.34,
        tone_hz: 420.0,
        noise: 0.06,
        snap: 0.40,
        gain_db: 0.0,
    },
    GmSlot {
        key: 37,
        name: "Rim",
        model: DrumModel::Rim,
        family: Family::Snare,
        tune_hz: 420.0,
        bend_semitones: 12.0,
        bend_s: 0.006,
        decay_s: 0.045,
        tone_hz: 1_900.0,
        noise: 0.55,
        snap: 0.55,
        gain_db: -3.0,
    },
    GmSlot {
        key: 38,
        name: "Snare",
        model: DrumModel::Snare,
        family: Family::Snare,
        tune_hz: 190.0,
        bend_semitones: 6.0,
        bend_s: 0.012,
        decay_s: 0.19,
        tone_hz: 1_500.0,
        noise: 0.62,
        snap: 0.35,
        gain_db: -1.0,
    },
    GmSlot {
        key: 39,
        name: "Clap",
        model: DrumModel::Clap,
        family: Family::Snare,
        tune_hz: 300.0,
        bend_semitones: 0.0,
        bend_s: 0.010,
        decay_s: 0.22,
        tone_hz: 1_300.0,
        noise: 1.0,
        snap: 0.20,
        gain_db: -2.0,
    },
    GmSlot {
        key: 40,
        name: "Snare 2",
        model: DrumModel::Snare,
        family: Family::Snare,
        tune_hz: 240.0,
        bend_semitones: 5.0,
        bend_s: 0.010,
        decay_s: 0.13,
        tone_hz: 2_100.0,
        noise: 0.72,
        snap: 0.45,
        gain_db: -2.0,
    },
    GmSlot {
        key: 41,
        name: "Floor Tom",
        model: DrumModel::Tom,
        family: Family::Tom,
        tune_hz: 92.0,
        bend_semitones: 8.0,
        bend_s: 0.070,
        decay_s: 0.50,
        tone_hz: 700.0,
        noise: 0.10,
        snap: 0.20,
        gain_db: -3.0,
    },
    GmSlot {
        key: 42,
        name: "Closed Hat",
        model: DrumModel::ClosedHat,
        family: Family::Hat,
        tune_hz: 320.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.055,
        tone_hz: 8_200.0,
        noise: 1.0,
        snap: 0.15,
        gain_db: -7.0,
    },
    GmSlot {
        key: 43,
        name: "Tom Low",
        model: DrumModel::Tom,
        family: Family::Tom,
        tune_hz: 110.0,
        bend_semitones: 8.0,
        bend_s: 0.065,
        decay_s: 0.45,
        tone_hz: 780.0,
        noise: 0.10,
        snap: 0.20,
        gain_db: -3.0,
    },
    GmSlot {
        key: 44,
        name: "Pedal Hat",
        model: DrumModel::ClosedHat,
        family: Family::Hat,
        tune_hz: 300.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.075,
        tone_hz: 6_800.0,
        noise: 1.0,
        snap: 0.10,
        gain_db: -8.0,
    },
    GmSlot {
        key: 45,
        name: "Tom Mid",
        model: DrumModel::Tom,
        family: Family::Tom,
        tune_hz: 132.0,
        bend_semitones: 8.0,
        bend_s: 0.060,
        decay_s: 0.40,
        tone_hz: 850.0,
        noise: 0.10,
        snap: 0.20,
        gain_db: -3.0,
    },
    GmSlot {
        key: 46,
        name: "Open Hat",
        model: DrumModel::OpenHat,
        family: Family::Hat,
        tune_hz: 320.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.42,
        tone_hz: 7_800.0,
        noise: 1.0,
        snap: 0.12,
        gain_db: -8.0,
    },
    GmSlot {
        key: 47,
        name: "Tom Mid 2",
        model: DrumModel::Tom,
        family: Family::Tom,
        tune_hz: 158.0,
        bend_semitones: 8.0,
        bend_s: 0.055,
        decay_s: 0.36,
        tone_hz: 900.0,
        noise: 0.10,
        snap: 0.20,
        gain_db: -3.0,
    },
    GmSlot {
        key: 48,
        name: "Tom High",
        model: DrumModel::Tom,
        family: Family::Tom,
        tune_hz: 186.0,
        bend_semitones: 8.0,
        bend_s: 0.050,
        decay_s: 0.32,
        tone_hz: 980.0,
        noise: 0.10,
        snap: 0.20,
        gain_db: -3.0,
    },
    GmSlot {
        key: 49,
        name: "Crash",
        model: DrumModel::Cymbal,
        family: Family::Cymbal,
        tune_hz: 520.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 1.60,
        tone_hz: 5_200.0,
        noise: 1.0,
        snap: 0.10,
        gain_db: -10.0,
    },
    GmSlot {
        key: 50,
        name: "Tom Top",
        model: DrumModel::Tom,
        family: Family::Tom,
        tune_hz: 220.0,
        bend_semitones: 8.0,
        bend_s: 0.045,
        decay_s: 0.28,
        tone_hz: 1_050.0,
        noise: 0.10,
        snap: 0.20,
        gain_db: -3.0,
    },
    GmSlot {
        key: 51,
        name: "Ride",
        model: DrumModel::Cymbal,
        family: Family::Cymbal,
        tune_hz: 620.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 1.20,
        tone_hz: 7_400.0,
        noise: 1.0,
        snap: 0.22,
        gain_db: -11.0,
    },
    GmSlot {
        key: 52,
        name: "China",
        model: DrumModel::Cymbal,
        family: Family::Cymbal,
        tune_hz: 470.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 1.10,
        tone_hz: 4_200.0,
        noise: 1.0,
        snap: 0.14,
        gain_db: -11.0,
    },
    GmSlot {
        key: 53,
        name: "Ride Bell",
        model: DrumModel::Cowbell,
        family: Family::Cymbal,
        tune_hz: 760.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.45,
        tone_hz: 3_400.0,
        noise: 0.30,
        snap: 0.30,
        gain_db: -10.0,
    },
    GmSlot {
        key: 54,
        name: "Tambourine",
        model: DrumModel::ClosedHat,
        family: Family::Perc,
        tune_hz: 500.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.16,
        tone_hz: 9_500.0,
        noise: 1.0,
        snap: 0.30,
        gain_db: -10.0,
    },
    GmSlot {
        key: 55,
        name: "Splash",
        model: DrumModel::Cymbal,
        family: Family::Cymbal,
        tune_hz: 700.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.70,
        tone_hz: 8_400.0,
        noise: 1.0,
        snap: 0.12,
        gain_db: -11.0,
    },
    GmSlot {
        key: 56,
        name: "Cowbell",
        model: DrumModel::Cowbell,
        family: Family::Perc,
        tune_hz: 540.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.32,
        tone_hz: 2_600.0,
        noise: 0.15,
        snap: 0.25,
        gain_db: -8.0,
    },
    GmSlot {
        key: 57,
        name: "Crash 2",
        model: DrumModel::Cymbal,
        family: Family::Cymbal,
        tune_hz: 580.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 1.40,
        tone_hz: 6_000.0,
        noise: 1.0,
        snap: 0.10,
        gain_db: -10.0,
    },
    GmSlot {
        key: 58,
        name: "Vibraslap",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 340.0,
        bend_semitones: 4.0,
        bend_s: 0.020,
        decay_s: 0.60,
        tone_hz: 3_000.0,
        noise: 0.70,
        snap: 0.40,
        gain_db: -10.0,
    },
    GmSlot {
        key: 59,
        name: "Ride 2",
        model: DrumModel::Cymbal,
        family: Family::Cymbal,
        tune_hz: 660.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 1.00,
        tone_hz: 8_000.0,
        noise: 1.0,
        snap: 0.20,
        gain_db: -11.0,
    },
    GmSlot {
        key: 60,
        name: "Bongo High",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 340.0,
        bend_semitones: 5.0,
        bend_s: 0.018,
        decay_s: 0.16,
        tone_hz: 1_800.0,
        noise: 0.16,
        snap: 0.30,
        gain_db: -6.0,
    },
    GmSlot {
        key: 61,
        name: "Bongo Low",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 260.0,
        bend_semitones: 5.0,
        bend_s: 0.020,
        decay_s: 0.19,
        tone_hz: 1_500.0,
        noise: 0.16,
        snap: 0.28,
        gain_db: -6.0,
    },
    GmSlot {
        key: 62,
        name: "Conga Mute",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 300.0,
        bend_semitones: 4.0,
        bend_s: 0.014,
        decay_s: 0.10,
        tone_hz: 1_600.0,
        noise: 0.22,
        snap: 0.34,
        gain_db: -6.0,
    },
    GmSlot {
        key: 63,
        name: "Conga High",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 230.0,
        bend_semitones: 5.0,
        bend_s: 0.024,
        decay_s: 0.24,
        tone_hz: 1_300.0,
        noise: 0.14,
        snap: 0.26,
        gain_db: -6.0,
    },
    GmSlot {
        key: 64,
        name: "Conga Low",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 175.0,
        bend_semitones: 5.0,
        bend_s: 0.028,
        decay_s: 0.30,
        tone_hz: 1_100.0,
        noise: 0.12,
        snap: 0.24,
        gain_db: -6.0,
    },
    GmSlot {
        key: 65,
        name: "Timbale High",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 380.0,
        bend_semitones: 6.0,
        bend_s: 0.016,
        decay_s: 0.22,
        tone_hz: 2_400.0,
        noise: 0.26,
        snap: 0.40,
        gain_db: -7.0,
    },
    GmSlot {
        key: 66,
        name: "Timbale Low",
        model: DrumModel::Perc,
        family: Family::Perc,
        tune_hz: 290.0,
        bend_semitones: 6.0,
        bend_s: 0.020,
        decay_s: 0.28,
        tone_hz: 2_000.0,
        noise: 0.24,
        snap: 0.38,
        gain_db: -7.0,
    },
    GmSlot {
        key: 67,
        name: "Agogo High",
        model: DrumModel::Cowbell,
        family: Family::Perc,
        tune_hz: 800.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.26,
        tone_hz: 3_200.0,
        noise: 0.12,
        snap: 0.24,
        gain_db: -9.0,
    },
    GmSlot {
        key: 68,
        name: "Agogo Low",
        model: DrumModel::Cowbell,
        family: Family::Perc,
        tune_hz: 640.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.30,
        tone_hz: 2_700.0,
        noise: 0.12,
        snap: 0.24,
        gain_db: -9.0,
    },
    GmSlot {
        key: 69,
        name: "Cabasa",
        model: DrumModel::ClosedHat,
        family: Family::Perc,
        tune_hz: 400.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.08,
        tone_hz: 7_000.0,
        noise: 1.0,
        snap: 0.34,
        gain_db: -11.0,
    },
    GmSlot {
        key: 70,
        name: "Maraca",
        model: DrumModel::ClosedHat,
        family: Family::Perc,
        tune_hz: 400.0,
        bend_semitones: 0.0,
        bend_s: 0.005,
        decay_s: 0.05,
        tone_hz: 9_800.0,
        noise: 1.0,
        snap: 0.40,
        gain_db: -11.0,
    },
];

/// How one style's drums differ from a plain studio kit.
///
/// Every field is a **multiplier** except the two that are not, and that is
/// deliberate: a multiplier composes with whatever the slot already said, so a
/// style never has to know that the floor tom is lower than the top tom — it
/// says "toms ring longer here" and all six move together.
#[derive(Debug, Clone, Copy)]
struct KitCharacter {
    /// Multiplies every hit's tuning.
    tune: f32,
    /// And the kick's again on top, because a kit's kick pitch is the single
    /// most recognisable thing about it.
    kick_tune: f32,
    /// Multiplies every hit's decay, then the family multipliers on top.
    decay: f32,
    kick_decay: f32,
    snare_decay: f32,
    hat_decay: f32,
    tom_decay: f32,
    /// Multiplies every hit's noise filter corner: the brightness of the kit.
    tone: f32,
    /// The snare's noise balance, **absolute** — it is the one number that
    /// says brush kit or gated eighties snare, and scaling it would make the
    /// extremes unreachable.
    snare_noise: f32,
    /// Saturation, absolute and applied to every hit.
    drive: f32,
    /// Multiplies every hit's transient.
    snap: f32,
    /// The pitched half's waveform, on everything but the kick.
    body: DrumBody,
    /// And the kick's own. Separately, because a kit's kick is the one hit
    /// that is heard on its own: a metal kit wants a triangle kick under a
    /// sine snare, and a chiptune kit wants squares everywhere.
    kick_body: DrumBody,
    /// Multiplies how far the kick's and the toms' pitch falls — the
    /// *punch*. An 808 has almost none (a sine that rings); a 909 is mostly
    /// sweep. The single most audible thing about a kick after its length.
    bend: f32,
    /// And how long that fall takes.
    bend_time: f32,
    /// The hats' and cymbals' own tuning, on top of `tune`. Moves the metal
    /// bank, so it is what separates a 606's hats from an 808's.
    hat_tune: f32,
    /// The hats' and cymbals' brightness, on top of `tone`.
    hat_tone: f32,
    /// How metallic the hats and cymbals are — [`DrumVoice::metal`],
    /// **absolute**. One is the 808's six squares; zero is a brush on a
    /// cymbal, which is noise.
    metal: f32,
    /// Bit reduction on every hit — [`DrumVoice::crush`], absolute. The
    /// LinnDrum, the SP-1200 and every chiptune, and nothing acoustic.
    crush: f32,
    /// The snare's pitch, on top of `tune`. A piccolo snare and a marching
    /// snare are the same drum an octave apart.
    snare_tune: f32,
}

/// The identity: a plain studio kit is the table above with nothing done to it.
const PLAIN: KitCharacter = KitCharacter {
    tune: 1.0,
    kick_tune: 1.0,
    decay: 1.0,
    kick_decay: 1.0,
    snare_decay: 1.0,
    hat_decay: 1.0,
    tom_decay: 1.0,
    tone: 1.0,
    snare_noise: 0.62,
    drive: 0.10,
    snap: 1.0,
    body: DrumBody::Sine,
    kick_body: DrumBody::Sine,
    bend: 1.0,
    bend_time: 1.0,
    hat_tune: 1.0,
    hat_tone: 1.0,
    metal: 0.0,
    crush: 0.0,
    snare_tune: 1.0,
};

/// A kit, by genre.
///
/// > *"lots of presets for different styles and genres of kits."*
///
/// The classic machines are named by number the way everybody refers to them,
/// and the genres by what somebody would search for. In the order a menu should
/// list them: the neutral one first, then the machines, then the genres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DrumKitStyle {
    /// Neutral and acoustic. The one a kit arrives as, because it is the one
    /// that is not a statement.
    #[default]
    Studio,
    EightOhEight,
    NineOhNine,
    SevenOhSeven,
    SixOhSix,
    LinnDrum,
    Trap,
    BoomBap,
    LoFi,
    House,
    Techno,
    DrumAndBass,
    Garage,
    Rock,
    Metal,
    Funk,
    JazzBrushes,
    Latin,
    Cinematic,
    Chiptune,
    Industrial,
    Ambient,
}

impl DrumKitStyle {
    /// Every kit, in the order the panel's chips list them.
    pub const ALL: [Self; 22] = [
        Self::Studio,
        Self::EightOhEight,
        Self::NineOhNine,
        Self::SevenOhSeven,
        Self::SixOhSix,
        Self::LinnDrum,
        Self::Trap,
        Self::BoomBap,
        Self::LoFi,
        Self::House,
        Self::Techno,
        Self::DrumAndBass,
        Self::Garage,
        Self::Rock,
        Self::Metal,
        Self::Funk,
        Self::JazzBrushes,
        Self::Latin,
        Self::Cinematic,
        Self::Chiptune,
        Self::Industrial,
        Self::Ambient,
    ];

    /// What the chip says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Studio => "Studio",
            Self::EightOhEight => "808",
            Self::NineOhNine => "909",
            Self::SevenOhSeven => "707",
            Self::SixOhSix => "606",
            Self::LinnDrum => "LinnDrum",
            Self::Trap => "Trap",
            Self::BoomBap => "Boom Bap",
            Self::LoFi => "Lo-Fi",
            Self::House => "House",
            Self::Techno => "Techno",
            Self::DrumAndBass => "Drum & Bass",
            Self::Garage => "Garage",
            Self::Rock => "Rock",
            Self::Metal => "Metal",
            Self::Funk => "Funk",
            Self::JazzBrushes => "Jazz Brushes",
            Self::Latin => "Latin",
            Self::Cinematic => "Cinematic",
            Self::Chiptune => "Chiptune",
            Self::Industrial => "Industrial",
            Self::Ambient => "Ambient",
        }
    }

    /// The recipe. One row per kit, and each row is meant to be readable as a
    /// description of the genre rather than as a pile of constants.
    fn character(self) -> KitCharacter {
        match self {
            Self::Studio => PLAIN,

            // The long ringing sine kick everybody knows — almost no sweep,
            // just a tone that goes on — six-square metal hats, a snare that
            // is more tone than snares, and a low tuning throughout.
            Self::EightOhEight => KitCharacter {
                kick_tune: 0.72,
                kick_decay: 4.2,
                bend: 0.4,
                bend_time: 2.4,
                hat_decay: 0.75,
                hat_tune: 0.64,
                metal: 1.0,
                hat_tone: 0.9,
                snare_noise: 0.45,
                snare_tune: 0.85,
                tone: 0.85,
                drive: 0.22,
                snap: 0.8,
                ..PLAIN
            },
            // The 808's louder cousin: a short kick that is mostly sweep with
            // a click on the front, a noisy high snare, hats with some metal
            // in them and open hats that go on.
            Self::NineOhNine => KitCharacter {
                kick_tune: 0.92,
                kick_decay: 0.85,
                bend: 1.7,
                bend_time: 0.8,
                snare_noise: 0.78,
                snare_tune: 1.15,
                hat_decay: 1.25,
                hat_tune: 1.2,
                metal: 0.55,
                tone: 1.15,
                snap: 1.5,
                drive: 0.26,
                ..PLAIN
            },
            // Dry, thin, unmistakably eighties: sampled through a small
            // converter, everything short and bright, metal hats.
            Self::SevenOhSeven => KitCharacter {
                decay: 0.7,
                kick_decay: 0.8,
                bend: 0.7,
                bend_time: 0.6,
                tone: 1.25,
                hat_tone: 1.2,
                hat_tune: 1.1,
                metal: 0.8,
                snare_noise: 0.55,
                snare_tune: 1.2,
                snap: 1.3,
                drive: 0.05,
                crush: 0.2,
                ..PLAIN
            },
            // The little one. Tiny fast kick, hissy metallic hats tuned high.
            Self::SixOhSix => KitCharacter {
                tune: 1.15,
                kick_tune: 1.2,
                kick_decay: 0.55,
                decay: 0.75,
                bend: 1.2,
                bend_time: 0.5,
                tone: 1.45,
                hat_tone: 1.3,
                hat_tune: 1.5,
                metal: 1.0,
                snare_noise: 0.85,
                snap: 1.4,
                drive: 0.14,
                ..PLAIN
            },
            // Sampled acoustic drums through eight bits: fat, a bit dull, a
            // snare with a long tail, and the converter audible on all of it.
            Self::LinnDrum => KitCharacter {
                kick_decay: 1.5,
                bend: 0.7,
                bend_time: 1.4,
                snare_decay: 1.8,
                snare_noise: 0.68,
                hat_decay: 1.1,
                tone: 0.8,
                drive: 0.30,
                snap: 0.8,
                crush: 0.45,
                ..PLAIN
            },

            // Sub kick that goes on for ever with almost no sweep, ticking
            // metal hats, a thin high snare.
            Self::Trap => KitCharacter {
                kick_tune: 0.6,
                kick_decay: 5.0,
                bend: 0.3,
                bend_time: 3.0,
                hat_decay: 0.5,
                hat_tune: 0.8,
                metal: 0.7,
                tone: 1.2,
                hat_tone: 1.3,
                snare_noise: 0.5,
                snare_tune: 1.3,
                snap: 1.2,
                drive: 0.30,
                ..PLAIN
            },
            // Dusty, mid-heavy, hard-hitting: short kick, fat low snare, and
            // the twelve-bit sampler that made the genre.
            Self::BoomBap => KitCharacter {
                kick_tune: 0.85,
                kick_decay: 0.7,
                bend: 1.3,
                bend_time: 0.6,
                snare_decay: 1.0,
                snare_noise: 0.66,
                snare_tune: 0.85,
                hat_decay: 0.7,
                tone: 0.7,
                hat_tone: 0.7,
                drive: 0.5,
                snap: 1.1,
                crush: 0.35,
                ..PLAIN
            },
            // Everything darker, softer, slower and crushed.
            Self::LoFi => KitCharacter {
                tune: 0.94,
                decay: 1.1,
                kick_decay: 1.3,
                bend: 0.6,
                bend_time: 1.8,
                snare_decay: 1.2,
                snare_noise: 0.58,
                snare_tune: 0.9,
                tone: 0.45,
                hat_tone: 0.5,
                drive: 0.4,
                snap: 0.4,
                crush: 0.65,
                ..PLAIN
            },

            // The four-to-the-floor kick: tight, punchy, a big fast sweep
            // with a click, and hats with a little metal in them.
            Self::House => KitCharacter {
                kick_tune: 0.95,
                kick_decay: 0.65,
                bend: 1.6,
                bend_time: 0.5,
                hat_decay: 0.6,
                hat_tune: 1.4,
                hat_tone: 1.3,
                metal: 0.5,
                snare_noise: 0.72,
                snare_tune: 1.05,
                tone: 1.1,
                snap: 1.5,
                drive: 0.30,
                ..PLAIN
            },
            // Harder and drier than house, and darker: a driven kick that is
            // nearly all sweep, low metallic hats.
            Self::Techno => KitCharacter {
                kick_tune: 0.8,
                kick_decay: 1.1,
                bend: 1.9,
                bend_time: 0.9,
                decay: 0.85,
                tone: 0.92,
                hat_tone: 1.1,
                hat_tune: 0.9,
                metal: 0.5,
                snare_noise: 0.74,
                drive: 0.55,
                snap: 1.2,
                ..PLAIN
            },
            // Breakbeat: very short kick, a cracking high snare, bright
            // everything, and a hint of the sampler it came off.
            Self::DrumAndBass => KitCharacter {
                kick_tune: 0.78,
                kick_decay: 0.7,
                bend: 1.5,
                bend_time: 0.5,
                snare_decay: 0.8,
                snare_noise: 0.8,
                snare_tune: 1.35,
                tone: 1.3,
                hat_tone: 1.2,
                snap: 1.6,
                drive: 0.38,
                crush: 0.15,
                ..PLAIN
            },
            // Shuffling and metallic: a round kick with little sweep under
            // very short, very bright, very high metal hats and a snare
            // tuned up to snap.
            Self::Garage => KitCharacter {
                decay: 0.8,
                kick_tune: 0.85,
                kick_decay: 1.0,
                bend: 0.9,
                snare_decay: 0.7,
                hat_decay: 0.45,
                tone: 1.4,
                hat_tone: 1.5,
                hat_tune: 1.6,
                metal: 0.7,
                snare_noise: 0.76,
                snare_tune: 1.4,
                snap: 1.45,
                drive: 0.20,
                ..PLAIN
            },

            // Big room, long toms, a snare with body, a kick with a slow
            // skin bend.
            Self::Rock => KitCharacter {
                kick_tune: 0.9,
                kick_decay: 1.15,
                bend: 1.2,
                bend_time: 1.3,
                snare_decay: 1.4,
                tom_decay: 1.5,
                snare_noise: 0.6,
                snare_tune: 0.95,
                tone: 1.05,
                drive: 0.28,
                snap: 1.15,
                ..PLAIN
            },
            // Clicky triggered kick — a fast steep bend on a triangle — a
            // tight gated snare, dark toms.
            Self::Metal => KitCharacter {
                kick_tune: 1.05,
                kick_decay: 0.45,
                bend: 1.5,
                bend_time: 0.4,
                snare_decay: 0.7,
                tom_decay: 0.8,
                snare_noise: 0.55,
                snare_tune: 1.1,
                tone: 1.2,
                hat_tone: 1.15,
                drive: 0.62,
                snap: 1.9,
                body: DrumBody::Triangle,
                kick_body: DrumBody::Triangle,
                ..PLAIN
            },
            // Tight, dry and forward — a kit that gets out of the way of a
            // bass line, with a snare tuned up to crack.
            Self::Funk => KitCharacter {
                decay: 0.82,
                kick_decay: 0.7,
                bend: 1.3,
                bend_time: 0.8,
                snare_decay: 0.6,
                snare_noise: 0.7,
                snare_tune: 1.4,
                hat_decay: 0.8,
                tone: 1.12,
                hat_tone: 0.8,
                snap: 1.4,
                drive: 0.16,
                ..PLAIN
            },
            // Almost all noise, almost no tone, a soft slow kick and a long
            // soft tail on everything.
            Self::JazzBrushes => KitCharacter {
                kick_tune: 1.1,
                kick_decay: 0.7,
                bend: 0.6,
                bend_time: 1.5,
                snare_decay: 2.2,
                snare_noise: 0.95,
                hat_decay: 1.6,
                tone: 0.75,
                hat_tone: 0.8,
                snap: 0.35,
                drive: 0.04,
                ..PLAIN
            },
            // Hand percussion: a high round conga of a kick with a slow
            // fall, a timbale of a snare that is more tone than wires, and
            // long shaker hats.
            Self::Latin => KitCharacter {
                tune: 1.12,
                kick_tune: 1.5,
                kick_decay: 1.4,
                bend: 0.5,
                bend_time: 1.4,
                snare_decay: 0.7,
                snare_noise: 0.45,
                snare_tune: 1.55,
                hat_decay: 1.7,
                tone: 1.2,
                hat_tone: 1.3,
                snap: 1.0,
                drive: 0.08,
                ..PLAIN
            },

            // Enormous and slow: a kick like a door with a slow fall, toms
            // like a hall, a low snare.
            Self::Cinematic => KitCharacter {
                tune: 0.8,
                kick_tune: 0.62,
                kick_decay: 2.6,
                bend: 0.7,
                bend_time: 2.5,
                tom_decay: 2.4,
                snare_decay: 1.8,
                decay: 1.6,
                hat_decay: 1.4,
                tone: 0.7,
                hat_tone: 0.6,
                snare_noise: 0.5,
                snare_tune: 0.75,
                drive: 0.18,
                snap: 0.7,
                ..PLAIN
            },
            // Square waves through four bits and a very short everything: a
            // kit made out of the same two channels the melody is on.
            Self::Chiptune => KitCharacter {
                tune: 1.25,
                kick_tune: 1.3,
                decay: 0.55,
                kick_decay: 0.5,
                bend: 1.8,
                bend_time: 0.5,
                tone: 1.5,
                hat_tone: 1.4,
                snare_noise: 0.9,
                snap: 1.5,
                drive: 0.0,
                crush: 0.8,
                body: DrumBody::Square,
                kick_body: DrumBody::Square,
                ..PLAIN
            },
            // Metal on metal: driven to bits, harsh and long, low metallic
            // hats and a crushed low snare.
            Self::Industrial => KitCharacter {
                tune: 0.9,
                kick_tune: 0.85,
                kick_decay: 1.4,
                bend: 1.4,
                bend_time: 0.7,
                decay: 1.25,
                tone: 1.25,
                hat_tone: 1.2,
                hat_tune: 0.75,
                metal: 0.85,
                snare_noise: 0.85,
                snare_tune: 0.8,
                drive: 0.85,
                snap: 1.7,
                crush: 0.3,
                body: DrumBody::Square,
                kick_body: DrumBody::Triangle,
                ..PLAIN
            },
            // Soft, dark and enormous — a kit you put reverb on and barely
            // hear: no sweep, no click, a little bell in the long hats.
            Self::Ambient => KitCharacter {
                tune: 0.85,
                kick_tune: 0.75,
                decay: 2.0,
                kick_decay: 1.8,
                hat_decay: 2.2,
                bend: 0.5,
                bend_time: 2.0,
                tone: 0.6,
                hat_tone: 0.5,
                hat_tune: 0.7,
                metal: 0.3,
                snare_noise: 0.55,
                snap: 0.3,
                drive: 0.02,
                ..PLAIN
            },
        }
    }

    /// Which kit these hits are, if they are exactly one of them.
    ///
    /// > *"not noticing much feedback for when i actually change a selection
    /// > of kit visually"*
    ///
    /// A preset writes the knobs and then has nothing further to say (rule
    /// 10), so nothing *remembers* which kit a channel holds — and it must
    /// not, or an edited hit would be a hit the kit could take back. But the
    /// panel can *look*: thirty-six layers that are exactly the 808's are the
    /// 808, and the chip can say so. The moment one hit is touched this is
    /// `None`, which is the truth, and the same rule an effect's presets
    /// follow in `EffectConfig::matching_preset`.
    ///
    /// Whole layers, not just the sounds: a layer's gain and pan are part of
    /// the kit as written, and a kit with the crash turned down is somebody's
    /// kit rather than the Studio kit.
    pub fn matching(patch: &Patch) -> Option<Self> {
        if patch.layers.is_empty()
            || !patch
                .layers
                .iter()
                .all(|layer| matches!(layer.source, Source::Drum(_)))
        {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|style| drum_kit(*style).layers == patch.layers)
    }
}

/// The hits of one kit, in key order.
///
/// This is where a [`KitCharacter`] becomes thirty-six concrete
/// [`DrumVoice`]s — and once it has, nothing remembers which style it came
/// from: what lands in the patch is an ordinary kit whose every hit can be
/// edited on its own. A preset writes the knobs and then has nothing further
/// to say, which is rule 10 and the same thing an effect preset does.
pub fn drum_slots(style: DrumKitStyle) -> Vec<DrumSlot> {
    let c = style.character();
    GM_DRUM_MAP
        .iter()
        .map(|slot| {
            // Which family's decay multiplier applies. A hat has no business
            // being lengthened by the setting that lengthens toms.
            let family_decay = match slot.family {
                Family::Kick => c.kick_decay,
                Family::Snare => c.snare_decay,
                Family::Hat => c.hat_decay,
                Family::Tom => c.tom_decay,
                // A cymbal follows the kit and a percussion hit follows
                // nothing in particular: both are already the length they are.
                Family::Cymbal | Family::Perc => 1.0,
            };
            // Which of the style's tunings this hit follows, and whether it
            // has a skin to bend: the kick and the toms sweep, the snare has
            // its own tuning, and the metalwork moves with the hats.
            let is_metalwork = matches!(
                slot.model,
                DrumModel::ClosedHat | DrumModel::OpenHat | DrumModel::Cymbal
            );
            let is_snare = slot.family == Family::Snare && slot.model == DrumModel::Snare;
            let bends = matches!(slot.family, Family::Kick | Family::Tom);
            let tune = slot.tune_hz
                * c.tune
                * match slot.family {
                    Family::Kick => c.kick_tune,
                    _ if is_snare => c.snare_tune,
                    _ if is_metalwork => c.hat_tune,
                    _ => 1.0,
                };
            // Clamped where the style table meets the slot table rather than
            // trusted: two multipliers of 2.5 on a hit that was already high
            // is a tune nobody chose, and the sound of that is a bug.
            let voice = DrumVoice {
                model: slot.model,
                body: if slot.family == Family::Kick {
                    c.kick_body
                } else {
                    c.body
                },
                tune_hz: tune.clamp(20.0, 16_000.0),
                bend_semitones: if bends {
                    (slot.bend_semitones * c.bend).clamp(0.0, 48.0)
                } else {
                    slot.bend_semitones
                },
                bend_s: if bends {
                    (slot.bend_s * c.bend_time).clamp(0.001, 2.0)
                } else {
                    slot.bend_s
                },
                decay_s: (slot.decay_s * c.decay * family_decay).clamp(0.005, 8.0),
                tone_hz: (slot.tone_hz * c.tone * if is_metalwork { c.hat_tone } else { 1.0 })
                    .clamp(20.0, 20_000.0),
                // The snare's noise is the style's; everything else keeps its
                // own, because "how much hiss is in a conga" is a fact about
                // congas rather than about a genre.
                noise: if is_snare {
                    c.snare_noise.clamp(0.0, 1.0)
                } else {
                    slot.noise.clamp(0.0, 1.0)
                },
                snap: (slot.snap * c.snap).clamp(0.0, 1.0),
                drive: c.drive.clamp(0.0, 1.0),
                gain_db: slot.gain_db.clamp(-24.0, 12.0),
                // The metal is the hats' and the cymbals'; a kick with six
                // square waves in its beater is not a kick anybody asked for.
                metal: if is_metalwork {
                    c.metal.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                crush: c.crush.clamp(0.0, 1.0),
            };
            DrumSlot {
                key: slot.key,
                name: slot.name,
                voice,
            }
        })
        .collect()
}

/// How far under full scale each hit sits in a kit.
///
/// **A drum part is a chord.** The kick, the snare and the hat land on the same
/// tick, and a hit that peaks at full scale on its own puts three of them at
/// three times it. Measured rather than guessed: seven at once — more than any
/// real bar — summed to 2.5 before this existed, and ten decibels brings that
/// back to about 0.8 with room to spare.
///
/// It is the same argument `Patch::basic_synth` makes about its saw, and the
/// same answer: the master's brickwall limiter *would* catch the overshoot, and
/// a kit that spends its life in the limiter is a kit that sounds squashed with
/// nothing to point at.
///
/// On the **layer** rather than on the hit, so that a hit's own `gain_db` stays
/// readable as what it is — "the crash is ten decibels under the kick" — rather
/// than carrying a headroom constant nobody set.
pub const KIT_HEADROOM_DB: f32 = -10.0;

/// A kit as a [`Patch`] — one layer per key, and nothing else.
///
/// The amp envelope is deliberately **open**: attack and decay at zero and
/// sustain at full, so the patch's envelope does nothing at all and the length
/// of a hit is the hit's own `decay_s`. A drum whose length is set in two
/// places is a drum whose knob appears not to work.
///
/// Everything else is `basic_synth`'s — the two filters, the mod matrix, the
/// voice config — because a kit is an ordinary patch and every one of those
/// still applies to it.
pub fn drum_kit(style: DrumKitStyle) -> Patch {
    let layers = drum_slots(style)
        .into_iter()
        .map(|slot| Layer {
            source: Source::Drum(slot.voice),
            // Exactly its own key. That is what makes `fontelle_app::key_map`
            // read the patch as a key map and write each hit's name on its
            // row — the labelling is not a special case, it is this.
            key_range: (slot.key, slot.key),
            vel_range: (0, 127),
            // Its own key, so the hit plays at the pitch the kit tuned it to
            // rather than being transposed by where it happens to sit.
            root_key: slot.key,
            fine_tune_cents: 0.0,
            playback: Default::default(),
            gain_db: KIT_HEADROOM_DB,
            pan: 0.0,
        })
        .collect();

    let mut patch = Patch {
        layers,
        ..Patch::basic_synth()
    };
    if let Some(env) = patch.envelopes.first_mut() {
        env.delay_s = 0.0;
        env.attack_s = 0.0;
        env.hold_s = 0.0;
        env.decay_s = 0.0;
        env.sustain_level = 1.0;
        // A short one rather than zero: a drum machine is played with
        // zero-length notes and a hard gate at note-off would clip every hit
        // to nothing. Long enough that the hit's own decay is always what is
        // heard, short enough that it is not a tail of its own.
        env.release_s = 4.0;
    }
    patch
}
