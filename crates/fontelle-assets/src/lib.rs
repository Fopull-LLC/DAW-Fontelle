//! SF2/sf3/sfz and audio-clip sample loading, MIDI file import, streaming, peak
//! generation, and soundfont library indexing
//! (FONTELLE_TDD.md §7.7, §14.6, §15.3, §17.5).

mod library;
mod midi_import;
mod peaks;
mod sf2_import;
mod sfz_import;

pub use library::{LibraryEntry, SoundfontLibrary};
pub use midi_import::{
    ImportedMidiChannel, MidiChannelSummary, MidiChannels, MidiImport, import_midi,
};
pub use peaks::{PeakData, generate_peaks};
pub use sf2_import::{
    ImportError, ImportedPatch, ImportedZone, ImportedZones, PresetInfo, import_sf2,
    import_sf2_preset, list_presets,
};
pub use sfz_import::import_sfz;
