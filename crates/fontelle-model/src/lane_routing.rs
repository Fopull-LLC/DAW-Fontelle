//! Lane-style routing: a project in which every lane owns a mixer track.
//!
//! `docs/ux-routing-and-learning-plan.md` §1. A project is **rack-style**
//! (FL's, and every project written before this) or **lane-style**
//! (Reaper's, Logic's). In rack-style a channel's route chip says where its
//! sound goes and an audio clip names its own track; in lane-style each lane
//! has a track and what is on the lane plays through it.
//!
//! Everything lane-style keeps is in one place, [`LaneRouting`], and is read
//! only through [`Project::channel_route`] and [`Project::clip_route`] — so a
//! rack-style project never looks at it, and a lane-style one never obeys a
//! route chip. Ty's rule above all: *the two must never interfere.*
//!
//! **One instrument, one lane.** An instrument is one running copy with one
//! output, so it cannot sound through two lanes' tracks at once. Ty chose
//! (2026-09-30, over running a copy per lane — *"i definitely do NOT want to
//! have duplicate copies of instruments"*): the first lane an instrument's
//! clip lands on is the lane it **claims**; a clip of it drawn on another
//! lane is a [conflict](lane_conflicts) the window asks about — move the
//! instrument there, or duplicate the channel.

use fontelle_types::{ChannelId, ClipId, LaneId, MixerTrackId};

use crate::command::{Command, CommandError};
use crate::{ClipSource, MixerTrack, Project};

/// Which way a project routes its sound.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoutingMode {
    /// A channel's route chip decides; lanes are only places to put clips.
    #[default]
    Rack,
    /// Each lane owns a mixer track and plays what is on it through it.
    Lane,
}

/// What a lane-style project keeps. Pairs rather than maps so the file stays
/// JSON with ids as values, the way every other id in it is written.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LaneRouting {
    pub mode: RoutingMode,
    /// Each lane's own mixer track. Kept when the project goes back to
    /// rack-style (Ty: *keeps the mixer tracks*), so coming back finds them.
    #[serde(default)]
    pub tracks: Vec<(LaneId, MixerTrackId)>,
    /// The lane each instrument claimed.
    #[serde(default)]
    pub claims: Vec<(ChannelId, LaneId)>,
}

impl LaneRouting {
    fn track_of(&self, lane: LaneId) -> Option<MixerTrackId> {
        self.tracks
            .iter()
            .find(|(l, _)| *l == lane)
            .map(|(_, t)| *t)
    }

    fn claim_of(&self, channel: ChannelId) -> Option<LaneId> {
        self.claims
            .iter()
            .find(|(c, _)| *c == channel)
            .map(|(_, l)| *l)
    }

    fn set_track(&mut self, lane: LaneId, track: Option<MixerTrackId>) {
        self.tracks.retain(|(l, _)| *l != lane);
        if let Some(track) = track {
            self.tracks.push((lane, track));
        }
    }

    fn set_claim(&mut self, channel: ChannelId, lane: Option<LaneId>) {
        self.claims.retain(|(c, _)| *c != channel);
        if let Some(lane) = lane {
            self.claims.push((channel, lane));
        }
    }
}

impl Project {
    /// The mixer track `lane` owns in lane-style, while it exists.
    pub fn lane_track(&self, lane: LaneId) -> Option<MixerTrackId> {
        self.lane_routing
            .track_of(lane)
            .filter(|track| self.mixer.tracks.get(*track).is_some())
    }

    /// The lane `channel` claimed in lane-style.
    pub fn claimed_lane(&self, channel: ChannelId) -> Option<LaneId> {
        self.lane_routing.claim_of(channel)
    }

    /// **Where `channel`'s sound goes**, in whichever mode the project is in.
    /// `None` is the master. The one question the engine asks of a channel.
    pub fn channel_route(&self, channel: ChannelId) -> Option<MixerTrackId> {
        match self.lane_routing.mode {
            RoutingMode::Rack => self.channels.get(channel)?.mixer_track,
            RoutingMode::Lane => {
                let lane = self.claimed_lane(channel)?;
                self.lanes.get(lane)?;
                self.lane_track(lane)
            }
        }
    }

    /// Where an audio clip's sound goes: its own track in rack-style, its
    /// lane's in lane-style. `None` is the master, and the answer for a clip
    /// that is not audio.
    pub fn clip_route(&self, clip: ClipId) -> Option<MixerTrackId> {
        let clip = self.clips.get(clip)?;
        let ClipSource::Audio(data) = &clip.source else {
            return None;
        };
        match self.lane_routing.mode {
            RoutingMode::Rack => data.mixer_track,
            RoutingMode::Lane => self.lane_track(clip.lane),
        }
    }

    /// The instrument a notes clip plays, read through a prefab if it follows
    /// one.
    fn notes_channel(&self, clip: ClipId) -> Option<ChannelId> {
        match self.clip_source(clip)?.as_ref() {
            ClipSource::Notes(data) => Some(data.channel),
            _ => None,
        }
    }

