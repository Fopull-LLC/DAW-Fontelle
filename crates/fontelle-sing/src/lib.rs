//! What a singer is asked to sing, and what it answers.
//!
//! Every way Fontelle sings goes through these types: the built-in
//! **Fontelle Singer** with its two voicebank backends (classic UTAU banks,
//! `fontelle-utau`; DiffSinger banks in the OpenUtau layout,
//! `fontelle-diffsinger`) and any **external singer engine** loaded through
//! `fontelle-singer-abi`. The vocal track owns the part — notes, lyrics,
//! expression, curves — and hands each backend the same [`Phrase`]; a
//! backend uses what it can and says up front, in [`Capabilities`], what that
//! is, so the piano roll can show which controls the chosen voice answers
//! to. Nothing here knows about any one backend. See `docs/vocal-plan.md`.
//!
//! # Time
//!
//! A phrase's time is seconds from its first note's start. Curves are
//! sampled every [`Curve::step`] seconds from [`Curve::start`] (usually
//! negative, so a consonant before the first note has a value too). Pitch is
//! MIDI with a fraction: 60.5 is C4 plus 50 cents.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The step the vocal track samples its curves at, in seconds.
pub const CURVE_STEP: f64 = 0.005;

/// The lyric that carries the previous note's syllable on to this one (see
/// `fontelle-singer-abi`'s note on lyrics for the full rules).
pub const LYRIC_SLUR: &str = "-";

/// One phrase to sing: a run of notes with no long rest inside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    /// The voice: a voicebank id for the built-in singer, a singer id for an
    /// external engine.
    pub voice: String,
    /// BCP 47 tag.
    pub language: String,
    pub notes: Vec<PhraseNote>,
    /// The standard lanes the part has drawn; a lane not drawn is absent
    /// and the backend uses its own default.
    pub curves: BTreeMap<Lane, Curve>,
    /// Lanes a backend declared itself ([`Capabilities::extra_lanes`]), by
    /// the backend's id for them.
    pub extra_curves: BTreeMap<String, Curve>,
    /// Settings a backend declared ([`Capabilities::settings`]) as the part
    /// has them, by id.
    pub settings: BTreeMap<String, f32>,
    /// The track's whole-part state blob for this backend, if it keeps one
    /// (an external engine's own window may store anything here).
    pub state: Option<Vec<u8>>,
    /// **Identity, not content.** Where the phrase is in the song: song
    /// seconds of its first note. With [`note_ids`](Self::note_ids) it lets
    /// a backend key per-phrase state (a take chosen in its own window) to
    /// a place in the song rather than to what is sung.
    ///
    /// Neither field is part of the host's render key, which hashes the
    /// phrase **in its own time** so that a phrase moved, copied or put back
    /// by an undo finds its take already sung: a backend must not let them
    /// change what it sings, only which of its own stored choices it applies.
    #[serde(default)]
    pub start: f64,
    /// The host's id of each note, parallel to [`notes`](Self::notes) — the
    /// ids a [`PartView`] and [`PartEdit`] name notes by. Empty when the host
    /// has none to give.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub note_ids: Vec<u64>,
}

impl Phrase {
    /// A phrase of `notes` with nothing drawn, at the top of the song.
    pub fn new(
        voice: impl Into<String>,
        language: impl Into<String>,
        notes: Vec<PhraseNote>,
    ) -> Self {
        Self {
            voice: voice.into(),
            language: language.into(),
            notes,
            curves: BTreeMap::new(),
            extra_curves: BTreeMap::new(),
            settings: BTreeMap::new(),
            state: None,
            start: 0.0,
            note_ids: Vec::new(),
        }
    }
}

/// One note of a phrase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhraseNote {
    pub start: f64,
    pub duration: f64,
    pub pitch: f32,
    pub lyric: String,
    /// Phonemes the writer set by hand, replacing the backend's reading.
    pub phonemes: Option<Vec<PhonemeEdit>>,
    /// Consonant speed: 1 is the voice's own, 2 twice as quick, 0.5 half.
    pub velocity: f32,
    pub vibrato: Option<Vibrato>,
    /// The glide into this note from the one before; `None` is the
    /// backend's default.
    pub portamento: Option<Portamento>,
    /// Per-note choices a backend declared ([`Capabilities::note_options`]),
    /// e.g. a voicebank's voice colour (an UTAU "append" suffix, a
    /// DiffSinger speaker), by option id.
    pub options: BTreeMap<String, String>,
}

