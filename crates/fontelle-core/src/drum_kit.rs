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
use fontelle_types::{
    BandChannel, BandType, CompressorConfig, DetectionMode, DistortionConfig, DistortionCurve,
    EffectConfig, EqBand, EqConfig, ReverbConfig,
};

use crate::patch::{Layer, MAX_PATCH_FX, Patch, PatchFx, Source};

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
    /// **How much of this kit is a struck instrument rather than a circuit**
    /// — [`DrumVoice::modes`], absolute, on the pitched hits.
    ///
    /// The axis that was missing, and the reason the kits still read as
    /// "tweaked versions of the same synthesised sound" after `metal` and
    /// `crush` had told them apart on paper. Those two changed the *noise*.
    /// This changes the **body**: at one, a kick, a tom and a conga ring at
    /// their own inharmonic modes like heads on shells; at zero they are the
    /// one oscillator they have always been.
    ///
    /// It is not a quality knob and a high setting is not "better". An 808 is
    /// a sine on purpose, a 909's kick is a sweep on purpose, and a chiptune
    /// kit has no membrane anywhere in it. What it is, is the difference
    /// between a machine and a room, and until now the machine was the only
    /// thing this instrument could be.
    modes: f32,
    /// The slow ring under the fast decay — [`DrumVoice::tail`], absolute.
    ///
    /// A shell that is still moving after the head has stopped. Acoustic kits
    /// have it, gated ones have it taken away on purpose, and a drum machine
    /// never had it at all.
    tail: f32,
    /// How much the snare's noise is **wires** rather than hiss —
    /// [`DrumVoice::rattle`], absolute, on the snare and the rim.
    rattle: f32,
    /// The snare's pitch, on top of `tune`. A piccolo snare and a marching
    /// snare are the same drum an octave apart.
    snare_tune: f32,
    /// How long the **cymbals** ring, and the axis the third report was
    /// missing.
    ///
    /// A cymbal used to follow nothing: `Family::Cymbal` took a family decay
    /// of one, so every kit in the program had the same 1.2-second ride and
    /// the same 1.6-second crash, and measured across six hits that was two of
    /// them contributing almost nothing to how different two kits were. A jazz
    /// ride washes for four seconds and a trap crash is a tick.
    cymbal_decay: f32,
    /// How much **bronze** is in the cymbals — [`DrumVoice::metal`] for
    /// [`Family::Cymbal`], separately from the hats'.
    ///
    /// Separately, because they are not the same metal. `metal` at one is the
    /// 808's six squares and reads as *that machine*, so every acoustic kit
    /// had to leave it at zero — which left every acoustic ride as a band of
    /// white noise, and a band of white noise is the same band of white noise
    /// in every kit. A ride is a struck plate whether or not the hats are a
    /// circuit, and this is the knob that says so.
    cymbal_metal: f32,
    /// How long the hand percussion rings. A conga in a Latin kit is a drum
    /// with a shell; in a techno kit it is a tick.
    perc_decay: f32,
    /// The converter the **hats and cymbals** went through —
    /// [`DrumVoice::crush`] for the metalwork, absolute, over the top of the
    /// kit's own `crush`.
    ///
    /// Its own knob because a drum machine did not sample everything at the
    /// same depth. Memory was expensive and the cymbals are the longest
    /// sounds in the box, so they are what got cut down — a 909's hats are a
    /// few bits of a real hat and its kick is a circuit, and no single `crush`
    /// can say that. Measured, this is the highest-authority knob the
    /// metalwork has: a sixth of it moves a hat further than doubling its
    /// length does. It is also the one that must stay at zero on anything
    /// acoustic, which is precisely why it could not be the global one.
    hat_crush: f32,
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
    // **`PLAIN` is a kit, not a null.** It is the Studio kit's basis and the
    // Studio kit is acoustic, so the modes are most of the way up and the
    // shell rings under the hit. A machine kit turns them *down*, which is the
    // right way round: a circuit is the special case and a struck drum is the
    // ordinary one.
    modes: 0.8,
    tail: 0.35,
    rattle: 0.5,
    cymbal_decay: 1.0,
    // Not zero: a real cymbal has bronze in it, and the reason every acoustic
    // kit's ride sounded like the same hiss is that this had nowhere to live
    // but the hats' own `metal`.
    cymbal_metal: 0.35,
    perc_decay: 1.0,
    hat_crush: 0.0,
};

// ---------------------------------------------------------------- the bus ---
//
// > *"we did some work on improving the drum machine built in plugin however
// > all of the presets still sound nearly the same"*
//
// The third report of the same thing, and the first one the two rounds above
// could not have fixed. `metal` and `crush` changed the noise; `modes` and
// `tail` changed the body; both worked, and both worked on **the hit**. But a
// kit is not thirty-six one-shots. It is thirty-six one-shots *and a bus*, and
// every kit in this program went through the same one: an empty chain, and two
// filters left wide open. Measured, all twenty-two were bone dry, and the only
// kit that stood clear of the huddle was Chiptune — the one whose *source* is
// different.
//
// That is the whole of it. A rock kit is a room. A gated snare is a room cut
// off. An 808 is dry on purpose and a hall is what makes a cinematic kit
// cinematic, and none of those live in an oscillator. So a style now says what
// its bus is as well as what its hits are, and the chain it produces is an
// ordinary [`Patch::fx`] — the same four slots Flopsynth's presets already
// carry, run by the same code in `SamplerNode`, editable and automatable
// afterwards like anything else. Nothing below the panel had to be told.