    /// Whether `channel` has a clip on `lane`.
    fn has_clip_on(&self, channel: ChannelId, lane: LaneId) -> bool {
        self.clips
            .iter()
            .any(|(id, clip)| clip.lane == lane && self.notes_channel(id) == Some(channel))
    }
}

/// A clip of an instrument on a lane other than the one it claimed — what
/// the window asks about (move the instrument, or duplicate the channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneConflict {
    pub channel: ChannelId,
    /// The lane it plays through.
    pub claimed: LaneId,
    /// The lane the stray clip is on.
    pub lane: LaneId,
    pub clip: ClipId,
}

/// Every clip sitting on a lane its instrument has not claimed, in the
/// arena's order. Empty in rack-style.
///
/// A claim whose lane holds none of the instrument's clips is not a claim
/// worth defending — [`LaneUpkeep`] moves it — so it raises no conflict.
pub fn lane_conflicts(project: &Project) -> Vec<LaneConflict> {
    if project.lane_routing.mode != RoutingMode::Lane {
        return Vec::new();
    }
    project
        .clips
        .iter()
        .filter_map(|(id, clip)| {
            let channel = project.notes_channel(id)?;
            let claimed = project.claimed_lane(channel)?;
            (claimed != clip.lane
                && project.lanes.get(claimed).is_some()
                && project.has_clip_on(channel, claimed))
            .then_some(LaneConflict {
                channel,
                claimed,
                lane: clip.lane,
                clip: id,
            })
        })
        .collect()
}

/// Switches a project between rack-style and lane-style.
///
/// **Resets the routes and keeps the tracks** (Ty's answer B): every
/// channel's route chip and every audio clip's track go back to the master,
/// and nothing is converted — a conversion that could not be exact would be
/// the "why did this go there" the modes exist to avoid. In lane-style the
/// lanes' tracks and claims are then made by [`LaneUpkeep`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetRoutingMode {
    mode: RoutingMode,
    /// What it replaced, once applied — what the undo puts back.
    saved: Option<SavedRoutes>,
    /// Set on the inverse: this puts `saved` back rather than switching.
    #[serde(default)]
    undoing: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SavedRoutes {
    mode: RoutingMode,
    channels: Vec<(ChannelId, Option<MixerTrackId>)>,
    clips: Vec<(ClipId, Option<MixerTrackId>)>,
}

impl SetRoutingMode {
    pub fn new(mode: RoutingMode) -> Self {
        Self {
            mode,
            saved: None,
            undoing: false,
        }
    }
}

impl Command for SetRoutingMode {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetRoutingMode(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.undoing {
            let Some(saved) = &self.saved else {
                return Err(CommandError("nothing to put back".into()));
            };
            doc.lane_routing.mode = saved.mode;
            for (id, track) in &saved.channels {
                if let Some(channel) = doc.channels.get_mut(*id) {
                    channel.mixer_track = *track;
                }
            }
            for (id, track) in &saved.clips {
                if let Some(clip) = doc.clips.get_mut(*id)
                    && let ClipSource::Audio(data) = &mut clip.source
                {
                    data.mixer_track = *track;
                }
            }
            return Ok(());
        }
        let saved = SavedRoutes {
            mode: doc.lane_routing.mode,
            channels: doc
                .channels
                .iter()
                .map(|(id, channel)| (id, channel.mixer_track))
                .collect(),
            clips: doc
                .clips
                .iter()
                .filter_map(|(id, clip)| match &clip.source {
                    ClipSource::Audio(data) => Some((id, data.mixer_track)),
                    _ => None,
                })
                .collect(),
        };
        doc.lane_routing.mode = self.mode;
        for (_, channel) in doc.channels.iter_mut() {
            channel.mixer_track = None;
        }
        for (_, clip) in doc.clips.iter_mut() {
            if let ClipSource::Audio(data) = &mut clip.source {
                data.mixer_track = None;
            }
        }
        self.saved = Some(saved);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        let mut inverse = self.clone();
        inverse.undoing = !self.undoing;
        Box::new(inverse)
    }

    fn label(&self) -> &str {
        match self.mode {
            RoutingMode::Rack => "Switch to rack-style routing",
            RoutingMode::Lane => "Switch to lane-style routing",
        }
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.saved.as_ref().map_or(0, |s| {
                s.channels.len() * std::mem::size_of::<(ChannelId, Option<MixerTrackId>)>()
                    + s.clips.len() * std::mem::size_of::<(ClipId, Option<MixerTrackId>)>()
            })
    }
}

/// What lane-style keeps true after an edit: **every lane has a track, and
/// every instrument with clips has claimed a lane that holds one of them.**
///
/// One command computed from the document ([`LaneUpkeep::due`]) rather than
/// a step inside every command that makes a lane — an import, a take, a
/// `.mid` of twelve parts, a plain *Add lane* all make rows, and each would
/// have had to learn about modes. The session runs it after a local edit and
/// folds it into that edit's undo entry, so one Ctrl+Z takes back both.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LaneUpkeep {
    tracks: Vec<MadeTrack>,
    /// `(channel, what it claimed before, what it claims now)`.
    claims: Vec<(ChannelId, Option<LaneId>, LaneId)>,
    #[serde(default)]
    undoing: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct MadeTrack {
    lane: LaneId,
    name: String,
    /// The id and colour it was given the first time, so a redo and a peer
    /// make the same track.
    track: Option<MixerTrackId>,
    color: Option<[u8; 4]>,
    /// What the lane's entry named before — a track since deleted, or none.
    previous: Option<MixerTrackId>,
}

