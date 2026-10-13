//! The vocal track's part of the document: who sings a channel, and the
//! commands that change a voice or the words on its notes.
//!
//! What a song stores is only *what to sing*: the engine and singer by name,
//! the language, and each note's lyric. The singing is done elsewhere — by
//! a singer engine the app finds at run time — and is a cache, never part of
//! the song: a song opened where its singer is not installed keeps every word
//! and sings again where it is.

use fontelle_types::{ChannelId, NoteId};

use crate::command::{Command, CommandError};
use crate::commands::{NotApplied, notes_of};
use crate::note::NoteHome;
use crate::project::Project;

/// Who sings a vocal channel.
///
/// By name rather than by any number an engine hands out at run time,
/// because the song outlives the run: `engine` is the engine's display name
/// (what it reports in its table), `singer` the singer's stable id inside it,
/// `language` a BCP 47 tag among the singer's languages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VocalVoice {
    pub engine: String,
    pub singer: String,
    pub language: String,
}

impl VocalVoice {
    pub fn new(
        engine: impl Into<String>,
        singer: impl Into<String>,
        language: impl Into<String>,
    ) -> Self {
        Self {
            engine: engine.into(),
            singer: singer.into(),
            language: language.into(),
        }
    }

    /// The engine whose instrument sings this voice, or `None` for the
    /// built-in Fontelle Singer — whose runtimes' keys start
    /// [`BUILTIN_ENGINE_PREFIX`]. A vocal track's instrument *is* this: one
    /// part format for every singer, so changing instrument keeps the notes,
    /// lyrics, expression and lanes.
    pub fn engine_instrument(&self) -> Option<&str> {
        (!is_builtin_engine(&self.engine)).then_some(self.engine.as_str())
    }
}

/// What the keys of the Fontelle Singer's built-in runtimes start with
/// (`fontelle-utau`, `fontelle-diffsinger`); no external engine may use one.
pub const BUILTIN_ENGINE_PREFIX: &str = "fontelle-";

/// Whether `engine` (a [`VocalVoice::engine`]) is one of the Fontelle
/// Singer's runtimes rather than an external engine.
pub fn is_builtin_engine(engine: &str) -> bool {
    engine.starts_with(BUILTIN_ENGINE_PREFIX)
}

/// Each of a set of notes gets its own lyric — a typed line spread over a
/// selection, one note's word retyped, a syllable cleared — in one undo step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetLyrics {
    home: NoteHome,
    ids: Vec<NoteId>,
    lyrics: Vec<String>,
    /// Each note's lyric before, in `ids` order; `None` until applied, which
    /// is how `invert` knows it has nothing to say yet (an empty lyric is a
    /// real previous value, so emptiness cannot be the flag).
    previous: Option<Vec<String>>,
    label: String,
}

impl SetLyrics {
    pub fn new(home: impl Into<NoteHome>, ids: Vec<NoteId>, lyrics: Vec<String>) -> Self {
        let label = match ids.len() {
            1 => "Set a note's lyric".to_string(),
            n => format!("Set the lyrics of {n} notes"),
        };
        Self {
            home: home.into(),
            ids,
            lyrics,
            previous: None,
            label,
        }
    }
}

impl Command for SetLyrics {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetLyrics(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.ids.len() != self.lyrics.len() {
            return Err(CommandError(format!(
                "{} lyrics for {} notes",
                self.lyrics.len(),
                self.ids.len()
            )));
        }
        let data = notes_of(doc, self.home)?;
        // Checked before anything is written, so a refusal changes nothing.
        for id in &self.ids {
            if data.notes.get(*id).is_none() {
                return Err(CommandError(format!("no note {id:?} here")));
            }
        }
        // Only on the first apply: a redo restores what the first run saw.
        if self.previous.is_none() {
            self.previous = Some(
                self.ids
                    .iter()
                    .map(|id| data.notes[*id].lyric.clone())
                    .collect(),
            );
        }
        for (id, lyric) in self.ids.iter().zip(&self.lyrics) {
            if let Some(note) = data.notes.get_mut(*id) {
                note.lyric.clone_from(lyric);
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => {
                let mut back = SetLyrics::new(self.home, self.ids.clone(), previous.clone());
                back.label.clone_from(&self.label);
                Box::new(back)
            }
            None => Box::new(NotApplied::new("setting lyrics")),
        }
    }

    fn label(&self) -> &str {
        &self.label
    }

    /// Never: every lyric commit is its own step, so Tab through a line and
    /// Ctrl+Z takes back one word at a time.
    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        let text = |v: &[String]| v.iter().map(|s| s.len() + 24).sum::<usize>();
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + text(&self.lyrics)
            + self.previous.as_deref().map_or(0, text)
            + self.label.len()
    }
}

