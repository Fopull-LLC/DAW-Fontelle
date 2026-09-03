//! The command set the first usable version needs (TDD §10.6).
//!
//! Every one of these is inverse-based rather than snapshot-based, and the
//! test that matters is in `tests/commands.rs`: apply a command, apply its
//! inverse, and the serialised document is exactly where it started —
//! including the ids, which is what [`crate::Arena`] exists for.
//!
//! Three rules hold throughout:
//!
//! - **A command that cannot do its whole job does nothing.** Half an edit is
//!   worse than a refused one, because its inverse then does not describe it.
//! - **Nothing is clamped.** A drag that would push a note off the keyboard or
//!   behind the start of its clip is refused: clamping is not invertible, so
//!   undo would put the note where the clamp left it rather than where it was.
//!   Bounding the gesture is the caller's job, and the caller is the one that
//!   knows what the pointer is doing.
//! - **A creating command remembers what it created**, and puts it back under
//!   the same id on a redo. Anything else breaks the command above it in the
//!   history.

use fontelle_types::{ChannelId, ClipId, LaneId, MixerTrackId, NoteId, Tick};

use crate::channel::Channel;
use crate::clip::{Clip, ClipSource};
use crate::command::{Command, CommandError};
use crate::arena::Arena;
use crate::lane::Lane;
use crate::mixer::{MixerTrack, Send};
use crate::note::{Note, NoteData, NoteProperty};
use crate::project::Project;

/// The inverse of a command that has not been applied yet.
///
/// `invert` can be called at any time by the type system's reckoning, but only
/// means something after `apply` has recorded what to undo. Rather than
/// silently doing nothing — which would look like a successful undo that lost
/// an edit — this says so.
struct NotApplied(&'static str);

impl Command for NotApplied {
    fn apply(&mut self, _doc: &mut Project) -> Result<(), CommandError> {
        Err(CommandError(format!(
            "cannot undo {}: it has not been applied",
            self.0
        )))
    }
    fn invert(&self) -> Box<dyn Command> {
        Box::new(NotApplied(self.0))
    }
    fn label(&self) -> &str {
        self.0
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

fn no_clip(clip: ClipId) -> CommandError {
    CommandError(format!("no clip {clip:?} in this project"))
}

fn notes_of(doc: &mut Project, clip: ClipId) -> Result<&mut crate::note::NoteData, CommandError> {
    match doc.clips.get_mut(clip).map(|c| &mut c.source) {
        Some(ClipSource::Notes(data)) => Ok(data),
        Some(_) => Err(CommandError(format!("clip {clip:?} does not hold notes"))),
        None => Err(no_clip(clip)),
    }
}

// --- Channels --------------------------------------------------------------

/// Adds an instrument channel **and a mixer track for it**, as one entry.
///
/// One command rather than two, because choosing an instrument is one action:
/// needing two presses of Ctrl+Z to take it back would be a bug report.
pub struct AddChannel {
    name: String,
    patch_data: Option<fontelle_types::PatchData>,
    pan: f32,
    /// Which track the channel plays through. `None` — the master — is what a
    /// new channel gets; the MIDI importer is the one caller that asks for a
    /// track, because a file with sixteen parts really does want sixteen
    /// strips carrying their own CC7.
    route: Option<MixerTrackId>,
    created: Option<ChannelId>,
    label: String,
}

impl AddChannel {
    pub fn new(name: impl Into<String>, patch_data: Option<fontelle_types::PatchData>) -> Self {
        let name = name.into();
        Self {
            label: format!("Add channel \"{name}\""),
            name,
            patch_data,
            pan: 0.0,
            route: None,
            created: None,
        }
    }

    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }

    /// Puts the channel on a track that already exists rather than on the
    /// master. The importer's; nothing in the UI needs it, because routing is
    /// its own command and its own undo entry.
    pub fn routed_to(mut self, track: Option<MixerTrackId>) -> Self {
        self.route = track;
        self
    }

    /// The id, once this has been applied.
    pub fn channel(&self) -> Option<ChannelId> {
        self.created
    }
}

impl Command for AddChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if let Some(track) = self.route
            && !doc.mixer.tracks.contains_key(track)
        {
            return Err(CommandError(format!("no mixer track {track:?}")));
        }
        // No mixer track is minted here any more: a strip is a destination
        // somebody builds (see `AddMixerTrack`), and a new channel plays
        // through the master until it is pointed somewhere else.
        let channel = Channel {
            name: self.name.clone(),
            color: [0x4f, 0x8f, 0xd0, 0xff],
            mixer_track: self.route,
            patch_data: self.patch_data.clone(),
            pan: self.pan,
            muted: false,
            soloed: false,
            named_keys: false,
            gain_db: 0.0,
        };

        match self.created {
            // A redo: the same id, or the commands stacked above this one are
            // pointing at nothing.
            Some(channel_id) => {
                if !doc.channels.insert_at(channel_id, channel) {
                    return Err(CommandError("that channel id is taken".into()));
                }
            }
            None => self.created = Some(doc.channels.insert(channel)),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some(channel) => Box::new(RemoveChannel::new(channel)),
            None => Box::new(NotApplied("adding a channel")),
        }
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
        std::mem::size_of::<Self>() + self.name.len() + self.label.len()
    }
}

/// What a channel took with it, so the inverse can put all of it back.
struct RemovedChannel {
    channel: Channel,
    /// The clips that played this channel. Left behind they would be orphans:
    /// silent, and invisible to a user trying to work out why.
    clips: Vec<(ClipId, Clip)>,
}

pub struct RemoveChannel {
    channel: ChannelId,
    removed: Option<RemovedChannel>,
}

impl RemoveChannel {
    pub fn new(channel: ChannelId) -> Self {
        Self {
            channel,
            removed: None,
        }
    }
}

impl Command for RemoveChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(channel) = doc.channels.remove(self.channel) else {
            return Err(CommandError(format!(
                "no channel {:?} in this project",
                self.channel
            )));
        };

        let clip_ids: Vec<ClipId> = doc
            .clips
            .iter()
            .filter(|(_, clip)| match &clip.source {
                ClipSource::Notes(data) => data.channel == self.channel,
                _ => false,
            })
            .map(|(id, _)| id)
            .collect();
        let clips: Vec<(ClipId, Clip)> = clip_ids
            .into_iter()
            .filter_map(|id| doc.clips.remove(id).map(|clip| (id, clip)))
            .collect();

        // The channel's mixer track is **left alone**. It is a destination
        // somebody built and other channels may be playing through it; before
        // routing was a thing the user could see, deleting a channel taking
        // "its" strip with it was the only coherent reading, and now it is
        // simply wrong.
        self.removed = Some(RemovedChannel { channel, clips });
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(removed) => Box::new(RestoreChannel {
                id: self.channel,
                channel: removed.channel.clone(),
                clips: removed.clips.clone(),
            }),
            None => Box::new(NotApplied("removing a channel")),
        }
    }

    fn label(&self) -> &str {
        "Delete channel"
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
                .removed
                .as_ref()
                .map(|r| r.clips.len() * std::mem::size_of::<Clip>())
                .unwrap_or(0)
    }
}

/// The inverse half of [`RemoveChannel`]. Not a user-facing command.
struct RestoreChannel {
    id: ChannelId,
    channel: Channel,
    clips: Vec<(ClipId, Clip)>,
}

impl Command for RestoreChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc.channels.insert_at(self.id, self.channel.clone()) {
            return Err(CommandError("that channel id is taken".into()));
        }
        for (id, clip) in &self.clips {
            if !doc.clips.insert_at(*id, clip.clone()) {
                return Err(CommandError("that clip id is taken".into()));
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveChannel::new(self.id))
    }

    fn label(&self) -> &str {
        "Restore channel"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.clips.len() * std::mem::size_of::<Clip>()
    }
}

/// Copies a channel, its instrument, and the clips that play it.
///
/// *"stuff like being able to right click and duplicate too, for instruments in
/// the channel rack for example."* The whole of it is one command and therefore
/// one Ctrl+Z: a duplicate that took four presses to take back is one nobody
/// tries twice.
///
/// **Onto a lane of its own.** The copy's clips keep the times they had, so
/// leaving them on the original's row would hide one part behind the other; a
/// new row is the only reading that lets you see what you just made.
///
/// It does **not** copy the mixer track — it points at the same one. A track is
/// a destination somebody built (see [`AddMixerTrack`]), and a second strip
/// appearing per duplicate is exactly the mistake `AddChannel` used to make.
pub struct DuplicateChannel {
    source: ChannelId,
    /// Everything this minted, kept so a redo re-uses the same ids.
    made: Option<Made>,
}

struct Made {
    channel: ChannelId,
    lane: LaneId,
    clips: Vec<ClipId>,
}

impl DuplicateChannel {
    pub fn new(source: ChannelId) -> Self {
        Self {
            source,
            made: None,
        }
    }

    /// The copy, once this has been applied.
    pub fn channel(&self) -> Option<ChannelId> {
        self.made.as_ref().map(|made| made.channel)
    }
}

impl Command for DuplicateChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(original) = doc.channels.get(self.source).cloned() else {
            return Err(CommandError(format!("no channel {:?}", self.source)));
        };
        let mut copy = original.clone();
        copy.name = copy_name(&original.name);

        // Everything the copy will carry, worked out before anything is
        // written: a command that cannot do its whole job does nothing.
        let sources: Vec<(ClipId, Clip)> = doc
            .clips
            .iter()
            .filter(|(_, clip)| match &clip.source {
                ClipSource::Notes(data) => data.channel == self.source,
                _ => false,
            })
            .map(|(id, clip)| (id, clip.clone()))
            .collect();

        let (channel, lane) = match &self.made {
            Some(made) => {
                if !doc.channels.insert_at(made.channel, copy) {
                    return Err(CommandError("that channel id is taken".into()));
                }
                if !doc.lanes.insert_at(made.lane, a_lane(String::new())) {
                    return Err(CommandError("that lane id is taken".into()));
                }
                (made.channel, made.lane)
            }
            None => (
                doc.channels.insert(copy),
                doc.lanes.insert(a_lane(String::new())),
            ),
        };
        // Named after the copy rather than numbered, so the row says what is
        // on it.
        if let Some(row) = doc.lanes.get_mut(lane)
            && let Some(name) = doc.channels.get(channel).map(|c| c.name.clone())
        {
            row.name = name;
        }

        let mut clips = Vec::with_capacity(sources.len());
        for (index, (_, source)) in sources.iter().enumerate() {
            let mut clip = source.clone();
            clip.lane = lane;
            if let ClipSource::Notes(data) = &mut clip.source {
                data.channel = channel;
            }
            // A duplicate's clips are **its own notes**, not a second view of
            // the original's — see `Clip::loop_length` for the difference
            // between copying and looping. `Clip: Clone` clones the arena, so
            // this is already true; it is written down because the whole
            // command is a lie if it ever stops being.
            match self.made.as_ref().and_then(|made| made.clips.get(index)) {
                Some(id) => {
                    if !doc.clips.insert_at(*id, clip) {
                        return Err(CommandError("that clip id is taken".into()));
                    }
                    clips.push(*id);
                }
                None => clips.push(doc.clips.insert(clip)),
            }
        }

        self.made = Some(Made {
            channel,
            lane,
            clips,
        });
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.made {
            // The channel takes its clips with it (see `RemoveChannel`), and
            // the lane it was given goes too — it was made by this command and
            // nothing else is on it.
            Some(made) => Box::new(Compound::new(
                "Undo duplicate",
                vec![
                    Box::new(RemoveChannel::new(made.channel)),
                    Box::new(RemoveLane::new(made.lane)),
                ],
            )),
            None => Box::new(NotApplied("duplicating a channel")),
        }
    }

    fn label(&self) -> &str {
        "Duplicate channel"
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

/// `"Keys"` becomes `"Keys copy"`, and `"Keys copy"` becomes `"Keys copy 2"` —
/// so duplicating the duplicate does not give two rows with the same name.
fn copy_name(name: &str) -> String {
    let Some(stem) = name.strip_suffix(" copy") else {
        // "Keys copy 2" -> "Keys copy 3"
        if let Some((head, tail)) = name.rsplit_once(' ')
            && let Ok(n) = tail.parse::<u32>()
            && head.ends_with(" copy")
        {
            return format!("{head} {}", n + 1);
        }
        return format!("{name} copy");
    };
    format!("{stem} copy 2")
}

// --- Lanes: the arrangement's rows ------------------------------------------

/// Makes a lane (TDD §10.3).
///
/// A lane is visual only — no routing, no instrument, no audio identity — so
/// there is nothing to decide here beyond its name. It is a command all the
/// same, because everything that changes the document is (INVARIANT 9) and
/// because a row added by mistake has to come off again.
pub struct AddLane {
    name: String,
    created: Option<LaneId>,
}

impl AddLane {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            created: None,
        }
    }

    /// The lane this made, once it has been applied.
    pub fn id(&self) -> Option<LaneId> {
        self.created
    }
}

/// Moves one row up or down the stack (TDD §10.1).
///
/// By **position**, not by id, because that is what the gesture is: "put this
/// row above the one above it". The two rows swap their
/// [`order`](crate::Lane::order) and nothing else moves — in particular the
/// lanes themselves stay where they are in the arena, so every clip that names
/// one still names the same row. Swapping the lanes' *contents* instead would
/// leave every clip pointing at the wrong row, which is what
/// `moving_a_row_takes_its_clips_with_it` checks.
///
/// A move off either end is a no-op rather than an error: the menu greys them,
/// and a command that failed would make a keyboard shortcut for it something
/// somebody has to handle.
pub struct MoveLane {
    from: usize,
    delta: isize,
}

impl MoveLane {
    pub fn new(from: usize, delta: isize) -> Self {
        Self { from, delta }
    }

    pub fn up(from: usize) -> Self {
        Self::new(from, -1)
    }

    pub fn down(from: usize) -> Self {
        Self::new(from, 1)
    }
}

impl Command for MoveLane {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let ids = doc.lane_ids();
        let Some(to) = self.from.checked_add_signed(self.delta) else {
            return Ok(());
        };
        if self.from >= ids.len() || to >= ids.len() {
            return Ok(());
        }
        // Renumbered densely from the order they are *in* before anything is
        // swapped, so a project whose rows all carry the default still ends up
        // with a total order rather than a pile of ties.
        for (position, id) in ids.iter().enumerate() {
            if let Some(lane) = doc.lanes.get_mut(*id) {
                lane.order = position as u32;
            }
        }
        if let Some(lane) = doc.lanes.get_mut(ids[self.from]) {
            lane.order = to as u32;
        }
        if let Some(lane) = doc.lanes.get_mut(ids[to]) {
            lane.order = self.from as u32;
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        // The row is at `to` now, and putting it back is the same distance the
        // other way.
        Box::new(MoveLane::new(
            self.from.saturating_add_signed(self.delta),
            -self.delta,
        ))
    }

    fn label(&self) -> &str {
        if self.delta < 0 { "move lane up" } else { "move lane down" }
    }

    /// Repeated presses of "move up" **do not** coalesce into one history
    /// entry.
    ///
    /// A rename does, because the thing being undone is "the name I typed" and
    /// nobody wants Ctrl+Z per letter. A move is a step, and each one is a
    /// place somebody might want to come back to.
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

/// What a lane looks like when nothing has said otherwise. The same numbers
/// `blank_project` uses, in one place rather than three.
fn a_lane(name: String) -> Lane {
    Lane {
        name,
        height: 32.0,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        muted: false,
        locked: false,
        order: 0,
    }
}

impl Command for AddLane {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let mut lane = a_lane(self.name.clone());
        // Past the bottom of the stack, which is where somebody adding a row
        // is looking for it — and not wherever a default of zero would sort it
        // once the rows have been reordered.
        lane.order = doc
            .lanes
            .values()
            .map(|lane| lane.order)
            .max()
            .map_or(0, |highest| highest.saturating_add(1));
        match self.created {
            // The same id on a redo, or the clips a later command moved onto
            // this lane would be pointing at nothing.
            Some(id) => {
                if !doc.lanes.insert_at(id, lane) {
                    return Err(CommandError("that lane id is taken".into()));
                }
            }
            None => self.created = Some(doc.lanes.insert(lane)),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some(id) => Box::new(RemoveLane::new(id)),
            None => Box::new(NotApplied("adding a lane")),
        }
    }

