//! The flanger (`docs/flopsynth-next.md` §4.5; `docs/effects-catalogue.md`
//! §2.4's row): one short modulated delay with signed feedback, and a
//! through-zero mode. The chorus's line read at a tenth of the chorus's
//! delay, which is where a comb's teeth stop being thickness and start
//! being a swept resonance.
//!
//! Wet only, like the chorus and for its reason (rule 7): what this writes
//! is the delayed copy, and the comb is the sum with the dry that
//! `EffectNode` makes. So it opens half wet.
//!
//! # Through zero
//!
//! A tape flange was two machines, one of them slowed by a thumb on the
//! reel, and the sweep passed *through* the point where the two were in
//! step. One delay against a dry signal cannot do that: the dry is at
//! zero and the copy can only approach it. So `through_zero` writes a
//! second, **fixed** copy at the centre beside the swept one, and the sweep
//! runs from nothing to twice the centre — through the fixed copy, where
//! the two cancel everything and reinforce everything once a cycle each.
//! The node's dry is then a third copy; set the mix to full for the pure
//! thing.

use crate::lines::read_at;
use fontelle_types::{FlangerConfig, MAX_FLANGER_DELAY_MS, MIN_FLANGER_DELAY_MS};

const MAX_CHANNELS: usize = 2;

/// The longest the line has to hold: twice the widest centre, plus room
/// for the interpolator.
const MAX_LINE_MS: f32 = MAX_FLANGER_DELAY_MS * 2.0 + 1.0;

/// How close to the write head a read is allowed to get, in samples: the
/// interpolator reads one ahead, and one ahead of the write head is the
/// oldest sample in the line.
const MIN_READ_SAMPLES: f32 = 1.5;

/// A short modulated delay with signed feedback.
pub struct Flanger {
    lines: [Vec<f32>; MAX_CHANNELS],
    write: usize,
    phase: f64,
    sample_rate: f32,
}

impl Flanger {
    pub fn new() -> Self {
        Self {
            lines: [Vec::new(), Vec::new()],
            write: 0,
            phase: 0.0,
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
        self.phase = 0.0;
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &FlangerConfig, bpm: f32) {
        let length = self.lines[0].len();
        if length == 0 || channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let rate = config.effective_rate_hz(bpm);
        let step = f64::from(rate) / f64::from(self.sample_rate);
        let per_ms = self.sample_rate / 1000.0;
        let centre = config
            .delay_ms
            .clamp(MIN_FLANGER_DELAY_MS, MAX_FLANGER_DELAY_MS)
            * per_ms;
        // The swing is the room the centre has above the shortest read, so
        // at full depth the copy passes from almost nothing to twice the
        // centre and never into the write head.
        let swing = config.depth.clamp(0.0, 1.0) * (centre - MIN_READ_SAMPLES).max(0.0);
        let feedback = config.feedback.clamp(-0.95, 0.95);
        let spread = f64::from(config.spread.clamp(0.0, 1.0)) * 0.5;

        for frame in 0..frames {
            let mut wet = [0.0f32; MAX_CHANNELS];
            for channel in 0..used {
                let offset = if channel == 0 { 0.0 } else { spread };
                let lfo = (std::f64::consts::TAU * (self.phase + offset)).sin() as f32;
                let behind = centre + swing * lfo;
                let swept = read_at(&self.lines[channel], self.write as f32 - behind);
                wet[channel] = if config.through_zero {
                    let fixed = read_at(&self.lines[channel], self.write as f32 - centre);
                    (swept + fixed) * 0.5
                } else {
                    swept
                };
            }
            for channel in 0..used {
                self.lines[channel][self.write] =
                    channels[channel][frame] + feedback * wet[channel];
                channels[channel][frame] = wet[channel];
            }
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
            self.phase = (self.phase + step).fract();
            self.write = (self.write + 1) % length;
        }
    }
}

impl Default for Flanger {
    fn default() -> Self {
        Self::new()
    }
}