/// Chooses who sings a channel — engine, singer and language together, so
/// changing singer and language at once is one step — or `None` for nobody.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetSinger {
    channel: ChannelId,
    voice: Option<VocalVoice>,
    #[serde(with = "crate::wire::nested")]
    previous: Option<Option<VocalVoice>>,
}

impl SetSinger {
    pub fn new(channel: ChannelId, voice: Option<VocalVoice>) -> Self {
        Self {
            channel,
            voice,
            previous: None,
        }
    }
}

impl Command for SetSinger {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetSinger(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(channel) = doc.channels.get_mut(self.channel) else {
            return Err(CommandError("that channel is not there".into()));
        };
        let previous = std::mem::replace(&mut channel.vocal, self.voice.clone());
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(SetSinger::new(self.channel, previous.clone())),
            None => Box::new(NotApplied::new("choosing a singer")),
        }
    }

    fn label(&self) -> &str {
        "Choose a singer"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + 64
    }
}

// ------------------------------------------------------------ expression
//
// Everything below is how a part is sung beyond its notes and words: what
// `fontelle_sing::Phrase` carries to a backend. The types a phrase uses
// (vibrato, glide, phoneme edits, the standard lanes) are the contract's
// own, so the document stores exactly what a singer is handed.
//
// **Lanes are in ticks**, from the start of the notes' clip (or prefab):
// they belong to the music, so a tempo change keeps a swell on the note it
// was drawn under, and a clip moved or copied carries its curves the way it
// carries its notes. `fontelle-vocal` samples them into a phrase's seconds.

use std::collections::BTreeMap;

use fontelle_sing::{Lane as SungLane, PhonemeEdit, Portamento, Vibrato};
use fontelle_types::Tick;

/// How a note is sung beyond its lyric. Every field is optional; an empty
/// voice is stored as none at all.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NoteVoice {
    /// Phonemes placed by hand, replacing the backend's reading of the
    /// lyric; offsets are seconds from the note's start (a consonant does
    /// not get longer because the song is slower).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phonemes: Option<Vec<PhonemeEdit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibrato: Option<Vibrato>,
    /// The glide in from the note before; `None` is the backend's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portamento: Option<Portamento>,
    /// Per-note options a backend declared, by backend then option id —
    /// kept when another backend sings, which never sees them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, BTreeMap<String, String>>,
}

// Every number in a voice is finite (`SetNoteVoices` refuses anything else),
// so equality is an equivalence and `Note` stays `Eq`.
impl Eq for NoteVoice {}

impl NoteVoice {
    pub fn is_empty(&self) -> bool {
        self.phonemes.is_none()
            && self.vibrato.is_none()
            && self.portamento.is_none()
            && self.options.values().all(BTreeMap::is_empty)
    }

    /// Whether every number in it is a number.
    pub fn is_finite(&self) -> bool {
        let vibrato = self.vibrato.is_none_or(|v| {
            [v.depth, v.rate, v.delay, v.fade_in, v.fade_out]
                .iter()
                .all(|x| x.is_finite())
        });
        let glide = self
            .portamento
            .is_none_or(|p| p.length.is_finite() && p.position.is_finite());
        let phonemes = self
            .phonemes
            .iter()
            .flatten()
            .all(|p| p.offset.is_none_or(f64::is_finite));
        vibrato && glide && phonemes
    }

    /// The options `backend` sees.
    pub fn options_for(&self, backend: &str) -> BTreeMap<String, String> {
        self.options.get(backend).cloned().unwrap_or_default()
    }
}

