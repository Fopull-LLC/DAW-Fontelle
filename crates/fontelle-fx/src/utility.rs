//! The plumbing insert: gain, pan, width, the phase and mute switches, the
//! mono-maker and the rumble filter (`docs/effects-catalogue.md` §2.5). Its
//! parameters are `fontelle_types::UtilityConfig`.
//!
//! # The path
//!
//! ```text
//! L,R → mute → invert → swap → mid/side → × width → (side high-pass) ─┐
//!                                                          L,R ←──────┘
//!                       out ← × gain ← × balance ← (DC / rumble) ←────┘
//! ```
//!
//! Every stage here is a *repair*, and the order is the order a repair
//! happens in: what arrives is silenced or flipped, then the two sides are
//! put where they belong, then the image is set, then the bottom is cleaned,
//! and only then is it placed and levelled. Two of those orderings are load
//! bearing and are measured in `tests/utility.rs`:
//!
//! - **Width runs before the mono-maker.** The other way round, widening
//!   would put back exactly the low-end side content the mono-maker had just
//!   taken out, and the knob a person turned last would be the one that lost.
//! - **Invert runs before the width.** That is what makes width at 0 with one
//!   side flipped a *null* — the standard way of checking that two takes are
//!   the same take.
//!
//! # What is not here
//!
//! No smoothing on the gain or the pan. An insert's parameters arrive per
//! block from `EffectNode`, which is where a knob's ramp belongs if one is
//! ever wanted; putting one here would give this effect a memory it does not
//! otherwise have, and the two filters are the only state it keeps.

use fontelle_dsp::{SvfFilter, SvfMode};
use fontelle_types::{PanLaw, UTILITY_DC_OFF_HZ, UTILITY_MONO_OFF_HZ, UtilityConfig};

const MAX_CHANNELS: usize = 2;

/// The Q of one section of a fourth-order Linkwitz–Riley pair — two
/// Butterworth sections in series. What the mono-maker's high-pass is built
/// from, and the same constant the distortion's clean-low split uses.
const LR_SECTION_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// One notch above the bottom of a knob is where "off" stops. A hair above
/// the stop rather than at it, so a knob that has been dragged to the end and
/// landed on 20.0001 Hz still reads as off.
const OFF_MARGIN_HZ: f32 = 0.5;

/// Gain, pan, width, polarity, the mono-maker and the rumble filter
/// (`docs/effects-catalogue.md` §2.5).
pub struct Utility {
    /// The mono-maker's high-pass, on the **side** signal — of which there is
    /// one, so this is one filter rather than one per channel. Two sections
    /// for the Linkwitz–Riley slope.
    mono: [SvfFilter; 2],
    /// The DC / rumble filter, one per channel.
    dc: [SvfFilter; MAX_CHANNELS],
    sample_rate: f32,
}

impl Utility {
    pub fn new() -> Self {
        Self {
            mono: Default::default(),
            dc: Default::default(),
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        for filter in self.mono.iter_mut().chain(self.dc.iter_mut()) {
            filter.reset();
        }
    }

    /// Runs `channels` through the plumbing in place.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &UtilityConfig) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let rate = self.sample_rate;

        let gain = 10f32.powf(config.gain_db.clamp(-60.0, 24.0) / 20.0);
        // A balance rather than a pan law, for the reason written on
        // `UtilityConfig::pan`: what arrives is already stereo.
        let (pan_left, pan_right) = PanLaw::Linear.gains(config.pan);
        let width = config.width.clamp(0.0, 2.0);

        let mono_on = config.mono_below_hz > UTILITY_MONO_OFF_HZ + OFF_MARGIN_HZ;
        let mono = SvfFilter::coeffs(
            SvfMode::Highpass,
            config.mono_below_hz,
            LR_SECTION_Q,
            0.0,
            rate,
        );
        let dc_on = config.dc_hz > UTILITY_DC_OFF_HZ + OFF_MARGIN_HZ;
        let dc = SvfFilter::coeffs(
            SvfMode::Highpass,
            config.dc_hz,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            rate,
        );

        // Whether the mid/side stage is in the path at all. Skipped when
        // neither knob has anything to say, and that is not only for the
        // arithmetic: `(l + r) / 2 + (l - r) / 2` is *nearly* `l` in floating
        // point, and a utility at rest has to be the wire exactly —
        // `a_fresh_utility_is_a_wire_sample_for_sample`.
        let imaging = width != 1.0 || mono_on;

        // The whole block runs one frame at a time rather than one stage at a
        // time, so nothing here needs a scratch buffer: this is the audio
        // thread (INVARIANT 1).
        for frame in 0..frames {
            if used >= 2 {
                let mut left = channels[0][frame];
                let mut right = channels[1][frame];
                // What arrived, repaired. The switches name the channel that
                // arrives; the swap happens after them.
                if config.mute_left {
                    left = 0.0;
                }
                if config.mute_right {
                    right = 0.0;
                }
                if config.invert_left {
                    left = -left;
                }
                if config.invert_right {
                    right = -right;
                }
                if config.swap {
                    std::mem::swap(&mut left, &mut right);
                }
                if imaging {
                    let mid = (left + right) * 0.5;
                    let mut side = (left - right) * 0.5 * width;
                    // The mono-maker: the *side* signal high-passed, which is
                    // the only thing "mono below 120 Hz" can mean — the middle
                    // is what is left when there is no side, so filtering the
                    // middle would take the bass out rather than centre it.
                    if mono_on {
                        for section in 0..2 {
                            side = self.mono[section].process(side, &mono);
                        }
                    }
                    left = mid + side;
                    right = mid - side;
                }
                if dc_on {
                    left = self.dc[0].process(left, &dc);
                    right = self.dc[1].process(right, &dc);
                }
                channels[0][frame] = left * pan_left * gain;
                channels[1][frame] = right * pan_right * gain;
            } else {
                // One channel: the stereo controls have nothing to act on,
                // and the two that do not need a pair still work. Nothing in
                // this build's graph hands an insert a mono bus, and an
                // effect that indexed the second channel anyway would be a
                // panic waiting for the day something does.
                let mut sample = channels[0][frame];
                if config.mute_left {
                    sample = 0.0;
                }
                if config.invert_left {
                    sample = -sample;
                }
                if dc_on {
                    sample = self.dc[0].process(sample, &dc);
                }
                channels[0][frame] = sample * gain;
            }
            // A bus with more channels than this knows what to do with is
            // silenced past the pair, the way every other effect here does.
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
        }
    }
}

impl Default for Utility {
    fn default() -> Self {
        Self::new()
    }
}