/// The kit's **bus**: the one chain every hit in it goes through together.
///
/// Four slots, in the order a drum bus is actually built — tone, then glue,
/// then character, then the room — and each is left out entirely when it is
/// set to a wire, so a kit that wants to be dry costs nothing and renders
/// exactly as it did before this existed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct KitSpace {
    /// Low shelf at 90 Hz: the weight.
    low_db: f32,
    /// Bell at 3.2 kHz: the stick, the beater, the edge of the snare.
    mid_db: f32,
    /// High shelf at 8 kHz: the air.
    high_db: f32,
    /// Bus compression. `1.0` is a wire and the slot is not written.
    ratio: f32,
    threshold_db: f32,
    /// The two numbers that make compression a *sound* rather than a level:
    /// a slow attack lets the beater through and a fast one flattens it, and
    /// the release is what pumps.
    attack_ms: f32,
    release_ms: f32,
    /// Parallel amount — one is the squashed kit alone, and less than one is
    /// the squashed kit under the one that still has its transients.
    comp_mix: f32,
    /// Bus saturation. `0.0` dB is a wire and the slot is not written.
    drive_db: f32,
    curve: DistortionCurve,
    drive_mix: f32,
    /// The room. A `room_mix` of zero is no room at all, and no slot.
    room_size: f32,
    room_decay_s: f32,
    room_damping_hz: f32,
    room_pre_delay_ms: f32,
    room_mix: f32,
    /// What the compressor puts back, in dB.
    ///
    /// Explicit rather than automatic, and this is where the level a kit lost
    /// to being glued is restored — **inside the chain, where it was lost**.
    /// Restoring it on the layer instead would turn the trim into a
    /// compression make-up and make the *dry* voices loud, which is a kit that
    /// clips the moment somebody switches its bus off.
    makeup_db: f32,
    /// What this kit's headroom is, on top of [`KIT_HEADROOM_DB`].
    ///
    /// **A bus is a gain stage**, and once every kit had one they stopped
    /// being the same loudness: measured, the six loudest hits of Drum & Bass
    /// landed at 5.43 of full scale and Ambient — which is seven tenths
    /// reverb — at 0.19. Twenty-nine decibels between two presets in the same
    /// menu is a jump somebody has to ride the fader for, and the ten that
    /// went over one clipped before they ever reached the master.
    ///
    /// So each kit pays for its own bus here, at the layer, where
    /// `KIT_HEADROOM_DB` already lives — rather than in the chain, where it
    /// would have to be either a fifth slot or a compressor make-up doing a
    /// second job. Each number brings that kit's worst case to the same 0.8.
    trim_db: f32,
}

/// No bus at all: what every kit in the program had until now.
const DRY: KitSpace = KitSpace {
    low_db: 0.0,
    mid_db: 0.0,
    high_db: 0.0,
    ratio: 1.0,
    threshold_db: -18.0,
    attack_ms: 10.0,
    release_ms: 120.0,
    comp_mix: 1.0,
    drive_db: 0.0,
    curve: DistortionCurve::SoftClip,
    drive_mix: 1.0,
    room_size: 0.4,
    room_decay_s: 1.0,
    room_damping_hz: 6_000.0,
    room_pre_delay_ms: 0.0,
    room_mix: 0.0,
    makeup_db: 0.0,
    trim_db: 0.0,
};