/// A vocal note's velocity read as **consonant speed** — OpenUtau's VEL, on
/// the note's own MIDI velocity so the roll's velocity lane edits it: 100 is
/// the voice's own speed, and every 27 either way doubles or halves it (127
/// is twice as quick, 73 half).
pub fn consonant_speed(velocity: u8) -> f32 {
    2f32.powf((f32::from(velocity) - 100.0) / 27.0)
}

/// The velocity that sings at consonant speed `speed`, within 1..=127; a
/// speed that is not a number is the voice's own (100).
pub fn velocity_for_speed(speed: f32) -> u8 {
    if speed.is_nan() {
        return 100;
    }
    let v = 100.0 + 27.0 * speed.max(0.0).log2();
    v.round().clamp(1.0, 127.0) as u8
}

/// What the roll shows a standard lane as.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneSpec {
    pub name: &'static str,
    /// Short, for a tab.
    pub short: &'static str,
    pub min: f32,
    pub max: f32,
    pub unit: &'static str,
}

/// A standard lane's name, range and unit (`fontelle_sing::Lane`'s own
/// meanings).
pub fn lane_spec(lane: SungLane) -> LaneSpec {
    let spec = |name, short, min, max, unit| LaneSpec {
        name,
        short,
        min,
        max,
        unit,
    };
    match lane {
        SungLane::PitchBend => spec("Pitch bend", "pitch", -1200.0, 1200.0, "cents"),
        SungLane::Dynamics => spec("Dynamics", "dyn", -24.0, 12.0, "dB"),
        SungLane::Breathiness => spec("Breathiness", "breath", -1.0, 1.0, ""),
        SungLane::Voicing => spec("Voicing", "voice", -1.0, 1.0, ""),
        SungLane::Tension => spec("Tension", "tension", -1.0, 1.0, ""),
        SungLane::Gender => spec("Gender", "gender", -600.0, 600.0, "cents"),
    }
}

/// A lane drawn under the roll: points `(tick from the clip's start,
/// value)`, in time order, straight lines between them, the first and last
/// values held beyond the ends. Empty is the lane's neutral everywhere.
///
/// Drawing never leaves a ramp across what was not drawn: [`write`] puts
/// the old values back right beside a stroke, and simplifies straight runs
/// to their ends, so a lane is a handful of points per gesture.
///
/// [`write`]: LaneCurve::write
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LaneCurve {
    points: Vec<(Tick, f32)>,
}

impl LaneCurve {
    pub fn from_points(mut points: Vec<(Tick, f32)>) -> Self {
        points.retain(|(_, v)| v.is_finite());
        points.sort_by_key(|(t, _)| *t);
        points.dedup_by_key(|(t, _)| *t);
        Self { points }
    }

    /// A lane from samples (a curve an engine drew, in ticks): straight runs
    /// kept to their ends, held ends to one point, all-neutral to nothing.
    pub fn from_samples(points: Vec<(Tick, f32)>, neutral: f32) -> Self {
        let mut curve = Self::from_points(points);
        curve.simplify(neutral);
        curve
    }

