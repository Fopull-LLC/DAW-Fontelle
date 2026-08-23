use std::path::Path;

#[derive(Debug)]
pub struct MidiImportError(pub String);

/// `.mid` import maps tracks to instrument channels and creates note clips; export
/// writes the resolved timeline. Both go through `midly`. Tempo and time-signature
/// meta events must feed the tempo map, and channel/CC data must not be silently
/// discarded (TDD §14.6).
pub fn import_midi_file(_path: &Path) -> Result<(), MidiImportError> {
    todo!("midly parse -> tracks -> note clips + tempo map events")
}

pub fn export_midi_file(_path: &Path) -> Result<(), MidiImportError> {
    todo!("resolved CompiledTimeline -> midly Smf -> write")
}