impl LaneUpkeep {
    /// What is due in `project`, or `None` when nothing is — always `None`
    /// in rack-style.
    pub fn due(project: &Project) -> Option<Self> {
        if project.lane_routing.mode != RoutingMode::Lane {
            return None;
        }
        let tracks: Vec<MadeTrack> = project
            .lane_ids()
            .into_iter()
            .filter(|lane| project.lane_track(*lane).is_none())
            .map(|lane| MadeTrack {
                lane,
                name: project.lanes[lane].name.clone(),
                track: None,
                color: None,
                previous: project.lane_routing.track_of(lane),
            })
            .collect();
        // Each instrument's clips, earliest first by where they start and
        // then by which row is higher — the first one drawn, as far as the
        // document can tell.
        let order: std::collections::HashMap<LaneId, usize> = project
            .lane_ids()
            .into_iter()
            .enumerate()
            .map(|(at, lane)| (lane, at))
            .collect();
        let mut first: Vec<(ChannelId, (i64, usize), LaneId)> = Vec::new();
        for (id, clip) in project.clips.iter() {
            let Some(channel) = project.notes_channel(id) else {
                continue;
            };
            let key = (
                clip.start,
                order.get(&clip.lane).copied().unwrap_or(usize::MAX),
            );
            match first.iter_mut().find(|(c, _, _)| *c == channel) {
                Some(entry) if key < entry.1 => *entry = (channel, key, clip.lane),
                Some(_) => {}
                None => first.push((channel, key, clip.lane)),
            }
        }
        let claims: Vec<(ChannelId, Option<LaneId>, LaneId)> = first
            .into_iter()
            .filter_map(|(channel, _, lane)| {
                let current = project.claimed_lane(channel);
                let holds = current.is_some_and(|claimed| {
                    project.lanes.get(claimed).is_some() && project.has_clip_on(channel, claimed)
                });
                (!holds).then_some((channel, current, lane))
            })
            .collect();
        (!tracks.is_empty() || !claims.is_empty()).then_some(Self {
            tracks,
            claims,
            undoing: false,
        })
    }
}

impl Command for LaneUpkeep {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::LaneUpkeep(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.undoing {
            for (channel, previous, _) in self.claims.iter().rev() {
                doc.lane_routing.set_claim(*channel, *previous);
            }
            for made in self.tracks.iter().rev() {
                if let Some(track) = made.track {
                    doc.mixer.tracks.remove(track);
                }
                doc.lane_routing.set_track(made.lane, made.previous);
            }
            return Ok(());
        }
        for made in &mut self.tracks {
            let mut track = MixerTrack::new(made.name.clone());
            track.color = *made
                .color
                .get_or_insert_with(|| doc.mixer.next_track_color());
            track.output = doc.mixer.master;
            let id = match made.track {
                Some(id) => {
                    if !doc.mixer.tracks.insert_at(id, track) {
                        return Err(CommandError("that mixer track id is taken".into()));
                    }
                    id
                }
                None => doc.mixer.tracks.insert(track),
            };
            made.track = Some(id);
            doc.lane_routing.set_track(made.lane, Some(id));
        }
        for (channel, _, lane) in &self.claims {
            doc.lane_routing.set_claim(*channel, Some(*lane));
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        let mut inverse = self.clone();
        inverse.undoing = !self.undoing;
        Box::new(inverse)
    }

    fn label(&self) -> &str {
        "Give lanes their tracks"
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
                .tracks
                .iter()
                .map(|t| std::mem::size_of::<MadeTrack>() + t.name.len())
                .sum::<usize>()
            + self.claims.len() * std::mem::size_of::<(ChannelId, Option<LaneId>, LaneId)>()
    }
}

/// Points a notes clip at another instrument — what *duplicate the channel*
/// does with the clip that raised the question.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetClipChannel {
    clip: ClipId,
    channel: ChannelId,
    previous: Option<ChannelId>,
}

impl SetClipChannel {
    pub fn new(clip: ClipId, channel: ChannelId) -> Self {
        Self {
            clip,
            channel,
            previous: None,
        }
    }
}

impl Command for SetClipChannel {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetClipChannel(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if doc.channels.get(self.channel).is_none() {
            return Err(CommandError(format!("no channel {:?}", self.channel)));
        }
        let Some(clip) = doc.clips.get_mut(self.clip) else {
            return Err(CommandError(format!("no clip {:?}", self.clip)));
        };
        let ClipSource::Notes(data) = &mut clip.source else {
            return Err(CommandError("only a notes clip plays an instrument".into()));
        };
        self.previous = Some(std::mem::replace(&mut data.channel, self.channel));
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetClipChannel::new(self.clip, previous)),
            None => Box::new(crate::commands::NotApplied::new("pointing a clip")),
        }
    }

    fn label(&self) -> &str {
        "Point clip at channel"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}
