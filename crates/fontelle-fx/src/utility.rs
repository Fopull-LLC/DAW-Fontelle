pub struct Gain {
    pub gain_db: f32,
}

pub struct Pan {
    pub pan: f32,
}

pub struct Width {
    pub width: f32,
}

pub struct PhaseInvert {
    pub left: bool,
    pub right: bool,
}

pub struct MonoMaker {
    pub crossover_hz: f32,
}

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