    fn label(&self) -> &str {
        "Add lane"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.len()
    }
}

/// Deletes a lane **and the clips on it**.
///
/// A clip names its lane, so a lane removed on its own would leave clips
/// nothing can draw and nothing can reach. The same reading `RemoveChannel`
/// gives, and the inverse puts both halves back under their own ids.
///
/// **The last lane stays.** An arrangement with no rows has nowhere to draw a
/// clip and no way back to having one, since every "add" in the window puts
/// something *on* a lane.
pub struct RemoveLane {
    lane: LaneId,
    removed: Option<(Lane, Vec<(ClipId, Clip)>)>,
}

impl RemoveLane {
    pub fn new(lane: LaneId) -> Self {
        Self {
            lane,
            removed: None,
        }
    }
}

impl Command for RemoveLane {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc.lanes.contains_key(self.lane) {
            return Err(CommandError(format!("no lane {:?} in this project", self.lane)));
        }
        if doc.lanes.len() <= 1 {
            return Err(CommandError(
                "an arrangement has to keep one row to draw on".into(),
            ));
        }
        let clip_ids: Vec<ClipId> = doc
            .clips
            .iter()
            .filter(|(_, clip)| clip.lane == self.lane)
            .map(|(id, _)| id)
            .collect();
        let clips: Vec<(ClipId, Clip)> = clip_ids
            .into_iter()
            .filter_map(|id| doc.clips.remove(id).map(|clip| (id, clip)))
            .collect();
        let lane = doc
            .lanes
            .remove(self.lane)
            .ok_or_else(|| CommandError("that lane went away".into()))?;
        self.removed = Some((lane, clips));
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some((lane, clips)) => Box::new(RestoreLane {
                id: self.lane,
                lane: lane.clone(),
                clips: clips.clone(),
            }),
            None => Box::new(NotApplied("removing a lane")),
        }
    }

    fn label(&self) -> &str {
        "Delete lane"
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
                .removed
                .as_ref()
                .map_or(0, |(_, clips)| clips.len() * std::mem::size_of::<Clip>())
    }
}

/// The inverse half of [`RemoveLane`]. Not a user-facing command.
struct RestoreLane {
    id: LaneId,
    lane: Lane,
    clips: Vec<(ClipId, Clip)>,
}

impl Command for RestoreLane {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc.lanes.insert_at(self.id, self.lane.clone()) {
            return Err(CommandError("that lane id is taken".into()));
        }
        for (id, clip) in &self.clips {
            if !doc.clips.insert_at(*id, clip.clone()) {
                return Err(CommandError("that clip id is taken".into()));
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveLane::new(self.id))
    }

    fn label(&self) -> &str {
        "Restore lane"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.clips.len() * std::mem::size_of::<Clip>()
    }
}

/// Renames a lane. Coalescing, like every other rename here: typing is one
/// gesture and one undo entry.
pub struct RenameLane {
    lane: LaneId,
    name: String,
    previous: Option<String>,
}

impl RenameLane {
    pub fn new(lane: LaneId, name: impl Into<String>) -> Self {
        Self {
            lane,
            name: name.into(),
            previous: None,
        }
    }
}

impl Command for RenameLane {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let lane = doc
            .lanes
            .get_mut(self.lane)
            .ok_or_else(|| CommandError(format!("no lane {:?}", self.lane)))?;
        let previous = std::mem::replace(&mut lane.name, self.name.clone());
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(RenameLane::new(self.lane, previous.clone())),
            None => Box::new(NotApplied("renaming a lane")),
        }
    }

    fn label(&self) -> &str {
        "Rename lane"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<RenameLane>() else {
            return false;
        };
        if next.lane != self.lane {
            return false;
        }
        self.name = next.name.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.len()
            + self.previous.as_ref().map_or(0, |p| p.len())
    }
}

// --- Mixer tracks, and what plays through them ------------------------------

/// Makes a mixer track (TDD §13.1).
///
/// Its own command because a strip is a **destination somebody builds**, not
/// something that appears when a soundfont is loaded. That distinction is the
/// whole of the routing model: `AddChannel` used to mint one per channel, so a
/// project with twenty instruments had twenty strips nobody asked for and no
/// way to say "these four go to the drum bus".
pub struct AddMixerTrack {
    name: String,
    created: Option<MixerTrackId>,
    label: String,
}

impl AddMixerTrack {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            label: format!("Add mixer track \"{name}\""),
            name,
            created: None,
        }
    }

    /// The id, once this has been applied.
    pub fn track(&self) -> Option<MixerTrackId> {
        self.created
    }
}

impl Command for AddMixerTrack {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let mut track = MixerTrack::new(self.name.clone());
        // Into the master, which is the only destination that is always there.
        track.output = doc.mixer.master;
        match self.created {
            Some(id) => {
                if !doc.mixer.tracks.insert_at(id, track) {
                    return Err(CommandError("that mixer track id is taken".into()));
                }
            }
            None => self.created = Some(doc.mixer.tracks.insert(track)),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some(id) => Box::new(RemoveMixerTrack::new(id)),
            None => Box::new(NotApplied("adding a mixer track")),
        }
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
        std::mem::size_of::<Self>() + self.name.len() + self.label.len()
    }
}

/// What a deleted track took with it, so the inverse can put all of it back.
#[derive(Clone)]
struct RemovedTrack {
    track: MixerTrack,
    /// The channels that were playing through it.
    channels: Vec<ChannelId>,
    /// And the tracks that were feeding it.
    feeders: Vec<MixerTrackId>,
    /// The sends that pointed at it, and where each sat in its own track's
    /// list — `(track, index, the send itself)`.
    ///
    /// Kept whole rather than remade on undo: a send carries a level somebody
    /// set, and restoring a deleted reverb bus with every send to it wide open
    /// would be a worse surprise than the deletion was.
    senders: Vec<(MixerTrackId, usize, crate::mixer::Send)>,
}

/// Deletes a mixer track, sending everything that pointed at it back to the
/// master.
///
/// Back to the master rather than nowhere: a channel pointing at a track that
/// does not exist is silent for a reason nobody can see in the panel, and a
/// track routed into thin air is a bus you cannot hear. Audible-and-wrong is a
/// state a person can fix; silent-and-wrong is one they have to debug.
pub struct RemoveMixerTrack {
    id: MixerTrackId,
    removed: Option<RemovedTrack>,
}

impl RemoveMixerTrack {
    pub fn new(id: MixerTrackId) -> Self {
        Self { id, removed: None }
    }
}

impl Command for RemoveMixerTrack {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // Every project has a master (TDD §13.1) and `realise` refuses to
        // build a graph without one, so this is not a thing to allow and then
        // report later.
        if doc.mixer.master == Some(self.id) {
            return Err(CommandError(
                "the master track is where everything arrives — it cannot be deleted".into(),
            ));
        }
        let Some(track) = doc.mixer.tracks.remove(self.id) else {
            return Err(CommandError(format!("no mixer track {:?}", self.id)));
        };

        let channels: Vec<ChannelId> = doc
            .channels
            .iter()
            .filter(|(_, channel)| channel.mixer_track == Some(self.id))
            .map(|(id, _)| id)
            .collect();
        for id in &channels {
            doc.channels[*id].mixer_track = None;
        }

        let feeders: Vec<MixerTrackId> = doc
            .mixer
            .tracks
            .iter()
            .filter(|(_, other)| other.output == Some(self.id))
            .map(|(id, _)| id)
            .collect();
        for id in &feeders {
            doc.mixer.tracks[*id].output = doc.mixer.master;
        }

        // A send at a track that is not there is either silent or a panic, and
        // both are worse than the send going with the bus it fed. Recorded
        // back to front so restoring them by `insert` puts each one back at
        // its own index.
        let mut senders = Vec::new();
        for (id, other) in doc.mixer.tracks.iter() {
            for (index, send) in other.sends.iter().enumerate() {
                if send.target == self.id {
                    senders.push((id, index, *send));
                }
            }
        }
        for (id, index, _) in senders.iter().rev() {
            doc.mixer.tracks[*id].sends.remove(*index);
        }

        self.removed = Some(RemovedTrack {
            track,
            channels,
            feeders,
            senders,
        });
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(removed) => Box::new(RestoreMixerTrack {
                id: self.id,
                removed: removed.clone(),
            }),
            None => Box::new(NotApplied("deleting a mixer track")),
        }
    }

    fn label(&self) -> &str {
        "Delete mixer track"
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

/// The inverse half of [`RemoveMixerTrack`]. Not a user-facing command.
struct RestoreMixerTrack {
    id: MixerTrackId,
    removed: RemovedTrack,
}

impl Command for RestoreMixerTrack {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc
            .mixer
            .tracks
            .insert_at(self.id, self.removed.track.clone())
        {
            return Err(CommandError("that mixer track id is taken".into()));
        }
        // Everything that was pointing at it goes back, which is what makes
        // this an undo rather than "a track called the same thing".
        for channel in &self.removed.channels {
            if let Some(channel) = doc.channels.get_mut(*channel) {
                channel.mixer_track = Some(self.id);
            }
        }
        for track in &self.removed.feeders {
            if let Some(track) = doc.mixer.tracks.get_mut(*track) {
                track.output = Some(self.id);
            }
        }
        // Forwards, so each send goes back to the index it came from.
        for (track, index, send) in &self.removed.senders {
            if let Some(track) = doc.mixer.tracks.get_mut(*track) {
                let at = (*index).min(track.sends.len());
                track.sends.insert(at, *send);
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveMixerTrack::new(self.id))
    }

    fn label(&self) -> &str {
        "Restore mixer track"
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

pub struct RenameMixerTrack {
    id: MixerTrackId,
    name: String,
    previous: Option<String>,
}

impl RenameMixerTrack {
    pub fn new(id: MixerTrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            previous: None,
        }
    }
}

impl Command for RenameMixerTrack {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.id)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.id)))?;
        let previous = std::mem::replace(&mut track.name, self.name.clone());
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(RenameMixerTrack::new(self.id, previous.clone())),
            None => Box::new(NotApplied("renaming a mixer track")),
        }
    }

    fn label(&self) -> &str {
        "Rename mixer track"
    }

    /// Typing is one gesture. The same reason `RenameChannel` coalesces.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<RenameMixerTrack>() else {
            return false;
        };
        if next.id != self.id {
            return false;
        }
        self.name = next.name.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.len()
            + self.previous.as_ref().map_or(0, |p| p.len())
    }
}

/// Points a mixer track's output at another track, or back at the master
/// (TDD §13.2).
///
/// `None` is the master, for the same reason [`SetChannelRoute`] spells it
/// that way: a track that names the master by id stops being routed to the
/// master the moment somebody makes a different one.
///
/// # The check is the point
///
/// §13.2 requires the routing graph to be validated acyclic on **every**
/// mutation — reject the command, never let a feedback loop reach the graph
/// compiler. So this writes the field, asks [`Mixer::has_cycle`], and puts it
/// back if the answer is yes. Tentative-then-check rather than a bespoke
/// reachability walk, because `has_cycle` already reads `output` *and* `sends`
/// and a second implementation is somewhere for the two to disagree — which
/// would mean a loop that one of them permits.
pub struct SetTrackOutput {
    track: MixerTrackId,
    output: Option<MixerTrackId>,
    previous: Option<Option<MixerTrackId>>,
}

impl SetTrackOutput {
    pub fn new(track: MixerTrackId, output: Option<MixerTrackId>) -> Self {
        Self {
            track,
            output,
            previous: None,
        }
    }
}

impl Command for SetTrackOutput {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // The master is where everything arrives. Giving it an output is
        // either a loop or a second master, and neither is a thing this
        // document can mean.
        if doc.mixer.master == Some(self.track) {
            return Err(CommandError("the master's output is the speakers".into()));
        }
        // Checked before the write, like `SetChannelRoute`: a track pointing
        // at one that is not there feeds the master by accident rather than by
        // decision, which only ever shows up as a mix being wrong.
        if let Some(output) = self.output
            && !doc.mixer.tracks.contains_key(output)
        {
            return Err(CommandError(format!("no mixer track {output:?}")));
        }
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let previous = std::mem::replace(&mut track.output, self.output);
        if doc.mixer.has_cycle() {
            doc.mixer.tracks[self.track].output = previous;
            return Err(CommandError(
                "that routing would feed a track back into itself".into(),
            ));
        }
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetTrackOutput::new(self.track, previous)),
            None => Box::new(NotApplied("routing a mixer track")),
        }
    }

    fn label(&self) -> &str {
        "Route mixer track"
    }

    /// Picking from a menu is one decision each time, not a gesture: two
    /// choices made a second apart are two things to be able to take back.
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

/// The level a new send starts at.
///
/// **Silence.** A send that arrived wide open would change the mix the moment
/// it was made, which is the opposite of what making one is for: you add a
/// reverb send and then bring it up until you can hear it.
pub const NEW_SEND_DB: f32 = -60.0;

/// Takes a copy of one track's signal into another (TDD §13.2).
///
/// # A send is not an output
///
/// Routing a track's *output* into a reverb sends all of it and nothing stays
/// dry. A send takes a copy at a level and leaves the dry signal on its own
/// path, which is what a reverb bus is and why both exist.
///
/// §13.2 requires the routing graph to be validated acyclic on every
/// mutation, and it is explicit that both edge kinds count: a cycle through a
/// send is the same feedback loop as one through an output, "just harder to
/// see in the UI". So this checks with [`Mixer::has_cycle`], the same way
/// [`SetTrackOutput`] does and for the same reason — one implementation of
/// what a loop is.
pub struct AddSend {
    track: MixerTrackId,
    target: MixerTrackId,
    /// Where it landed, so the inverse can take exactly this one off.
    index: Option<usize>,
}

impl AddSend {
    pub fn new(track: MixerTrackId, target: MixerTrackId) -> Self {
        Self {
            track,
            target,
            index: None,
        }
    }
}