    pub fn points(&self) -> &[(Tick, f32)] {
        &self.points
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The value at `tick`.
    pub fn at(&self, tick: Tick, neutral: f32) -> f32 {
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else {
            return neutral;
        };
        if tick <= first.0 {
            return first.1;
        }
        if tick >= last.0 {
            return last.1;
        }
        let i = self.points.partition_point(|(t, _)| *t <= tick);
        let (t0, v0) = self.points[i - 1];
        let (t1, v1) = self.points[i];
        if t1 == t0 {
            return v1;
        }
        v0 + (v1 - v0) * ((tick - t0) as f32 / (t1 - t0) as f32)
    }

    /// Replaces `from..=to` with `value(tick)` sampled every `step` ticks
    /// (and at `to`), leaving everything outside as it was.
    pub fn write(
        &mut self,
        from: Tick,
        to: Tick,
        step: Tick,
        value: impl Fn(Tick) -> f32,
        neutral: f32,
    ) {
        let (from, to) = (from.min(to), from.max(to));
        let step = step.max(1);
        let before = self.at(from - 1, neutral);
        let after = self.at(to + 1, neutral);
        let mut out: Vec<(Tick, f32)> = self
            .points
            .iter()
            .copied()
            .filter(|(t, _)| *t < from - 1)
            .collect();
        out.push((from - 1, before));
        let mut t = from;
        while t < to {
            out.push((t, value(t)));
            t += step;
        }
        out.push((to, value(to)));
        out.push((to + 1, after));
        out.extend(self.points.iter().copied().filter(|(t, _)| *t > to + 1));
        out.retain(|(_, v)| v.is_finite());
        self.points = out;
        self.simplify(neutral);
    }

    /// Returns `from..=to` to the lane's neutral.
    pub fn erase(&mut self, from: Tick, to: Tick, neutral: f32) {
        self.write(from, to, Tick::MAX / 4, |_| neutral, neutral);
    }

    /// The same curve `delta` ticks later.
    pub fn shifted(&self, delta: Tick) -> Self {
        Self {
            points: self.points.iter().map(|(t, v)| (t + delta, *v)).collect(),
        }
    }

    fn simplify(&mut self, neutral: f32) {
        const EPS: f32 = 1e-5;
        let p = &mut self.points;
        p.sort_by_key(|(t, _)| *t);
        // Of two points at one tick, the later written wins.
        let mut dedup: Vec<(Tick, f32)> = Vec::with_capacity(p.len());
        for point in p.drain(..) {
            match dedup.last_mut() {
                Some(last) if last.0 == point.0 => *last = point,
                _ => dedup.push(point),
            }
        }
        // Straight runs to their ends.
        let mut kept: Vec<(Tick, f32)> = Vec::with_capacity(dedup.len());
        for i in 0..dedup.len() {
            if i == 0 || i + 1 == dedup.len() {
                kept.push(dedup[i]);
                continue;
            }
            let (t0, v0) = *kept.last().expect("the first is kept");
            let (t1, v1) = dedup[i];
            let (t2, v2) = dedup[i + 1];
            let on_line = v0 + (v2 - v0) * ((t1 - t0) as f32 / (t2 - t0).max(1) as f32);
            let scale = v0.abs().max(v1.abs()).max(v2.abs()).max(1.0);
            if (on_line - v1).abs() > EPS * scale {
                kept.push(dedup[i]);
            }
        }
        // Held ends need no second point saying the same thing.
        while kept.len() >= 2 && (kept[0].1 - kept[1].1).abs() <= EPS {
            kept.remove(0);
        }
        while kept.len() >= 2 && (kept[kept.len() - 1].1 - kept[kept.len() - 2].1).abs() <= EPS {
            kept.pop();
        }
        if kept.iter().all(|(_, v)| (v - neutral).abs() <= EPS) {
            kept.clear();
        }
        *p = kept;
    }
}

/// Which lane: one of the standard ones, or one a backend declared.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum LaneKey {
    Standard(SungLane),
    Extra { backend: String, id: String },
}

/// A vocal part's lanes, each backend's own lanes and settings, and the
/// state blob a backend (an external engine's window) keeps in the song.
///
/// Keyed by backend — the key a song names a singer's backend by
/// (`VocalVoice::engine`) — and **never pruned when the singer changes**:
/// switching to another voice and back finds every curve where it was.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VocalPart {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lanes: BTreeMap<SungLane, LaneCurve>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, BTreeMap<String, LaneCurve>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, BTreeMap<String, f32>>,
    /// Written as base64 text, not a list of numbers.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "blob::map"
    )]
    pub state: BTreeMap<String, Vec<u8>>,
}

