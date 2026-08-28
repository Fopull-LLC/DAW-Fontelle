//! MIDI file export (TDD §14.6).
//!
//! **Import is not here.** It lives in `fontelle_assets::import_midi`, which
//! is where the working implementation is: tracks to channels, notes to clips,
//! the file's whole tempo curve into a `TempoMap`, percussion and
//! zero-velocity note-ons handled. This module used to declare an
//! `import_midi_file` alongside it that was a `todo!()` — a second, obvious
//! name for a feature that already existed, which anyone reaching for it would
//! find by panicking rather than by being sent to the real one.

use std::path::Path;

#[derive(Debug)]
pub struct MidiExportError(pub String);

impl std::fmt::Display for MidiExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MidiExportError {}

/// Writes the resolved timeline out as a `.mid` (TDD §14.6), through `midly`.
///
/// Not built. Export is the direction that needs the inverse of everything
/// import does — samples back to ticks through the tempo map, one track per
/// channel, tempo and time-signature meta events reconstructed — and it has no
/// caller yet.
pub fn export_midi_file(_path: &Path) -> Result<(), MidiExportError> {
    todo!("resolved CompiledTimeline -> midly Smf -> write (TDD §14.6)")
}