impl Command for AddSend {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc.mixer.tracks.contains_key(self.target) {
            return Err(CommandError(format!("no mixer track {:?}", self.target)));
        }
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let index = self.index.unwrap_or(track.sends.len()).min(track.sends.len());
        track.sends.insert(
            index,
            Send {
                target: self.target,
                level_db: NEW_SEND_DB,
                pan: 0.0,
                // Post-fader, which is what a reverb send wants: pull the
                // fader down and the reverb follows it rather than being left
                // ringing over a part that is no longer there.
                pre_fader: false,
            },
        );
        if doc.mixer.has_cycle() {
            doc.mixer.tracks[self.track].sends.remove(index);
            return Err(CommandError(
                "that send would feed a track back into itself".into(),
            ));
        }
        self.index = Some(index);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.index {
            Some(index) => Box::new(RemoveSend::new(self.track, index)),
            None => Box::new(NotApplied("adding a send")),
        }
    }

    fn label(&self) -> &str {
        "Add send"
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

/// Takes one off, keeping it so undo puts it back where it was set.
pub struct RemoveSend {
    track: MixerTrackId,
    index: usize,
    removed: Option<Send>,
}

impl RemoveSend {
    pub fn new(track: MixerTrackId, index: usize) -> Self {
        Self {
            track,
            index,
            removed: None,
        }
    }
}

impl Command for RemoveSend {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        if self.index >= track.sends.len() {
            return Err(CommandError("no such send".into()));
        }
        let removed = track.sends.remove(self.index);
        self.removed.get_or_insert(removed);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.removed {
            Some(send) => Box::new(RestoreSend {
                track: self.track,
                index: self.index,
                send,
            }),
            None => Box::new(NotApplied("deleting a send")),
        }
    }

    fn label(&self) -> &str {
        "Delete send"
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

/// Puts a deleted send back at its own index, with the level it was set to.
struct RestoreSend {
    track: MixerTrackId,
    index: usize,
    send: Send,
}

impl Command for RestoreSend {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let at = self.index.min(track.sends.len());
        track.sends.insert(at, self.send);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveSend::new(self.track, self.index))
    }

    fn label(&self) -> &str {
        "Restore send"
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

/// How much of a track goes down one of its sends.
pub struct SetSendLevel {
    track: MixerTrackId,
    index: usize,
    level_db: f32,
    previous: Option<f32>,
}

impl SetSendLevel {
    pub fn new(track: MixerTrackId, index: usize, level_db: f32) -> Self {
        Self {
            track,
            index,
            level_db,
            previous: None,
        }
    }
}

impl Command for SetSendLevel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let send = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .and_then(|track| track.sends.get_mut(self.index))
            .ok_or_else(|| CommandError("no such send".into()))?;
        let previous = std::mem::replace(&mut send.level_db, self.level_db);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetSendLevel::new(self.track, self.index, previous)),
            None => Box::new(NotApplied("setting a send level")),
        }
    }

    fn label(&self) -> &str {
        "Send level"
    }

    /// Dragging is one gesture. The same rule a fader follows — sixty entries
    /// for one drag is an undo stack nobody can use.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetSendLevel>() else {
            return false;
        };
        if next.track != self.track || next.index != self.index {
            return false;
        }
        self.level_db = next.level_db;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Whether a send is taken before the fader or after it.
///
/// The difference is audible and is the reason the switch exists: a post-fader
/// send follows the fader down, and a pre-fader one does not — which is what a
/// cue mix wants and what a reverb send does not.
pub struct SetSendPreFader {
    track: MixerTrackId,
    index: usize,
    pre_fader: bool,
    previous: Option<bool>,
}

impl SetSendPreFader {
    pub fn new(track: MixerTrackId, index: usize, pre_fader: bool) -> Self {
        Self {
            track,
            index,
            pre_fader,
            previous: None,
        }
    }
}

impl Command for SetSendPreFader {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let send = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .and_then(|track| track.sends.get_mut(self.index))
            .ok_or_else(|| CommandError("no such send".into()))?;
        let previous = std::mem::replace(&mut send.pre_fader, self.pre_fader);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetSendPreFader::new(self.track, self.index, previous)),
            None => Box::new(NotApplied("switching a send pre-fader")),
        }
    }

    fn label(&self) -> &str {
        "Send tap point"
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

/// Points a channel at a mixer track, or back at the master.
///
/// `None` is the master. See [`Channel::mixer_track`] for why that is a
/// `None` rather than the master's own id.
pub struct SetChannelRoute {
    channel: ChannelId,
    track: Option<MixerTrackId>,
    previous: Option<Option<MixerTrackId>>,
}

impl SetChannelRoute {
    pub fn new(channel: ChannelId, track: Option<MixerTrackId>) -> Self {
        Self {
            channel,
            track,
            previous: None,
        }
    }
}

impl Command for SetChannelRoute {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // Checked before the write, not after: a channel pointing at a track
        // that is not there plays into the master by accident rather than by
        // decision, which is a bug that only shows up as a mix being wrong.
        if let Some(track) = self.track
            && !doc.mixer.tracks.contains_key(track)
        {
            return Err(CommandError(format!("no mixer track {track:?}")));
        }
        let channel = doc
            .channels
            .get_mut(self.channel)
            .ok_or_else(|| CommandError(format!("no channel {:?}", self.channel)))?;
        let previous = std::mem::replace(&mut channel.mixer_track, self.track);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetChannelRoute::new(self.channel, previous)),
            None => Box::new(NotApplied("routing a channel")),
        }
    }

    fn label(&self) -> &str {
        "Route channel"
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

/// Puts an instrument on a channel, or takes it off.
///
/// Separate from [`AddChannel`] because replacing the instrument on a channel
/// that already has notes on it is an ordinary edit — and one worth being able
/// to take back, since the patch it replaces is the user's own edited copy of
/// a soundfont's defaults, not something re-derivable from the file.
pub struct SetChannelPatch {
    channel: ChannelId,
    patch_data: Option<fontelle_types::PatchData>,
    previous: Option<Option<fontelle_types::PatchData>>,
}

impl SetChannelPatch {
    pub fn new(channel: ChannelId, patch_data: Option<fontelle_types::PatchData>) -> Self {
        Self {
            channel,
            patch_data,
            previous: None,
        }
    }
}

impl Command for SetChannelPatch {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let channel = doc
            .channels
            .get_mut(self.channel)
            .ok_or_else(|| CommandError(format!("no channel {:?}", self.channel)))?;
        let previous = std::mem::replace(&mut channel.patch_data, self.patch_data.clone());
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(SetChannelPatch::new(self.channel, previous.clone())),
            None => Box::new(NotApplied("choosing an instrument")),
        }
    }

    fn label(&self) -> &str {
        match self.patch_data {
            Some(_) => "Choose instrument",
            None => "Clear instrument",
        }
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetChannelPatch>() else {
            return false;
        };
        if next.channel != self.channel {
            return false;
        }
        // The instrument editor writes the whole patch on every step of a knob
        // drag (§7.2's "every parameter is the user's"), so without this one
        // drag would leave forty entries on the history and one Ctrl+Z would go
        // back a pixel. The new patch, the old `previous`: one drag, one entry,
        // one undo back to where the drag started.
        //
        // Choosing an instrument from the browser is not caught by this: it
        // goes through a `Compound` and breaks the gesture on either side.
        self.patch_data = next.patch_data.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        // A patch body is a JSON tree of unknown size; charging a flat kilobyte
        // is closer than charging nothing, and the ceiling only needs to be
        // approximately right to stop a history eating the heap.
        std::mem::size_of::<Self>() + 1024
    }
}

/// A channel's name, which is what the rack shows.
///
/// It merges with itself so typing a name is one undo rather than one per
/// keystroke, and so is swapping a channel's soundfont twice in a row.
pub struct RenameChannel {
    channel: ChannelId,
    name: String,
    /// The name before this ran. `None` until applied, which is how `invert`
    /// knows it has nothing to say yet, and what makes a merge keep the
    /// *original* name to go back to.
    previous: Option<String>,
}

impl RenameChannel {
    pub fn new(channel: ChannelId, name: impl Into<String>) -> Self {
        Self {
            channel,
            name: name.into(),
            previous: None,
        }
    }
}

impl Command for RenameChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let channel = doc
            .channels
            .get_mut(self.channel)
            .ok_or_else(|| CommandError(format!("no channel {:?}", self.channel)))?;
        let previous = std::mem::replace(&mut channel.name, self.name.clone());
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(RenameChannel::new(self.channel, previous.clone())),
            None => Box::new(NotApplied("renaming a channel")),
        }
    }

    fn label(&self) -> &str {
        "Rename channel"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<RenameChannel>() else {
            return false;
        };
        if next.channel != self.channel {
            return false;
        }
        self.name = next.name.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.len()
            + self.previous.as_ref().map_or(0, |p| p.len())
    }
}

/// Several commands that are one thing a person did.
///
/// Choosing an instrument is a patch *and* a channel name; a compound makes
/// that one entry in the history, so one Ctrl+Z takes back one gesture. §10.6
/// is about merging repeats of the same command; this is the other half — two
/// different commands that arrived together.
///
/// **All or nothing.** If a part fails, the parts that already ran are undone
/// before the error comes back: half a compound is exactly the state its own
/// inverse cannot describe.
pub struct Compound {
    label: String,
    parts: Vec<Box<dyn Command>>,
    applied: bool,
}