impl VocalPart {
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
            && self.extra.values().all(BTreeMap::is_empty)
            && self.settings.values().all(BTreeMap::is_empty)
            && self.state.is_empty()
    }

    pub fn curve(&self, key: &LaneKey) -> Option<&LaneCurve> {
        match key {
            LaneKey::Standard(lane) => self.lanes.get(lane),
            LaneKey::Extra { backend, id } => self.extra.get(backend)?.get(id),
        }
    }

    pub fn set_curve(&mut self, key: &LaneKey, curve: Option<LaneCurve>) {
        let curve = curve.filter(|c| !c.is_empty());
        match key {
            LaneKey::Standard(lane) => match curve {
                Some(c) => {
                    self.lanes.insert(*lane, c);
                }
                None => {
                    self.lanes.remove(lane);
                }
            },
            LaneKey::Extra { backend, id } => {
                let lanes = self.extra.entry(backend.clone()).or_default();
                match curve {
                    Some(c) => {
                        lanes.insert(id.clone(), c);
                    }
                    None => {
                        lanes.remove(id);
                    }
                }
                if lanes.is_empty() {
                    self.extra.remove(backend);
                }
            }
        }
    }

    /// Every curve `delta` ticks later — the second half of a cut clip.
    pub fn shifted(&self, delta: Tick) -> Self {
        let mut out = self.clone();
        for c in out.lanes.values_mut() {
            *c = c.shifted(delta);
        }
        for lanes in out.extra.values_mut() {
            for c in lanes.values_mut() {
                *c = c.shifted(delta);
            }
        }
        out
    }
}

