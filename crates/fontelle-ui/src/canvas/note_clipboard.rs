//! The window's one clipboard of notes (`docs/analyze-musically-plan.md`
//! §3.6): what the piano roll copies, and what Analyze Musically puts there
//! for the roll to paste.
//!
//! Lifted out of the roll, which still holds it (it is the one place notes
//! are pasted), so another window can fill it. A phrase is kept from zero —
//! its earliest note at tick 0 — and, when it came from somewhere in the
//! song rather than from a clip, the song tick that note was heard at: what
//! *Paste at original position* puts it back under.

use fontelle_model::Note;
use fontelle_types::Tick;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteClipboard {
    notes: Vec<Note>,
    origin: Option<Tick>,
}

impl NoteClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds `notes` (any order, any offset): sorted by start then key, and
    /// moved so the earliest starts at zero. `origin` is the song tick the
    /// earliest was heard at, or `None` for a phrase with no place in the
    /// song.
    pub fn put(&mut self, mut notes: Vec<Note>, origin: Option<Tick>) {
        notes.sort_by(|a, b| a.start.cmp(&b.start).then(a.key.cmp(&b.key)));
        let earliest = notes.first().map_or(0, |n| n.start);
        // A path is counted from its note's own start, so it moves with it.
        for note in &mut notes {
            note.start -= earliest;
        }
        self.notes = notes;
        self.origin = origin;
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Where the earliest note was heard, in song ticks.
    pub fn origin(&self) -> Option<Tick> {
        self.origin
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}