impl Compound {
    pub fn new(label: impl Into<String>, parts: Vec<Box<dyn Command>>) -> Self {
        Self {
            label: label.into(),
            parts,
            applied: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

impl Command for Compound {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        for index in 0..self.parts.len() {
            if let Err(e) = self.parts[index].apply(doc) {
                // Back out, newest first, so the document is where it started.
                // The inverses cannot themselves be checked here — if one of
                // them fails there is nothing left to try — so the first error
                // is the one reported.
                for done in (0..index).rev() {
                    let _ = self.parts[done].invert().apply(doc);
                }
                return Err(e);
            }
        }
        self.applied = true;
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if !self.applied {
            return Box::new(NotApplied("a compound edit"));
        }
        Box::new(Compound::new(
            self.label.clone(),
            // Reverse order: the last thing done is the first thing undone.
            self.parts.iter().rev().map(|part| part.invert()).collect(),
        ))
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
            + self.label.len()
            + self.parts.iter().map(|p| p.memory_cost()).sum::<usize>()
    }
}

// --- Notes -----------------------------------------------------------------

fn note_count_label(verb: &str, count: usize) -> String {
    match count {
        1 => format!("{verb} a note"),
        n => format!("{verb} {n} notes"),
    }
}

pub struct AddNotes {
    clip: ClipId,
    notes: Vec<Note>,
    ids: Vec<NoteId>,
    label: String,
}

impl AddNotes {
    pub fn new(clip: ClipId, notes: Vec<Note>) -> Self {
        Self {
            label: note_count_label("Draw", notes.len()),
            clip,
            notes,
            ids: Vec::new(),
        }
    }

    /// The ids, once this has been applied.
    pub fn ids(&self) -> &[NoteId] {
        &self.ids
    }
}

impl Command for AddNotes {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        if self.ids.is_empty() {
            self.ids = self
                .notes
                .iter()
                .map(|note| data.notes.insert(*note))
                .collect();
        } else {
            for (id, note) in self.ids.iter().zip(&self.notes) {
                if !data.notes.insert_at(*id, *note) {
                    return Err(CommandError("that note id is taken".into()));
                }
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.ids.is_empty() {
            return Box::new(NotApplied("drawing notes"));
        }
        Box::new(RemoveNotes::new(self.clip, self.ids.clone()))
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
            + self.notes.len() * std::mem::size_of::<Note>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.label.len()
    }
}

pub struct RemoveNotes {
    clip: ClipId,
    ids: Vec<NoteId>,
    removed: Vec<(NoteId, Note)>,
    label: String,
}

impl RemoveNotes {
    pub fn new(clip: ClipId, ids: Vec<NoteId>) -> Self {
        Self {
            label: note_count_label("Delete", ids.len()),
            clip,
            ids,
            removed: Vec::new(),
        }
    }
}

impl Command for RemoveNotes {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        // Checked before anything is taken out, so a selection with one stale
        // id does not leave the rest half-deleted.
        if let Some(missing) = self.ids.iter().find(|id| !data.notes.contains_key(**id)) {
            return Err(CommandError(format!("no note {missing:?} in this clip")));
        }
        self.removed = self
            .ids
            .iter()
            .filter_map(|id| data.notes.remove(*id).map(|note| (*id, note)))
            .collect();
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.removed.is_empty() && !self.ids.is_empty() {
            return Box::new(NotApplied("deleting notes"));
        }
        Box::new(RestoreNotes {
            clip: self.clip,
            notes: self.removed.clone(),
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
            + self.removed.len() * std::mem::size_of::<(NoteId, Note)>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.label.len()
    }
}

/// The inverse half of [`RemoveNotes`].
struct RestoreNotes {
    clip: ClipId,
    notes: Vec<(NoteId, Note)>,
}

impl Command for RestoreNotes {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        for (id, note) in &self.notes {
            if !data.notes.insert_at(*id, *note) {
                return Err(CommandError("that note id is taken".into()));
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveNotes::new(
            self.clip,
            self.notes.iter().map(|(id, _)| *id).collect(),
        ))
    }

    fn label(&self) -> &str {
        "Restore notes"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.notes.len() * std::mem::size_of::<(NoteId, Note)>()
    }
}

/// Moves notes by a **delta**, which is what makes a drag both mergeable and
/// exactly invertible: coalescing is adding, and undoing is negating.
pub struct MoveNotes {
    clip: ClipId,
    ids: Vec<NoteId>,
    tick_delta: Tick,
    key_delta: i16,
    label: String,
}

impl MoveNotes {
    pub fn new(clip: ClipId, ids: Vec<NoteId>, tick_delta: Tick, key_delta: i16) -> Self {
        Self {
            label: note_count_label("Move", ids.len()),
            clip,
            ids,
            tick_delta,
            key_delta,
        }
    }
}

impl Command for MoveNotes {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let (tick_delta, key_delta) = (self.tick_delta, self.key_delta);
        let data = notes_of(doc, self.clip)?;
        // Every note checked before any note moves.
        for id in &self.ids {
            let Some(note) = data.notes.get(*id) else {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            };
            let start = note.start + tick_delta;
            let key = note.key as i32 + key_delta as i32;
            if start < 0 {
                return Err(CommandError(
                    "that would move a note before its clip".into(),
                ));
            }
            if !(0..=127).contains(&key) {
                return Err(CommandError(
                    "that would move a note off the keyboard".into(),
                ));
            }
        }
        for id in &self.ids {
            if let Some(note) = data.notes.get_mut(*id) {
                note.start += tick_delta;
                note.key = (note.key as i32 + key_delta as i32) as u8;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(MoveNotes::new(
            self.clip,
            self.ids.clone(),
            -self.tick_delta,
            -self.key_delta,
        ))
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<MoveNotes>() else {
            return false;
        };
        if next.clip != self.clip || next.ids != self.ids {
            return false;
        }
        self.tick_delta += next.tick_delta;
        self.key_delta += next.key_delta;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.label.len()
    }
}

/// Lengthens or shortens notes by a delta, on the same terms as [`MoveNotes`].
pub struct ResizeNotes {
    clip: ClipId,
    ids: Vec<NoteId>,
    length_delta: Tick,
    label: String,
}

impl ResizeNotes {
    pub fn new(clip: ClipId, ids: Vec<NoteId>, length_delta: Tick) -> Self {
        Self {
            label: note_count_label("Resize", ids.len()),
            clip,
            ids,
            length_delta,
        }
    }
}

impl Command for ResizeNotes {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let delta = self.length_delta;
        let data = notes_of(doc, self.clip)?;
        for id in &self.ids {
            let Some(note) = data.notes.get(*id) else {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            };
            // A note of zero length is a note-on and note-off on the same
            // sample, which the sequencer emits and the sampler cannot sound.
            if note.length + delta < 1 {
                return Err(CommandError("that would resize a note to nothing".into()));
            }
        }
        for id in &self.ids {
            if let Some(note) = data.notes.get_mut(*id) {
                note.length += delta;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(ResizeNotes::new(
            self.clip,
            self.ids.clone(),
            -self.length_delta,
        ))
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<ResizeNotes>() else {
            return false;
        };
        if next.clip != self.clip || next.ids != self.ids {
            return false;
        }
        self.length_delta += next.length_delta;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.label.len()
    }
}

/// Sets the velocity of every named note to one value.
///
/// The piano roll's velocity lane (TDD §16.5's first note property lane), as a
/// command like everything else. Two things about it are not obvious:
///
/// - **The inverse is per note.** A selection dragged flat came from notes that
///   each had their own velocity, and an undo that restored one shared value
///   would be a second edit wearing an undo's clothes. So `previous` is a
///   parallel vector, captured on apply.
/// - **It merges with itself.** Dragging up the lane produces one of these per
///   mouse-move; without `merge_with` an undo would step back through the drag
///   a pixel at a time. Merging keeps the *original* `previous`, which is what
///   makes one undo go back to where the drag started.
pub struct SetNoteVelocity {
    clip: ClipId,
    ids: Vec<NoteId>,
    velocity: u8,
    /// Each note's velocity before this ran, in `ids` order. Empty until
    /// applied, which is also how `invert` knows it has nothing to say yet.
    previous: Vec<u8>,
    label: String,
}

impl SetNoteVelocity {
    pub fn new(clip: ClipId, ids: Vec<NoteId>, velocity: u8) -> Self {
        Self {
            label: note_count_label("Set velocity of", ids.len()),
            clip,
            ids,
            velocity,
            previous: Vec::new(),
        }
    }
}

impl Command for SetNoteVelocity {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        // Checked before anything is written: a command that half-applies is
        // one whose inverse cannot put the document back.
        for id in &self.ids {
            if data.notes.get(*id).is_none() {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            }
        }
        // Only on the first apply. A redo must restore the same `previous` the
        // original run captured, not whatever is there the second time round.
        if self.previous.is_empty() {
            self.previous = self
                .ids
                .iter()
                .filter_map(|id| data.notes.get(*id).map(|n| n.velocity))
                .collect();
        }
        for id in &self.ids {
            if let Some(note) = data.notes.get_mut(*id) {
                note.velocity = self.velocity;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.previous.is_empty() {
            return Box::new(NotApplied("setting velocity"));
        }
        Box::new(RestoreNoteVelocities {
            clip: self.clip,
            ids: self.ids.clone(),
            velocities: self.previous.clone(),
            label: self.label.clone(),
        })
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetNoteVelocity>() else {
            return false;
        };
        if next.clip != self.clip || next.ids != self.ids {
            return false;
        }
        // The new value, the old `previous`: one drag, one entry, one undo back
        // to where it started.
        self.velocity = next.velocity;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.previous.len()
            + self.label.len()
    }
}

/// [`SetNoteVelocity`]'s inverse: each note back to its own value.
struct RestoreNoteVelocities {
    clip: ClipId,
    ids: Vec<NoteId>,
    velocities: Vec<u8>,
    label: String,
}

impl Command for RestoreNoteVelocities {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        for (id, velocity) in self.ids.iter().zip(&self.velocities) {
            let Some(note) = data.notes.get_mut(*id) else {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            };
            note.velocity = *velocity;
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        // Every note went back to its own value, so the forward direction is
        // only expressible as one-value-for-all when they agreed. They did,
        // because that is what `SetNoteVelocity` had just done.
        Box::new(SetNoteVelocity::new(
            self.clip,
            self.ids.clone(),
            self.velocities.first().copied().unwrap_or(0),
        ))
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
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.velocities.len()
            + self.label.len()
    }
}

/// One note property, set on a selection — the property lane's whole
/// vocabulary (§16.5), and the only way pan, tuning, release or the two free
/// modulation values can be edited at all (INVARIANT 9).
///
/// [`SetNoteVelocity`] is the same shape for the one property that had a
/// command before this, and everything its documentation says applies here:
/// the inverse is per note, and it merges with itself so a drag is one undo.
/// The one addition is that it merges **only with itself on the same
/// property** — cycling the lane mid-gesture must not fold a pan edit into a
/// velocity one.
/// Marks notes as **slide notes**, or unmarks them.
///
/// Its own command rather than a `NoteProperty`, because a slide is not a
/// *value* a note carries — it changes what the note **is**. A property lane
/// draws a bar per note and a slide has no height; and the compiler treats a
/// slide as a bend rather than as a note at all, which no amount of a value
/// could express. See `Note::slide`.
pub struct SetNoteSlide {
    clip: ClipId,
    ids: Vec<NoteId>,
    slide: bool,
    /// Each note's flag before this ran, in `ids` order. Empty until applied.
    previous: Vec<bool>,
    label: String,
}

impl SetNoteSlide {
    pub fn new(clip: ClipId, ids: Vec<NoteId>, slide: bool) -> Self {
        Self {
            label: note_count_label(if slide { "Slide" } else { "Unslide" }, ids.len()),
            clip,
            ids,
            slide,
            previous: Vec::new(),
        }
    }
}

impl Command for SetNoteSlide {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let slide = self.slide;
        let first = self.previous.is_empty();
        let mut previous = Vec::new();
        let data = notes_of(doc, self.clip)?;
        for id in &self.ids {
            let Some(note) = data.notes.get_mut(*id) else {
                continue;
            };
            previous.push(note.slide);
            note.slide = slide;
        }
        // Only the first apply records what was there: a redo re-runs this
        // from a document the inverse has already moved.
        if first {
            self.previous = previous;
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.previous.is_empty() {
            return Box::new(NotApplied("marking a slide"));
        }
        Box::new(RestoreNoteSlides {
            clip: self.clip,
            ids: self.ids.clone(),
            previous: self.previous.clone(),
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
        std::mem::size_of::<Self>() + self.ids.len() * std::mem::size_of::<NoteId>()
    }
}

/// The inverse half of [`SetNoteSlide`]: puts each note's own flag back.
///
/// Its own command rather than a second `SetNoteSlide`, because a selection
/// that was half slides has no single value to set back to.
struct RestoreNoteSlides {
    clip: ClipId,
    ids: Vec<NoteId>,
    previous: Vec<bool>,
}

impl Command for RestoreNoteSlides {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        for (id, was) in self.ids.iter().zip(self.previous.iter()) {
            if let Some(note) = data.notes.get_mut(*id) {
                note.slide = *was;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(NotApplied("restoring slides"))
    }

    fn label(&self) -> &str {
        "Restore slides"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.ids.len() * std::mem::size_of::<NoteId>()
    }
}

/// Cuts notes in two — the piano roll's slice tool.
///
/// One command rather than "shorten this, add that", because a slice is **one
/// gesture**: a line drawn across a chord cuts every note it crosses, and
/// taking that back should be one press of Ctrl+Z rather than one per note.
///
/// Each cut is `(note, tick)`, in the clip's own ticks. A cut at or outside a
/// note's own edges is **skipped**, not refused: the line that produced it
/// crossed some notes usefully and some not, and a command that failed
/// outright would lose the good cuts along with the empty ones. A zero-length
/// note is a note-on and a note-off at the same sample — silence you can
/// neither see nor select.
pub struct SliceNotes {
    clip: ClipId,
    cuts: Vec<(NoteId, Tick)>,
    /// What each sliced note's length was, in the order the cuts ran, so the
    /// inverse can put the front halves back.
    previous: Vec<(NoteId, Tick)>,
    /// The ids of the second halves, minted on the first apply and re-used on
    /// a redo — the rule every creating command here follows, or the commands
    /// stacked above this one point at nothing.
    created: Vec<NoteId>,
    label: String,
}

impl SliceNotes {
    pub fn new(clip: ClipId, cuts: Vec<(NoteId, Tick)>) -> Self {
        Self {
            label: note_count_label("Cut", cuts.len()),
            clip,
            cuts,
            previous: Vec::new(),
            created: Vec::new(),
        }
    }

    /// The second halves, once this has been applied.
    pub fn created(&self) -> &[NoteId] {
        &self.created
    }
}

impl Command for SliceNotes {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let redo = !self.created.is_empty();
        let data = notes_of(doc, self.clip)?;
        let mut previous = Vec::new();
        let mut created = Vec::new();

        for (id, at) in &self.cuts {
            let Some(note) = data.notes.get(*id) else {
                continue; // a selection can outlive the notes in it
            };
            // Strictly inside: a cut on an edge would make a note of nothing.
            if *at <= note.start || *at >= note.start + note.length {
                continue;
            }
            let mut tail = *note;
            tail.start = *at;
            tail.length = note.start + note.length - *at;
            let head_length = *at - note.start;

            // On a redo, the same id the first apply minted — so the
            // commands stacked above this one still point at something. The
            // cuts run in the same order every time, so position in `created`
            // is the same key both ways.
            let tail_id = match redo
                .then(|| self.created.get(created.len()).copied())
                .flatten()
            {
                Some(id) => {
                    if !data.notes.insert_at(id, tail) {
                        return Err(CommandError("that note id is taken".into()));
                    }
                    id
                }
                None => data.notes.insert(tail),
            };
            created.push(tail_id);

            // The front half last, so the read above is not fighting the write.
            if let Some(note) = data.notes.get_mut(*id) {
                previous.push((*id, note.length));
                note.length = head_length;
            }
        }

        // Only the first apply records what was there: a redo re-runs this
        // from a document the inverse has already moved back.
        if self.previous.is_empty() {
            self.previous = previous;
        }
        self.created = created;
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.created.is_empty() {
            return Box::new(NotApplied("cutting notes"));
        }
        Box::new(MergeSlices {
            clip: self.clip,
            restore: self.previous.clone(),
            remove: self.created.clone(),
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
            + self.cuts.len() * std::mem::size_of::<(NoteId, Tick)>()
            + self.label.len()
    }
}

/// The inverse half of [`SliceNotes`]: takes the second halves away and puts
/// the first halves back to the length they were.
struct MergeSlices {
    clip: ClipId,
    restore: Vec<(NoteId, Tick)>,
    remove: Vec<NoteId>,
}

impl Command for MergeSlices {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let data = notes_of(doc, self.clip)?;
        for id in &self.remove {
            data.notes.remove(*id);
        }
        for (id, length) in &self.restore {
            if let Some(note) = data.notes.get_mut(*id) {
                note.length = *length;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(NotApplied("merging slices"))
    }

    fn label(&self) -> &str {
        "Merge slices"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.remove.len() * std::mem::size_of::<NoteId>()
    }
}

pub struct SetNoteProperty {
    clip: ClipId,
    ids: Vec<NoteId>,
    property: NoteProperty,
    value: i32,
    /// Each note's value before this ran, in `ids` order. Empty until applied,
    /// which is also how `invert` knows it has nothing to say yet.
    previous: Vec<i32>,
    label: String,
}

impl SetNoteProperty {
    pub fn new(clip: ClipId, ids: Vec<NoteId>, property: NoteProperty, value: i32) -> Self {
        Self {
            label: note_count_label(&format!("Set {} of", property.label()), ids.len()),
            clip,
            ids,
            property,
            value,
            previous: Vec::new(),
        }
    }

    pub fn property(&self) -> NoteProperty {
        self.property
    }
}

impl Command for SetNoteProperty {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let property = self.property;
        let data = notes_of(doc, self.clip)?;
        // Checked before anything is written: a command that half-applies is
        // one whose inverse cannot put the document back.
        for id in &self.ids {
            if data.notes.get(*id).is_none() {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            }
        }
        // Only on the first apply. A redo must restore the same `previous` the
        // original run captured, not whatever is there the second time round.
        if self.previous.is_empty() {
            self.previous = self
                .ids
                .iter()
                .filter_map(|id| data.notes.get(*id).map(|n| property.get(n)))
                .collect();
        }
        for id in &self.ids {
            if let Some(note) = data.notes.get_mut(*id) {
                property.set(note, self.value);
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.previous.is_empty() {
            return Box::new(NotApplied("setting a note property"));
        }
        Box::new(RestoreNoteProperties {
            clip: self.clip,
            ids: self.ids.clone(),
            property: self.property,
            values: self.previous.clone(),
            label: self.label.clone(),
        })
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetNoteProperty>() else {
            return false;
        };
        if next.clip != self.clip || next.ids != self.ids || next.property != self.property {
            return false;
        }
        // The new value, the old `previous`: one drag, one entry, one undo back
        // to where it started.
        self.value = next.value;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.previous.len() * std::mem::size_of::<i32>()
            + self.label.len()
    }
}

/// [`SetNoteProperty`]'s inverse: each note back to its own value.
struct RestoreNoteProperties {
    clip: ClipId,
    ids: Vec<NoteId>,
    property: NoteProperty,
    values: Vec<i32>,
    label: String,
}

impl Command for RestoreNoteProperties {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let property = self.property;
        let data = notes_of(doc, self.clip)?;
        for (id, value) in self.ids.iter().zip(&self.values) {
            let Some(note) = data.notes.get_mut(*id) else {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            };
            property.set(note, *value);
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        // Every note went back to its own value, so the forward direction is
        // only expressible as one-value-for-all when they agreed. They did,
        // because that is what `SetNoteProperty` had just done.
        Box::new(SetNoteProperty::new(
            self.clip,
            self.ids.clone(),
            self.property,
            self.values.first().copied().unwrap_or(0),
        ))
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
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.values.len() * std::mem::size_of::<i32>()
            + self.label.len()
    }
}

/// Moves every named note's property by the **same amount**, each from wherever
/// it already was.
///
/// The difference from [`SetNoteProperty`] is the whole point of it: that one
/// writes one value over every note, which flattens exactly the differences
/// somebody adjusting a phrase is adjusting. *"Everything I have selected, ten
/// louder"* keeps the shape and moves it.
///
/// Clamped rather than refused at the ends, because a held stepper means "as
/// far as it goes" and not "an error" — which is why the inverse cannot be
/// "the other way by the same amount": once three notes have all been clamped
/// to 127 their differences are gone from the document. It restores the value
/// each note actually had, like [`SetNoteProperty`]'s does.
pub struct NudgeNoteProperty {
    clip: ClipId,
    ids: Vec<NoteId>,
    property: NoteProperty,
    delta: i32,
    /// Each note's value before the **first** apply, in `ids` order.
    previous: Vec<i32>,
    label: String,
}

impl NudgeNoteProperty {
    pub fn new(clip: ClipId, ids: Vec<NoteId>, property: NoteProperty, delta: i32) -> Self {
        Self {
            label: note_count_label(&format!("Nudge {} of", property.label()), ids.len()),
            clip,
            ids,
            property,
            delta,
            previous: Vec::new(),
        }
    }

    pub fn property(&self) -> NoteProperty {
        self.property
    }
}

impl Command for NudgeNoteProperty {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let property = self.property;
        let data = notes_of(doc, self.clip)?;
        // Checked before anything is written: a command that half-applies is
        // one whose inverse cannot put the document back.
        for id in &self.ids {
            if data.notes.get(*id).is_none() {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            }
        }
        if self.previous.is_empty() {
            self.previous = self
                .ids
                .iter()
                .filter_map(|id| data.notes.get(*id).map(|n| property.get(n)))
                .collect();
        }
        // From the values captured on the first apply rather than from
        // whatever is there now. Without that a **merged** run of nudges
        // would compound: four presses of +1 would move a note by 1, 2, 4, 8
        // as each re-application read the result of the last.
        for (id, was) in self.ids.iter().zip(&self.previous) {
            if let Some(note) = data.notes.get_mut(*id) {
                property.set(note, was.saturating_add(self.delta));
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.previous.is_empty() {
            return Box::new(NotApplied("nudging a note property"));
        }
        Box::new(RestoreNoteProperties {
            clip: self.clip,
            ids: self.ids.clone(),
            property: self.property,
            values: self.previous.clone(),
            label: self.label.clone(),
        })
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<NudgeNoteProperty>() else {
            return false;
        };
        if next.clip != self.clip || next.ids != self.ids || next.property != self.property {
            return false;
        }
        // The deltas add and the captured `previous` stays: a held stepper is
        // one history entry, and one undo goes back to where the press began.
        self.delta += next.delta;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + self.previous.len() * std::mem::size_of::<i32>()
            + self.label.len()
    }
}

/// One value **per note** — what a randomizer produces.
///
/// [`SetNoteProperty`] cannot express it (one value for all) and
/// [`NudgeNoteProperty`] cannot either (one offset for all), and doing it as
/// N commands would be N undo entries for one press.
///
/// Deliberately **not** mergeable: rolling the dice again is a new answer to
/// the same question rather than the continuation of a gesture, and undo
/// should walk back through the rolls one at a time.
pub struct SetNotePropertyEach {
    clip: ClipId,
    ids: Vec<NoteId>,
    property: NoteProperty,
    values: Vec<i32>,
    previous: Vec<i32>,
    label: String,
}

impl SetNotePropertyEach {
    pub fn new(
        clip: ClipId,
        ids: Vec<NoteId>,
        property: NoteProperty,
        values: Vec<i32>,
    ) -> Self {
        Self {
            label: note_count_label(&format!("Set {} of", property.label()), ids.len()),
            clip,
            ids,
            property,
            values,
            previous: Vec::new(),
        }
    }

    pub fn property(&self) -> NoteProperty {
        self.property
    }
}

impl Command for SetNotePropertyEach {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // A list that does not line up with its notes would write the second
        // note's value onto the third — a scramble rather than an error, and
        // the sort of thing that is only noticed a week later.
        if self.values.len() != self.ids.len() {
            return Err(CommandError(format!(
                "{} values for {} notes",
                self.values.len(),
                self.ids.len()
            )));
        }
        let property = self.property;
        let data = notes_of(doc, self.clip)?;
        for id in &self.ids {
            if data.notes.get(*id).is_none() {
                return Err(CommandError(format!("no note {id:?} in this clip")));
            }
        }
        if self.previous.is_empty() {
            self.previous = self
                .ids
                .iter()
                .filter_map(|id| data.notes.get(*id).map(|n| property.get(n)))
                .collect();
        }
        for (id, value) in self.ids.iter().zip(&self.values) {
            if let Some(note) = data.notes.get_mut(*id) {
                // Clamped by `set`, so a value off the end of the range is
                // the end rather than a refusal.
                property.set(note, *value);
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.previous.is_empty() {
            return Box::new(NotApplied("setting a value on each note"));
        }
        Box::new(RestoreNoteProperties {
            clip: self.clip,
            ids: self.ids.clone(),
            property: self.property,
            values: self.previous.clone(),
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
            + self.ids.len() * std::mem::size_of::<NoteId>()
            + (self.values.len() + self.previous.len()) * std::mem::size_of::<i32>()
            + self.label.len()
    }
}

// --- Importing -------------------------------------------------------------

/// One part of a file being brought in: an instrument, a row, and its notes.
#[derive(Debug, Clone)]
pub struct ImportPart {
    /// What the file called it. What the channel, the strip and the
    /// arrangement row are all named — see `fontelle_assets::part_name` for
    /// where it comes from.
    pub name: String,
    /// Already on this project's grid, and relative to the clip's start.
    pub notes: Vec<Note>,
    /// Where the part sits in the stereo field, `-1.0`..=`1.0`.
    pub pan: f32,
    /// The part's level, on the strip made for it.
    pub volume_db: f32,
    pub color: [u8; 4],
}

/// What one part turned into, so the window can go and look at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MadePart {
    pub channel: ChannelId,
    pub lane: LaneId,
    pub track: MixerTrackId,
    pub clip: ClipId,
}

/// Brings one audio file into the arrangement: a row, and a clip on it
/// (TDD §15).
///
/// **One command rather than a [`Compound`]** of `AddLane`/`AddClip`, for the
/// reason [`ImportParts`] is one: the clip has to name the lane this same
/// command is about to mint, and a `Compound` holds commands built before any
/// of them ran. Two entries in the history would be worse still — an undo that
/// leaves an empty row behind is not an undo.
pub struct AddAudioClip {
    label: String,
    name: String,
    data: fontelle_types::AudioClipData,
    start: Tick,
    length: Tick,
    /// What it made, kept so a **redo** puts everything back under the ids it
    /// minted the first time. Anything stacked above this entry names them.
    made: Option<(LaneId, ClipId)>,
}

impl AddAudioClip {
    pub fn new(
        name: impl Into<String>,
        data: fontelle_types::AudioClipData,
        start: Tick,
        length: Tick,
    ) -> Self {
        let name = name.into();
        Self {
            label: format!("Import {name}"),
            name,
            data,
            start,
            length,
            made: None,
        }
    }

    /// The clip it made, once it has been applied.
    pub fn clip(&self) -> Option<ClipId> {
        self.made.map(|(_, clip)| clip)
    }

    /// And the row it put it on.
    pub fn lane(&self) -> Option<LaneId> {
        self.made.map(|(lane, _)| lane)
    }
}

impl Command for AddAudioClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // Past the bottom of the stack, so a file dropped onto a song does not
        // push what is already there down the arrangement.
        let order = doc
            .lanes
            .values()
            .map(|lane| lane.order)
            .max()
            .map_or(0, |highest| highest.saturating_add(1));
        let lane = Lane {
            name: self.name.clone(),
            height: DEFAULT_LANE_HEIGHT,
            color: AUDIO_LANE_COLOR,
            muted: false,
            locked: false,
            order,
        };
        let clip = Clip {
            lane: LaneId::default(),
            start: self.start,
            length: self.length,
            source: ClipSource::Audio(self.data.clone()),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        };

        let (lane_id, clip_id) = match self.made {
            Some((lane_id, clip_id)) => {
                if !doc.lanes.insert_at(lane_id, lane) {
                    return Err(CommandError("that row id is taken".into()));
                }
                let mut clip = clip;
                clip.lane = lane_id;
                if !doc.clips.insert_at(clip_id, clip) {
                    doc.lanes.remove(lane_id);
                    return Err(CommandError("that clip id is taken".into()));
                }
                (lane_id, clip_id)
            }
            None => {
                let lane_id = doc.lanes.insert(lane);
                let mut clip = clip;
                clip.lane = lane_id;
                (lane_id, doc.clips.insert(clip))
            }
        };
        self.made = Some((lane_id, clip_id));
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.made {
            Some((lane, clip)) => Box::new(RemoveAudioClip { lane, clip }),
            None => Box::new(NotApplied("importing a sound")),
        }
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.len()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// What undoing an [`AddAudioClip`] does: the clip and the row it arrived on,
/// both gone.
struct RemoveAudioClip {
    lane: LaneId,
    clip: ClipId,
}

impl Command for RemoveAudioClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        doc.clips.remove(self.clip);
        doc.lanes.remove(self.lane);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        // Never reached: the history holds the `AddAudioClip` and re-applies
        // it for a redo, which is what puts the original ids back.
        Box::new(NotApplied("un-importing a sound"))
    }

    fn label(&self) -> &str {
        "Remove imported sound"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The colour a row made for an imported sound gets.
///
/// A different hue from the blue a note row opens on, so an arrangement of both
/// reads as two kinds of thing at a glance — which is the one question a
/// colour on a row answers.
const AUDIO_LANE_COLOR: [u8; 4] = [0x5f, 0xa8, 0x88, 0xff];

/// How tall a row arrives, matching the one `AddLane` makes.
const DEFAULT_LANE_HEIGHT: f32 = 32.0;

/// Brings a file's parts into the project that is **already open**.
///
/// `fontelle_assets::import_midi` builds a whole new `Project`, which is the
/// right shape for opening a `.mid` as a song and the wrong one for the thing
/// people actually do: dropping a file onto a piece they are working on.
///
/// **One command rather than a [`Compound`]** of `AddChannel`/`AddLane`/
/// `AddClip`, because those cannot be built in advance: the clip has to name
/// the channel id that the `AddChannel` beside it is going to mint, and a
/// `Compound` holds commands that were made before any of them ran. Being one
/// command is also what makes importing eight tracks one entry in the history
/// — eight presses of Ctrl+Z to undo one drop would be a bug report.
pub struct ImportParts {
    parts: Vec<ImportPart>,
    /// What it made, kept so a **redo** puts everything back under the ids it
    /// minted the first time. Without that, anything stacked above this entry
    /// would be pointing at nothing after an undo and a redo.
    made: Vec<MadePart>,
    label: String,
}

impl ImportParts {
    pub fn new(what: impl std::fmt::Display, parts: Vec<ImportPart>) -> Self {
        Self {
            label: format!("Import {what}"),
            // A part with no notes is a row that would arrive empty, which is
            // a thing to tidy up rather than a thing you asked for. MIDI
            // files are full of channels that carry only a program change.
            parts: parts.into_iter().filter(|part| !part.notes.is_empty()).collect(),
            made: Vec::new(),
        }
    }

    /// What this made, once it has been applied. Empty before that.
    pub fn made(&self) -> &[MadePart] {
        &self.made
    }
}

impl Command for ImportParts {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if self.parts.is_empty() {
            return Err(CommandError(
                "there is nothing in that file to import".into(),
            ));
        }
        // Past the bottom of the stack, so a file dropped onto a song does
        // not push what is already there down the arrangement.
        let mut order = doc
            .lanes
            .values()
            .map(|lane| lane.order)
            .max()
            .map_or(0, |highest| highest.saturating_add(1));

        // A redo re-uses the ids of the first run; a first run mints them.
        let redoing = !self.made.is_empty();
        let mut made = Vec::with_capacity(self.parts.len());

        for (index, part) in self.parts.iter().enumerate() {
            let previous = redoing.then(|| self.made[index]);

            let mut track = MixerTrack::new(part.name.clone());
            track.gain_db = part.volume_db;
            track.output = doc.mixer.master;
            let track_id = match previous {
                Some(ids) => {
                    if !doc.mixer.tracks.insert_at(ids.track, track) {
                        return Err(CommandError("that mixer track id is taken".into()));
                    }
                    ids.track
                }
                None => doc.mixer.tracks.insert(track),
            };

            let channel = Channel {
                name: part.name.clone(),
                color: part.color,
                // A file's parts really do each want a strip: it carries a
                // level per part, and that is a fader.
                mixer_track: Some(track_id),
                patch_data: None,
                pan: part.pan,
                muted: false,
                soloed: false,
                named_keys: false,
                gain_db: 0.0,
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
                height: 32.0,
                color: part.color,
                muted: false,
                locked: false,
                order,
            };
            order = order.saturating_add(1);
            let lane_id = match previous {
                Some(ids) => {
                    if !doc.lanes.insert_at(ids.lane, lane) {
                        return Err(CommandError("that lane id is taken".into()));
                    }
                    ids.lane
                }
                None => doc.lanes.insert(lane),
            };

            let mut notes = Arena::default();
            let mut length = 0;
            for note in &part.notes {
                length = length.max(note.start + note.length);
                notes.insert(*note);
            }
            let clip = Clip {
                lane: lane_id,
                start: 0,
                length,
                source: ClipSource::Notes(NoteData {
                    channel: channel_id,
                    notes,
                }),
                prefab_link: None,
                color: None,
                muted: false,
                loop_length: None,
            };
            let clip_id = match previous {
                Some(ids) => {
                    if !doc.clips.insert_at(ids.clip, clip) {
                        return Err(CommandError("that clip id is taken".into()));
                    }
                    ids.clip
                }
                None => doc.clips.insert(clip),
            };

            made.push(MadePart {
                channel: channel_id,
                lane: lane_id,
                track: track_id,
                clip: clip_id,
            });
        }

        self.made = made;
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        if self.made.is_empty() {
            return Box::new(NotApplied("importing a file"));
        }
        Box::new(UnimportParts {
            made: self.made.clone(),
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
            + self.label.len()
            + self
                .parts
                .iter()
                .map(|part| part.name.len() + part.notes.len() * std::mem::size_of::<Note>())
                .sum::<usize>()
    }
}

/// [`ImportParts`]'s inverse: everything it made, taken back out.
///
/// **Newest first**, so a clip is gone before the lane it names is.
struct UnimportParts {
    made: Vec<MadePart>,
    label: String,
}

impl Command for UnimportParts {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        for ids in self.made.iter().rev() {
            doc.clips.remove(ids.clip);
            doc.lanes.remove(ids.lane);
            doc.channels.remove(ids.channel);
            doc.mixer.tracks.remove(ids.track);
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        // Not expressible as an `ImportParts` — the parts themselves are
        // gone. Redoing an undone import goes through `History::redo`, which
        // re-applies the original command rather than inverting this one.
        Box::new(NotApplied("taking an import back out"))
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
            + self.made.len() * std::mem::size_of::<MadePart>()
            + self.label.len()
    }
}

// --- Clips -----------------------------------------------------------------

pub struct AddClip {
    clip: Clip,
    created: Option<ClipId>,
}

impl AddClip {
    pub fn new(clip: Clip) -> Self {
        Self {
            clip,
            created: None,
        }
    }

    pub fn id(&self) -> Option<ClipId> {
        self.created
    }
}

impl Command for AddClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        match self.created {
            Some(id) => {
                if !doc.clips.insert_at(id, self.clip.clone()) {
                    return Err(CommandError("that clip id is taken".into()));
                }
            }
            None => self.created = Some(doc.clips.insert(self.clip.clone())),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some(id) => Box::new(RemoveClip::new(id)),
            None => Box::new(NotApplied("adding a clip")),
        }
    }

    fn label(&self) -> &str {
        "Add clip"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<Clip>()
    }
}

pub struct RemoveClip {
    clip: ClipId,
    removed: Option<Clip>,
}

impl RemoveClip {
    pub fn new(clip: ClipId) -> Self {
        Self {
            clip,
            removed: None,
        }
    }
}

impl Command for RemoveClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        match doc.clips.remove(self.clip) {
            Some(clip) => {
                self.removed = Some(clip);
                Ok(())
            }
            None => Err(no_clip(self.clip)),
        }
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(clip) => Box::new(RestoreClip {
                id: self.clip,
                clip: clip.clone(),
            }),
            None => Box::new(NotApplied("deleting a clip")),
        }
    }

    fn label(&self) -> &str {
        "Delete clip"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<Clip>()
    }
}

/// The inverse half of [`RemoveClip`].
struct RestoreClip {
    id: ClipId,
    clip: Clip,
}

impl Command for RestoreClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc.clips.insert_at(self.id, self.clip.clone()) {
            return Err(CommandError("that clip id is taken".into()));
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveClip::new(self.id))
    }

    fn label(&self) -> &str {
        "Restore clip"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<Clip>()
    }
}

/// Moves a clip along the timeline and, optionally, onto another lane.
pub struct MoveClip {
    clip: ClipId,
    tick_delta: Tick,
    lane: Option<LaneId>,
    previous_lane: Option<LaneId>,
}

impl MoveClip {
    pub fn new(clip: ClipId, tick_delta: Tick, lane: Option<LaneId>) -> Self {
        Self {
            clip,
            tick_delta,
            lane,
            previous_lane: None,
        }
    }
}

impl Command for MoveClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if let Some(lane) = self.lane
            && !doc.lanes.contains_key(lane)
        {
            return Err(CommandError(format!("no lane {lane:?} in this project")));
        }
        let Some(clip) = doc.clips.get_mut(self.clip) else {
            return Err(no_clip(self.clip));
        };
        if clip.start + self.tick_delta < 0 {
            return Err(CommandError(
                "that would move a clip before the start".into(),
            ));
        }
        clip.start += self.tick_delta;
        if let Some(lane) = self.lane {
            self.previous_lane = Some(clip.lane);
            clip.lane = lane;
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(MoveClip::new(
            self.clip,
            -self.tick_delta,
            self.previous_lane,
        ))
    }

    fn label(&self) -> &str {
        "Move clip"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<MoveClip>() else {
            return false;
        };
        if next.clip != self.clip {
            return false;
        }
        self.tick_delta += next.tick_delta;
        // The lane the drag started from is the one to go back to, so an
        // absorbed lane change keeps the *first* previous lane.
        if next.lane.is_some() {
            if self.lane.is_none() {
                self.previous_lane = next.previous_lane;
            }
            self.lane = next.lane;
        }
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// A clip's length, dragged by its right-hand edge on the arrangement.
///
/// A delta rather than a length, for the same reason [`MoveClip`] is: that is
/// what lets a drag coalesce into one history entry (§10.6). Clamped so a clip
/// can never reach zero length — a clip with no width is one nobody can grab
/// again, and the only way back would be an undo the user has no reason to
/// know they need.
pub struct ResizeClip {
    clip: ClipId,
    tick_delta: Tick,
    /// What the length actually became, which is not `start + delta` when the
    /// clamp bit. The inverse is measured from this.
    applied: Option<Tick>,
    previous: Option<Tick>,
}

/// The shortest a clip may be dragged: a sixteenth, which is one cell of the
/// roll's default grid.
pub const MIN_CLIP_LENGTH: Tick = fontelle_types::PPQN / 4;

/// Makes a clip repeat its content, or stops it.
///
/// **Not a copy.** A copy makes new clips with their own notes; this is one
/// clip whose one set of notes plays again every `loop_length` until the clip
/// runs out — so editing bar 1 changes every repeat, which is the whole point
/// and the thing a copy can never give you. See [`Clip::loop_length`].
pub struct SetClipLoop {
    clip: ClipId,
    loop_length: Option<Tick>,
    previous: Option<Option<Tick>>,
}

impl SetClipLoop {
    pub fn new(clip: ClipId, loop_length: Option<Tick>) -> Self {
        Self {
            clip,
            loop_length,
            previous: None,
        }
    }
}

impl Command for SetClipLoop {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // A period of nothing repeats for ever in no time at all, which is an
        // infinite loop in the compiler rather than a musical statement.
        if self.loop_length.is_some_and(|length| length <= 0) {
            return Err(CommandError(
                "a loop has to be some length — a period of zero repeats for ever".into(),
            ));
        }
        let Some(clip) = doc.clips.get_mut(self.clip) else {
            return Err(no_clip(self.clip));
        };
        let previous = std::mem::replace(&mut clip.loop_length, self.loop_length);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetClipLoop::new(self.clip, previous)),
            None => Box::new(NotApplied("looping a clip")),
        }
    }

    fn label(&self) -> &str {
        "Loop clip"
    }

    /// Dragging a loop out is one gesture, so it is one undo entry — the same
    /// rule every dragged value on the arrangement follows.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetClipLoop>() else {
            return false;
        };
        if next.clip != self.clip {
            return false;
        }
        self.loop_length = next.loop_length;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl ResizeClip {
    pub fn new(clip: ClipId, tick_delta: Tick) -> Self {
        Self {
            clip,
            tick_delta,
            applied: None,
            previous: None,
        }
    }
}

impl Command for ResizeClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(clip) = doc.clips.get_mut(self.clip) else {
            return Err(no_clip(self.clip));
        };
        let previous = clip.length;
        clip.length = (previous + self.tick_delta).max(MIN_CLIP_LENGTH);
        self.previous.get_or_insert(previous);
        self.applied = Some(clip.length);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match (self.previous, self.applied) {
            (Some(previous), Some(applied)) => {
                Box::new(ResizeClip::new(self.clip, previous - applied))
            }
            _ => Box::new(NotApplied("resizing a clip")),
        }
    }

    fn label(&self) -> &str {
        "Resize clip"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<ResizeClip>() else {
            return false;
        };
        if next.clip != self.clip {
            return false;
        }
        // The new end, the old start: one drag, one entry, one undo back to
        // the length it had before the drag began.
        self.tick_delta += next.tick_delta;
        self.applied = next.applied;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Copies a clip, notes and all, `tick_offset` further along the same lane.
///
/// The copy's notes keep the ids they had inside the original clip, because a
/// clip owns its own note arena — two clips holding a note with the same id is
/// no more a collision than two files holding a line 1.
pub struct DuplicateClip {
    source: ClipId,
    tick_offset: Tick,
    created: Option<ClipId>,
}

impl DuplicateClip {
    pub fn new(source: ClipId, tick_offset: Tick) -> Self {
        Self {
            source,
            tick_offset,
            created: None,
        }
    }

    pub fn id(&self) -> Option<ClipId> {
        self.created
    }
}

impl Command for DuplicateClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(source) = doc.clips.get(self.source) else {
            return Err(no_clip(self.source));
        };
        let mut copy = source.clone();
        copy.start += self.tick_offset;
        if copy.start < 0 {
            return Err(CommandError(
                "that would place a copy before the start".into(),
            ));
        }
        match self.created {
            Some(id) => {
                if !doc.clips.insert_at(id, copy) {
                    return Err(CommandError("that clip id is taken".into()));
                }
            }
            None => self.created = Some(doc.clips.insert(copy)),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some(id) => Box::new(RemoveClip::new(id)),
            None => Box::new(NotApplied("duplicating a clip")),
        }
    }

    fn label(&self) -> &str {
        "Duplicate clip"
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

// --- Single values ---------------------------------------------------------

/// Cuts a clip in two at a song tick — the arrangement's cut tool (`C`).
///
/// *"theres no tool for cutting up clips in the arrangement right now (should be
/// c key) should work like the same tool in fl studio and correctly split up
/// looped clips and everything taking into account all edge cases cleanly."*
///
/// One command, so a cut is one Ctrl+Z. The left half is the clip you cut — it
/// keeps its id, so anything pointing at it (the roll's open clip, a selection)
/// still points at something — and the right half is a new clip beginning at
/// the cut.
///
/// # The edge cases, spelled out
///
/// - **A cut on either edge, or outside, is not a cut.** It would make a clip
///   of nothing. It is not an error either: a cut tool swept across a row
///   crosses gaps, and the gaps are not mistakes.
/// - **A note lying across the cut is cut too**, into a head in the left half
///   and a tail at the top of the right one — the same reading
///   [`SliceNotes`] gives in the piano roll, and what makes the two halves
///   sound like the one clip did.
/// - **A looped clip stays two looped clips.** Looping is one set of notes
///   played again every period (see [`crate::Clip::loop_length`]), so both
///   halves keep the period. If the cut lands part-way through a pass, the
///   right half's content is **rotated** to the phase the loop was at — a
///   second half that restarted the pattern would be a cut you can hear.
/// - **An automation clip** cuts by its points, each half keeping the ones
///   that fall in it.
pub struct SplitClip {
    clip: ClipId,
    /// In **song** ticks, not the clip's own: the arrangement is where the cut
    /// is aimed, and the clip's start is what turns one into the other.
    at: Tick,
    /// What the left half looked like before, so the inverse can put the one
    /// clip back exactly as it was.
    previous: Option<Clip>,
    /// The right half, minted once and re-used on a redo.
    created: Option<ClipId>,
}

impl SplitClip {
    pub fn new(clip: ClipId, at: Tick) -> Self {
        Self {
            clip,
            at,
            previous: None,
            created: None,
        }
    }

    /// The right-hand half, once this has been applied and it made one.
    pub fn created(&self) -> Option<ClipId> {
        self.created
    }
}

impl Command for SplitClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let Some(original) = doc.clips.get(self.clip).cloned() else {
            return Err(CommandError(format!("no clip {:?}", self.clip)));
        };
        // Strictly inside. `<=` and `>=` rather than `<` and `>` because a cut
        // on an edge would leave a clip of zero length, which is a clip you
        // cannot see, cannot grab and cannot delete.
        if self.at <= original.start || self.at >= original.start + original.length {
            self.created = None;
            return Ok(());
        }
        let offset = self.at - original.start;

        let mut left = original.clone();
        left.length = offset;
        let mut right = original.clone();
        right.start = self.at;
        right.length = original.length - offset;

        match (&original.source, &mut left.source, &mut right.source) {
            (ClipSource::Notes(source), ClipSource::Notes(head), ClipSource::Notes(tail)) => {
                let (front, back) = split_notes(source, offset, original.loop_length);
                head.notes = front;
                tail.notes = back;
                // **The piece you cut off stops being a loop.** Reported from
                // using the window: *"we should do more of what garage band
                // does where you split the cut section from the rest of the
                // loops so the rest remains a looped clip and the first part
                // is just a cut clip of what you made."*
                //
                // Its notes are the repeats written out (see `split_notes`),
                // so it plays exactly what it did — which is the other half of
                // the same report: *"without making any edits the user didnt
                // intend to make themselves"*. Left as a loop it would be a
                // second thing that repeats when you drag its edge, which is
                // not what somebody who cut a piece off asked for.
                left.loop_length = None;
            }
            (
                ClipSource::Automation(source),
                ClipSource::Automation(head),
                ClipSource::Automation(tail),
            ) => {
                head.points = Arena::default();
                tail.points = Arena::default();
                // **The value the curve actually has where the cut lands**,
                // read before the points are dealt out — after that, neither
                // half has both sides of the seam to interpolate between.
                //
                // Without this a ramp cut in the middle *steps*: the head ends
                // at its last point and holds, the tail begins at its first,
                // and the two halves do not sound like the clip they came
                // from. A note lying across the cut is cut in two for exactly
                // the same reason.
                let seam = source.value_at(offset);
                // The shape of the segment the cut falls *inside*, so the
                // curve either side of the new point keeps the bend it had.
                // `curve` describes the segment following its point, so this
                // is the last point at or before the cut.
                let (curve, tension) = source
                    .points
                    .values()
                    .filter(|point| point.tick <= offset)
                    .max_by_key(|point| point.tick)
                    .map_or((crate::automation::CurveShape::Linear, 0.0), |point| {
                        (point.curve, point.tension)
                    });

                let mut tail_starts_on_a_point = false;
                for point in source.points.values() {
                    if point.tick < offset {
                        head.points.insert(*point);
                    } else {
                        let mut moved = *point;
                        moved.tick -= offset;
                        tail_starts_on_a_point |= moved.tick == 0;
                        tail.points.insert(moved);
                    }
                }

                if let Some(value) = seam {
                    // The head ends *on* the seam. Nothing can already be
                    // there: only points strictly before the cut came here.
                    head.points.insert(crate::automation::AutomationPoint {
                        tick: offset,
                        value,
                        curve,
                        tension,
                    });
                    // And the tail starts on it — unless a point already sat
                    // exactly on the cut, in which case that *is* the seam and
                    // a second one there would be two points at one tick.
                    if !tail_starts_on_a_point {
                        tail.points.insert(crate::automation::AutomationPoint {
                            tick: 0,
                            value,
                            curve,
                            tension,
                        });
                    }
                }
            }
            // An audio clip has nothing to divide yet (§15); the two halves
            // are the extent, which is already set above.
            _ => {}
        }

        self.previous.get_or_insert(original);
        doc.clips
            .get_mut(self.clip)
            .map(|clip| *clip = left)
            .ok_or_else(|| CommandError("that clip went away".into()))?;
        match self.created {
            Some(id) => {
                if !doc.clips.insert_at(id, right) {
                    return Err(CommandError("that clip id is taken".into()));
                }
            }
            None => self.created = Some(doc.clips.insert(right)),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match (&self.previous, self.created) {
            (Some(previous), Some(created)) => Box::new(UnsplitClip {
                clip: self.clip,
                previous: previous.clone(),
                created,
            }),
            // A cut that missed changed nothing, so its inverse is nothing —
            // and it has to be a real command rather than a refusal, because
            // the history will run it.
            (_, None) => Box::new(Compound::new("Undo cut", Vec::new())),
            _ => Box::new(NotApplied("cutting a clip")),
        }
    }

    fn label(&self) -> &str {
        "Cut clip"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<Clip>()
    }
}

/// The inverse half of [`SplitClip`]: the right half goes, the left one is put
/// back as it was. Not a user-facing command.
struct UnsplitClip {
    clip: ClipId,
    previous: Clip,
    created: ClipId,
}

impl Command for UnsplitClip {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        doc.clips.remove(self.created);
        doc.clips
            .get_mut(self.clip)
            .map(|clip| *clip = self.previous.clone())
            .ok_or_else(|| CommandError("that clip went away".into()))
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(NotApplied("undoing a cut"))
    }

    fn label(&self) -> &str {
        "Undo cut"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<Clip>()
    }
}

/// Divides a clip's notes at `offset` — in the clip's own ticks — into the
/// notes of the two halves.
///
/// `period` is the clip's loop length, and it changes what the second half
/// *is*. Without one, the notes simply belong to the side they fall on and a
/// note across the cut is split. With one, the content repeats every `period`,
/// so the second half plays the same pattern **rotated** to the phase the cut
/// landed on: a cut two beats into a one-bar loop leaves a half whose first
/// note is whatever was on beat three.
/// A looped clip's passes written out as real notes, over `length` ticks.
///
/// What the clip **sounds like**, by the compiler's own rules, so a clip that
/// stops being a loop keeps playing what it played: a note starting at or past
/// the period is content the loop does not contain and is left out, and a note
/// still ringing at the end is cut there, exactly as
/// `fontelle_sequencer::compile` does it. Two answers to "what does this loop
/// play" would be a place for them to disagree, and the disagreement is a cut
/// that changes the song.
fn flatten_loop(source: &NoteData, period: Tick, length: Tick) -> Arena<NoteId, Note> {
    let mut out = Arena::default();
    if period <= 0 || length <= 0 {
        return out;
    }
    let mut base = 0;
    while base < length {
        for note in source.notes.values() {
            if note.start >= period {
                continue;
            }
            let start = base + note.start;
            if start >= length {
                continue;
            }
            let mut copy = *note;
            copy.start = start;
            copy.length = note.length.min(length - start);
            if copy.length > 0 {
                out.insert(copy);
            }
        }
        base += period;
    }
    out
}

fn split_notes(
    source: &NoteData,
    offset: Tick,
    period: Option<Tick>,
) -> (Arena<NoteId, Note>, Arena<NoteId, Note>) {
    let mut front = Arena::default();
    let mut back = Arena::default();

    match period.filter(|p| *p > 0) {
        // A loop. The **front** is the passes written out — it is a plain clip
        // now (see `SplitClip::apply`), so the pattern alone would go silent
        // after its first pass, and going silent is an edit nobody asked for.
        // The **back** is still the loop, turned round to the point in the
        // pattern the cut fell on, so it carries on saying what it was saying.
        Some(period) => {
            front = flatten_loop(source, period, offset);
            let phase = offset.rem_euclid(period);
            for note in source.notes.values() {
                if phase == 0 {
                    back.insert(*note);
                    continue;
                }
                if note.start >= phase {
                    let mut moved = *note;
                    moved.start -= phase;
                    back.insert(moved);
                } else if note.start + note.length > phase {
                    // It is sounding when the second half begins, so the
                    // second half starts part-way through it.
                    let mut tail = *note;
                    tail.length = note.start + note.length - phase;
                    tail.start = 0;
                    back.insert(tail);
                    // And the head of it comes round again at the end of the
                    // pattern, where it always was.
                    let mut head = *note;
                    head.start = note.start + period - phase;
                    back.insert(head);
                } else {
                    let mut moved = *note;
                    moved.start = note.start + period - phase;
                    back.insert(moved);
                }
            }
        }
        // Not a loop: each note belongs to the side it falls on, and one lying
        // across the cut is cut.
        None => {
            for note in source.notes.values() {
                if note.start >= offset {
                    let mut moved = *note;
                    moved.start -= offset;
                    back.insert(moved);
                } else if note.start + note.length > offset {
                    let mut head = *note;
                    head.length = offset - note.start;
                    front.insert(head);
                    let mut tail = *note;
                    tail.start = 0;
                    tail.length = note.start + note.length - offset;
                    back.insert(tail);
                } else {
                    front.insert(*note);
                }
            }
        }
    }
    (front, back)
}


/// A continuous value one command can set.
///
/// One command over a typed address rather than four near-identical structs:
/// the only thing that differs between them is where the value lives, and
/// keeping numbers and flags apart is what makes it impossible to write a
/// boolean into a fader.
///
/// TDD §8.2 wants this addressed by `ParamAddress` eventually — one scheme for
/// automation, MIDI learn, plugin parameters, presets and undo. That needs the
/// `PersistentId` half of §10.2, which nothing in `Project` carries yet, so
/// these are typed for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberTarget {
    TrackGainDb(MixerTrackId),
    TrackPan(MixerTrackId),
    ChannelPan(ChannelId),
    /// The channel's own level, which is **not** its mixer track's — see
    /// [`crate::Channel::gain_db`].
    ChannelGainDb(ChannelId),
    /// The tempo in force at the start of the piece. Later tempo changes are
    /// left alone: an imported file's curve must survive somebody nudging the
    /// BPM box.
    Tempo,
    /// How many beats there are in a bar — `Project::beats_per_bar`.
    ///
    /// A whole number carried in the `f64` every other target uses, rounded on
    /// the way in and clamped to a bar a person could count. A second command
    /// type for one integer would duplicate the gesture coalescing that makes
    /// a dragged value one undo entry, which is the whole of what this command
    /// is for.
    BeatsPerBar,
}

pub struct SetNumber {
    target: NumberTarget,
    value: f64,
    previous: Option<f64>,
    label: String,
}

impl SetNumber {
    pub fn new(target: NumberTarget, value: f64) -> Self {
        Self {
            label: match target {
                NumberTarget::TrackGainDb(_) => "Set level",
                NumberTarget::TrackPan(_) => "Set track pan",
                NumberTarget::ChannelPan(_) => "Set pan",
                NumberTarget::ChannelGainDb(_) => "Set level",
                NumberTarget::Tempo => "Set tempo",
                NumberTarget::BeatsPerBar => "Set time signature",
            }
            .to_string(),
            target,
            value,
            previous: None,
        }
    }
}

impl Command for SetNumber {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let previous = match self.target {
            NumberTarget::TrackGainDb(id) => {
                let track = doc
                    .mixer
                    .tracks
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no mixer track {id:?}")))?;
                std::mem::replace(&mut track.gain_db, self.value as f32) as f64
            }
            NumberTarget::TrackPan(id) => {
                let track = doc
                    .mixer
                    .tracks
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no mixer track {id:?}")))?;
                std::mem::replace(&mut track.pan, self.value as f32) as f64
            }
            NumberTarget::ChannelPan(id) => {
                let channel = doc
                    .channels
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no channel {id:?}")))?;
                std::mem::replace(&mut channel.pan, self.value as f32) as f64
            }
            NumberTarget::ChannelGainDb(id) => {
                let channel = doc
                    .channels
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no channel {id:?}")))?;
                std::mem::replace(&mut channel.gain_db, self.value as f32) as f64
            }
            NumberTarget::Tempo => {
                let previous = doc.tempo_map.tempo_at(0);
                let mut segments = doc.tempo_map.segments().to_vec();
                segments[0].bpm = self.value;
                let rate = doc.tempo_map.sample_rate_hz();
                doc.tempo_map = crate::project::TempoMap::from_segments(segments, rate);
                previous
            }
            NumberTarget::BeatsPerBar => {
                // Clamped here rather than trusted from the caller: this is
                // the only way into the field, and a bar of zero beats divides
                // by zero in every grid that counts by it.
                let beats = (self.value.round() as i64).clamp(1, 16) as u32;
                std::mem::replace(&mut doc.beats_per_bar, beats) as f64
            }
        };
        // Only the *first* apply records what was there. A coalesced gesture
        // is one entry whose inverse has to reach back past every step of it,
        // and a redo re-runs this from a document the inverse already moved.
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetNumber::new(self.target, previous)),
            None => Box::new(NotApplied("setting a value")),
        }
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetNumber>() else {
            return false;
        };
        if next.target != self.target {
            return false;
        }
        // Keep the earliest `previous` and the latest value: the merged entry
        // has to undo the whole gesture.
        self.value = next.value;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.label.len()
    }
}

/// A boolean one command can set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagTarget {
    TrackMute(MixerTrackId),
    TrackSolo(MixerTrackId),
    ClipMuted(ClipId),
    /// A sequencer mute, not a mixer operation (TDD §10.3).
    LaneMuted(LaneId),
    /// The channel rack's own mute. Also a sequencer mute, and it **has** to
    /// be: since a channel plays through the master by default, a rack switch
    /// that reached for the mixer track would silence the whole song.
    ChannelMuted(ChannelId),
    /// And its solo. The mixer keeps its own, over tracks; the two answer
    /// different questions and a project may want both.
    ChannelSoloed(ChannelId),
    /// Draw the roll's key strip as a list of names rather than as a keyboard
    /// — a per-channel view, saved with the song (TDD §16.4).
    ChannelNamedKeys(ChannelId),
}

pub struct SetFlag {
    target: FlagTarget,
    value: bool,
    previous: Option<bool>,
}

impl SetFlag {
    pub fn new(target: FlagTarget, value: bool) -> Self {
        Self {
            target,
            value,
            previous: None,
        }
    }
}

impl Command for SetFlag {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let previous = match self.target {
            FlagTarget::TrackMute(id) => {
                let track = doc
                    .mixer
                    .tracks
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no mixer track {id:?}")))?;
                std::mem::replace(&mut track.mute, self.value)
            }
            FlagTarget::TrackSolo(id) => {
                let track = doc
                    .mixer
                    .tracks
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no mixer track {id:?}")))?;
                std::mem::replace(&mut track.solo, self.value)
            }
            FlagTarget::ClipMuted(id) => {
                let clip = doc.clips.get_mut(id).ok_or_else(|| no_clip(id))?;
                std::mem::replace(&mut clip.muted, self.value)
            }
            FlagTarget::LaneMuted(id) => {
                let lane = doc
                    .lanes
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no lane {id:?}")))?;
                std::mem::replace(&mut lane.muted, self.value)
            }
            FlagTarget::ChannelMuted(id) => {
                let channel = doc
                    .channels
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no channel {id:?}")))?;
                std::mem::replace(&mut channel.muted, self.value)
            }
            FlagTarget::ChannelSoloed(id) => {
                let channel = doc
                    .channels
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no channel {id:?}")))?;
                std::mem::replace(&mut channel.soloed, self.value)
            }
            FlagTarget::ChannelNamedKeys(id) => {
                let channel = doc
                    .channels
                    .get_mut(id)
                    .ok_or_else(|| CommandError(format!("no channel {id:?}")))?;
                std::mem::replace(&mut channel.named_keys, self.value)
            }
        };
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetFlag::new(self.target, previous)),
            None => Box::new(NotApplied("setting a switch")),
        }
    }

    fn label(&self) -> &str {
        match (self.target, self.value) {
            (FlagTarget::TrackSolo(_), true) => "Solo",
            (FlagTarget::TrackSolo(_), false) => "Unsolo",
            (_, true) => "Mute",
            (_, false) => "Unmute",
        }
    }

    /// Never. A toggle is not a drag, and merging two presses would swallow
    /// one of them.
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

