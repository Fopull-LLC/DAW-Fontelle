use fontelle_types::Tick;

use crate::arena::Arena;
use fontelle_types::{ChannelId, NoteId};

/// Per-note pan, fine pitch, release, and two free modulation values are cheap to
/// store and route through the mod matrix — exactly the per-note character control
/// that makes sample-based writing expressive (TDD §10.4).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Note {
    /// Relative to clip start.
    pub start: Tick,
    pub length: Tick,
    pub key: u8,
    pub velocity: u8,
    pub pan: i8,
    pub fine_pitch: i16,
    pub release: u8,
    pub mod_x: u8,
    pub mod_y: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteData {
    /// The clip carries the instrument (TDD §10.3) — a lane has no instrument of
    /// its own.
    pub channel: ChannelId,
    pub notes: Arena<NoteId, Note>,
}
