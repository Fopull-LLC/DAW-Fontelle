use std::collections::HashMap;
use std::sync::Arc;

use fontelle_types::AssetId;

/// Below the resident threshold (default 64MB): fully resident and memory-mapped
/// where possible. Above it: first N ms resident (default 500ms), the remainder
/// streamed by the disk thread into per-voice ring buffers that `fontelle-core`
/// reads. Sample data is shared by `AssetId` across every patch referencing the
/// same file (TDD §7.7).
#[derive(Debug, Clone, Copy)]
pub struct StreamingConfig {
    pub resident_threshold_bytes: u64,
    pub resident_head_ms: u32,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            resident_threshold_bytes: 64 * 1024 * 1024,
            resident_head_ms: 500,
        }
    }
}

#[derive(Default)]
pub struct SampleStore {
    resident: HashMap<AssetId, Arc<[f32]>>,
    config: StreamingConfig,
}

impl SampleStore {
    pub fn new(config: StreamingConfig) -> Self {
        Self {
            resident: HashMap::new(),
            config,
        }
    }

    pub fn load(
        &mut self,
        _asset: AssetId,
        _path: &std::path::Path,
    ) -> Result<(), crate::sf2_import::ImportError> {
        let _ = &self.config;
        todo!(
            "decide resident vs streamed by self.config, decode via `symphonia` for non-SF2 sample data"
        )
    }

    pub fn ref_count(&self, _asset: AssetId) -> usize {
        let _ = &self.resident;
        todo!("Arc strong_count for the given asset")
    }
}
