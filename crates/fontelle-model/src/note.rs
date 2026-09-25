use fontelle_types::Tick;

use crate::arena::Arena;
use fontelle_types::{ChannelId, NoteId};

/// Per-note pan, fine pitch, release, and two free modulation values are cheap to
/// store and route through the mod matrix — exactly the per-note character control
/// that makes sample-based writing expressive (TDD §10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Note {
    /// Relative to clip start.
    pub start: Tick,
    pub length: Tick,
    pub key: u8,
    pub velocity: u8,
    pub pan: i8,
    pub fine_pitch: i16,
    pub release: u8,
    pub mod_x: u8,
    pub mod_y: u8,
    /// A **slide note**: it starts no voice of its own, and instead bends
    /// whatever is already sounding on this channel to its pitch, over its own
    /// length (FL Studio's).
    ///
    /// That is what makes a glide something you *draw* rather than something
    /// you automate: a bass line with one slide in it is one extra note, laid
    /// over the note it bends, and the pitch it lands on is the one you can
    /// see. Portamento — the instrument's own `VoiceConfig::glide_time_s` — is
    /// the other end of the same machinery: that one glides *every* note, this
    /// one glides the notes you say.
    ///
    /// Defaulted, so a project written before slides existed opens as the
    /// ordinary notes it was made of.
    #[serde(default)]
    pub slide: bool,
    /// The instrument this note plays, when it is not the clip's own.
    ///
    /// *"clips can have multiple instruments, we just base our interactions
    /// on what your currently selected instrument in the channel rack is."*
    /// FL's pattern: one block on the arrangement, a drum part and a bass
    /// part inside it, and the roll showing whichever the rack has selected.
    /// `None` is the clip's channel ([`NoteData::channel`]) — so every note
    /// ever saved reads back exactly as it was written, and a clip that
    /// never mixes instruments never writes a channel per note.
    ///
    /// Resolved by [`Note::channel_or`]; the compiler, the roll and the
    /// caption all go through it rather than three readings of `None`.
    #[serde(default)]
    pub channel: Option<ChannelId>,
}

impl Note {
    /// The channel this note plays: its own, or `home` — the clip's — when
    /// it has none.
    pub fn channel_or(&self, home: ChannelId) -> ChannelId {
        self.channel.unwrap_or(home)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteData {
    /// The clip's **home** channel (TDD §10.3): what a note with no channel
    /// of its own plays, what the block is captioned with, and what a lane
    /// never has. A note may name another — see [`Note::channel`].
    pub channel: ChannelId,
    pub notes: Arena<NoteId, Note>,
}

impl NoteData {
    /// Every channel this clip plays: its own first, then each other channel
    /// a note names, once, in the order they appear.
    ///
    /// What the arrangement captions a block with, and what says whether a
    /// clip holds one instrument or several.
    pub fn channels(&self) -> Vec<ChannelId> {
        let mut channels = vec![self.channel];
        for note in self.notes.values() {
            let channel = note.channel_or(self.channel);
            if !channels.contains(&channel) {
                channels.push(channel);
            }
        }
        channels
    }

    /// The notes that play `channel` — the ones the roll shows and edits
    /// while the rack has that channel selected.
    pub fn notes_on(&self, channel: ChannelId) -> impl Iterator<Item = (NoteId, &Note)> + '_ {
        self.notes
            .iter()
            .filter(move |(_, note)| note.channel_or(self.channel) == channel)
    }
}

/// One of a note's editable properties, named so a command and a lane can talk
/// about the same thing.
///
/// `Note` has carried all of these since the model was scaffolded (TDD §10.4's
/// per-note character control) and only `velocity` had any way to reach it. The
/// piano roll's property lane is what needed the rest, but the *range* of each
/// one is a document fact, not a view one, so it lives here — one source of
/// truth for what a note may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NoteProperty {
    Velocity,
    Pan,
    /// Cents off the note's own pitch.
    FinePitch,
    Release,
    ModX,
    ModY,
}

impl NoteProperty {
    /// The lowest and highest the property may be.
    ///
    /// Pan stops at -127 rather than -128 so it is symmetric: an i8 reaching
    /// one step further left than it can right would put "hard left" and "hard
    /// right" at different distances from centre.
    pub fn range(self) -> (i32, i32) {
        match self {
            // Never zero: a note-on with velocity zero *is* a note-off in MIDI,
            // so a document that can hold one can silently lose a note.
            Self::Velocity => (1, 127),
            Self::Pan => (-127, 127),
            // An octave each way, in cents. Not a pitch bend's ±8192, which
            // is the number this began as and which — read as the cents it is
            // documented in — is ±81 semitones: a lane fifty pixels tall
            // would then move a note by a minor third per pixel, and the one
            // thing a control called *fine* pitch has to be able to do is
            // move a note a little. Nothing read the field until the audio
            // path did, so no project can hold a value this narrows away
            // that anybody chose.
            Self::FinePitch => (-1_200, 1_200),
            Self::Release | Self::ModX | Self::ModY => (0, 127),
        }
    }

    pub fn get(self, note: &Note) -> i32 {
        match self {
            Self::Velocity => i32::from(note.velocity),
            Self::Pan => i32::from(note.pan),
            Self::FinePitch => i32::from(note.fine_pitch),
            Self::Release => i32::from(note.release),
            Self::ModX => i32::from(note.mod_x),
            Self::ModY => i32::from(note.mod_y),
        }
    }

    /// Writes `value`, **clamped** into [`range`](Self::range).
    ///
    /// Clamping rather than refusing: a lane dragged off its own end is asking
    /// for the end, not for an error, and every caller would otherwise have to
    /// clamp for itself.
    pub fn set(self, note: &mut Note, value: i32) {
        let (min, max) = self.range();
        let value = value.clamp(min, max);
        match self {
            Self::Velocity => note.velocity = value as u8,
            Self::Pan => note.pan = value as i8,
            Self::FinePitch => note.fine_pitch = value as i16,
            Self::Release => note.release = value as u8,
            Self::ModX => note.mod_x = value as u8,
            Self::ModY => note.mod_y = value as u8,
        }
    }

    /// What a command made of this property calls itself.
    pub fn label(self) -> &'static str {
        match self {
            Self::Velocity => "velocity",
            Self::Pan => "pan",
            Self::FinePitch => "fine pitch",
            Self::Release => "release",
            Self::ModX => "mod X",
            Self::ModY => "mod Y",
        }
    }
}

/// Where a note edit lands: a clip on the arrangement, or a **prefab's** own
/// content (TDD §10.5).
///
/// A note command used to take a `ClipId` and there was nothing else it could
/// have taken — every set of notes in a project was a clip's. A prefab is the
/// second kind of place notes live, and it is not a clip: it has no row, no
/// start and no length, and the whole point of it is that many clips show it
/// at once.
///
/// **Every note command takes `impl Into<NoteHome>`**, so a `ClipId` still
/// reads as one and every caller written before prefabs existed still says
/// what it meant. Which of the two the piano roll sends is decided in one
/// place — [`Project::note_home`] — because "editing an instance edits the
/// prefab" is a rule about the document, not about a canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NoteHome {
    Clip(fontelle_types::ClipId),
    Prefab(fontelle_types::PrefabId),
}

impl From<fontelle_types::ClipId> for NoteHome {
    fn from(clip: fontelle_types::ClipId) -> Self {
        Self::Clip(clip)
    }
}

impl From<fontelle_types::PrefabId> for NoteHome {
    fn from(prefab: fontelle_types::PrefabId) -> Self {
        Self::Prefab(prefab)
    }
}
