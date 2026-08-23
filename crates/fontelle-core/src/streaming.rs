use fontelle_types::AssetId;

/// Sample data shared across every patch referencing the same file — a 2GB
/// orchestral SF2 loaded on twelve channels occupies memory once (TDD §7.7).
pub enum SampleResidency {
    /// Below the resident threshold (default 64MB): fully resident, memory-mapped
    /// where possible.
    Resident(std::sync::Arc<[f32]>),
    /// Above it: first N ms resident, remainder streamed by the disk thread into
    /// per-voice ring buffers. An underrun holds the last sample and logs — it
    /// never produces a discontinuity click.
    Streamed {
        head: std::sync::Arc<[f32]>,
        // Streaming tail is owned by the disk-thread side (`fontelle-assets`); the
        // RT-visible half is a lock-free ring, wired up once `fontelle-engine`
        // exists to own the disk thread.
    },
}

#[derive(Debug, Default)]
pub struct SampleStore {
    // Ref-counted by `AssetId`. Populated by `fontelle-assets`, read here.
}

impl SampleStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, _asset: AssetId) -> Option<&SampleResidency> {
        todo!("look up shared sample data by AssetId")
    }
}