fn eq_band(band_type: BandType, freq_hz: f32, gain_db: f32, q: f32) -> EqBand {
    EqBand {
        band_type,
        freq_hz,
        gain_db,
        q,
        // A band with no gain on it is still switched on: it is a shelf at
        // zero, which is a wire, and leaving it enabled keeps every kit's EQ
        // the same three bands in the same three slots for anybody who opens
        // one to edit it.
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

impl KitSpace {
    /// Whether the tone stage would do anything.
    fn tilted(&self) -> bool {
        self.low_db != 0.0 || self.mid_db != 0.0 || self.high_db != 0.0
    }

    /// The chain this bus is, as a patch carries one.
    ///
    /// At most [`MAX_PATCH_FX`] slots, and fewer whenever a stage is set to a
    /// wire — which is what keeps a dry kit dry rather than paying for four
    /// effects that do nothing.
    fn chain(&self) -> Vec<PatchFx> {
        let mut out = Vec::new();
        if self.tilted() {
            let mut bands = [EqBand::new(); fontelle_types::BANDS];
            bands[0] = eq_band(BandType::LowShelf, 90.0, self.low_db, 0.7);
            bands[1] = eq_band(BandType::Bell, 3_200.0, self.mid_db, 1.0);
            bands[2] = eq_band(BandType::HighShelf, 8_000.0, self.high_db, 0.7);
            out.push(PatchFx {
                config: EffectConfig::Eq(EqConfig {
                    bands,
                    ..EqConfig::new()
                }),
                enabled: true,
            });
        }
        if self.ratio > 1.0 {
            out.push(PatchFx {
                config: EffectConfig::Compressor(CompressorConfig {
                    threshold_db: self.threshold_db,
                    ratio: self.ratio,
                    attack_ms: self.attack_ms,
                    release_ms: self.release_ms,
                    knee_db: 6.0,
                    // **Off, and the make-up is the kit's trim instead.**
                    //
                    // Auto make-up puts back exactly what the threshold took,
                    // which makes the compressor a level *regulator* — and a
                    // regulator downstream of a fader ignores the fader.
                    // Measured: Drum & Bass sat at 1.31 of full scale and no
                    // amount of layer trim moved it at all, because every
                    // decibel taken off its input came straight back as
                    // make-up. What a kit weighs is decided in one place
                    // (`KitSpace::trim_db`) or it is not decided anywhere.
                    auto_makeup: false,
                    makeup_db: self.makeup_db,
                    detection: DetectionMode::Peak,
                    mix: self.comp_mix,
                    ..CompressorConfig::new()
                }),
                enabled: true,
            });
        }
        if self.drive_db > 0.0 {
            out.push(PatchFx {
                config: EffectConfig::Distortion(DistortionConfig {
                    curve: self.curve,
                    drive_db: self.drive_db,
                    shape: 0.4,
                    mix: self.drive_mix,
                    ..DistortionConfig::new()
                }),
                enabled: true,
            });
        }
        if self.room_mix > 0.0 {
            out.push(PatchFx {
                config: EffectConfig::Reverb(ReverbConfig {
                    size: self.room_size,
                    decay_s: self.room_decay_s,
                    damping_hz: self.room_damping_hz,
                    pre_delay_ms: self.room_pre_delay_ms,
                    width: 1.0,
                    mix: self.room_mix,
                }),
                enabled: true,
            });
        }
        debug_assert!(out.len() <= MAX_PATCH_FX);
        out
    }
}

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
                kick_tune: 0.58,
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
                modes: 0.06,
                tail: 0.0,
                rattle: 0.0,
                cymbal_decay: 0.55,
                cymbal_metal: 1.00,
                perc_decay: 0.70,
                ..PLAIN
            },
            // The 808's louder cousin: a short kick that is mostly sweep with
            // a click on the front, a noisy high snare, hats with some metal
            // in them and open hats that go on.
            Self::NineOhNine => KitCharacter {
                kick_tune: 0.90,
                kick_decay: 0.90,
                bend: 1.7,
                bend_time: 0.8,
                snare_noise: 0.85,
                snare_tune: 1.25,
                hat_decay: 1.25,
                hat_tune: 1.50,
                metal: 0.60,
                tone: 1.15,
                snap: 1.5,
                drive: 0.34,
                modes: 0.14,
                tail: 0.0,
                rattle: 0.18,
                cymbal_decay: 0.70,
                cymbal_metal: 0.80,
                perc_decay: 0.80,
                tune: 1.05,
                decay: 0.82,
                hat_crush: 0.22,
                ..PLAIN
            },
            // Dry, thin, unmistakably eighties: sampled through a small
            // converter, everything short and bright, metal hats.
            Self::SevenOhSeven => KitCharacter {
                decay: 0.65,
                kick_decay: 0.60,
                bend: 0.7,
                bend_time: 0.6,
                tone: 1.15,
                hat_tone: 1.2,
                hat_tune: 0.80,
                metal: 0.90,
                snare_noise: 0.35,
                snare_tune: 1.35,
                snap: 1.3,
                drive: 0.20,
                crush: 0.2,
                modes: 0.30,
                tail: 0.08,
                rattle: 0.30,
                kick_tune: 1.25,
                cymbal_decay: 0.40,
                cymbal_metal: 0.75,
                perc_decay: 0.65,
                tune: 1.22,
                hat_crush: 0.10,
                hat_decay: 0.60,
                ..PLAIN
            },
            // The little one. Tiny fast kick, hissy metallic hats tuned high.
            Self::SixOhSix => KitCharacter {
                tune: 1.30,
                kick_tune: 1.55,
                kick_decay: 0.50,
                decay: 0.40,
                bend: 1.2,
                bend_time: 0.5,
                tone: 1.55,
                hat_tone: 1.3,
                hat_tune: 1.80,
                metal: 1.0,
                snare_noise: 0.90,
                snap: 1.4,
                drive: 0.46,
                modes: 0.10,
                tail: 0.0,
                rattle: 0.12,
                cymbal_decay: 0.25,
                cymbal_metal: 1.00,
                perc_decay: 0.60,
                hat_crush: 0.55,
                snare_tune: 1.35,
                snare_decay: 0.60,
                hat_decay: 0.45,
                ..PLAIN
            },
            // Sampled acoustic drums through eight bits: fat, a bit dull, a
            // snare with a long tail, and the converter audible on all of it.
            Self::LinnDrum => KitCharacter {
                kick_decay: 1.5,
                bend: 0.7,
                bend_time: 1.4,
                snare_decay: 1.80,
                snare_noise: 0.68,
                hat_decay: 1.1,
                tone: 0.95,
                drive: 0.30,
                snap: 0.8,
                crush: 0.45,
                modes: 0.45,
                tail: 0.12,
                rattle: 0.45,
                hat_tune: 0.90,
                kick_tune: 0.88,
                snare_tune: 1.12,
                metal: 0.35,
                cymbal_decay: 1.05,
                cymbal_metal: 0.30,
                perc_decay: 0.85,
                hat_crush: 0.0,
                ..PLAIN
            },

            // Sub kick that goes on for ever with almost no sweep, ticking
            // metal hats, a thin high snare.
            Self::Trap => KitCharacter {
                kick_tune: 0.55,
                kick_decay: 6.00,
                bend: 0.3,
                bend_time: 3.0,
                hat_decay: 0.30,
                hat_tune: 1.95,
                metal: 0.85,
                tone: 1.45,
                hat_tone: 1.60,
                snare_noise: 0.55,
                snare_tune: 1.35,
                snap: 1.2,
                drive: 0.30,
                modes: 0.05,
                tail: 0.0,
                rattle: 0.0,
                cymbal_decay: 0.30,
                cymbal_metal: 0.80,
                perc_decay: 0.45,
                hat_crush: 0.05,
                tune: 1.05,
                decay: 0.80,
                snare_decay: 0.60,
                ..PLAIN
            },
            // Dusty, mid-heavy, hard-hitting: short kick, fat low snare, and
            // the twelve-bit sampler that made the genre.
            Self::BoomBap => KitCharacter {
                kick_tune: 0.85,
                kick_decay: 0.7,
                bend: 1.3,
                bend_time: 0.6,
                snare_decay: 1.00,
                snare_noise: 0.66,
                snare_tune: 0.85,
                hat_decay: 0.7,
                tone: 0.62,
                hat_tone: 0.7,
                drive: 0.38,
                snap: 1.1,
                crush: 0.35,
                modes: 0.70,
                tail: 0.28,
                rattle: 0.62,
                hat_tune: 0.78,
                cymbal_decay: 0.60,
                cymbal_metal: 0.22,
                perc_decay: 0.90,
                tune: 0.88,
                decay: 1.12,
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
                // A **sampled** acoustic kit: the membrane is there, and then
                // a twelve-bit converter takes the top off it. Short, because
                // the sampler's memory was.
                modes: 0.72,
                tail: 0.06,
                rattle: 0.55,
                hat_tune: 0.70,
                cymbal_decay: 0.95,
                cymbal_metal: 0.12,
                perc_decay: 1.00,
                ..PLAIN
            },

            // The four-to-the-floor kick: tight, punchy, a big fast sweep
            // with a click, and hats with a little metal in them.
            Self::House => KitCharacter {
                kick_tune: 0.72,
                kick_decay: 0.38,
                bend: 2.5,
                bend_time: 0.5,
                hat_decay: 2.50,
                hat_tune: 1.55,
                hat_tone: 1.3,
                metal: 0.65,
                snare_noise: 0.80,
                snare_tune: 1.35,
                tone: 1.50,
                snap: 1.6,
                drive: 0.22,
                modes: 0.05,
                tail: 0.0,
                rattle: 0.10,
                cymbal_decay: 2.00,
                cymbal_metal: 0.45,
                perc_decay: 1.00,
                tune: 0.95,
                decay: 0.95,
                hat_crush: 0.05,
                snare_decay: 0.90,
                tom_decay: 0.70,
                ..PLAIN
            },
            // Harder and drier than house, and darker: a driven kick that is
            // nearly all sweep, low metallic hats.
            Self::Techno => KitCharacter {
                kick_tune: 1.25,
                kick_decay: 1.15,
                bend: 0.80,
                bend_time: 1.20,
                decay: 0.75,
                tone: 1.05,
                hat_tone: 1.1,
                hat_tune: 0.72,
                metal: 0.40,
                snare_noise: 0.92,
                drive: 0.85,
                snap: 1.2,
                modes: 0.08,
                tail: 0.0,
                rattle: 0.15,
                snare_tune: 0.88,
                cymbal_decay: 0.38,
                cymbal_metal: 0.35,
                perc_decay: 0.55,
                tune: 0.90,
                hat_crush: 0.08,
                hat_decay: 0.55,
                snare_decay: 0.55,
                ..PLAIN
            },
            // Breakbeat: very short kick, a cracking high snare, bright
            // everything, and a hint of the sampler it came off.
            Self::DrumAndBass => KitCharacter {
                kick_tune: 0.62,
                kick_decay: 2.20,
                bend: 1.5,
                bend_time: 0.5,
                snare_decay: 0.55,
                snare_noise: 0.75,
                snare_tune: 1.45,
                tone: 1.3,
                hat_tone: 1.2,
                snap: 2.0,
                drive: 0.26,
                crush: 0.15,
                modes: 0.55,
                tail: 0.20,
                rattle: 0.55,
                hat_tune: 0.95,
                metal: 0.30,
                cymbal_decay: 0.85,
                cymbal_metal: 0.30,
                perc_decay: 0.75,
                tune: 0.92,
                decay: 0.55,
                hat_crush: 0.38,
                hat_decay: 0.55,
                ..PLAIN
            },
            // Shuffling and metallic: a round kick with little sweep under
            // very short, very bright, very high metal hats and a snare
            // tuned up to snap.
            Self::Garage => KitCharacter {
                decay: 0.78,
                kick_tune: 1.15,
                kick_decay: 0.55,
                bend: 0.9,
                snare_decay: 0.38,
                hat_decay: 0.45,
                tone: 1.4,
                hat_tone: 1.50,
                hat_tune: 1.90,
                metal: 0.7,
                snare_noise: 0.85,
                snare_tune: 1.45,
                snap: 1.45,
                drive: 0.22,
                modes: 0.42,
                tail: 0.15,
                rattle: 0.48,
                cymbal_decay: 0.45,
                cymbal_metal: 0.65,
                perc_decay: 0.80,
                tune: 1.20,
                hat_crush: 0.15,
                ..PLAIN
            },

            // Big room, long toms, a snare with body, a kick with a slow
            // skin bend.
            Self::Rock => KitCharacter {
                kick_tune: 0.88,
                kick_decay: 1.40,
                bend: 1.2,
                bend_time: 1.3,
                snare_decay: 1.40,
                tom_decay: 1.80,
                snare_noise: 0.58,
                snare_tune: 0.95,
                tone: 0.90,
                drive: 0.14,
                snap: 1.15,
                modes: 1.00,
                tail: 0.80,
                rattle: 0.75,
                hat_tune: 0.85,
                cymbal_decay: 2.10,
                cymbal_metal: 0.45,
                perc_decay: 1.10,
                tune: 0.94,
                decay: 1.50,
                hat_decay: 1.35,
                ..PLAIN
            },
            // Clicky triggered kick — a fast steep bend on a triangle — a
            // tight gated snare, dark toms.
            Self::Metal => KitCharacter {
                kick_tune: 0.95,
                kick_decay: 0.75,
                bend: 1.5,
                bend_time: 0.4,
                snare_decay: 0.60,
                tom_decay: 0.85,
                snare_noise: 0.45,
                snare_tune: 1.30,
                tone: 1.2,
                hat_tone: 1.90,
                drive: 0.38,
                snap: 1.9,
                body: DrumBody::Triangle,
                kick_body: DrumBody::Triangle,
                modes: 0.30,
                tail: 0.0,
                rattle: 0.55,
                hat_tune: 1.60,
                cymbal_decay: 2.70,
                cymbal_metal: 0.85,
                perc_decay: 0.90,
                tune: 1.00,
                decay: 0.80,
                hat_decay: 0.55,
                ..PLAIN
            },
            // Tight, dry and forward — a kit that gets out of the way of a
            // bass line, with a snare tuned up to crack.
            Self::Funk => KitCharacter {
                decay: 0.55,
                kick_decay: 0.55,
                bend: 1.3,
                bend_time: 0.8,
                snare_decay: 0.45,
                snare_noise: 0.7,
                snare_tune: 1.60,
                hat_decay: 0.42,
                tone: 1.12,
                hat_tone: 0.75,
                snap: 1.8,
                drive: 0.04,
                modes: 0.75,
                tail: 0.30,
                rattle: 0.80,
                hat_tune: 1.30,
                kick_tune: 1.30,
                cymbal_decay: 0.58,
                cymbal_metal: 0.28,
                perc_decay: 0.95,
                tune: 1.14,
                tom_decay: 0.45,
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
                hat_decay: 1.60,
                tone: 0.75,
                hat_tone: 0.8,
                snap: 0.35,
                drive: 0.02,
                modes: 1.00,
                tail: 0.75,
                rattle: 0.85,
                hat_tune: 0.72,
                snare_tune: 1.30,
                cymbal_decay: 2.70,
                cymbal_metal: 0.58,
                perc_decay: 1.25,
                tune: 1.05,
                decay: 1.45,
                ..PLAIN
            },
            // Hand percussion: a high round conga of a kick with a slow
            // fall, a timbale of a snare that is more tone than wires, and
            // long shaker hats.
            Self::Latin => KitCharacter {
                tune: 1.18,
                kick_tune: 1.5,
                kick_decay: 1.4,
                bend: 0.5,
                bend_time: 1.4,
                snare_decay: 0.50,
                snare_noise: 0.45,
                snare_tune: 1.70,
                hat_decay: 1.7,
                tone: 1.2,
                hat_tone: 1.3,
                snap: 1.0,
                drive: 0.06,
                modes: 1.0,
                tail: 0.40,
                rattle: 0.35,
                cymbal_decay: 1.25,
                cymbal_metal: 0.40,
                perc_decay: 1.65,
                decay: 1.10,
                tom_decay: 1.30,
                ..PLAIN
            },

            // Enormous and slow: a kick like a door with a slow fall, toms
            // like a hall, a low snare.
            Self::Cinematic => KitCharacter {
                tune: 0.72,
                kick_tune: 0.55,
                kick_decay: 4.00,
                bend: 0.7,
                bend_time: 2.5,
                tom_decay: 2.20,
                snare_decay: 2.50,
                decay: 1.6,
                hat_decay: 1.4,
                tone: 0.7,
                hat_tone: 0.6,
                snare_noise: 0.5,
                snare_tune: 0.75,
                drive: 0.18,
                snap: 0.7,
                modes: 0.85,
                tail: 0.95,
                rattle: 0.30,
                cymbal_decay: 2.40,
                cymbal_metal: 0.50,
                perc_decay: 1.50,
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
                modes: 0.0,
                tail: 0.0,
                rattle: 0.0,
                cymbal_decay: 0.25,
                cymbal_metal: 0.00,
                perc_decay: 0.30,
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
                // Struck **metal**, not skin: the modes are low because there
                // is no membrane, and the tail is long because a sheet of
                // steel does not stop when you stop hitting it.
                modes: 0.22,
                tail: 0.70,
                rattle: 0.20,
                cymbal_decay: 1.60,
                cymbal_metal: 0.92,
                perc_decay: 1.30,
                hat_crush: 0.36,
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
                modes: 0.75,
                tail: 0.85,
                rattle: 0.40,
                cymbal_decay: 3.10,
                cymbal_metal: 0.30,
                perc_decay: 2.00,
                ..PLAIN
            },
        }
    }

    /// The bus this style plays through — see [`KitSpace`].
    ///
    /// Read these as rooms rather than as settings. The machines are dry
    /// because a machine went straight to tape; the acoustic kits are in
    /// spaces of the size the music was made in; and the two that are almost
    /// all room — Cinematic and Ambient — are the reason somebody would pick
    /// them over Studio at all.
    fn space(self) -> KitSpace {
        match self {
            // The one everything else is heard against: a small close room,
            // gently glued, tilted at nothing in particular.
            Self::Studio => KitSpace {
                low_db: 1.0,
                mid_db: 1.0,
                high_db: 2.0,
                ratio: 2.5,
                threshold_db: -16.0,
                attack_ms: 12.0,
                release_ms: 140.0,
                room_size: 0.26,
                room_decay_s: 0.7,
                room_damping_hz: 7_800.0,
                room_pre_delay_ms: 9.0,
                room_mix: 0.15,
                makeup_db: 5.31,
                trim_db: -0.22,
                ..DRY
            },
            // Straight to tape: no room whatsoever, an enormous shelf
            // underneath, no top, and the warmth of the desk it was always
            // plugged into. The sub *is* the sound.
            Self::EightOhEight => KitSpace {
                low_db: 7.5,
                mid_db: -2.0,
                high_db: -5.0,
                ratio: 2.0,
                threshold_db: -20.0,
                attack_ms: 25.0,
                release_ms: 220.0,
                drive_db: 7.0,
                curve: DistortionCurve::Tube,
                drive_mix: 0.6,
                makeup_db: 3.42,
                trim_db: -7.69,
                ..DRY
            },
            // A club record: dry, hard and bright, with the mid pulled out
            // from under the hats.
            Self::NineOhNine => KitSpace {
                low_db: 3.0,
                mid_db: -1.5,
                high_db: 6.5,
                ratio: 4.0,
                threshold_db: -18.0,
                attack_ms: 3.0,
                release_ms: 85.0,
                room_size: 0.20,
                room_decay_s: 0.45,
                room_damping_hz: 9_500.0,
                room_pre_delay_ms: 3.0,
                room_mix: 0.06,
                makeup_db: 2.70,
                trim_db: -0.01,
                ..DRY
            },
            // Thin and papery, and famously so: nothing at all underneath and
            // everything in the middle.
            Self::SevenOhSeven => KitSpace {
                low_db: -7.0,
                mid_db: 5.5,
                high_db: 1.0,
                ratio: 2.0,
                threshold_db: -16.0,
                attack_ms: 8.0,
                release_ms: 110.0,
                room_size: 0.18,
                room_decay_s: 0.4,
                room_damping_hz: 8_000.0,
                room_pre_delay_ms: 2.0,
                room_mix: 0.05,
                makeup_db: 4.41,
                trim_db: 0.07,
                ..DRY
            },
            // The little one: no bottom to speak of, all fizz, and a circuit
            // that clipped whenever it was asked for anything at all.
            Self::SixOhSix => KitSpace {
                low_db: -8.0,
                mid_db: -1.0,
                high_db: 8.5,
                ratio: 2.5,
                threshold_db: -20.0,
                attack_ms: 5.0,
                release_ms: 75.0,
                drive_db: 11.0,
                curve: DistortionCurve::HardClip,
                drive_mix: 0.55,
                room_size: 0.15,
                room_decay_s: 0.3,
                room_damping_hz: 12_000.0,
                room_pre_delay_ms: 1.0,
                room_mix: 0.04,
                makeup_db: 6.03,
                trim_db: 1.59,
                ..DRY
            },
            // Twelve-bit samples through a desk: crunchy in the middle, no
            // top because there was none to have, and a short bright plate.
            Self::LinnDrum => KitSpace {
                low_db: 0.5,
                mid_db: 4.5,
                high_db: -4.0,
                ratio: 3.0,
                threshold_db: -17.0,
                attack_ms: 6.0,
                release_ms: 130.0,
                drive_db: 5.0,
                curve: DistortionCurve::Tube,
                drive_mix: 0.7,
                room_size: 0.30,
                room_decay_s: 0.6,
                room_damping_hz: 7_500.0,
                room_pre_delay_ms: 5.0,
                room_mix: 0.13,
                makeup_db: 6.57,
                trim_db: -0.53,
                ..DRY
            },
            // Enormous underneath, hollow through the middle, bone dry: all
            // of it is the sub and the hats over the top of it.
            Self::Trap => KitSpace {
                low_db: 10.0,
                mid_db: -6.0,
                high_db: 7.0,
                ratio: 5.0,
                threshold_db: -22.0,
                attack_ms: 2.0,
                release_ms: 65.0,
                room_size: 0.16,
                room_decay_s: 0.3,
                room_damping_hz: 11_000.0,
                room_pre_delay_ms: 0.0,
                room_mix: 0.0,
                makeup_db: 2.88,
                trim_db: -4.60,
                ..DRY
            },
            // The SP sound, which is a compressor as much as it is a
            // converter: squashed flat, weighted low, and with the top eaten
            // clean off. The room is dark and short, like a dusty record.
            Self::BoomBap => KitSpace {
                low_db: 5.0,
                mid_db: 0.0,
                high_db: -9.5,
                ratio: 8.0,
                threshold_db: -24.0,
                attack_ms: 1.0,
                release_ms: 55.0,
                comp_mix: 0.85,
                drive_db: 4.0,
                curve: DistortionCurve::Tube,
                drive_mix: 0.5,
                room_size: 0.26,
                room_decay_s: 0.65,
                room_damping_hz: 2_600.0,
                room_pre_delay_ms: 6.0,
                room_mix: 0.17,
                makeup_db: 11.43,
                trim_db: -1.73,
                ..DRY
            },
            // Tape, not a room: soft, swimmy and with the top gone entirely,
            // through a space so damped it is more of a blanket.
            Self::LoFi => KitSpace {
                low_db: 2.0,
                mid_db: -2.5,
                high_db: -13.0,
                ratio: 3.5,
                threshold_db: -20.0,
                attack_ms: 15.0,
                release_ms: 260.0,
                drive_db: 8.0,
                curve: DistortionCurve::Tube,
                drive_mix: 0.65,
                room_size: 0.45,
                room_decay_s: 1.4,
                room_damping_hz: 1_300.0,
                room_pre_delay_ms: 14.0,
                room_mix: 0.30,
                makeup_db: 10.98,
                trim_db: -2.94,
                ..DRY
            },
            // Warm and round on a dark plate, no gap before it: the record was
            // mixed in a room and this is that room.
            Self::House => KitSpace {
                low_db: 5.5,
                mid_db: -0.5,
                high_db: -1.5,
                ratio: 3.0,
                threshold_db: -18.0,
                attack_ms: 26.0,
                release_ms: 190.0,
                room_size: 0.62,
                room_decay_s: 1.6,
                room_damping_hz: 2_800.0,
                room_pre_delay_ms: 0.0,
                room_mix: 0.36,
                makeup_db: 9.54,
                trim_db: -0.51,
                ..DRY
            },
            // Hard and driven and very nearly dry, with the mids taken right
            // out so the kick has the whole bottom to itself.
            Self::Techno => KitSpace {
                low_db: 6.0,
                mid_db: -7.5,
                high_db: 2.5,
                ratio: 4.5,
                threshold_db: -20.0,
                attack_ms: 1.5,
                release_ms: 95.0,
                drive_db: 16.0,
                curve: DistortionCurve::HardClip,
                drive_mix: 0.6,
                room_size: 0.28,
                room_decay_s: 0.5,
                room_damping_hz: 6_500.0,
                room_pre_delay_ms: 3.0,
                room_mix: 0.09,
                makeup_db: 8.10,
                trim_db: -0.11,
                ..DRY
            },
            // Bright, fast and flattened by a limiter, in a short bright room:
            // the break is squashed and the sub sits under it untouched.
            Self::DrumAndBass => KitSpace {
                low_db: 5.5,
                mid_db: 1.0,
                high_db: 7.5,
                ratio: 10.0,
                threshold_db: -25.0,
                attack_ms: 0.5,
                release_ms: 40.0,
                comp_mix: 0.9,
                room_size: 0.36,
                room_decay_s: 0.9,
                room_damping_hz: 10_500.0,
                room_pre_delay_ms: 5.0,
                room_mix: 0.16,
                makeup_db: 5.58,
                trim_db: 0.21,
                ..DRY
            },
            // A snare in a tiled room: crisp, glassy and very bright, with a
            // short plate that keeps all of its top.
            Self::Garage => KitSpace {
                low_db: 0.5,
                mid_db: 2.0,
                high_db: 9.0,
                ratio: 3.5,
                threshold_db: -19.0,
                attack_ms: 4.0,
                release_ms: 105.0,
                room_size: 0.44,
                room_decay_s: 0.95,
                room_damping_hz: 15_000.0,
                room_pre_delay_ms: 11.0,
                room_mix: 0.28,
                makeup_db: 3.78,
                trim_db: 0.06,
                ..DRY
            },
            // **A rock kit is a room**, and this is the one place that is the
            // whole point of the exercise: a big live space a long way behind
            // the hit, and a slow attack that lets the beater through first.
            Self::Rock => KitSpace {
                low_db: 3.0,
                mid_db: 3.0,
                high_db: 2.0,
                ratio: 3.0,
                threshold_db: -15.0,
                attack_ms: 24.0,
                release_ms: 230.0,
                comp_mix: 0.8,
                room_size: 0.76,
                room_decay_s: 2.4,
                room_damping_hz: 6_200.0,
                room_pre_delay_ms: 26.0,
                room_mix: 0.42,
                makeup_db: 7.65,
                trim_db: -4.91,
                ..DRY
            },
            // The eighties gate: a large bright room cut off before it can
            // decay. A short reverb in a big space is exactly what a gated
            // snare is, and nobody mistakes it for anything else.
            Self::Metal => KitSpace {
                low_db: 5.5,
                mid_db: -9.0,
                high_db: 6.5,
                ratio: 6.0,
                threshold_db: -20.0,
                attack_ms: 1.0,
                release_ms: 70.0,
                drive_db: 0.0,
                curve: DistortionCurve::HardClip,
                drive_mix: 0.45,
                room_size: 0.70,
                room_decay_s: 0.26,
                room_damping_hz: 3_400.0,
                room_pre_delay_ms: 5.0,
                room_mix: 0.50,
                makeup_db: 9.00,
                trim_db: -0.29,
                ..DRY
            },
            // A dead room and a compressor doing the playing: tight,
            // mid-forward, parallel-squashed, and with almost no space at all.
            Self::Funk => KitSpace {
                low_db: -1.5,
                mid_db: 7.0,
                high_db: 3.0,
                ratio: 5.0,
                threshold_db: -22.0,
                attack_ms: 2.0,
                release_ms: 85.0,
                comp_mix: 0.7,
                room_size: 0.20,
                room_decay_s: 0.35,
                room_damping_hz: 7_500.0,
                room_pre_delay_ms: 7.0,
                room_mix: 0.08,
                makeup_db: 3.78,
                trim_db: 1.22,
                ..DRY
            },
            // A small wooden room, warm and dark and barely touched — brushes
            // are all dynamics, and a compressor on them is the one thing that
            // would ruin them.
            Self::JazzBrushes => KitSpace {
                low_db: -1.0,
                mid_db: 0.5,
                high_db: -3.5,
                ratio: 1.8,
                threshold_db: -12.0,
                attack_ms: 30.0,
                release_ms: 320.0,
                comp_mix: 0.5,
                room_size: 0.40,
                room_decay_s: 1.35,
                room_damping_hz: 3_300.0,
                room_pre_delay_ms: 15.0,
                room_mix: 0.34,
                makeup_db: 9.72,
                trim_db: 2.01,
                ..DRY
            },
            // Hand drums in a real room with the mics a long way back: open,
            // bright, live, and with nothing underneath them.
            Self::Latin => KitSpace {
                low_db: -4.0,
                mid_db: 4.5,
                high_db: 5.5,
                ratio: 2.2,
                threshold_db: -14.0,
                attack_ms: 20.0,
                release_ms: 210.0,
                room_size: 0.52,
                room_decay_s: 1.6,
                room_damping_hz: 12_500.0,
                room_pre_delay_ms: 18.0,
                room_mix: 0.32,
                makeup_db: 6.75,
                trim_db: -0.01,
                ..DRY
            },
            // A hall, heard from the back of it: a long gap before the room
            // answers, a bright tail, and a great deal of weight underneath.
            Self::Cinematic => KitSpace {
                low_db: 7.0,
                mid_db: -3.0,
                high_db: 2.0,
                ratio: 2.0,
                threshold_db: -14.0,
                attack_ms: 35.0,
                release_ms: 420.0,
                room_size: 0.95,
                room_decay_s: 4.0,
                room_damping_hz: 6_800.0,
                room_pre_delay_ms: 48.0,
                room_mix: 0.44,
                makeup_db: 5.76,
                trim_db: -4.79,
                ..DRY
            },
            // Nothing at all. No room, no glue, no valve — a chip had none of
            // them, and putting any of it on is the one thing that would stop
            // this being a chiptune kit.
            Self::Chiptune => KitSpace {
                low_db: -9.0,
                mid_db: 6.5,
                high_db: 0.5,
                makeup_db: 0.27,
                trim_db: 0.23,
                ..DRY
            },
            // Metal in a concrete space, already broken before it gets there:
            // bright, long, and distorted hard enough that the room is ringing
            // on the distortion rather than on the drum.
            Self::Industrial => KitSpace {
                low_db: 1.5,
                mid_db: 3.0,
                high_db: 4.5,
                ratio: 6.5,
                threshold_db: -23.0,
                attack_ms: 1.0,
                release_ms: 60.0,
                drive_db: 18.0,
                curve: DistortionCurve::Diode,
                drive_mix: 0.75,
                room_size: 0.84,
                room_decay_s: 3.0,
                room_damping_hz: 9_500.0,
                room_pre_delay_ms: 8.0,
                room_mix: 0.42,
                makeup_db: 8.82,
                trim_db: 2.96,
                ..DRY
            },
            // All wash and no gap: an enormous soft space the hit is already
            // inside, with the top rolled a long way off. The far end of the
            // axis Chiptune sits at zero of.
            Self::Ambient => KitSpace {
                low_db: 1.0,
                mid_db: -6.5,
                high_db: -4.5,
                ratio: 1.6,
                threshold_db: -12.0,
                attack_ms: 40.0,
                release_ms: 520.0,
                comp_mix: 0.6,
                room_size: 1.0,
                room_decay_s: 8.0,
                room_damping_hz: 2_200.0,
                room_pre_delay_ms: 0.0,
                room_mix: 0.70,
                makeup_db: 15.48,
                trim_db: -3.99,
                ..DRY
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
        Self::ALL.into_iter().find(|style| {
            let kit = drum_kit(*style);
            // The bus counts. A kit whose room has been changed is not that
            // kit any more, and rule 10 says the chip has to stop claiming it
            // is — the same as for any other edit.
            kit.layers == patch.layers && kit.fx == patch.fx
        })
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
                Family::Cymbal => c.cymbal_decay,
                Family::Perc => c.perc_decay,
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
                // The metal is the hats' and the cymbals' — a kick with six
                // square waves in its beater is not a kick anybody asked for —
                // and the two are asked for separately, because a bronze ride
                // over acoustic hats is an ordinary kit and the one knob could
                // not say it.
                metal: if slot.family == Family::Cymbal {
                    c.cymbal_metal.clamp(0.0, 1.0)
                } else if is_metalwork {
                    c.metal.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                // The metalwork takes whichever converter is coarser: a lo-fi
                // kit crushes everything, and a machine crushes only what it
                // could not afford to store.
                crush: if is_metalwork {
                    c.hat_crush.max(c.crush).clamp(0.0, 1.0)
                } else {
                    c.crush.clamp(0.0, 1.0)
                },
                // The modes are the **pitched** hits'. A hat and a cymbal have
                // no membrane to ring, and `DrumModel::modes` gives them an
                // empty bank anyway — setting it here would be a knob whose
                // effect depended on which pad you were looking at.
                modes: if is_metalwork {
                    0.0
                } else {
                    c.modes.clamp(0.0, 1.0)
                },
                // A hat's tail is the hat's own decay, which a kit already
                // sets: a ring under a closed hat is an open hat.
                tail: if is_metalwork {
                    0.0
                } else {
                    c.tail.clamp(0.0, 1.0)
                },
                // Wires belong to the snare and, faintly, to the rim that
                // sits on the same drum. Everything else has none.
                rattle: match slot.model {
                    fontelle_dsp::DrumModel::Snare => c.rattle.clamp(0.0, 1.0),
                    fontelle_dsp::DrumModel::Rim => (c.rattle * 0.4).clamp(0.0, 1.0),
                    _ => 0.0,
                },
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

/// Where a hit sits across the stereo image.
///
/// A real kit is not a point source. It is a person sitting behind six things
/// arranged in an arc, and a recording of one puts them where they were: the
/// kick and the snare down the middle because that is where they are, the
/// toms sweeping left to right as they get bigger, the hats to one side and
/// the ride to the other.
///
/// A kit with every pad at dead centre is the single most "drum machine" thing
/// about a drum machine, and it costs nothing to fix — the layer already had a
/// `pan` and every kit was leaving it at zero.
///
/// **Modest numbers on purpose.** These are ±0.35 at the widest, not hard
/// left and right: a kit is heard from a few feet away through two
/// overheads, not from inside it. Wide enough that the image opens, narrow
/// enough that a mono fold-down loses nothing and a part still reads as one
/// kit rather than as two.
fn pan_for(slot: &DrumSlot) -> f32 {
    use fontelle_dsp::DrumModel;
    match slot.voice.model {
        // The two that carry the beat stay where the listener is pointed.
        DrumModel::Kick | DrumModel::Snare => 0.0,
        // Hats and the ride to the player's right, which is the listener's
        // left on a recording made from in front — the convention nearly every
        // record follows.
        DrumModel::ClosedHat | DrumModel::OpenHat => -0.22,
        DrumModel::Cymbal => 0.28,
        // The toms sweep. Their keys run low to high in General MIDI order, so
        // the key *is* the position: a floor tom is to the right of a rack tom
        // because it is a bigger drum further round the arc.
        DrumModel::Tom => {
            // GM's toms live between 41 and 50. Mapped across the arc, with
            // the low ones right — a drummer's floor tom is on their right.
            let at = (f32::from(slot.key) - 41.0) / 9.0;
            (0.30 - at.clamp(0.0, 1.0) * 0.60).clamp(-0.30, 0.30)
        }
        // The odds and ends go off to the sides, alternating by key so that a
        // conga and a cowbell on adjacent pads are not on top of each other.
        DrumModel::Rim | DrumModel::Cowbell | DrumModel::Perc | DrumModel::Clap => {
            if slot.key.is_multiple_of(2) {
                0.35
            } else {
                -0.35
            }
        }
    }
}

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
    let trim = style.space().trim_db;
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
            // The kit's own headroom on top of the instrument's — see
            // `KitSpace::trim_db`, which is what keeps twenty-two buses at one
            // loudness and all of them under full scale.
            gain_db: (KIT_HEADROOM_DB + trim).clamp(-24.0, 12.0),
            pan: pan_for(&slot),
        })
        .collect();

    let mut patch = Patch {
        layers,
        // **The kit's bus**, and the answer to the third report that the kits
        // all sound alike: an ordinary patch chain, so it is edited,
        // automated, saved and reloaded by everything that already handles
        // one. A style that wants no bus produces no slots.
        fx: style.space().chain(),
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
