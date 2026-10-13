//! Vocal song files in the document: a file's parts brought in as vocal
//! clips ([`ImportVocal`]), and a vocal track **frozen** to audio and back
//! ([`FreezeVocal`], [`UnfreezeVocal`]) — `docs/vocal-plan.md` §3.
//!
//! Each is **one command**, so one undo step and one edit on the wire, for
//! the reason [`ImportParts`](crate::ImportParts) is: what it adds has to
//! name the ids it is minting, and a redo has to put them back under the
//! same ids.

use fontelle_types::{AudioClipData, ChannelId, ClipId, InstrumentKind, LaneId, Tick};

use crate::arena::Arena;
use crate::channel::Channel;
use crate::clip::{Clip, ClipSource};
use crate::command::{Command, CommandError};
use crate::commands::{NotApplied, open_rows};
use crate::lane::Lane;
use crate::note::{Note, NoteData};
use crate::project::{Project, TempoMap, TempoSegment};
use crate::vocal::{VocalPart, VocalVoice};

/// A vocal row's colour.
const VOCAL_COLOR: [u8; 4] = [0xd0, 0x6f, 0xa8, 0xff];
/// A frozen render's row's colour: the audio rows'.
const FROZEN_COLOR: [u8; 4] = [0x5f, 0xa8, 0x88, 0xff];
const ROW_HEIGHT: f32 = 32.0;

/// One sung part of a file, as the clip it becomes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VocalClipImport {
    /// What the file called it: a new track's (and row's) name.
    pub name: String,
    /// Where the clip starts on the song, and how long it is.
    pub start: Tick,
    pub length: Tick,
    /// From the clip's start.
    pub notes: Vec<Note>,
    /// Lanes from the clip's start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocal: Option<Box<VocalPart>>,
}

/// Where a file's parts land.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum VocalLanding {
    /// A vocal track and a row of its own per part, the rows as one block
    /// from row `at` (past the bottom when `None`), each sung by `voice`.
    NewTracks {
        at: Option<usize>,
        voice: Option<VocalVoice>,
    },
    /// Onto a vocal track that is already there, on row `lane`: the parts'
    /// clips, no track or row made.
    Onto { channel: ChannelId, lane: LaneId },
}

/// What one part turned into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MadeVocal {
    pub channel: ChannelId,
    pub lane: LaneId,
    pub clip: ClipId,
    /// Whether the channel and the row were made by the import (and go with
    /// its undo).
    pub new_track: bool,
}

/// A vocal song file's parts brought into the song, with the file's tempo
/// and bar when asked ([`with_tempo`](Self::with_tempo)) — one undo step.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportVocal {
    parts: Vec<VocalClipImport>,
    landing: VocalLanding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tempo: Option<(Vec<TempoSegment>, u32)>,
    /// What it made, so a redo mints the same ids.
    made: Vec<MadeVocal>,
    /// The song's tempo and bar before, when this changed them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_tempo: Option<(Vec<TempoSegment>, u32)>,
    label: String,
}

impl ImportVocal {
    pub fn new(
        what: impl std::fmt::Display,
        parts: Vec<VocalClipImport>,
        landing: VocalLanding,
    ) -> Self {
        Self {
            label: format!("Import {what}"),
            parts: parts.into_iter().filter(|p| !p.notes.is_empty()).collect(),
            landing,
            tempo: None,
            made: Vec::new(),
            previous_tempo: None,
        }
    }

    /// Also sets the song's tempo map and bar (beats a bar) to the file's.
    pub fn with_tempo(mut self, segments: Vec<TempoSegment>, beats_per_bar: u32) -> Self {
        self.tempo = Some((segments, beats_per_bar));
        self
    }

    /// What it made, once applied.
    pub fn made(&self) -> &[MadeVocal] {
        &self.made
    }
}

