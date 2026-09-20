//! The frequency shifter (`docs/flopsynth-next.md` §4.5;
//! `docs/effects-catalogue.md` §2.3's row): every partial moved by the same
//! number of hertz. Not a pitch shift — a harmonic series shifted by 200 Hz
//! is no longer a harmonic series, which is exactly the point — and the
//! one sound an EQ cannot make.
//!
//! # How
//!
//! Single-sideband modulation. The input is split into two copies ninety
//! degrees apart (a Hilbert pair: two cascades of second-order all-passes
//! whose phase responses differ by 90° across the band, Olli Niemitalo's
//! coefficients), and each is multiplied by one half of a quadrature
//! oscillator at the shift. `I·cos − Q·sin` keeps the upper sideband,
//! `I·cos + Q·sin` the lower, and both together is a ring modulator with
//! the carrier taken out.
//!
//! # The barber-pole
//!
//! Feedback: the shifted output goes back into the input, gets shifted
//! again, and again — a line at `f` becomes lines at `f + s`, `f + 2s`,
//! `f + 3s`, each quieter, and with a shift of a few hertz that is the
//! endless climb.

use fontelle_types::{MAX_SHIFT_HZ, ShiftDirection, ShifterConfig};

const MAX_CHANNELS: usize = 2;

/// The two all-pass cascades' pole positions — Niemitalo's Hilbert
/// transformer, good to a fraction of a degree from 20 Hz to 20 kHz at
/// 44.1 kHz and a little less at 48. Written to `f32`'s precision; the
/// published figures carry more digits than the type does.
const PATH_A: [f32; 4] = [0.692_387_8, 0.936_065_4, 0.988_229_5, 0.998_748_8];
const PATH_B: [f32; 4] = [0.402_192_1, 0.856_171_1, 0.972_291, 0.995_288_5];

/// One second-order all-pass section's memory: `y = a²(x + y₂) − x₂`.
#[derive(Clone, Copy, Default)]
struct Section {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Section {
    fn run(&mut self, x: f32, a2: f32) -> f32 {
        let y = a2 * (x + self.y2) - self.x2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Copy, Default)]
struct Hilbert {
    a: [Section; 4],
    b: [Section; 4],
    /// Path A runs a sample behind path B.
    a_delay: f32,
}

impl Hilbert {
    /// The in-phase and quadrature halves of `x`.
    ///
    /// Which path carries the sample of delay was measured, not assumed:
    /// on the wrong one the two come out a quarter turn apart *less* the
    /// delay's own phase, which at 1 kHz is fifteen degrees and a sideband
    /// rejected by eighteen decibels instead of fifty.
    fn run(&mut self, x: f32) -> (f32, f32) {
        let mut i = self.a_delay;
        self.a_delay = x;
        for (section, a2) in self.a.iter_mut().zip(PATH_A) {
            i = section.run(i, a2 * a2);
        }
        let mut q = x;
        for (section, a2) in self.b.iter_mut().zip(PATH_B) {
            q = section.run(q, a2 * a2);
        }
        (i, q)
    }
}

pub struct Shifter {
    hilbert: [Hilbert; MAX_CHANNELS],
    fed: [f32; MAX_CHANNELS],
    /// The carrier's phase, in turns.
    phase: f64,
    sample_rate: f32,
}

impl Shifter {
    pub fn new() -> Self {
        Self {
            hilbert: [Hilbert::default(); MAX_CHANNELS],
            fed: [0.0; MAX_CHANNELS],
            phase: 0.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.hilbert = [Hilbert::default(); MAX_CHANNELS];
        self.fed = [0.0; MAX_CHANNELS];
        self.phase = 0.0;
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &ShifterConfig) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let shift = config
            .total_hz()
            .clamp(-MAX_SHIFT_HZ - 20.0, MAX_SHIFT_HZ + 20.0);
        // A negative shift up is a shift down: the sign folds into the
        // direction so the oscillator always runs forward.
        let (direction, shift) = match (config.direction, shift < 0.0) {
            (ShiftDirection::Up, true) => (ShiftDirection::Down, -shift),
            (ShiftDirection::Down, true) => (ShiftDirection::Up, -shift),
            (ShiftDirection::Both, true) => (ShiftDirection::Both, -shift),
            (direction, false) => (direction, shift),
        };
        let step = f64::from(shift) / f64::from(self.sample_rate);
        let feedback = config.feedback.clamp(0.0, 0.9);

        for frame in 0..frames {
            let angle = std::f64::consts::TAU * self.phase;
            let (sin, cos) = (angle.sin() as f32, angle.cos() as f32);
            for channel in 0..used {
                let x = channels[channel][frame] + feedback * self.fed[channel];
                let (i, q) = self.hilbert[channel].run(x);
                // Which sign is which was measured, not assumed: path B
                // here comes out a quarter turn *ahead* of path A, so the
                // textbook's `I·cos − Q·sin` for the upper sideband is the
                // lower one with these coefficients.
                let out = match direction {
                    ShiftDirection::Up => i * cos + q * sin,
                    ShiftDirection::Down => i * cos - q * sin,
                    // Both sidebands at the level one would have: the
                    // `Q·sin` terms cancel and what is left is `I·cos`.
                    ShiftDirection::Both => i * cos,
                };
                self.fed[channel] = out;
                channels[channel][frame] = out;
            }
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
            self.phase = (self.phase + step).fract();
        }
    }
}

impl Default for Shifter {
    fn default() -> Self {
        Self::new()
    }
}