/// Byte blobs as base64 text, in files and on the wire.
pub(crate) mod blob {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub fn decode(text: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in text.bytes() {
            if c == b'=' {
                break;
            }
            let v = ALPHABET
                .iter()
                .position(|a| *a == c)
                .ok_or_else(|| format!("{:?} is not base64", c as char))?;
            acc = (acc << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        Ok(out)
    }

    pub mod map {
        use std::collections::BTreeMap;

        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub fn serialize<S: Serializer>(
            value: &BTreeMap<String, Vec<u8>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            value
                .iter()
                .map(|(k, v)| (k.clone(), super::encode(v)))
                .collect::<BTreeMap<String, String>>()
                .serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<BTreeMap<String, Vec<u8>>, D::Error> {
            BTreeMap::<String, String>::deserialize(deserializer)?
                .into_iter()
                .map(|(k, v)| {
                    super::decode(&v)
                        .map(|b| (k, b))
                        .map_err(serde::de::Error::custom)
                })
                .collect()
        }
    }

    pub mod option {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub fn serialize<S: Serializer>(
            value: &Option<Vec<u8>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            value.as_deref().map(super::encode).serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Vec<u8>>, D::Error> {
            Option::<String>::deserialize(deserializer)?
                .map(|s| super::decode(&s).map_err(serde::de::Error::custom))
                .transpose()
        }
    }

    /// `Option<Option<bytes>>` with its two `None`s told apart, as
    /// `crate::wire::nested` does.
    pub mod nested {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub fn serialize<S: Serializer>(
            value: &Option<Option<Vec<u8>>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            value
                .as_ref()
                .map(|inner| (inner.as_deref().map(super::encode),))
                .serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Option<Vec<u8>>>, D::Error> {
            Option::<(Option<String>,)>::deserialize(deserializer)?
                .map(|(inner,)| {
                    inner
                        .map(|s| super::decode(&s).map_err(serde::de::Error::custom))
                        .transpose()
                })
                .transpose()
        }
    }
}

/// The part `home`'s notes carry, made if there is none.
fn part_mut(data: &mut crate::note::NoteData) -> &mut VocalPart {
    data.vocal.get_or_insert_with(Default::default)
}

/// Drops a part left with nothing in it, so an undone first stroke leaves
/// the clip exactly as it was (and the song in the format it was).
fn tidy(data: &mut crate::note::NoteData) {
    if data.vocal.as_ref().is_some_and(|p| p.is_empty()) {
        data.vocal = None;
    }
}

/// Each of a set of notes gets a whole new [`NoteVoice`] (or none) — a
/// vibrato added, a handle dragged, phonemes typed, an option chosen — in
/// one undo step; a drag's moves merge into one.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetNoteVoices {
    home: NoteHome,
    ids: Vec<NoteId>,
    voices: Vec<Option<NoteVoice>>,
    previous: Option<Vec<Option<NoteVoice>>>,
    label: String,
}

impl SetNoteVoices {
    pub fn new(
        home: impl Into<NoteHome>,
        ids: Vec<NoteId>,
        voices: Vec<Option<NoteVoice>>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            home: home.into(),
            ids,
            voices,
            previous: None,
            label: label.into(),
        }
    }
}

impl Command for SetNoteVoices {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetNoteVoices(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.ids.len() != self.voices.len() {
            return Err(CommandError(format!(
                "{} voices for {} notes",
                self.voices.len(),
                self.ids.len()
            )));
        }
        if self.voices.iter().flatten().any(|v| !v.is_finite()) {
            return Err(CommandError("a note's voice is not a number".into()));
        }
        let data = notes_of(doc, self.home)?;
        for id in &self.ids {
            if data.notes.get(*id).is_none() {
                return Err(CommandError(format!("no note {id:?} here")));
            }
        }
        if self.previous.is_none() {
            self.previous = Some(
                self.ids
                    .iter()
                    .map(|id| data.notes[*id].voice.as_deref().cloned())
                    .collect(),
            );
        }
        for (id, voice) in self.ids.iter().zip(&self.voices) {
            if let Some(note) = data.notes.get_mut(*id) {
                note.voice = voice.clone().filter(|v| !v.is_empty()).map(Box::new);
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(SetNoteVoices::new(
                self.home,
                self.ids.clone(),
                previous.clone(),
                self.label.clone(),
            )),
            None => Box::new(NotApplied::new("changing how notes are sung")),
        }
    }

    fn label(&self) -> &str {
        &self.label
    }

    /// A handle dragged over the same notes is one step.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.home != self.home || next.ids != self.ids || next.label != self.label {
            return false;
        }
        self.voices.clone_from(&next.voices);
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + (self.ids.len() * 2) * (std::mem::size_of::<NoteVoice>() + 64)
            + self.label.len()
    }
}

/// Draws (or clears, with `None`) one lane of a part. A stroke's moves merge
/// into one step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetVocalLane {
    home: NoteHome,
    key: LaneKey,
    curve: Option<LaneCurve>,
    #[serde(with = "crate::wire::nested")]
    previous: Option<Option<LaneCurve>>,
    label: String,
}

impl SetVocalLane {
    pub fn new(home: impl Into<NoteHome>, key: LaneKey, curve: Option<LaneCurve>) -> Self {
        let label = match &key {
            LaneKey::Standard(lane) => format!("Draw {}", lane_spec(*lane).name.to_lowercase()),
            LaneKey::Extra { id, .. } => format!("Draw {id}"),
        };
        Self {
            home: home.into(),
            key,
            curve,
            previous: None,
            label,
        }
    }
}

impl Command for SetVocalLane {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetVocalLane(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self
            .curve
            .as_ref()
            .is_some_and(|c| c.points.iter().any(|(_, v)| !v.is_finite()))
        {
            return Err(CommandError("a lane value is not a number".into()));
        }
        let data = notes_of(doc, self.home)?;
        let part = part_mut(data);
        let before = part.curve(&self.key).cloned();
        self.previous.get_or_insert(before);
        part.set_curve(&self.key, self.curve.clone());
        tidy(data);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => {
                let mut back = SetVocalLane::new(self.home, self.key.clone(), previous.clone());
                back.label.clone_from(&self.label);
                Box::new(back)
            }
            None => Box::new(NotApplied::new("drawing a lane")),
        }
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.home != self.home || next.key != self.key {
            return false;
        }
        self.curve.clone_from(&next.curve);
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        let points = |c: &Option<LaneCurve>| c.as_ref().map_or(0, |c| c.points.len());
        std::mem::size_of::<Self>()
            + 12 * (points(&self.curve) + self.previous.as_ref().map_or(0, points))
            + self.label.len()
    }
}

/// A backend's whole-part settings, as a whole: every value it has for that
/// backend (an empty map clears them). A knob's drag merges into one step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetVocalSettings {
    home: NoteHome,
    backend: String,
    settings: BTreeMap<String, f32>,
    previous: Option<BTreeMap<String, f32>>,
}

impl SetVocalSettings {
    pub fn new(
        home: impl Into<NoteHome>,
        backend: impl Into<String>,
        settings: BTreeMap<String, f32>,
    ) -> Self {
        Self {
            home: home.into(),
            backend: backend.into(),
            settings,
            previous: None,
        }
    }
}