/// Sets, or clears, the loop region.
pub struct SetLoopRange {
    range: Option<(Tick, Tick)>,
    previous: Option<Option<(Tick, Tick)>>,
}

impl SetLoopRange {
    pub fn new(range: Option<(Tick, Tick)>) -> Self {
        Self {
            range,
            previous: None,
        }
    }
}

impl Command for SetLoopRange {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if let Some((from, to)) = self.range
            && to <= from
        {
            return Err(CommandError("a loop has to end after it starts".into()));
        }
        let previous = std::mem::replace(&mut doc.loop_range, self.range);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetLoopRange::new(previous)),
            None => Box::new(NotApplied("setting the loop")),
        }
    }

    fn label(&self) -> &str {
        match self.range {
            Some(_) => "Set loop",
            None => "Clear loop",
        }
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetLoopRange>() else {
            return false;
        };
        self.range = next.range;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

// ============================================================ insert chains

/// Puts an effect on the end of a track's insert chain (TDD §13.4).
///
/// The end, not the middle: a chain is an order and the order is the sound, so
/// an effect that inserted itself part-way would rearrange a mix that was
/// already balanced. Moving it up is [`MoveInsert`]'s job and a separate
/// gesture.
pub struct AddInsert {
    track: MixerTrackId,
    kind: fontelle_types::EffectKind,
    /// Where it landed, so the undo knows which one to take away.
    added: Option<usize>,
}

impl AddInsert {
    pub fn new(track: MixerTrackId, kind: fontelle_types::EffectKind) -> Self {
        Self {
            track,
            kind,
            added: None,
        }
    }
}

impl Command for AddInsert {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        track.inserts.push(crate::EffectSlot::new(self.kind));
        self.added = Some(track.inserts.len() - 1);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.added {
            Some(index) => Box::new(RemoveInsert::new(self.track, index)),
            None => Box::new(NotApplied("adding an effect")),
        }
    }

    fn label(&self) -> &str {
        "Add effect"
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

/// Takes one effect off a chain, keeping it for the undo.
///
/// The whole slot, not its kind: an undo that put back a fresh EQ where a
/// tuned one used to be would be a command that lost work while claiming not
/// to. And at the index it came from, because the order is the sound.
pub struct RemoveInsert {
    track: MixerTrackId,
    index: usize,
    removed: Option<crate::EffectSlot>,
}

impl RemoveInsert {
    pub fn new(track: MixerTrackId, index: usize) -> Self {
        Self {
            track,
            index,
            removed: None,
        }
    }
}

impl Command for RemoveInsert {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        if self.index >= track.inserts.len() {
            return Err(CommandError(format!(
                "no insert {} on that track",
                self.index
            )));
        }
        let removed = track.inserts.remove(self.index);
        self.removed.get_or_insert(removed);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(slot) => Box::new(RestoreInsert {
                track: self.track,
                index: self.index,
                slot: slot.clone(),
            }),
            None => Box::new(NotApplied("removing an effect")),
        }
    }

    fn label(&self) -> &str {
        "Remove effect"
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

/// The undo of a [`RemoveInsert`]: this exact effect, back where it was.
pub struct RestoreInsert {
    track: MixerTrackId,
    index: usize,
    slot: crate::EffectSlot,
}

impl RestoreInsert {
    pub fn new(track: MixerTrackId, index: usize, slot: crate::EffectSlot) -> Self {
        Self { track, index, slot }
    }
}

impl Command for RestoreInsert {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let index = self.index.min(track.inserts.len());
        track.inserts.insert(index, self.slot.clone());
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveInsert::new(self.track, self.index))
    }

    fn label(&self) -> &str {
        "Restore effect"
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

/// Drags one effect to another place in the chain.
pub struct MoveInsert {
    track: MixerTrackId,
    from: usize,
    to: usize,
}

impl MoveInsert {
    pub fn new(track: MixerTrackId, from: usize, to: usize) -> Self {
        Self { track, from, to }
    }
}

impl Command for MoveInsert {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let len = track.inserts.len();
        if self.from >= len || self.to >= len {
            return Err(CommandError("no such insert".to_string()));
        }
        // Refused rather than accepted as a no-op: a drag that ended where it
        // started is not an edit, and letting it through would put an entry in
        // the history that undoes to the same thing.
        if self.from == self.to {
            return Err(CommandError("that is where it already is".to_string()));
        }
        let slot = track.inserts.remove(self.from);
        track.inserts.insert(self.to, slot);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(MoveInsert::new(self.track, self.to, self.from))
    }

    fn label(&self) -> &str {
        "Reorder effects"
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

/// Switches one insert out of the chain, or back into it.
pub struct SetInsertBypassed {
    track: MixerTrackId,
    index: usize,
    bypassed: bool,
    previous: Option<bool>,
}

impl SetInsertBypassed {
    pub fn new(track: MixerTrackId, index: usize, bypassed: bool) -> Self {
        Self {
            track,
            index,
            bypassed,
            previous: None,
        }
    }
}

impl Command for SetInsertBypassed {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.index)
            .ok_or_else(|| CommandError(format!("no insert {}", self.index)))?;
        let previous = std::mem::replace(&mut slot.bypassed, self.bypassed);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetInsertBypassed::new(self.track, self.index, previous)),
            None => Box::new(NotApplied("bypassing an effect")),
        }
    }

    fn label(&self) -> &str {
        "Bypass effect"
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

/// Moves one parameter of one insert, by its stable id (INVARIANT 7).
///
/// The generic one: `SetEqBand` writes a whole band because an EQ's window
/// draws a band as a thing, and this writes a **single number** because every
/// other effect's window is a grid of knobs read straight off
/// [`EffectConfig::specs`](fontelle_types::EffectConfig::specs). An effect
/// added later needs no command of its own.
///
/// Normalised, 0..1, for the reason automation is: the taper between a dial's
/// fraction and a value in decibels or seconds is a property of the parameter,
/// and `EffectConfig` already owns it.
pub struct SetInsertParam {
    track: MixerTrackId,
    index: usize,
    param: String,
    value: f32,
    previous: Option<f32>,
}

impl SetInsertParam {
    pub fn new(track: MixerTrackId, index: usize, param: impl Into<String>, value: f32) -> Self {
        Self {
            track,
            index,
            param: param.into(),
            value,
            previous: None,
        }
    }
}

impl Command for SetInsertParam {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.index)
            .ok_or_else(|| CommandError(format!("no insert {}", self.index)))?;
        // A parameter this effect does not have is refused rather than
        // silently ignored: unlike a *patch* address, which may legitimately
        // come from a later build's project file, this comes from a panel that
        // read `specs()` a moment ago.
        let previous = slot
            .config
            .normalised(&self.param)
            .ok_or_else(|| CommandError(format!("no parameter {}", self.param)))?;
        slot.config.set_normalised(&self.param, self.value);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetInsertParam::new(
                self.track,
                self.index,
                self.param.clone(),
                previous,
            )),
            None => Box::new(NotApplied("turning an effect's knob")),
        }
    }

    fn label(&self) -> &str {
        "Effect parameter"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetInsertParam>() else {
            return false;
        };
        // Per control: the same parameter of the same slot on the same track.
        // Two knobs dragged one after the other are two things a person did.
        if next.track != self.track || next.index != self.index || next.param != self.param {
            return false;
        }
        // Keeps the *first* previous value, so undoing the drag goes back to
        // before it started rather than to its middle.
        self.value = next.value;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>() + self.param.len()
    }
}