impl PhraseNote {
    pub fn new(start: f64, duration: f64, pitch: f32, lyric: impl Into<String>) -> Self {
        Self {
            start,
            duration,
            pitch,
            lyric: lyric.into(),
            phonemes: None,
            velocity: 1.0,
            vibrato: None,
            portamento: None,
            options: BTreeMap::new(),
        }
    }

    pub fn end(&self) -> f64 {
        self.start + self.duration
    }
}

/// A phoneme the writer placed by hand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhonemeEdit {
    pub symbol: String,
    /// Where it starts, seconds from its note's start (negative: before
    /// it); `None` leaves the timing to the backend.
    pub offset: Option<f64>,
}

/// A note's vibrato. Fractions are of the note's length.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vibrato {
    /// Peak deviation either side, in cents.
    pub depth: f32,
    /// Cycles a second.
    pub rate: f32,
    /// How far into the note it starts, 0..1.
    pub delay: f32,
    /// Ramp in and out, 0..1 each, of the part after `delay`.
    pub fade_in: f32,
    pub fade_out: f32,
}

impl Default for Vibrato {
    /// What a singer does without being asked: a light vibrato late in the
    /// note.
    fn default() -> Self {
        Self {
            depth: 25.0,
            rate: 5.5,
            delay: 0.4,
            fade_in: 0.3,
            fade_out: 0.2,
        }
    }
}

/// The glide from the previous note's pitch into this one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Portamento {
    /// Seconds the glide takes.
    pub length: f32,
    /// Where it sits against the note's start: 0 ends on it, 0.5 centres
    /// on it, 1 starts on it.
    pub position: f32,
    pub shape: GlideShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlideShape {
    /// Ease in and out (the default).
    S,
    Linear,
    /// Fast then settling.
    Out,
    /// Slow then arriving.
    In,
}

/// The lanes every backend may answer to, drawn under the piano roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Lane {
    /// Cents added to the pitch the notes, glides and vibrato make.
    PitchBend,
    /// Loudness, dB (0 is the voice's own).
    Dynamics,
    /// 0 is the voice's own; -1..1.
    Breathiness,
    /// How voiced: 0 the voice's own; -1..1.
    Voicing,
    /// Pressed (1) to lax (-1).
    Tension,
    /// Formant shift in cents: positive is younger/brighter.
    Gender,
}

impl Lane {
    pub const ALL: [Lane; 6] = [
        Lane::PitchBend,
        Lane::Dynamics,
        Lane::Breathiness,
        Lane::Voicing,
        Lane::Tension,
        Lane::Gender,
    ];

    /// The value meaning "as the voice would".
    pub fn neutral(self) -> f32 {
        0.0
    }
}

/// A sampled curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    /// Seconds from the phrase's first note.
    pub start: f64,
    pub step: f64,
    pub values: Vec<f32>,
}

impl Curve {
    /// The value at `t` (phrase seconds), linear between samples, held past
    /// the ends; `fallback` when the curve is empty.
    pub fn at(&self, t: f64, fallback: f32) -> f32 {
        let Some(&last) = self.values.last() else {
            return fallback;
        };
        let x = (t - self.start) / self.step;
        if x <= 0.0 {
            return self.values[0];
        }
        let i = x.floor() as usize;
        if i + 1 >= self.values.len() {
            return last;
        }
        let f = (x - i as f64) as f32;
        self.values[i] + (self.values[i + 1] - self.values[i]) * f
    }
}

/// What a backend sang.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sung {
    pub sample_rate: u32,
    /// Mono.
    pub samples: Vec<f32>,
    /// Seconds from the phrase's first note to the first sample.
    pub start: f64,
    /// Hz every `pitch_step` seconds from `start`, 0 where unvoiced.
    pub pitch: Vec<f32>,
    pub pitch_step: f64,
    /// What each note was sung as, for the roll.
    pub phonemes: Vec<SungNote>,
}

