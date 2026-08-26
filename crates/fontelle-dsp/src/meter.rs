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

    /// Folds `block` into the meter: the peak is held (it only ever rises
    /// until reset), the RMS is the block's own, and a sample at or past full
    /// scale latches the clip indicator.
    ///
    /// Peak is *held* rather than replaced because a meter that reported only
    /// the last block's peak would flicker past anything shorter than a
    /// refresh — the UI resets it when it has drawn it (TDD §13.3), which is
    /// what makes "peak hold" mean what it says.
    pub fn process_block(&mut self, block: &[f32]) {
        if block.is_empty() {
            return;
        }
        let mut peak = self.peak;
        let mut sum_squares = 0.0f64;
        for &sample in block {
            let magnitude = sample.abs();
            if magnitude > peak {
                peak = magnitude;
            }
            sum_squares += (sample as f64) * (sample as f64);
        }
        self.peak = peak;
        // f64 for the accumulation: a long block of quiet samples summed in
        // f32 loses the small ones entirely once the running total is large.
        self.rms_accum = (sum_squares / block.len() as f64).sqrt() as f32;
        // At full scale, not past it: a sample of exactly 1.0 is already the
        // largest value the output format can hold, and everything above it
        // will be clamped on the way out.
        self.clip_latched |= peak >= 1.0;
    }

    /// Drops the held peak back to the current block's level. The UI calls
    /// this once it has drawn the value it read.
    pub fn reset_peak(&mut self) {
        self.peak = 0.0;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_is_the_largest_magnitude_regardless_of_sign() {
        let mut meter = PeakRmsMeter::new();
        meter.process_block(&[0.1, -0.8, 0.3]);
        assert!((meter.peak() - 0.8).abs() < 1e-6, "got {}", meter.peak());
    }

    #[test]
    fn rms_of_a_full_scale_square_is_one_and_of_a_sine_is_root_two_over_two() {
        let mut meter = PeakRmsMeter::new();
        meter.process_block(&[1.0, -1.0, 1.0, -1.0]);
        assert!((meter.rms() - 1.0).abs() < 1e-6, "got {}", meter.rms());

        let sine: Vec<f32> = (0..1000)
            .map(|i| (std::f32::consts::TAU * i as f32 / 100.0).sin())
            .collect();
        let mut meter = PeakRmsMeter::new();
        meter.process_block(&sine);
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (meter.rms() - expected).abs() < 1e-3,
            "a sine's RMS is 0.707 of its peak, got {}",
            meter.rms()
        );
    }

    /// A meter that reported only the last block's peak would flicker past
    /// anything shorter than a UI refresh — which is every transient worth
    /// seeing.
    #[test]
    fn the_peak_is_held_until_it_is_reset() {
        let mut meter = PeakRmsMeter::new();
        meter.process_block(&[0.9]);
        meter.process_block(&[0.1]);
        assert!((meter.peak() - 0.9).abs() < 1e-6, "got {}", meter.peak());

        meter.reset_peak();
        meter.process_block(&[0.1]);
        assert!((meter.peak() - 0.1).abs() < 1e-6, "got {}", meter.peak());
    }

    #[test]
    fn the_clip_indicator_latches_at_full_scale_and_stays_until_cleared() {
        let mut meter = PeakRmsMeter::new();
        meter.process_block(&[0.99]);
        assert!(!meter.clip_latched());

        meter.process_block(&[1.0]);
        assert!(meter.clip_latched(), "exactly full scale already clips");

        meter.process_block(&[0.0]);
        assert!(meter.clip_latched(), "and it must not clear itself");

        meter.clear_clip_latch();
        assert!(!meter.clip_latched());
    }

    #[test]
    fn an_empty_block_changes_nothing() {
        let mut meter = PeakRmsMeter::new();
        meter.process_block(&[0.5]);
        meter.process_block(&[]);
        assert!((meter.peak() - 0.5).abs() < 1e-6);
        assert!((meter.rms() - 0.5).abs() < 1e-6);
    }
}