impl Command for SetVocalSettings {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetVocalSettings(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.settings.values().any(|v| !v.is_finite()) {
            return Err(CommandError("a setting is not a number".into()));
        }
        let data = notes_of(doc, self.home)?;
        let part = part_mut(data);
        let before = part
            .settings
            .get(&self.backend)
            .cloned()
            .unwrap_or_default();
        self.previous.get_or_insert(before);
        if self.settings.is_empty() {
            part.settings.remove(&self.backend);
        } else {
            part.settings
                .insert(self.backend.clone(), self.settings.clone());
        }
        tidy(data);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(SetVocalSettings::new(
                self.home,
                self.backend.clone(),
                previous.clone(),
            )),
            None => Box::new(NotApplied::new("changing a singer's settings")),
        }
    }

    fn label(&self) -> &str {
        "Change the singer's settings"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.home != self.home || next.backend != self.backend {
            return false;
        }
        self.settings.clone_from(&next.settings);
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + 40 * (self.settings.len() + self.previous.as_ref().map_or(0, BTreeMap::len))
    }
}

/// The state blob a backend keeps in the part (an external engine's window
/// stores whatever it likes here). Successive writes merge into one step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetVocalState {
    home: NoteHome,
    backend: String,
    #[serde(with = "blob::option")]
    state: Option<Vec<u8>>,
    #[serde(with = "blob::nested")]
    previous: Option<Option<Vec<u8>>>,
}

impl SetVocalState {
    pub fn new(
        home: impl Into<NoteHome>,
        backend: impl Into<String>,
        state: Option<Vec<u8>>,
    ) -> Self {
        Self {
            home: home.into(),
            backend: backend.into(),
            state,
            previous: None,
        }
    }
}

impl Command for SetVocalState {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetVocalState(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.home)?;
        let part = part_mut(data);
        let before = part.state.get(&self.backend).cloned();
        self.previous.get_or_insert(before);
        match &self.state {
            Some(state) => {
                part.state.insert(self.backend.clone(), state.clone());
            }
            None => {
                part.state.remove(&self.backend);
            }
        }
        tidy(data);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(SetVocalState::new(
                self.home,
                self.backend.clone(),
                previous.clone(),
            )),
            None => Box::new(NotApplied::new("changing a singer's state")),
        }
    }

    fn label(&self) -> &str {
        "Change the singer's state"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.home != self.home || next.backend != self.backend {
            return false;
        }
        self.state.clone_from(&next.state);
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.state.as_ref().map_or(0, Vec::len)
            + self
                .previous
                .as_ref()
                .and_then(Option::as_ref)
                .map_or(0, Vec::len)
    }
}

// ------------------------------------------------------------ the style
//
// What the Fontelle Singer's instrument panel sets for a whole track. A
// part's notes and lanes say how *this* phrase is sung; the style is how
// the track's singer sings when a note or a lane says nothing — the
// instrument's own defaults, the way a patch is a synthesiser's. On the
// channel, like the patch, so every clip on the track (and every clip
// drawn later) sings with it, and a duplicated track keeps it.

/// Auto vibrato: what a note with no vibrato of its own is given, when it
/// is long enough to want one (Synthesizer V's and Vocaloid's "vibrato on
/// notes longer than").
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AutoVibrato {
    /// Off keeps the settings for when it is turned back on.
    pub enabled: bool,
    pub vibrato: Vibrato,
    /// Seconds: a shorter note is left alone.
    pub min_length: f32,
}

impl AutoVibrato {
    /// Notes shorter than this keep still: a quaver at 120 bpm.
    pub const MIN_LENGTH: f32 = 0.25;
}

impl Default for AutoVibrato {
    fn default() -> Self {
        Self {
            enabled: false,
            vibrato: Vibrato::default(),
            min_length: Self::MIN_LENGTH,
        }
    }
}

/// The glide a note with none of its own is given from the note before.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AutoGlide {
    pub enabled: bool,
    pub portamento: Portamento,
}

