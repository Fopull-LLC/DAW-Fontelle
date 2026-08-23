//! SF2/sf3/sfz and audio-clip sample loading, streaming, peak generation, and
//! soundfont library indexing (FONTELLE_TDD.md §7.7, §15.3, §17.5).

mod library;
mod peaks;
mod sf2_import;
mod sfz_import;

pub use library::{LibraryEntry, SoundfontLibrary};
pub use peaks::{PeakData, generate_peaks};
pub use sf2_import::{ImportError, ImportedZone, ImportedZones, import_sf2};
pub use sfz_import::import_sfz;
