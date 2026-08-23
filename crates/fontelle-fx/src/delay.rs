#[derive(Debug, Clone, Copy)]
pub enum DelayTime {
    Free(f32),
    TempoSynced { division_ticks: i64 },
}

#[derive(Debug, Clone, Copy)]
pub struct DelayConfig {
    pub time: DelayTime,
    pub feedback: f32,
    pub ping_pong: bool,
    pub filter_cutoff_hz: f32,
    pub saturation: f32,
    /// Variable delay-line read with pitch artefacts on time change — the mode that
    /// produces character (TDD §13.4).
    pub tape_mode: bool,
    pub mix: f32,
}

// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct Delay {
    buffer: Vec<f32>,
    write_pos: usize,
}

impl Delay {
    pub fn process(&mut self, _left: &mut [f32], _right: &mut [f32], _config: &DelayConfig) {
        todo!("tempo-aware delay line with filtered/saturated feedback loop")
    }
}
