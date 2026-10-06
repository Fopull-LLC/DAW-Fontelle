//! Polyphonic transcription: audio in, notes out (plan §2.2).

#[cfg(feature = "model")]
pub mod basic_pitch;
pub mod notes;

pub use notes::{
    NoteEvent, NoteParams, Posteriorgrams, notes_from_posteriorgrams, tidy_notes, tidy_notes_with,
};
