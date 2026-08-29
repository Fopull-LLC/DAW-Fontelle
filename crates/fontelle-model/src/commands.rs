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
use crate::mixer::MixerTrack;
use crate::note::Note;
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
    created: Option<(ChannelId, MixerTrackId)>,
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
            created: None,
        }
    }

    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }

    /// The id, once this has been applied.
    pub fn channel(&self) -> Option<ChannelId> {
        self.created.map(|(channel, _)| channel)
    }

    pub fn mixer_track(&self) -> Option<MixerTrackId> {
        self.created.map(|(_, track)| track)
    }
}

impl Command for AddChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let mut track = MixerTrack::new(self.name.clone());
        track.output = doc.mixer.master;
        let channel = Channel {
            name: self.name.clone(),
            color: [0x4f, 0x8f, 0xd0, 0xff],
            mixer_track: MixerTrackId::default(),
            patch_data: self.patch_data.clone(),
            pan: self.pan,
        };

        match self.created {
            // A redo: the same ids, or the commands stacked above this one are
            // pointing at nothing.
            Some((channel_id, track_id)) => {
                if !doc.mixer.tracks.insert_at(track_id, track) {
                    return Err(CommandError("that mixer track id is taken".into()));
                }
                let mut channel = channel;
                channel.mixer_track = track_id;
                if !doc.channels.insert_at(channel_id, channel) {
                    doc.mixer.tracks.remove(track_id);
                    return Err(CommandError("that channel id is taken".into()));
                }
            }
            None => {
                let track_id = doc.mixer.tracks.insert(track);
                let mut channel = channel;
                channel.mixer_track = track_id;
                let channel_id = doc.channels.insert(channel);
                self.created = Some((channel_id, track_id));
            }
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.created {
            Some((channel, _)) => Box::new(RemoveChannel::new(channel)),
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
    /// `Some` when this channel was the last user of its mixer track. A track
    /// several channels share outlives any one of them.
    track: Option<(MixerTrackId, MixerTrack)>,
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

        let shared = doc
            .channels
            .values()
            .any(|other| other.mixer_track == channel.mixer_track);
        let track = (!shared)
            .then(|| {
                doc.mixer
                    .tracks
                    .remove(channel.mixer_track)
                    .map(|track| (channel.mixer_track, track))
            })
            .flatten();

        self.removed = Some(RemovedChannel {
            channel,
            track,
            clips,
        });
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(removed) => Box::new(RestoreChannel {
                id: self.channel,
                channel: removed.channel.clone(),
                track: removed.track.clone(),
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
    track: Option<(MixerTrackId, MixerTrack)>,
    clips: Vec<(ClipId, Clip)>,
}

impl Command for RestoreChannel {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if let Some((id, track)) = &self.track
            && !doc.mixer.tracks.insert_at(*id, track.clone())
        {
            return Err(CommandError("that mixer track id is taken".into()));
        }
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

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
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
    /// The tempo in force at the start of the piece. Later tempo changes are
    /// left alone: an imported file's curve must survive somebody nudging the
    /// BPM box.
    Tempo,
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
                NumberTarget::Tempo => "Set tempo",
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
            NumberTarget::Tempo => {
                let previous = doc.tempo_map.tempo_at(0);
                let mut segments = doc.tempo_map.segments().to_vec();
                segments[0].bpm = self.value;
                let rate = doc.tempo_map.sample_rate_hz();
                doc.tempo_map = crate::project::TempoMap::from_segments(segments, rate);
                previous
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
