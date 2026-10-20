//! What the roll-tool tests share: a note in one line, and a scope with real
//! ids (`docs/roll-tools-plan.md`).
#![allow(dead_code)]

use fontelle_model::{Arena, Note, ToolResult};
use fontelle_types::NoteId;

pub fn note(start: i64, length: i64, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

pub fn loud(start: i64, length: i64, key: u8, velocity: u8) -> Note {
    Note {
        velocity,
        ..note(start, length, key)
    }
}

/// `notes` given ids, in the order given.
pub fn scope(notes: Vec<Note>) -> Vec<(NoteId, Note)> {
    let mut arena: Arena<NoteId, Note> = Arena::new();
    notes
        .into_iter()
        .map(|n| (arena.insert(n.clone()), n))
        .collect()
}

/// `(start, length, key)` of every note after `result`, in time order.
pub fn shape(scope: &[(NoteId, Note)], result: &ToolResult) -> Vec<(i64, i64, u8)> {
    result
        .after(scope)
        .iter()
        .map(|n| (n.start, n.length, n.key))
        .collect()
}

/// The note `id` as `result` leaves it (untouched if not rewritten).
pub fn as_left(scope: &[(NoteId, Note)], result: &ToolResult, id: NoteId) -> Option<Note> {
    if result.remove.contains(&id) {
        return None;
    }
    result
        .rewrite
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| n.clone())
        .or_else(|| scope.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()))
}
