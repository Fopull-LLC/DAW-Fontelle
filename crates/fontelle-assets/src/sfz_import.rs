use std::path::Path;

use crate::sf2_import::{ImportError, ImportedZones};

/// Text-based, references external samples. Import maps onto the same
/// `fontelle-core::Patch` structure as an SF2 import (TDD §7.3).
pub fn import_sfz(_path: &Path) -> Result<ImportedZones, ImportError> {
    todo!("SFZ opcode parse -> ImportedZone list, relative to the .sfz file's directory")
}