fn notes_clip(part: &VocalClipImport, channel: ChannelId, lane: LaneId) -> Clip {
    let mut notes = Arena::default();
    for note in &part.notes {
        notes.insert(note.clone());
    }
    Clip {
        name: None,
        lane,
        start: part.start,
        length: part.length.max(crate::MIN_CLIP_LENGTH),
        source: ClipSource::Notes(NoteData {
            channel,
            notes,
            vocal: part.vocal.clone().filter(|v| !v.is_empty()),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    }
}

impl Command for ImportVocal {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::ImportVocal(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.parts.is_empty() {
            return Err(CommandError(
                "there are no notes in that file to sing".into(),
            ));
        }
        let redoing = !self.made.is_empty();
        let mut made = Vec::with_capacity(self.parts.len());
        match &self.landing {
            VocalLanding::Onto { channel, lane } => {
                let sings = doc
                    .channels
                    .get(*channel)
                    .is_some_and(|c| c.instrument == Some(InstrumentKind::Vocal));
                if !sings {
                    return Err(CommandError("that track is not a vocal track".into()));
                }
                if doc.lanes.get(*lane).is_none() {
                    return Err(CommandError("that row is not there".into()));
                }
                for (index, part) in self.parts.iter().enumerate() {
                    let clip = notes_clip(part, *channel, *lane);
                    let id = if redoing {
                        let id = self.made[index].clip;
                        if !doc.clips.insert_at(id, clip) {
                            return Err(CommandError("that clip id is taken".into()));
                        }
                        id
                    } else {
                        doc.clips.insert(clip)
                    };
                    made.push(MadeVocal {
                        channel: *channel,
                        lane: *lane,
                        clip: id,
                        new_track: false,
                    });
                }
            }
            VocalLanding::NewTracks { at, voice } => {
                let mut order = open_rows(doc, *at, self.parts.len());
                for (index, part) in self.parts.iter().enumerate() {
                    let previous = redoing.then(|| self.made[index]);
                    let channel = Channel {
                        preset: None,
                        name: part.name.clone(),
                        color: VOCAL_COLOR,
                        instrument: Some(InstrumentKind::Vocal),
                        mixer_track: None,
                        patch_data: None,
                        plugin: None,
                        pan: 0.0,
                        muted: false,
                        soloed: false,
                        named_keys: false,
                        ab: Default::default(),
                        gain_db: 0.0,
                        vocal: voice.clone(),
                        vocal_style: Default::default(),
                    };
                    let channel_id = match previous {
                        Some(ids) => {
                            if !doc.channels.insert_at(ids.channel, channel) {
                                return Err(CommandError("that channel id is taken".into()));
                            }
                            ids.channel
                        }
                        None => doc.channels.insert(channel),
                    };
                    let lane = Lane {
                        name: part.name.clone(),
                        height: ROW_HEIGHT,
                        color: VOCAL_COLOR,
                        muted: false,
                        locked: false,
                        soloed: false,
                        order,
                    };
                    order = order.saturating_add(1);
                    let lane_id = match previous {
                        Some(ids) => {
                            if !doc.lanes.insert_at(ids.lane, lane) {
                                return Err(CommandError("that row id is taken".into()));
                            }
                            ids.lane
                        }
                        None => doc.lanes.insert(lane),
                    };
                    let clip = notes_clip(part, channel_id, lane_id);
                    let clip_id = match previous {
                        Some(ids) => {
                            if !doc.clips.insert_at(ids.clip, clip) {
                                return Err(CommandError("that clip id is taken".into()));
                            }
                            ids.clip
                        }
                        None => doc.clips.insert(clip),
                    };
                    made.push(MadeVocal {
                        channel: channel_id,
                        lane: lane_id,
                        clip: clip_id,
                        new_track: true,
                    });
                }
            }
        }
        if let Some((segments, beats)) = &self.tempo {
            let before = (doc.tempo_map.segments().to_vec(), doc.beats_per_bar);
            let rate = doc.tempo_map.sample_rate_hz();
            doc.tempo_map = TempoMap::from_segments(segments.clone(), rate);
            doc.beats_per_bar = (*beats).clamp(1, 16);
            self.previous_tempo.get_or_insert(before);
        }
        self.made = made;
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.made.is_empty() {
            return Box::new(NotApplied::new("importing a vocal file"));
        }
        Box::new(UnimportVocal {
            made: self.made.clone(),
            tempo: self.previous_tempo.clone(),
            label: self.label.clone(),
        })
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .parts
                .iter()
                .map(|p| p.notes.len() * std::mem::size_of::<Note>() + 256)
                .sum::<usize>()
    }
}

/// [`ImportVocal`]'s inverse: what it made, taken back out, and the tempo
/// as it was.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnimportVocal {
    made: Vec<MadeVocal>,
    tempo: Option<(Vec<TempoSegment>, u32)>,
    label: String,
}

impl Command for UnimportVocal {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::UnimportVocal(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        for ids in self.made.iter().rev() {
            doc.clips.remove(ids.clip);
            if ids.new_track {
                doc.lanes.remove(ids.lane);
                doc.channels.remove(ids.channel);
            }
        }
        if let Some((segments, beats)) = &self.tempo {
            let rate = doc.tempo_map.sample_rate_hz();
            doc.tempo_map = TempoMap::from_segments(segments.clone(), rate);
            doc.beats_per_bar = *beats;
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        // Redo goes through `History::redo`, which re-applies the import.
        Box::new(NotApplied::new("taking a vocal import back out"))
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.made.len() * std::mem::size_of::<MadeVocal>()
    }
}

// ------------------------------------------------------------------ freeze

/// A vocal track frozen: its render's clip and the row it is on.
///
/// While a track is frozen its part is kept whole and **not sung**
/// (`fontelle_vocal::phrases_with` asks [`Project::is_frozen`]); the render
/// plays instead, through the track's own mixer strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FrozenVocal {
    pub channel: ChannelId,
    pub clip: ClipId,
    pub lane: LaneId,
}

impl Project {
    /// Whether `channel` is a frozen vocal track.
    pub fn is_frozen(&self, channel: ChannelId) -> bool {
        self.frozen.iter().any(|f| f.channel == channel)
    }

    /// `channel`'s freeze, if it is frozen.
    pub fn frozen_of(&self, channel: ChannelId) -> Option<FrozenVocal> {
        self.frozen.iter().find(|f| f.channel == channel).copied()
    }
}

/// Freezes a vocal track: its render (`data`, already written into the
/// project's `renders/`) as an audio clip on a row of its own at row
/// `at_row`, named `row_name`, and the track marked frozen — one undo step.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FreezeVocal {
    channel: ChannelId,
    row_name: String,
    data: AudioClipData,
    start: Tick,
    length: Tick,
    at_row: usize,
    /// The row and clip it made, so a redo (or an undone unfreeze) mints
    /// the same ids.
    made: Option<(LaneId, ClipId)>,
}

