//! SF2/sf3/sfz and audio-clip sample loading, MIDI file import, streaming, peak
//! generation, and soundfont library indexing
//! (FONTELLE_TDD.md §7.7, §14.6, §15.3, §17.5).

pub mod audio_import;
pub mod fixtures;
mod fsc_import;
mod general_midi;
mod library;
mod midi_import;
mod peaks;
mod sf2_import;
mod sfz_import;
mod wav_writer;

pub use audio_import::{AudioAsset, import_audio, read_audio};
pub use fsc_import::{FscNote, FscScore, import_fsc, read_fsc};
pub use general_midi::{general_midi_family, general_midi_name};
pub use library::{LibraryEntry, SoundfontLibrary};
pub use midi_import::{
    ImportedMidiChannel, MidiChannelSummary, MidiChannels, MidiImport, MidiPart, MidiSurvey,
    import_midi, part_name, read_midi, read_midi_survey, survey_midi,
};
pub use peaks::{PEAK_BUCKET, PeakData, generate_peaks};
pub use sf2_import::{
    ImportError, ImportedPatch, ImportedZone, ImportedZones, LoadedSample, PresetInfo, import_sf2,
    import_sf2_preset, list_presets, load_sf2_samples,
};
pub use sfz_import::import_sfz;
pub use wav_writer::WavWriter;
