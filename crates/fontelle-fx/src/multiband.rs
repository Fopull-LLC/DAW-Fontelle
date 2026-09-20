//! The multiband distortion (`docs/flopsynth-next.md` §4.5): three bands
//! on two crossovers, each driven into its own `tanh`, summed back.
//!
//! # The crossovers
//!
//! Fourth-order Linkwitz–Riley at each corner — two Butterworth sections
//! in series, whose low-pass and high-pass sum to an all-pass of unit
//! magnitude. The low crossover splits the input into the low band and
//! the rest; the high crossover splits the rest into the mid and the high
//! band; and the low band is run through the high crossover's **all-pass**
//! (its low-pass plus its high-pass) so that it carries the same phase
//! turn the other two took there. The three then sum to the input through
//! two all-passes: flat, whatever the corners are, which is what an
//! undriven multiband must be.
//!
//! The first version took the mid band as "the input less the other two",
//! which sums exactly and is wrong everywhere else: below the low corner
//! the low-pass's phase lag left `x − low` *larger* than `x`, and a tone an
//! octave under the low crossover came out of the mid band at 130 % and
//! was distorted by a drive that was meant for the mids alone.

use fontelle_dsp::{SvfFilter, SvfMode};
use fontelle_types::MultibandConfig;

const MAX_CHANNELS: usize = 2;

/// The Q of each Butterworth section a Linkwitz–Riley pair is made of.
const SECTION_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub struct Multiband {
    /// Per channel: the low crossover's low-pass and high-pass pairs, the
    /// high crossover's pair on the rest, and the high crossover's pair
    /// again on the low band for its all-pass.
    low_lp: [[SvfFilter; 2]; MAX_CHANNELS],
    low_hp: [[SvfFilter; 2]; MAX_CHANNELS],
    high_lp: [[SvfFilter; 2]; MAX_CHANNELS],
    high_hp: [[SvfFilter; 2]; MAX_CHANNELS],
    ap_lp: [[SvfFilter; 2]; MAX_CHANNELS],
    ap_hp: [[SvfFilter; 2]; MAX_CHANNELS],
    sample_rate: f32,
}

impl Multiband {
    pub fn new() -> Self {
        Self {
            low_lp: [[SvfFilter::new(); 2]; MAX_CHANNELS],
            low_hp: [[SvfFilter::new(); 2]; MAX_CHANNELS],
            high_lp: [[SvfFilter::new(); 2]; MAX_CHANNELS],
            high_hp: [[SvfFilter::new(); 2]; MAX_CHANNELS],
            ap_lp: [[SvfFilter::new(); 2]; MAX_CHANNELS],
            ap_hp: [[SvfFilter::new(); 2]; MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        for bank in [
            &mut self.low_lp,
            &mut self.low_hp,
            &mut self.high_lp,
            &mut self.high_hp,
            &mut self.ap_lp,
            &mut self.ap_hp,
        ] {
            for pair in bank.iter_mut() {
                for section in pair.iter_mut() {
                    section.reset();
                }
            }
        }
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &MultibandConfig) {
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let low_hz = config.low_hz.clamp(20.0, self.sample_rate * 0.4);
        let high_hz = config.high_hz.clamp(low_hz, self.sample_rate * 0.45);
        let rate = self.sample_rate;
        let low_lp = SvfFilter::coeffs(SvfMode::Lowpass, low_hz, SECTION_Q, 0.0, rate);
        let low_hp = SvfFilter::coeffs(SvfMode::Highpass, low_hz, SECTION_Q, 0.0, rate);
        let high_lp = SvfFilter::coeffs(SvfMode::Lowpass, high_hz, SECTION_Q, 0.0, rate);
        let high_hp = SvfFilter::coeffs(SvfMode::Highpass, high_hz, SECTION_Q, 0.0, rate);
        let drives = [
            config.low_drive_db,
            config.mid_drive_db,
            config.high_drive_db,
        ]
        .map(|db| db.clamp(0.0, 40.0));
        let output = 10f32.powf(config.output_db.clamp(-24.0, 24.0) / 20.0);

        for channel in 0..used {
            for frame in 0..frames {
                let x = channels[channel][frame];
                let lo = run(&mut self.low_lp[channel], x, &low_lp);
                let rest = run(&mut self.low_hp[channel], x, &low_hp);
                let mid = run(&mut self.high_lp[channel], rest, &high_lp);
                let hi = run(&mut self.high_hp[channel], rest, &high_hp);
                let lo = run(&mut self.ap_lp[channel], lo, &high_lp)
                    + run(&mut self.ap_hp[channel], lo, &high_hp);
                channels[channel][frame] =
                    (drive(lo, drives[0]) + drive(mid, drives[1]) + drive(hi, drives[2])) * output;
            }
        }
        for channel in channels.iter_mut().skip(used) {
            channel.fill(0.0);
        }
    }
}

/// One Linkwitz–Riley half: two Butterworth sections in series.
fn run(pair: &mut [SvfFilter; 2], x: f32, coeffs: &fontelle_dsp::SvfCoeffs) -> f32 {
    let once = pair[0].process(x, coeffs);
    pair[1].process(once, coeffs)
}

/// `tanh(x · gain)` with the make-up that keeps the level, and **exactly**
/// the input at no drive — the same shape the synth's filter drive has,
/// for the same reason: a drive knob at the bottom is the absence of an
/// effect, and `tanh` is not an identity anywhere but zero.
fn drive(x: f32, db: f32) -> f32 {
    if db <= 0.0 {
        return x;
    }
    let gain = 10f32.powf(db / 20.0);
    (x * gain).tanh() / gain.tanh()
}

impl Default for Multiband {
    fn default() -> Self {
        Self::new()
    }
}
