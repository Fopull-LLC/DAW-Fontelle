/// Peak + RMS accumulator written by the RT thread and read by the UI through a
/// lock-free ring of downsampled values (TDD §13.3) — never a growable queue.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeakRmsMeter {
    peak: f32,
    rms_accum: f32,
    clip_latched: bool,
}

impl PeakRmsMeter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_block(&mut self, _block: &[f32]) {
        todo!("peak hold + running RMS over the block")
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }

    pub fn rms(&self) -> f32 {
        self.rms_accum
    }

    pub fn clip_latched(&self) -> bool {
        self.clip_latched
    }

    pub fn clear_clip_latch(&mut self) {
        self.clip_latched = false;
    }
}
