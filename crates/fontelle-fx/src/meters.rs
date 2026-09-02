//! The metering rows of `docs/effects-catalogue.md` §2.5, which are windows
//! rather than processors: they change nothing about the signal and exist to
//! draw it.
//!
//! Stubs. The shapes are the boundary; what each one needs is written in the
//! catalogue, and the analyser's maths already exists in
//! `fontelle_dsp::SpectrumAnalyser` — what is missing is the insert around it
//! and the window it draws in.

// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct SpectrumAnalyser {
    fft: realfft::RealFftPlanner<f32>,
}

pub struct Oscilloscope;

// Fields are wired up once the real DSP lands; the shape is the boundary for now.
#[allow(dead_code)]
pub struct Tuner {
    detected_hz: f32,
}