/// One note as it was sung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SungNote {
    /// Index into the phrase's notes.
    pub note: usize,
    pub phonemes: Vec<SungPhoneme>,
    /// The backend did not know the word and guessed how to say it.
    pub guessed: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SungPhoneme {
    pub symbol: String,
    /// Seconds from the note's start, when the backend knows.
    pub offset: Option<f64>,
}

/// What a voice answers to. Asked once per voice and kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    /// The standard lanes it uses; the roll greys the others out.
    pub lanes: Vec<Lane>,
    pub vibrato: bool,
    pub portamento: bool,
    pub velocity: bool,
    /// Whether hand-placed phoneme timings are honoured.
    pub phoneme_timing: bool,
    /// Its phoneme inventory, for the phoneme editor's suggestions.
    pub phonemes: Vec<String>,
    /// Lanes of its own, drawn under the standard ones.
    pub extra_lanes: Vec<LaneInfo>,
    /// Whole-part settings of its own, shown on the track.
    pub settings: Vec<SettingInfo>,
    /// Per-note choices of its own.
    pub note_options: Vec<NoteOptionInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaneInfo {
    pub id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// Shown after values, e.g. "dB", "%".
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingInfo {
    pub id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteOptionInfo {
    pub id: String,
    pub name: String,
    /// (value, label); the first is the default.
    pub choices: Vec<(String, String)>,
}

/// Why a phrase was not sung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for SingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SingError::Cancelled => write!(f, "cancelled"),
            SingError::Failed(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for SingError {}

/// One voice a backend offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceInfo {
    /// Stable, saved in songs.
    pub id: String,
    pub name: String,
    /// BCP 47; the first is the default.
    pub languages: Vec<String>,
    /// Comfortable range, MIDI.
    pub lowest: u8,
    pub highest: u8,
    /// A voice for testing only, whose output must not be shared.
    pub research_only: bool,
    /// The voice's own terms of use, as its author wrote them, for the
    /// install and pick screens.
    pub terms: Option<String>,
    /// A portrait, if the voicebank has one (file path).
    pub image: Option<String>,
}

/// A backend: something with voices that can sing a [`Phrase`].
///
/// One `Backend` is shared; each render worker asks it for its own
/// [`Voice`] session and sings through that, one phrase at a time.
pub trait Backend: Send + Sync {
    /// Shown to the user and part of every render key.
    fn name(&self) -> &str;
    /// Part of every render key: a new version sings again.
    fn version(&self) -> String;
    fn voices(&self) -> Vec<VoiceInfo>;
    fn capabilities(&self, voice: &str) -> Capabilities;
    /// A session for one worker thread.
    fn session(&self) -> Result<Box<dyn Session>, String>;
}

/// One worker's session with a backend.
pub trait Session: Send {
    /// Sings `phrase`; `cancelled` is polled when convenient and a raised
    /// flag ends the render with [`SingError::Cancelled`].
    fn sing(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError>;

    /// [`sing`](Self::sing) with the cancel flag a host's scheduler raises
    /// from another thread, as the flag itself — so a backend that hands
    /// rendering to something else (an external engine's `cancel` pointer)
    /// can pass it straight on and be stopped mid-render. The default polls
    /// it through `sing`; nobody has to override it.
    fn sing_flagged(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancel: &std::sync::atomic::AtomicU32,
    ) -> Result<Sung, SingError> {
        self.sing(phrase, quality, seed, &|| {
            cancel.load(std::sync::atomic::Ordering::Relaxed) != 0
        })
    }
}

/// How good a take to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quality {
    /// Fast enough to hear an edit soon.
    Preview,
    /// The best the backend does, for playback and export.
    Final,
}

/// The pitch a phrase's notes, glides and vibrato make, before the bend
/// lane, as MIDI every [`CURVE_STEP`] from `start`. Backends that synthesise
/// pitch themselves (UTAU) use it directly; neural backends with their own
/// pitch model may use it only where the writer asked for vibrato or a
/// glide. Rests (gaps between notes) hold the nearer note's pitch.
pub fn note_pitch(notes: &[PhraseNote], start: f64, end: f64) -> Curve {
    let n = (((end - start) / CURVE_STEP).ceil().max(0.0) as usize) + 1;
    let mut values = Vec::with_capacity(n);
    for i in 0..n {
        let t = start + i as f64 * CURVE_STEP;
        values.push(pitch_at(notes, t));
    }
    Curve {
        start,
        step: CURVE_STEP,
        values,
    }
}

fn pitch_at(notes: &[PhraseNote], t: f64) -> f32 {
    if notes.is_empty() {
        return 60.0;
    }
    // The note sounding at t, else the nearer neighbour.
    let idx = match notes.iter().position(|n| t < n.end()) {
        None => notes.len() - 1,
        Some(i) if t < notes[i].start && i > 0 => {
            if t - notes[i - 1].end() < notes[i].start - t {
                i - 1
            } else {
                i
            }
        }
        Some(i) => i,
    };
    // A glide into the next note may begin before that note does.
    let idx = match notes.get(idx + 1) {
        Some(next) if t >= glide(next).0 => idx + 1,
        _ => idx,
    };
    let note = &notes[idx];
    let mut pitch = note.pitch;
    // The glide in from the previous note.
    if idx > 0 {
        let prev = notes[idx - 1].pitch;
        let (begin, len, shape) = glide(note);
        if len > 0.0 {
            let x = ((t - begin) / len).clamp(0.0, 1.0) as f32;
            if x < 1.0 {
                let w = match shape {
                    GlideShape::S => x * x * (3.0 - 2.0 * x),
                    GlideShape::Linear => x,
                    GlideShape::Out => 1.0 - (1.0 - x) * (1.0 - x),
                    GlideShape::In => x * x,
                };
                pitch = prev + (note.pitch - prev) * w;
            }
        }
    }
    // Vibrato over the note's own span.
    if let Some(v) = note.vibrato
        && t >= note.start
        && t < note.end()
    {
        let len = note.duration.max(1e-6);
        let from = note.start + len * f64::from(v.delay.clamp(0.0, 1.0));
        if t >= from {
            let span = (note.end() - from).max(1e-6);
            let x = ((t - from) / span) as f32;
            let fade_in = if v.fade_in > 0.0 {
                (x / v.fade_in).min(1.0)
            } else {
                1.0
            };
            let fade_out = if v.fade_out > 0.0 {
                ((1.0 - x) / v.fade_out).min(1.0)
            } else {
                1.0
            };
            let phase = (t - from) as f32 * v.rate * std::f32::consts::TAU;
            pitch += v.depth / 100.0 * fade_in.min(fade_out) * phase.sin();
        }
    }
    pitch
}

/// A vocal part as an external engine's own window sees it
/// (`fontelle-singer-abi` v2's `get_part`). Times are song seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartView {
    pub track: String,
    pub voice: String,
    pub language: String,
    pub notes: Vec<ViewNote>,
    pub curves: BTreeMap<Lane, Curve>,
    pub extra_curves: BTreeMap<String, Curve>,
    pub settings: BTreeMap<String, f32>,
    pub state: Option<Vec<u8>>,
    /// Ids of the notes selected in the piano roll.
    pub selection: Vec<u64>,
    pub playhead: f64,
    pub playing: bool,
}

/// A note in a [`PartView`]: a [`PhraseNote`] in song time, with the id
/// edits name it by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewNote {
    pub id: u64,
    pub note: PhraseNote,
    /// What it was last sung as, when it has been.
    pub sung: Vec<String>,
}

