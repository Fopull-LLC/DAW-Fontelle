#[derive(Debug, Clone, Copy)]
pub struct ReverbConfig {
    pub size: f32,
    pub decay_s: f32,
    pub damping: f32,
    pub pre_delay_ms: f32,
    pub diffusion: f32,
    pub modulation: f32,
    pub width: f32,
    pub freeze: bool,
}

/// Feedback delay network, 8-16 lines with Householder mixing (TDD §13.4).
/// Algorithmic rather than convolution: more tweakable, no IR licensing questions.
// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct FdnReverb {
    lines: Vec<Vec<f32>>,
}

impl FdnReverb {
    pub fn process(&mut self, _left: &mut [f32], _right: &mut [f32], _config: &ReverbConfig) {
        todo!("Householder-mixed FDN, pre-delay + damping + modulation")
    }
}
