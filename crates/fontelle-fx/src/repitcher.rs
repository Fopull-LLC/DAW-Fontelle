/// v1: varispeed (pitch and time locked) plus high-quality resampling only — pure
/// Rust. Formant-preserving pitch shift is v2, behind a `stretch` feature flag once
/// `signalsmith-stretch` (C++, via `cxx`) is accepted (TDD §3.3).
#[derive(Debug, Clone, Copy)]
pub struct RepitcherConfig {
    pub semitones: f32,
}

pub struct Repitcher;

impl Repitcher {
    pub fn process(&mut self, _block: &mut [f32], _config: &RepitcherConfig) {
        todo!("varispeed via fontelle_dsp::interpolate at High/Ultra quality")
    }
}
