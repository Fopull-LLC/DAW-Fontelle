//! Built-in effects (FONTELLE_TDD.md §13.4). Each type here is DSP only — adapted
//! to the audio graph by `fontelle-engine::EffectNode`, which is what implements
//! `AudioNode`. This crate depends on `fontelle-dsp` only (§4.1).

mod bitcrush;
mod compressor;
mod delay;
mod distortion;
mod eq;
mod limiter;
mod repitcher;
mod reverb;
mod soften;
mod utility;

pub use bitcrush::{Bitcrush, BitcrushConfig, DitherMode};
pub use compressor::Compressor;
pub use delay::{Delay, DelayConfig, DelayTime};
pub use distortion::{Distortion, DistortionConfig, DistortionCurve};
pub use eq::ParametricEq;
pub use limiter::{Limiter, LimiterConfig};
pub use repitcher::{Repitcher, RepitcherConfig};
pub use reverb::{FdnReverb, ReverbConfig};
pub use soften::{Soften, SoftenConfig, SoftenPreset};
pub use utility::{
    Gain, MonoMaker, Oscilloscope, Pan, PhaseInvert, SpectrumAnalyser, Tuner, Width,
};
