//! Built-in effects (FONTELLE_TDD.md §13.4). Each type here is DSP only — adapted
//! to the audio graph by `fontelle-engine::EffectNode`, which is what implements
//! `AudioNode`. This crate depends on `fontelle-dsp` only (§4.1).

// Every effect here walks its block as `for frame in 0..frames` and
// `for channel in 0..used`, indexing several parallel arrays by both — the
// bus, a per-channel filter, a per-channel held value. Clippy's
// `needless_range_loop` wants each of those as an iterator chain, which for
// two indices over four arrays is less readable than the loop it replaces,
// and this is the one crate where that shape is the whole job.
#![allow(clippy::needless_range_loop)]

mod bitcrush;
mod chorus;
mod compressor;
mod delay;
mod distortion;
mod eq;
mod filter;
mod gate;
mod limiter;
mod meters;
mod repitcher;
mod reverb;
mod soften;
mod tune;
mod utility;

pub use bitcrush::Bitcrush;
pub use chorus::Chorus;
pub use compressor::Compressor;
pub use delay::Delay;
pub use distortion::Distortion;
pub use eq::ParametricEq;
pub use filter::Filter;
pub use gate::Gate;
pub use limiter::{Limiter, LimiterConfig};
pub use meters::{Oscilloscope, SpectrumAnalyser};
pub use repitcher::{Repitcher, RepitcherConfig};
pub use reverb::FdnReverb;
pub use soften::Soften;
pub use tune::{NoteInput, Tune};
pub use utility::Utility;
