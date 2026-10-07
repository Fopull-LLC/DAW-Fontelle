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
