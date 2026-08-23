use fontelle_types::AssetId;

/// Peak files generated on the asset loader thread at multiple zoom levels and
/// cached under the project's `cache/` directory (regenerable, safe to delete —
/// TDD §15.3, §17.1). Display must stay responsive while peaks are still
/// generating: draw what exists, fill in progressively.
pub struct PeakData {
    pub asset: AssetId,
    /// One `Vec<(min, max)>` per zoom level, coarsest first.
    pub levels: Vec<Vec<(f32, f32)>>,
}

pub fn generate_peaks(_asset: AssetId, _samples: &[f32]) -> PeakData {
    todo!("multi-resolution min/max downsampling")
}
