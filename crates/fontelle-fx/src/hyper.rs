//! Hyper (`docs/flopsynth-next.md` §4.5): unison as an effect. Up to four
//! copies of the signal, each detuned by a fixed number of cents and
//! panned across the image — Serum's Hyper, and the sound of a supersaw
//! put on something that is not a saw.
//!
//! # A copy
//!
//! A delay line read at a steady rate other than one: a head that falls
//! behind the write head by a constant fraction of a sample per sample
//! plays the line slower, which is a pitch below, and one that catches up
//! plays it higher. It cannot do that forever — it would run into the write
//! head or off the end of the line — so it runs over a **window**, and a
//! second head half a window behind takes over while the first jumps back,
//! the two crossfaded with a raised cosine so the jump is not heard. The
//! granular shifter every DAW's pitch effect starts as.
//!
//! Wet only: the copies, and none of the signal that made them. Half wet
//! by default, for the chorus's reason.

use crate::lines::read_at;
use fontelle_types::{HyperConfig, MAX_HYPER_VOICES, MAX_HYPER_WINDOW_MS, MIN_HYPER_WINDOW_MS};

const MAX_CHANNELS: usize = 2;
const VOICES: usize = MAX_HYPER_VOICES as usize;

/// The longest the line has to hold: the widest window, twice, plus room.
const MAX_LINE_MS: f32 = MAX_HYPER_WINDOW_MS * 2.0 + 2.0;

pub struct Hyper {
    lines: [Vec<f32>; MAX_CHANNELS],
    write: usize,
    /// Each voice's read position within its window, 0..1 — the ramp both
    /// heads are read from, half a window apart.
    ramps: [f32; VOICES],
    sample_rate: f32,
}

impl Hyper {
    pub fn new() -> Self {
        Self {
            lines: [Vec::new(), Vec::new()],
            write: 0,
            ramps: [0.0; VOICES],
            sample_rate: 48_000.0,
        }
    }

    /// Sizes the lines. **The only place this type allocates.**
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        let length = (MAX_LINE_MS * self.sample_rate / 1000.0).ceil() as usize + 4;
        for line in &mut self.lines {
            line.clear();
            line.resize(length, 0.0);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.write = 0;
        // Staggered, so the voices' crossfades do not all land together.
        for (voice, ramp) in self.ramps.iter_mut().enumerate() {
            *ramp = voice as f32 / VOICES as f32;
        }
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &HyperConfig) {
        let length = self.lines[0].len();
        if length == 0 || channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let voices = config.voices.clamp(1, MAX_HYPER_VOICES) as usize;
        let window = config
            .window_ms
            .clamp(MIN_HYPER_WINDOW_MS, MAX_HYPER_WINDOW_MS)
            * self.sample_rate
            / 1000.0;
        let spread = config.spread.clamp(0.0, 1.0);
        let detune = config.detune_cents.clamp(0.0, 100.0);

        // Each voice's pitch ratio, spread evenly across ±detune; one voice
        // sits at +detune, so one copy is still a detuned copy. And its
        // place in the image: alternately left and right of the middle.
        let mut ratios = [1.0f32; VOICES];
        let mut gains = [(1.0f32, 1.0f32); VOICES];
        for voice in 0..voices {
            let across = if voices > 1 {
                voice as f32 / (voices - 1) as f32 * 2.0 - 1.0
            } else {
                1.0
            };
            ratios[voice] = 2f32.powf(across * detune / 1_200.0);
            let pan = across * spread;
            let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
            gains[voice] = (angle.cos(), angle.sin());
        }
        // A copy's read head falls behind (or ahead) by `1 − ratio` a
        // sample; the ramp is that distance over the window.
        let share = 1.0 / voices as f32;
        // Equal-power pans sum to more than one voice's worth; the share
        // is over the count so four copies of a unit sine are not four.
        let norm = share * std::f32::consts::SQRT_2;

        for frame in 0..frames {
            let mut wet = [0.0f32; MAX_CHANNELS];
            for voice in 0..voices {
                let ramp = self.ramps[voice];
                // Two heads half a window apart, each read `ramp × window`
                // behind a base delay of half a window (so the crossfade
                // has room either side), crossfaded by a raised cosine.
                let mut sum = [0.0f32; MAX_CHANNELS];
                for head in 0..2 {
                    let position = (ramp + head as f32 * 0.5).fract();
                    let behind = 1.0 + position * window;
                    let fade = (std::f32::consts::PI * position).sin();
                    let fade = fade * fade;
                    for channel in 0..used {
                        sum[channel] +=
                            read_at(&self.lines[channel], self.write as f32 - behind) * fade;
                    }
                }
                let (gl, gr) = gains[voice];
                wet[0] += sum[0] * gl;
                if used > 1 {
                    wet[1] += sum[1] * gr;
                }
                // The heads move at `1 − ratio` samples a sample through
                // the window, wrapping.
                self.ramps[voice] = (ramp + (1.0 - ratios[voice]) / window).rem_euclid(1.0);
            }
            for channel in 0..used {
                self.lines[channel][self.write] = channels[channel][frame];
                channels[channel][frame] = wet[channel] * norm;
            }
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
            self.write = (self.write + 1) % length;
        }
    }
}

impl Default for Hyper {
    fn default() -> Self {
        Self::new()
    }
}
