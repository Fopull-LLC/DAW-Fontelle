#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMode {
    Peak,
    Rms,
}

#[derive(Debug, Clone, Copy)]
pub struct CompressorConfig {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
    pub auto_makeup: bool,
    pub detection: DetectionMode,
}

/// Sidechain input may come from any mixer track (TDD §13.4) — wired by
/// `fontelle-engine`'s graph compiler, not by the effect itself.
// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct Compressor {
    envelope: f32,
    gain_reduction_db: f32,
}

impl Compressor {
    pub fn process(
        &mut self,
        _main: &mut [f32],
        _sidechain: Option<&[f32]>,
        _config: &CompressorConfig,
    ) {
        todo!("envelope follower -> transfer curve -> gain reduction")
    }

    pub fn gain_reduction_db(&self) -> f32 {
        self.gain_reduction_db
    }
}