/// An edit an external engine's window asks the host for
/// (`fontelle-singer-abi` v2's `apply_edits`). A batch is one undo step.
/// Times are song seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PartEdit {
    /// **Merged** into the part's settings for the singing backend: each key
    /// given is set to its value, keys not given keep theirs. (Sending the
    /// whole map is therefore always right.)
    Settings(BTreeMap<String, f32>),
    State(Option<Vec<u8>>),
    Curve {
        lane: Lane,
        curve: Option<Curve>,
    },
    ExtraCurve {
        id: String,
        curve: Option<Curve>,
    },
    Lyrics(Vec<(u64, String)>),
    Phonemes {
        note: u64,
        phonemes: Option<Vec<PhonemeEdit>>,
    },
    Velocity {
        notes: Vec<u64>,
        velocity: f32,
    },
    Vibrato {
        notes: Vec<u64>,
        vibrato: Option<Vibrato>,
    },
    Portamento {
        notes: Vec<u64>,
        portamento: Option<Portamento>,
    },
    NoteOptions {
        notes: Vec<u64>,
        options: BTreeMap<String, String>,
    },
    Select(Vec<u64>),
}

/// Transport requests from an external engine's window; not edits, never
/// undone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Transport {
    Play,
    Stop,
    Seek(f64),
}

