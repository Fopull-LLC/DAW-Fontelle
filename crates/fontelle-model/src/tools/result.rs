//! What every tool hands back: the one shape a roll tool's answer takes.
//!
//! `docs/roll-tools-plan.md` §3.1. The arpeggiator used to answer with a
//! *remove* and an *insert*, two commands the history could not merge, so one
//! Ctrl+Z took the run away and left the clip empty (seen on `:99`,
//! 2026-10-20). Every tool now answers with this, and the window applies it as
//! one compound: one undo, whatever the tool did.

use crate::note::Note;
use fontelle_types::NoteId;

/// A tool's answer about a set of notes (its *scope*).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolResult {
    /// Notes that stay who they are, as they will be — same id, any field
    /// changed. Only notes that *did* change are here.
    pub rewrite: Vec<(NoteId, Note)>,
    /// Notes that go.
    pub remove: Vec<NoteId>,
    /// Notes that are new.
    pub insert: Vec<Note>,
}

impl ToolResult {
    /// `after` is `scope` as the tool left it, note for note in the same
    /// order. Only the notes that differ are kept.
    pub fn rewriting(scope: &[(NoteId, Note)], after: Vec<Note>) -> Self {
        let rewrite = scope
            .iter()
            .zip(after)
            .filter(|((_, was), now)| was != now)
            .map(|((id, _), now)| (*id, now))
            .collect();
        Self {
            rewrite,
            ..Self::default()
        }
    }

    /// Every note of `scope` goes and `made` takes its place.
    pub fn replacing(scope: &[(NoteId, Note)], made: Vec<Note>) -> Self {
        Self {
            rewrite: Vec::new(),
            remove: scope.iter().map(|(id, _)| *id).collect(),
            insert: made,
        }
    }

    /// Nothing would change.
    pub fn is_empty(&self) -> bool {
        self.rewrite.is_empty() && self.remove.is_empty() && self.insert.is_empty()
    }

    /// The notes of `scope` as they would be afterwards — untouched,
    /// rewritten and new together, by start and then key. What the roll
    /// ghosts and *Hear it* plays.
    pub fn after(&self, scope: &[(NoteId, Note)]) -> Vec<Note> {
        let mut out: Vec<Note> = scope
            .iter()
            .filter(|(id, _)| !self.remove.contains(id))
            .map(|(id, note)| {
                self.rewrite
                    .iter()
                    .find(|(i, _)| i == id)
                    .map_or(note, |(_, n)| n)
                    .clone()
            })
            .collect();
        out.extend(self.insert.iter().cloned());
        // Stable, so notes sharing a start and a key keep the order the tool
        // gave them.
        out.sort_by_key(|n| (n.start, n.key));
        out
    }

    /// The notes of the scope this changes or removes: the ones the roll
    /// dims while it shows the answer.
    pub fn touched(&self) -> Vec<NoteId> {
        self.rewrite
            .iter()
            .map(|(id, _)| *id)
            .chain(self.remove.iter().copied())
            .collect()
    }
}
