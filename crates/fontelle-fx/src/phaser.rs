//! The phaser (`docs/flopsynth-next.md` §4.5; `docs/effects-catalogue.md`
//! §2.4's row): a cascade of first-order all-passes at one corner, the
//! corner swept by an LFO, the chain's output fed back to its input.
//!
//! **What comes out is the all-passed signal alone.** An all-pass passes
//! every frequency at unit gain; the notches a phaser is known for are
//! what happens when that signal is summed with the one it came from, and
//! `EffectNode` owns that sum (rule 7). It is why a phaser opens half wet,
//! like the chorus — see `EffectKind::is_time_based`.
//!
//! # The stages
//!
//! Each stage is the bilinear first-order all-pass, `2·LP − x` on a TPT
//! one-pole, so the notches sit exactly where the closed form puts them:
//! `N` stages at corner `f` notch wherever `2N·atan(w)` is an odd
//! multiple of π. The synth's `FilterModel::Phaser` is the same chain
//! (`fontelle-dsp/src/synth_filter.rs`), per voice; this is the one for a
//! bus, with an LFO of its own.
//!
//! # The feedback
//!
//! Reads the chain's previous output. A chain of all-passes has unit gain
//! at every frequency, so a loop gain under one is stable wherever the
//! sample of delay puts its phase, and the algebraic solve the ladder
//! needed is not needed here.

use fontelle_types::{MAX_PHASER_STAGES, MIN_PHASER_STAGES, PHASER_SWEEP_OCTAVES, PhaserConfig};

const MAX_CHANNELS: usize = 2;
const STAGES: usize = MAX_PHASER_STAGES as usize;

/// A swept all-pass cascade.
pub struct Phaser {
    stages: [[f32; STAGES]; MAX_CHANNELS],
    fed: [f32; MAX_CHANNELS],
    /// The LFO's phase, 0..1. `f64` for the chorus's reason: a hundred
    /// thousand `f32` steps of a slow rate drift by a fraction of a cycle.
    phase: f64,
    sample_rate: f32,
}

impl Phaser {
    pub fn new() -> Self {
        Self {
            stages: [[0.0; STAGES]; MAX_CHANNELS],
            fed: [0.0; MAX_CHANNELS],
            phase: 0.0,
            sample_rate: 48_000.0,
        }
    }

    /// Nothing to allocate: the stages are an array.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.stages = [[0.0; STAGES]; MAX_CHANNELS];
        self.fed = [0.0; MAX_CHANNELS];
        self.phase = 0.0;
    }

    /// Runs `channels` through the chain in place, replacing them with the
    /// all-passed signal. `bpm` is for a rate set in note values.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &PhaserConfig, bpm: f32) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let stages = config.stages.clamp(MIN_PHASER_STAGES, MAX_PHASER_STAGES) as usize;
        let rate = config.effective_rate_hz(bpm);
        let step = f64::from(rate) / f64::from(self.sample_rate);
        let octaves = config.depth.clamp(0.0, 1.0) * PHASER_SWEEP_OCTAVES;
        // Never quite one — a loop at unity gain rings forever.
        let feedback = config.feedback.clamp(-0.9, 0.9);
        let spread = f64::from(config.spread.clamp(0.0, 1.0)) * 0.5;
        let centre = config.centre_hz.clamp(20.0, self.sample_rate * 0.45);
        let ceiling = self.sample_rate * 0.45;

        for frame in 0..frames {
            for channel in 0..used {
                // Each side at its own point of the sweep: half a cycle apart
                // at full spread.
                let offset = if channel == 0 { 0.0 } else { spread };
                let lfo = (std::f64::consts::TAU * (self.phase + offset)).sin() as f32;
                let corner = (centre * 2f32.powf(octaves * lfo)).min(ceiling);
                let g = (std::f32::consts::PI * corner / self.sample_rate).tan();
                let big_g = g / (1.0 + g);
                let mut x = channels[channel][frame] + feedback * self.fed[channel];
                for stage in self.stages[channel].iter_mut().take(stages) {
                    let v = (x - *stage) * big_g;
                    let low = v + *stage;
                    *stage = low + v;
                    x = 2.0 * low - x;
                }
                self.fed[channel] = x;
                channels[channel][frame] = x;
            }
            // Channels there is no chain for carry nothing rather than dry
            // signal in the wet path.
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
            self.phase = (self.phase + step).fract();
        }
    }
}

impl Default for Phaser {
    fn default() -> Self {
        Self::new()
    }
}
