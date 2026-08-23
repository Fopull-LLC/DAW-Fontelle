use fontelle_dsp::SvfFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandType {
    Bell,
    LowShelf,
    HighShelf,
    LowPass12,
    LowPass24,
    LowPass48,
    HighPass12,
    HighPass24,
    HighPass48,
    Notch,
    BandPass,
}

#[derive(Debug, Clone, Copy)]
pub struct EqBand {
    pub band_type: BandType,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
    pub solo: bool,
}

/// 8 bands, TPT/SVF topology per band, mid/side capable (TDD §13.4).
// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct ParametricEq {
    bands: [EqBand; 8],
    filters: [SvfFilter; 8],
    pub mid_side: bool,
}

impl ParametricEq {
    pub fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {
        todo!("per-band SVF cascade, mid/side matrix if enabled")
    }
}