/// Points one insert's detector at another track — the external sidechain
/// (`docs/effects-catalogue.md` §2.1, TDD §13.4).
///
/// A **routing** command rather than a parameter one, and it checks what every
/// other routing command checks: a key is an edge in the same graph `output`
/// and `sends` are edges in, so one that closed a loop is refused here rather
/// than handed to a compiler that cannot order it (§13.2).
pub struct SetInsertKey {
    track: MixerTrackId,
    index: usize,
    key: Option<MixerTrackId>,
    previous: Option<Option<MixerTrackId>>,
}

impl SetInsertKey {
    pub fn new(track: MixerTrackId, index: usize, key: Option<MixerTrackId>) -> Self {
        Self {
            track,
            index,
            key,
            previous: None,
        }
    }
}

impl Command for SetInsertKey {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // A track keying itself is the degenerate loop, and it is worth its
        // own message: it is the one somebody reaches for by accident when
        // what they wanted was the ordinary internal detector, which is what
        // `None` already is.
        if self.key == Some(self.track) {
            return Err(CommandError(
                "an insert cannot be keyed from the track it is on — that is what no key means"
                    .to_string(),
            ));
        }
        if let Some(key) = self.key
            && !doc.mixer.tracks.contains_key(key)
        {
            return Err(CommandError(format!("no mixer track {key:?}")));
        }
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.index)
            .ok_or_else(|| CommandError(format!("no insert {}", self.index)))?;
        if !slot.config.kind().takes_key() {
            return Err(CommandError(format!(
                "{:?} has no detector to key",
                slot.config.kind()
            )));
        }
        let previous = slot.key;
        slot.key = self.key;
        // Written and then checked, because a cycle is a property of the whole
        // graph and not of this edge: the cheapest correct test is to make the
        // change and take it back. The same shape `SetTrackOutput` uses.
        if doc.mixer.has_cycle() {
            let track = doc.mixer.tracks.get_mut(self.track).expect("just read");
            track.inserts[self.index].key = previous;
            return Err(CommandError(
                "that key would make the routing graph feed itself".to_string(),
            ));
        }
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetInsertKey::new(self.track, self.index, previous)),
            None => Box::new(NotApplied("keying an effect")),
        }
    }

    fn label(&self) -> &str {
        "Effect key"
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

/// Writes the knobs a named preset stands for, on one insert.
///
/// Its own command rather than a run of [`SetInsertParam`]s, and the reason is
/// the undo stack: a preset is **one thing a person did**, so it has to be one
/// entry to take back. Fourteen entries for one click is an undo stack nobody
/// can walk, and merging them would be worse — `SetInsertParam::merge_with` is
/// deliberately per-control, because two knobs dragged one after the other are
/// two things.
///
/// It keeps the whole previous config rather than the previous preset, because
/// there may not have been one: what a preset replaces is usually a panel
/// somebody has been turning by hand.
pub struct SetInsertPreset {
    track: MixerTrackId,
    index: usize,
    preset: usize,
    previous: Option<fontelle_types::EffectConfig>,
}

impl SetInsertPreset {
    pub fn new(track: MixerTrackId, index: usize, preset: usize) -> Self {
        Self {
            track,
            index,
            preset,
            previous: None,
        }
    }
}

impl Command for SetInsertPreset {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.index)
            .ok_or_else(|| CommandError(format!("no insert {}", self.index)))?;
        // A preset this effect does not have is refused rather than silently
        // ignored, for `SetInsertParam`'s reason: the row that was clicked was
        // built from `presets()` a moment ago.
        if self.preset >= slot.config.presets().len() {
            return Err(CommandError(format!(
                "no preset {} on {:?}",
                self.preset,
                slot.config.kind()
            )));
        }
        let previous = slot.config;
        slot.config.apply_preset(self.preset);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(RestoreInsertConfig {
                track: self.track,
                index: self.index,
                config: previous,
            }),
            None => Box::new(NotApplied("choosing a preset")),
        }
    }

    fn label(&self) -> &str {
        "Effect preset"
    }

    /// Never. Two presets chosen one after the other are two things a person
    /// did, and the first is somewhere they may want to go back to — unlike
    /// the middle of a knob drag, which is not.
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

