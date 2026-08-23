#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistortionCurve {
    SoftClip,
    HardClip,
    Tube,
    Fold,
    WaveShape,
}

#[derive(Debug, Clone, Copy)]
pub struct DistortionConfig {
    pub curve: DistortionCurve,
    pub drive: f32,
    pub oversample: u8, // 2..=8
    pub pre_filter_hz: Option<f32>,
    pub post_filter_hz: Option<f32>,
    pub mix: f32,
}

pub struct Distortion;

impl Distortion {
    pub fn process(&mut self, _block: &mut [f32], _config: &DistortionConfig) {
        todo!("oversampled waveshaping per DistortionCurve")
    }
}
