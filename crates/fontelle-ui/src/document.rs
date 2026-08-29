//! The window's view of the open studio, and the only way it may change it.
//!
//! The same shape as [`crate::transport::TransportHost`], for the same reason:
//! `fontelle-app` is the layer allowed to see everything, so the seam lives
//! here as a trait and this crate stays testable with a fake.
//!
//! **INVARIANT 2 and INVARIANT 9 are what these traits are for.** The roll
//! never writes to a `Project`; it produces [`RollEdit`](crate::canvas::RollEdit)
//! values, and the implementation on the other side turns each into a
//! `Command` and puts it through `History`. There is deliberately no method
//! here that hands out a mutable document.
//!
//! Two traits rather than one, and the split is the same split the crate
//! dependency graph has:
//!
//! - [`DocumentHost`] is the *clip* — notes in, edits out. The piano roll uses
//!   only this, which is why the roll's tests need nothing but an `Arena`.
//! - [`StudioHost`] is everything else the window shows: the channel rack, the
//!   soundfont bank, and the audition path that makes a drawn note audible.
//!   Reading a soundfont is a filesystem operation, so none of it can live in
//!   this crate — only the shape of the question.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, Sample, Tick};

use crate::canvas::RollEdit;

pub trait DocumentHost {
    /// The notes of the clip the roll is showing.
    fn notes(&self) -> &Arena<NoteId, Note>;

    /// Applies one edit as a command, recording it in the history.
    ///
    /// Returns the ids of any notes it **created** — empty for every edit that
    /// creates none. The roll needs them: drawing a note and dragging it to
    /// length is one gesture, and the second half of it cannot start until the
    /// id of the note the first half made is known.
    fn edit(&mut self, edit: RollEdit) -> Vec<NoteId>;

    fn undo(&mut self);
    fn redo(&mut self);

    /// Ends the current gesture, so the next edit starts a new undo entry
    /// rather than coalescing into the last. Called on mouse-up: only the
    /// caller knows the drag is over (see `History::break_gesture`).
    fn end_gesture(&mut self);

    /// The time signature's numerator, for the grid and the read-out.
    fn beats_per_bar(&self) -> u32;

    /// Where `position_sample` falls inside this clip, in the clip's own
    /// ticks, or `None` when the playhead is somewhere the clip does not
    /// cover.
    fn playhead_tick(&self, position_sample: Sample) -> Option<Tick>;

    /// The other direction: where a tick *in this clip* is in the song, in
    /// samples. What clicking the roll's ruler to move the playhead needs.
    ///
    /// It has to be asked rather than worked out, because a song with a tempo
    /// change has no single BPM to multiply by (INVARIANT 5) and only the
    /// document holds the map.
    fn sample_of_clip_tick(&self, tick: Tick) -> Sample;

    /// Whether there are changes not yet on disk.
    fn is_dirty(&self) -> bool;

    /// Writes the project out. `Err` carries something worth showing a person.
    fn save(&mut self) -> Result<(), String>;

    /// Where it would be saved, for the title bar.
    fn name(&self) -> &str;
}

/// One channel, as the rack draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    /// A channel with no soundfont on it yet. Drawn differently, because a
    /// silent channel that looks like every other channel is a mystery.
    pub has_instrument: bool,
}

/// One row of the browser: a soundfont, or a preset inside one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub name: String,
    /// The size of a file, or the bank and program of a preset — the second
    /// column, in the muted ink.
    pub detail: String,
}

/// Everything the window shows that is not the clip.
pub trait StudioHost: DocumentHost {
    /// Bumped whenever anything a panel draws has changed.
    ///
    /// The window caches the lists below and re-reads them when this moves,
    /// rather than rebuilding a `Vec<String>` every frame for a list that
    /// changes when somebody clicks something.
    fn revision(&self) -> u64;

    // --- the channel rack ---
    fn channels(&self) -> Vec<ChannelInfo>;
    /// Which channel the piano roll is showing.
    fn selected_channel(&self) -> usize;
    fn select_channel(&mut self, index: usize);
    fn toggle_mute(&mut self, index: usize);
    fn toggle_solo(&mut self, index: usize);

    // --- the soundfont bank (TDD §17.5) ---
    /// The bank, already filtered and ordered by the live search.
    fn library_files(&self) -> Vec<LibraryEntry>;
    /// The presets inside the open file, filtered by the same search.
    fn library_presets(&self) -> Vec<LibraryEntry>;
    fn query(&self) -> &str;
    fn set_query(&mut self, query: &str);
    /// Reads the preset list of the file at `index` in the filtered list.
    fn open_file(&mut self, index: usize) -> Result<(), String>;
    fn selected_file(&self) -> Option<usize>;
    /// Puts preset `index` of the open file onto a **new** channel.
    fn add_channel_with(&mut self, preset: usize) -> Result<(), String>;
    /// Puts it on the channel the rack has selected instead.
    fn set_channel_instrument(&mut self, preset: usize) -> Result<(), String>;
    /// One line about where the bank is and what is in it — the thing to say
    /// when it is empty, which on a first run it is.
    fn library_status(&self) -> String;
    fn rescan_library(&mut self);

    /// Shows the bank folder in the desktop's file manager, creating it if it
    /// is not there yet.
    ///
    /// The whole reason to press this is that the folder is empty and you want
    /// to put something in it, so a file manager opening on a folder that does
    /// not exist is not an answer.
    fn reveal_library_dir(&mut self);

    /// Asks the user for a folder and makes it the bank, remembering it.
    ///
    /// `add` keeps the folders already configured and appends this one;
    /// otherwise it replaces them. **Blocking** — the picker is the desktop's
    /// own, and the window is frozen while it is up, which is what every other
    /// application does too.
    fn choose_library_dir(&mut self, add: bool);

    /// The last thing that went wrong, for the status line. Taking it clears
    /// it, so a message shows once rather than forever.
    fn take_message(&mut self) -> Option<String>;

    // --- auditioning (TDD §14.1's live path, driven by the mouse) ---
    /// Sounds `key` now, on the selected channel, outside the timeline. What
    /// makes a note you draw audible the moment you draw it, whether or not
    /// the transport is rolling.
    fn audition_on(&mut self, key: u8, velocity: u8);
    fn audition_off(&mut self, key: u8);

    /// Called once per pass of the event loop, for whatever the host has to do
    /// off the audio thread — chiefly freeing the graph the RT side handed
    /// back (see `fontelle_engine::GraphPublisher::reclaim`).
    fn pump(&mut self);
}
