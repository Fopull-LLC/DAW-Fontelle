//! The words on Flopsynth's window (`docs/flopsynth-next.md` §3.1,
//! principle 12: *words are design*).
//!
//! A parameter has a **name** — "pos", "mod from", "key trk" — which is
//! what the generic panel, the automation lanes and the tests that walk the
//! addresses call it. The bridge draws a **caption**: the word a player
//! uses for that knob on a synthesiser, in capitals, short enough for a
//! 56-pixel cell at eleven pixels — *WT POS*, *FM FROM*, *KEY TRK*; §3.1's
//! *ATK CURVE* is fifty-seven and reads *A CURVE* here. This file is the whole
//! vocabulary, and `tests/flopsynth_ui.rs` holds every caption drawn to it
//! and every word in it to a control that draws it, so a knob can neither
//! be captioned off the cuff nor keep a word nobody sees.
//!
//! The macros are outside it (a macro's caption is its name, a person's
//! word as typed) and so are the effect cards (their captions are the
//! effect's own, until the rack of §3.6 redraws them).

/// Name → caption. The name is the label `instrument::describe_flopsynth`
/// gives the control; the caption is what the window draws over it.
pub const CAPTIONS: &[(&str, &str)] = &[
    // The oscillators.
    ("kind", "KIND"),
    ("table", "TABLE"),
    ("pos", "WT POS"),
    ("start", "START"),
    ("bright", "BRIGHT"),
    ("level", "LEVEL"),
    ("pan", "PAN"),
    ("warp", "WARP"),
    ("amount", "AMOUNT"),
    ("mod from", "FM FROM"),
    ("unison", "UNISON"),
    ("detune", "DETUNE"),
    ("blend", "BLEND"),
    ("width", "WIDTH"),
    ("phase", "PHASE"),
    ("random", "RAND PH"),
    ("semis", "SEMI"),
    ("fine", "FINE"),
    ("key", "KEY TRK"),
    ("route", "ROUTE"),
    ("octave", "OCTAVE"),
    ("tune", "TUNE"),
    ("quality", "QUALITY"),
    ("oversample", "OVERSAMP"),
    // A recording.
    ("loop", "LOOP"),
    ("loop in", "LOOP IN"),
    ("loop out", "LOOP OUT"),
    ("zone", "ZONE"),
    ("grain", "GRAIN"),
    ("spray", "SPRAY"),
    // A string.
    ("stiff", "STIFF"),
    ("damp", "DAMP"),
    ("strike", "STRIKE"),
    ("ring", "RING"),
    // The noise.
    ("colour", "COLOUR"),
    // The filters.
    ("on", "ON"),
    ("model", "MODEL"),
    ("shape", "SHAPE"),
    ("slope", "SLOPE"),
    ("cutoff", "CUTOFF"),
    ("res", "RES"),
    ("drive", "DRIVE"),
    ("key trk", "KEY TRK"),
    ("saturation", "SAT"),
    ("vowel", "VOWEL"),
    ("feedback", "FEEDBACK"),
    // The envelopes.
    ("delay", "DELAY"),
    ("attack", "ATTACK"),
    ("hold", "HOLD"),
    ("decay", "DECAY"),
    ("sustain", "SUSTAIN"),
    ("release", "RELEASE"),
    ("a shape", "A CURVE"),
    ("d shape", "D CURVE"),
    ("r shape", "R CURVE"),
    ("loop", "LOOP"),
    // The LFOs.
    ("wave", "WAVE"),
    ("sync", "SYNC"),
    ("rate", "RATE"),
    ("division", "DIVISION"),
    ("mode", "MODE"),
    ("depth", "DEPTH"),
    ("fade", "FADE"),
    ("smooth", "SMOOTH"),
    // The shape editor's (§3.4).
    ("draw", "DRAW"),
    ("grid", "GRID"),
    ("read", "READ"),
    // The voice, and the channel's two knobs beside it.
    ("voices", "VOICES"),
    ("glide", "GLIDE"),
    ("legato", "LEGATO"),
    ("bend", "BEND"),
    ("output", "OUTPUT"),
    ("volume", "VOLUME"),
];

/// The caption for a control called `name`, or `None` for a word the file
/// does not know — which the window draws in capitals anyway, so a new
/// control is legible before it has been given its word.
pub fn caption(name: &str) -> Option<&'static str> {
    CAPTIONS
        .iter()
        .find(|(word, _)| *word == name)
        .map(|(_, drawn)| *drawn)
}

/// [`caption`], with the fallback the window draws.
pub fn captioned(name: &str) -> String {
    caption(name).map_or_else(|| name.to_uppercase(), str::to_string)
}