impl FreezeVocal {
    pub fn new(
        channel: ChannelId,
        row_name: impl Into<String>,
        data: AudioClipData,
        start: Tick,
        length: Tick,
        at_row: usize,
    ) -> Self {
        Self {
            channel,
            row_name: row_name.into(),
            data,
            start,
            length,
            at_row,
            made: None,
        }
    }

    /// The render's clip, once applied.
    pub fn clip(&self) -> Option<ClipId> {
        self.made.map(|(_, clip)| clip)
    }
}

impl Command for FreezeVocal {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::FreezeVocal(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let sings = doc
            .channels
            .get(self.channel)
            .is_some_and(|c| c.instrument == Some(InstrumentKind::Vocal));
        if !sings {
            return Err(CommandError("only a vocal track freezes".into()));
        }
        if doc.is_frozen(self.channel) {
            return Err(CommandError("that track is already frozen".into()));
        }
        let lane = Lane {
            name: self.row_name.clone(),
            height: ROW_HEIGHT,
            color: FROZEN_COLOR,
            muted: false,
            locked: false,
            soloed: false,
            order: 0,
        };
        let clip = |lane: LaneId| Clip {
            name: None,
            lane,
            start: self.start,
            length: self.length.max(crate::MIN_CLIP_LENGTH),
            source: ClipSource::Audio(self.data.clone()),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        };
        let (lane_id, clip_id) = match self.made {
            Some((lane_id, clip_id)) => {
                // An undone unfreeze that left the row (something else was
                // put on it) finds it still there.
                if doc.lanes.get(lane_id).is_none() {
                    let mut lane = lane;
                    lane.order = open_rows(doc, Some(self.at_row), 1);
                    if !doc.lanes.insert_at(lane_id, lane) {
                        return Err(CommandError("that row id is taken".into()));
                    }
                }
                if !doc.clips.insert_at(clip_id, clip(lane_id)) {
                    return Err(CommandError("that clip id is taken".into()));
                }
                (lane_id, clip_id)
            }
            None => {
                let mut lane = lane;
                lane.order = open_rows(doc, Some(self.at_row), 1);
                let lane_id = doc.lanes.insert(lane);
                (lane_id, doc.clips.insert(clip(lane_id)))
            }
        };
        doc.frozen.push(FrozenVocal {
            channel: self.channel,
            clip: clip_id,
            lane: lane_id,
        });
        self.made = Some((lane_id, clip_id));
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.made {
            Some(_) => Box::new(UnfreezeVocal::new(self.channel)),
            None => Box::new(NotApplied::new("freezing a vocal track")),
        }
    }

    fn label(&self) -> &str {
        "Freeze vocal track"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.row_name.len()
    }
}

/// Unfreezes a vocal track: its render's clip taken away (and its row,
/// when nothing else is on it), and the part sung again — one undo step.
/// The render's file stays in `renders/`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnfreezeVocal {
    channel: ChannelId,
    /// What it took, so the inverse freezes it back under the same ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    taken: Option<Box<FreezeVocal>>,
}

impl UnfreezeVocal {
    pub fn new(channel: ChannelId) -> Self {
        Self {
            channel,
            taken: None,
        }
    }
}

impl Command for UnfreezeVocal {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::UnfreezeVocal(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(frozen) = doc.frozen_of(self.channel) else {
            return Err(CommandError("that track is not frozen".into()));
        };
        let at_row = doc
            .lane_ids()
            .iter()
            .position(|id| *id == frozen.lane)
            .unwrap_or(0);
        let row_name = doc
            .lanes
            .get(frozen.lane)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        let removed = doc.clips.remove(frozen.clip);
        let row_empty = !doc.clips.values().any(|c| c.lane == frozen.lane);
        if row_empty {
            doc.lanes.remove(frozen.lane);
        }
        doc.frozen.retain(|f| f.channel != self.channel);
        self.taken = removed.and_then(|clip| match clip.source {
            ClipSource::Audio(data) => Some(Box::new(FreezeVocal {
                channel: self.channel,
                row_name,
                data,
                start: clip.start,
                length: clip.length,
                at_row,
                made: Some((frozen.lane, frozen.clip)),
            })),
            _ => None,
        });
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.taken {
            Some(freeze) => Box::new((**freeze).clone()),
            None => Box::new(NotApplied::new("unfreezing a vocal track")),
        }
    }

    fn label(&self) -> &str {
        "Unfreeze vocal track"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<FreezeVocal>()
    }
}
