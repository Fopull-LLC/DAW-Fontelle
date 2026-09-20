//! Width (`docs/flopsynth-next.md` §4.5): mid/side width with the bass
//! kept in the middle, and a gain on each half. The utility has a width
//! and a mono-maker; this is the one reached for as an *effect*, with the
//! two gains, and the one a patch's own chain offers.
//!
//! The mono-maker is a high-pass on the **side** signal, which is the
//! only thing "mono below 120 Hz" can mean: the middle is left alone and
//! the difference between the sides is taken out down there. Second
//! order, Butterworth, so the corner is a corner and not a shelf.

use fontelle_dsp::{SvfFilter, SvfMode};
use fontelle_types::{WIDTH_MONO_OFF_HZ, WidthConfig};

/// Past which the mono-maker is on: the knob's bottom is "off", and a
/// corner a hair above it would be a filter at 20 Hz doing nothing.
const OFF_MARGIN_HZ: f32 = 0.5;

pub struct Width {
    /// The mono-maker's high-pass, on the side signal.
    mono: SvfFilter,
    sample_rate: f32,
}

impl Width {
    pub fn new() -> Self {
        Self {
            mono: SvfFilter::new(),
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.mono.reset();
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &WidthConfig) {
        if channels.len() < 2 {
            // Mono in: width means nothing, and the mid gain is the only
            // control that can.
            let mid = 10f32.powf(config.mid_db.clamp(-12.0, 12.0) / 20.0);
            for channel in channels.iter_mut() {
                for sample in channel.iter_mut() {
                    *sample *= mid;
                }
            }
            return;
        }
        let width = config.width.clamp(0.0, 2.0);
        let mid_gain = 10f32.powf(config.mid_db.clamp(-12.0, 12.0) / 20.0);
        let side_gain = 10f32.powf(config.side_db.clamp(-12.0, 12.0) / 20.0) * width;
        let mono_on = config.mono_below_hz > WIDTH_MONO_OFF_HZ + OFF_MARGIN_HZ;
        let mono = SvfFilter::coeffs(
            SvfMode::Highpass,
            config.mono_below_hz,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            self.sample_rate,
        );
        // A width at rest has to be the wire exactly: `(l + r)/2 + (l − r)/2`
        // is *nearly* `l` in floating point.
        if width == 1.0 && mid_gain == 1.0 && side_gain == 1.0 && !mono_on {
            return;
        }
        let (left, rest) = channels.split_at_mut(1);
        let (left, right) = (&mut *left[0], &mut *rest[0]);
        let frames = left.len().min(right.len());
        for frame in 0..frames {
            let (l, r) = (left[frame], right[frame]);
            let mid = (l + r) * 0.5 * mid_gain;
            let mut side = (l - r) * 0.5 * side_gain;
            if mono_on {
                side = self.mono.process(side, &mono);
            }
            left[frame] = mid + side;
            right[frame] = mid - side;
        }
    }
}

impl Default for Width {
    fn default() -> Self {
        Self::new()
    }
}