impl Default for AutoGlide {
    fn default() -> Self {
        Self {
            enabled: false,
            portamento: Portamento {
                length: 0.06,
                position: 0.5,
                shape: fontelle_sing::GlideShape::S,
            },
        }
    }
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

/// How a vocal track's singer sings by default: the Fontelle Singer
/// panel's knobs (an engine's panel uses the same, for what it declares).
///
/// Every field defaults to "as the notes and the voice say", and only what
/// differs from that is written, so a track never touched writes nothing
/// and a song from before the panel opens as it sang.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VocalStyle {
    /// The Voice section: an amount added to a standard lane over the whole
    /// track (gender in cents, breathiness, tension, voicing) — on what the
    /// part draws, or as the lane where it draws nothing.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lanes: BTreeMap<SungLane, f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibrato: Option<AutoVibrato>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glide: Option<AutoGlide>,
    /// Multiplies every note's own consonant speed (its velocity).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub consonant_speed: f32,
    /// A backend's declared settings for the whole track, by backend then
    /// id; a part's own setting is sung over it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, BTreeMap<String, f32>>,
    /// A backend's own lanes' values where the part draws nothing, by
    /// backend then lane id (a DiffSinger bank's expressiveness, a speaker's
    /// share of the mix).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, BTreeMap<String, f32>>,
    /// A backend's per-note options for notes that choose none (the voice
    /// colour), by backend then option id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for VocalStyle {
    fn default() -> Self {
        Self {
            lanes: BTreeMap::new(),
            vibrato: None,
            glide: None,
            consonant_speed: 1.0,
            settings: BTreeMap::new(),
            extra: BTreeMap::new(),
            options: BTreeMap::new(),
        }
    }
}

// Every number in a style is finite (`SetVocalStyle` refuses anything
// else), as `NoteVoice`'s are.
impl Eq for VocalStyle {}

impl VocalStyle {
    /// Nothing set: the notes and the voice as they are.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether every number in it is a number.
    pub fn is_finite(&self) -> bool {
        let vibrato = self.vibrato.is_none_or(|a| {
            let v = a.vibrato;
            [
                v.depth,
                v.rate,
                v.delay,
                v.fade_in,
                v.fade_out,
                a.min_length,
            ]
            .iter()
            .all(|x| x.is_finite())
        });
        let glide = self
            .glide
            .is_none_or(|g| g.portamento.length.is_finite() && g.portamento.position.is_finite());
        vibrato
            && glide
            && self.consonant_speed.is_finite()
            && self.lanes.values().all(|v| v.is_finite())
            && self
                .settings
                .values()
                .chain(self.extra.values())
                .flat_map(BTreeMap::values)
                .all(|v| v.is_finite())
    }
}

/// A vocal track's whole style, set (one knob turned is the whole style
/// with that knob changed). A drag's writes merge into one undo step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetVocalStyle {
    channel: ChannelId,
    style: VocalStyle,
    previous: Option<VocalStyle>,
}

impl SetVocalStyle {
    pub fn new(channel: ChannelId, style: VocalStyle) -> Self {
        Self {
            channel,
            style,
            previous: None,
        }
    }
}

impl Command for SetVocalStyle {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetVocalStyle(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !self.style.is_finite() {
            return Err(CommandError("a singer setting is not a number".into()));
        }
        let Some(channel) = doc.channels.get_mut(self.channel) else {
            return Err(CommandError("that channel is not there".into()));
        };
        let before = std::mem::replace(&mut channel.vocal_style, self.style.clone());
        self.previous.get_or_insert(before);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(SetVocalStyle::new(self.channel, previous.clone())),
            None => Box::new(NotApplied::new("changing how a singer sings")),
        }
    }

    fn label(&self) -> &str {
        "Change the singer's instrument settings"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.channel != self.channel {
            return false;
        }
        self.style.clone_from(&next.style);
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        let size = |s: &VocalStyle| {
            48 * (s.lanes.len()
                + s.settings.values().map(BTreeMap::len).sum::<usize>()
                + s.extra.values().map(BTreeMap::len).sum::<usize>()
                + s.options.values().map(BTreeMap::len).sum::<usize>())
        };
        std::mem::size_of::<Self>() + size(&self.style) + self.previous.as_ref().map_or(0, size)
    }
}