/// Where a note's glide in begins, how long it is and its shape.
fn glide(note: &PhraseNote) -> (f64, f64, GlideShape) {
    let p = note.portamento.unwrap_or(Portamento {
        length: 0.08,
        position: 0.3,
        shape: GlideShape::S,
    });
    let len = f64::from(p.length.max(0.0));
    let begin = note.start - len * f64::from(1.0 - p.position.clamp(0.0, 1.0));
    (begin, len, p.shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_reads_between_and_beyond_its_samples() {
        let c = Curve {
            start: -0.01,
            step: 0.01,
            values: vec![0.0, 1.0, 3.0],
        };
        assert_eq!(c.at(-1.0, 9.0), 0.0);
        assert!((c.at(-0.005, 9.0) - 0.5).abs() < 1e-5);
        assert!((c.at(0.005, 9.0) - 2.0).abs() < 1e-5);
        assert_eq!(c.at(5.0, 9.0), 3.0);
        let empty = Curve {
            start: 0.0,
            step: 0.01,
            values: vec![],
        };
        assert_eq!(empty.at(0.0, 9.0), 9.0);
    }

    #[test]
    fn note_pitch_holds_notes_and_glides_between_them() {
        let mut b = PhraseNote::new(0.5, 0.5, 64.0, "b");
        b.portamento = Some(Portamento {
            length: 0.1,
            position: 0.0,
            shape: GlideShape::Linear,
        });
        let notes = [PhraseNote::new(0.0, 0.5, 60.0, "a"), b];
        let c = note_pitch(&notes, 0.0, 1.0);
        assert!((c.at(0.2, 0.0) - 60.0).abs() < 1e-4);
        // Halfway through a linear glide that ends on the note.
        assert!((c.at(0.45, 0.0) - 62.0).abs() < 0.05);
        assert!((c.at(0.8, 0.0) - 64.0).abs() < 1e-4);
    }

    #[test]
    fn vibrato_swings_by_its_depth_after_its_delay() {
        let mut n = PhraseNote::new(0.0, 2.0, 60.0, "a");
        n.vibrato = Some(Vibrato {
            depth: 50.0,
            rate: 5.0,
            delay: 0.5,
            fade_in: 0.0,
            fade_out: 0.0,
        });
        let c = note_pitch(&[n], 0.0, 2.0);
        assert!(
            (c.at(0.5, 0.0) - 60.0).abs() < 1e-3,
            "nothing before the delay"
        );
        let peak = c.values.iter().cloned().fold(f32::MIN, f32::max);
        assert!((peak - 60.5).abs() < 0.01, "{peak}");
    }

    #[test]
    fn a_phrase_round_trips_through_json() {
        let mut p = Phrase {
            voice: "v".into(),
            language: "en".into(),
            notes: vec![PhraseNote::new(0.0, 1.0, 60.0, "la")],
            curves: BTreeMap::new(),
            extra_curves: BTreeMap::new(),
            settings: BTreeMap::new(),
            state: None,
            start: 12.5,
            note_ids: vec![7],
        };
        p.curves.insert(
            Lane::Breathiness,
            Curve {
                start: 0.0,
                step: CURVE_STEP,
                values: vec![0.1, 0.2],
            },
        );
        let back: Phrase = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}
