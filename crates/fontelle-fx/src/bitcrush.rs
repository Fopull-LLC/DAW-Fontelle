#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DitherMode {
    None,
    Triangular,
}

#[derive(Debug, Clone, Copy)]
pub struct BitcrushConfig {
    pub bit_depth: f32,
    pub dither: DitherMode,
    pub decimation_factor: u32,
    /// Defaults **off** — the aliasing from decimation is the point (TDD §13.4).
    pub anti_alias: bool,
    pub mix: f32,
}

// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct Bitcrush {
    hold_value: f32,
    hold_counter: u32,
}

impl Bitcrush {
    pub fn process(&mut self, _block: &mut [f32], _config: &BitcrushConfig) {
        todo!("bit-depth quantise + sample-and-hold decimation")
    }
}
