//! The window's view of the open document, and the only way it may change it.
//!
//! The same shape as [`crate::transport::TransportHost`], for the same reason:
//! `fontelle-app` is the layer allowed to see everything, so the seam lives
//! here as a trait and this crate stays testable with a fake.
//!
//! **INVARIANT 2 and INVARIANT 9 are what this trait is for.** The roll never
//! writes to a `Project`; it produces [`RollEdit`](crate::canvas::RollEdit)
//! values, and the implementation on the other side turns each into a
//! `Command` and puts it through `History`. There is deliberately no method
//! here that hands out a mutable document.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, Sample, Tick};

use crate::canvas::RollEdit;

pub trait DocumentHost {
    /// The notes of the clip the roll is showing.
    fn notes(&self) -> &Arena<NoteId, Note>;

    /// Applies one edit as a command, recording it in the history.
    fn edit(&mut self, edit: RollEdit);

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

    /// Whether there are changes not yet on disk.
    fn is_dirty(&self) -> bool;

    /// Writes the project out. `Err` carries something worth showing a person.
    fn save(&mut self) -> Result<(), String>;

    /// Where it would be saved, for the title bar.
    fn name(&self) -> &str;
}