/// Puts one insert's whole parameter set back where it was.
///
/// The inverse of [`SetInsertPreset`], and not something the window offers on
/// its own: there is no gesture that means "set every knob at once" except
/// choosing a preset, and undoing that is what this is.
pub struct RestoreInsertConfig {
    track: MixerTrackId,
    index: usize,
    config: fontelle_types::EffectConfig,
}

impl Command for RestoreInsertConfig {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.index)
            .ok_or_else(|| CommandError(format!("no insert {}", self.index)))?;
        // A kind that has changed under this is a slot that was removed and
        // replaced, which is a different insert wearing the same number.
        if slot.config.kind() != self.config.kind() {
            return Err(CommandError(format!(
                "insert {} is a {:?}, not a {:?}",
                self.index,
                slot.config.kind(),
                self.config.kind()
            )));
        }
        // Swapped rather than written, so this command carries what it
        // replaced and its own inverse is the same command again.
        std::mem::swap(&mut slot.config, &mut self.config);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RestoreInsertConfig {
            track: self.track,
            index: self.index,
            // `apply` swapped them, so this is what was there before the undo.
            config: self.config,
        })
    }

    fn label(&self) -> &str {
        "Effect preset"
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

/// Moves one insert's wet/dry mix.
///
/// Its own command rather than a `SetEqBand` for the effect as a whole: the
/// mix belongs to every effect, not to the EQ, and a compressor blended back
/// under the dry track is the commonest use there is.
///
/// Merges with itself while the same insert's knob is being dragged — the rule
/// every continuous control in this document follows.
pub struct SetInsertMix {
    track: MixerTrackId,
    index: usize,
    mix: f32,
    previous: Option<f32>,
}

impl SetInsertMix {
    /// `mix` is a gain: 0 is the signal that went in, 1 is the effect.
    pub fn new(track: MixerTrackId, index: usize, mix: f32) -> Self {
        Self {
            track,
            index,
            mix: mix.clamp(0.0, 1.0),
            previous: None,
        }
    }
}

impl Command for SetInsertMix {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.index)
            .ok_or_else(|| CommandError(format!("no insert {}", self.index)))?;
        let previous = slot.config.mix();
        slot.config.set_mix(self.mix);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => Box::new(SetInsertMix::new(self.track, self.index, previous)),
            None => Box::new(NotApplied("mixing an effect")),
        }
    }

    fn label(&self) -> &str {
        "Effect mix"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetInsertMix>() else {
            return false;
        };
        // Per control: the same slot on the same track. Two knobs dragged one
        // after the other are two things a person did.
        if next.track != self.track || next.index != self.index {
            return false;
        }
        // Keeps the *first* previous value, so undoing the drag goes back to
        // before it started rather than to its middle.
        self.mix = next.mix;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Writes one band of one EQ.
///
/// A whole band rather than one field of one, because that is what the EQ
/// display edits: dragging a node moves its frequency and its gain together,
/// and two commands for one gesture would undo in halves.
///
/// Merges with itself while the same band of the same EQ is being dragged —
/// the rule every continuous control in this project follows, and the reason
/// moving a knob leaves one entry in the history rather than sixty.
pub struct SetEqBand {
    track: MixerTrackId,
    insert: usize,
    band: usize,
    value: fontelle_types::EqBand,
    previous: Option<fontelle_types::EqBand>,
}

impl SetEqBand {
    pub fn new(
        track: MixerTrackId,
        insert: usize,
        band: usize,
        value: fontelle_types::EqBand,
    ) -> Self {
        Self {
            track,
            insert,
            band,
            value,
            previous: None,
        }
    }
}

impl Command for SetEqBand {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let track = doc
            .mixer
            .tracks
            .get_mut(self.track)
            .ok_or_else(|| CommandError(format!("no mixer track {:?}", self.track)))?;
        let slot = track
            .inserts
            .get_mut(self.insert)
            .ok_or_else(|| CommandError(format!("no insert {}", self.insert)))?;
        let fontelle_types::EffectConfig::Eq(eq) = &mut slot.config else {
            return Err(CommandError("that insert is not an EQ".to_string()));
        };
        let band = eq
            .bands
            .get_mut(self.band)
            .ok_or_else(|| CommandError(format!("no band {}", self.band)))?;
        let previous = std::mem::replace(band, self.value);
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.previous {
            Some(previous) => {
                Box::new(SetEqBand::new(self.track, self.insert, self.band, previous))
            }
            None => Box::new(NotApplied("editing an EQ band")),
        }
    }

    fn label(&self) -> &str {
        "Edit EQ"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetEqBand>() else {
            return false;
        };
        // Per control: the same band of the same EQ on the same track. Two
        // knobs dragged one after the other are two things a person did.
        if next.track != self.track || next.insert != self.insert || next.band != self.band {
            return false;
        }
        // The merged command keeps the *first* previous value, so undoing the
        // drag goes back to before it started rather than to its middle.
        self.value = next.value;
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

// ========================================================= automation points

/// The automation clip's points, or an error saying it is not one.
fn points_mut(
    doc: &mut Project,
    clip: ClipId,
) -> Result<&mut crate::Arena<fontelle_types::PointId, crate::AutomationPoint>, CommandError> {
    let clip = doc
        .clips
        .get_mut(clip)
        .ok_or_else(|| CommandError("no such clip".to_string()))?;
    match &mut clip.source {
        crate::ClipSource::Automation(data) => Ok(&mut data.points),
        _ => Err(CommandError("that clip is not automation".to_string())),
    }
}

/// Puts a point on an automation curve — a click on the editor.
pub struct AddAutomationPoint {
    clip: ClipId,
    point: crate::AutomationPoint,
    created: Option<fontelle_types::PointId>,
}

impl AddAutomationPoint {
    pub fn new(clip: ClipId, point: crate::AutomationPoint) -> Self {
        Self {
            clip,
            point,
            created: None,
        }
    }

    /// The id the point got, so the drag that follows the click can move it.
    /// The same handshake drawing a note has.
    pub fn id(&self) -> Option<fontelle_types::PointId> {
        self.created
    }
}

impl Command for AddAutomationPoint {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        // §12.1 stores values normalised. Refused at the door rather than
        // clamped silently, because a caller passing 1.5 has a bug and a
        // clamp would hide it — every gesture that produces one of these knows
        // the range it is drawing in.
        if !(0.0..=1.0).contains(&self.point.value) {
            return Err(CommandError(format!(
                "an automation value is 0..1, not {}",
                self.point.value
            )));
        }
        let point = self.point;
        let points = points_mut(doc, self.clip)?;
        match self.created {
            Some(id) => {
                if !points.insert_at(id, point) {
                    return Err(CommandError("that point id is taken".to_string()));
                }
            }
            None => self.created = Some(points.insert(point)),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some(id) => Box::new(RemoveAutomationPoints::new(self.clip, vec![id])),
            None => Box::new(NotApplied("adding an automation point")),
        }
    }

    fn label(&self) -> &str {
        "Add point"
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

/// Drags points about. **Deltas relative to the previous step of the same
/// drag**, which is what lets a whole gesture merge into one history entry.
pub struct MoveAutomationPoints {
    clip: ClipId,
    ids: Vec<fontelle_types::PointId>,
    tick_delta: Tick,
    value_delta: f64,
    /// Where each point was **before the drag began**.
    ///
    /// Positions rather than a running total of deltas, and the difference is
    /// not cosmetic: a drag is sixty commands merged into one, and summing
    /// sixty f64 deltas and then subtracting them leaves a point a rounding
    /// error away from where it started. This project's rule for a command is
    /// that applying it and inverting it puts the document back — exactly —
    /// and a total that is `0.4000000000000001` does not.
    previous: Option<Vec<(fontelle_types::PointId, Tick, f64)>>,
}

impl MoveAutomationPoints {
    pub fn new(
        clip: ClipId,
        ids: Vec<fontelle_types::PointId>,
        tick_delta: Tick,
        value_delta: f64,
    ) -> Self {
        Self {
            clip,
            ids,
            tick_delta,
            value_delta,
            previous: None,
        }
    }
}

impl Command for MoveAutomationPoints {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let (ids, tick_delta, value_delta) = (self.ids.clone(), self.tick_delta, self.value_delta);
        let points = points_mut(doc, self.clip)?;
        // Captured before anything moves, and only the first time this command
        // is applied — a merged drag keeps the positions the *first* step
        // started from.
        if self.previous.is_none() {
            self.previous = Some(
                ids.iter()
                    .filter_map(|id| points.get(*id).map(|p| (*id, p.tick, p.value)))
                    .collect(),
            );
        }
        // Clamped as a group, so a selection dragged into a wall keeps its
        // shape instead of collapsing against it — the same rule the roll's
        // note drag follows.
        let mut tick_delta = tick_delta;
        let mut value_delta = value_delta;
        for id in &ids {
            let Some(point) = points.get(*id) else {
                continue;
            };
            tick_delta = tick_delta.max(-point.tick);
            value_delta = value_delta.max(-point.value).min(1.0 - point.value);
        }
        for id in &ids {
            if let Some(point) = points.get_mut(*id) {
                point.tick += tick_delta;
                point.value = (point.value + value_delta).clamp(0.0, 1.0);
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(PlaceAutomationPoints {
                clip: self.clip,
                points: previous.clone(),
            }),
            None => Box::new(NotApplied("moving automation points")),
        }
    }

    fn label(&self) -> &str {
        "Move points"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<MoveAutomationPoints>() else {
            return false;
        };
        if next.clip != self.clip || next.ids != self.ids {
            return false;
        }
        // Nothing to accumulate: this command already holds where the points
        // were before the drag started, which is what the whole merged gesture
        // undoes to.
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.ids.len() * std::mem::size_of::<fontelle_types::PointId>()
    }
}

/// Puts named points at exact positions — the inverse of a drag.
pub struct PlaceAutomationPoints {
    clip: ClipId,
    points: Vec<(fontelle_types::PointId, Tick, f64)>,
}

impl Command for PlaceAutomationPoints {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let placing = self.points.clone();
        let points = points_mut(doc, self.clip)?;
        for (id, tick, value) in placing {
            if let Some(point) = points.get_mut(id) {
                point.tick = tick;
                point.value = value;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(NotApplied("placing automation points"))
    }

    fn label(&self) -> &str {
        "Move points"
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

/// Deletes points, keeping them for the undo — the same ids, because the
/// command above this one in the history names them.
pub struct RemoveAutomationPoints {
    clip: ClipId,
    ids: Vec<fontelle_types::PointId>,
    removed: Option<Vec<(fontelle_types::PointId, crate::AutomationPoint)>>,
}

impl RemoveAutomationPoints {
    pub fn new(clip: ClipId, ids: Vec<fontelle_types::PointId>) -> Self {
        Self {
            clip,
            ids,
            removed: None,
        }
    }
}

impl Command for RemoveAutomationPoints {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let ids = self.ids.clone();
        let points = points_mut(doc, self.clip)?;
        let mut removed = Vec::new();
        for id in ids {
            if let Some(point) = points.remove(id) {
                removed.push((id, point));
            }
        }
        self.removed.get_or_insert(removed);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(points) => Box::new(RestoreAutomationPoints {
                clip: self.clip,
                points: points.clone(),
            }),
            None => Box::new(NotApplied("deleting automation points")),
        }
    }

    fn label(&self) -> &str {
        "Delete points"
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

/// The undo of a [`RemoveAutomationPoints`].
pub struct RestoreAutomationPoints {
    clip: ClipId,
    points: Vec<(fontelle_types::PointId, crate::AutomationPoint)>,
}

impl Command for RestoreAutomationPoints {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let restoring = self.points.clone();
        let points = points_mut(doc, self.clip)?;
        for (id, point) in restoring {
            points.insert_at(id, point);
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveAutomationPoints::new(
            self.clip,
            self.points.iter().map(|(id, _)| *id).collect(),
        ))
    }

    fn label(&self) -> &str {
        "Restore points"
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

/// Changes the shape of the segment *after* each named point (§12.1).
pub struct SetPointCurve {
    clip: ClipId,
    ids: Vec<fontelle_types::PointId>,
    curve: crate::CurveShape,
    /// Each point's own shape, because two points in a selection may disagree
    /// and one restored value gets that wrong.
    previous: Option<Vec<(fontelle_types::PointId, crate::CurveShape)>>,
}

impl SetPointCurve {
    pub fn new(clip: ClipId, ids: Vec<fontelle_types::PointId>, curve: crate::CurveShape) -> Self {
        Self {
            clip,
            ids,
            curve,
            previous: None,
        }
    }
}

impl Command for SetPointCurve {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let (ids, curve) = (self.ids.clone(), self.curve);
        let points = points_mut(doc, self.clip)?;
        let mut previous = Vec::new();
        for id in ids {
            if let Some(point) = points.get_mut(id) {
                previous.push((id, point.curve));
                point.curve = curve;
            }
        }
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.previous {
            Some(previous) => Box::new(RestorePointCurves {
                clip: self.clip,
                previous: previous.clone(),
            }),
            None => Box::new(NotApplied("changing a curve shape")),
        }
    }

    fn label(&self) -> &str {
        "Curve shape"
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

/// The undo of a [`SetPointCurve`]: each point its own shape back.
pub struct RestorePointCurves {
    clip: ClipId,
    previous: Vec<(fontelle_types::PointId, crate::CurveShape)>,
}

impl Command for RestorePointCurves {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let previous = self.previous.clone();
        let points = points_mut(doc, self.clip)?;
        for (id, curve) in previous {
            if let Some(point) = points.get_mut(id) {
                point.curve = curve;
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(NotApplied("restoring curve shapes"))
    }

    fn label(&self) -> &str {
        "Restore curve shapes"
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
