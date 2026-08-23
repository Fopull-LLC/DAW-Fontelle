use std::path::Path;

use fontelle_types::AssetRef;

#[derive(Debug)]
pub struct ImportError(pub String);

/// One SF2 zone's raw generator/modulator data, prior to being seeded into a
/// `fontelle-core::Patch`. The SF2 2.04 generator model has many non-obvious rules
/// (offsets, coarse/fine tuning, key/vel ranges, timecent envelope units, absolute-
/// cent filter cutoff, modulator defaults) — read RustySynth's implementation as
/// the correctness reference when filling this in; getting import defaults wrong
/// makes every soundfont sound subtly wrong in a way that's hard to debug later
/// (TDD §7.3).
pub struct ImportedZone {
    pub name: String,
}

pub struct ImportedZones {
    pub zones: Vec<ImportedZone>,
}

/// Parsing only, via the `soundfont` crate — synthesis semantics are Fontelle's own
/// (TDD §3.1). Supports `.sf2` and `.sf3` (Vorbis-compressed sample data).
pub fn import_sf2(_path: &Path, _asset: &AssetRef) -> Result<ImportedZones, ImportError> {
    todo!("soundfont::SoundFont2::parse, then map generators/modulators to ImportedZone")
}
