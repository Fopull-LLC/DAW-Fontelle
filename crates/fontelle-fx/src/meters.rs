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

// The `Tuner` stub was here. `docs/tune-plan.md` §8 took its job: the pitch
// it was going to detect is `fontelle_dsp::PitchTracker`'s, built for the
// corrector and tested on sines, saws and vowels, and the needle it was going
// to draw is the corrector's viewport. What the catalogue's Tuner row still
// wants is a *read-out* insert around that tracker — a window with a needle
// and no processing — and it will wrap `PitchTracker` rather than grow a
// second pitch detector beside it.
