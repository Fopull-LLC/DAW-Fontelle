//! The soundfont-harshness effect (TDD §13.5). Its parameters are
//! `fontelle_types::SoftenConfig` — the document owns those, this owns the
//! filters and the envelopes.
//!
//! # Why four stages and not one low-pass
//!
//! A sampled instrument played at a pitch it was not recorded at, through a
//! filter an SF2 file specified decades ago, is harsh in ways that a treble
//! control cannot separate. Turning the top down fixes all of them by throwing
//! away the record, and the result sounds *dull* rather than *smooth* — which
//! is the difference this file exists to make.
//!
//! - The **dynamic shelf** cuts the top only in proportion to how much of it
//!   there is, so a quiet passage is not darkened along with a loud one.
//! - The **suppressor** ducks whichever of three bands across 1–6 kHz is
//!   currently hot. That region is where a resampled sample's resonance sits,
//!   and cutting all of it always is a notch you can hear on everything.
//! - The **transient softener** takes the edge off an attack, because the
//!   sharpest thing about a sampled note is usually the sample's own start.
//! - The **air restore** puts the very top back — above the harsh region, not
//!   inside it. Without this the first three add up to a low-pass.
//!
//! Every one of them is a *gain that moves*, which is what makes the effect
//! adaptive rather than a fixed curve, and what every test in
//! `tests/soften.rs` measures.

use fontelle_dsp::{SvfCoeffs, SvfFilter, SvfMode};
use fontelle_types::SoftenConfig;

const MAX_CHANNELS: usize = 2;

/// Where the three suppressor bands sit, in Hz.
///
/// Spread across the 1–6 kHz region §13.5 names, spaced roughly by ear rather
/// than evenly: the honk of a stretched sample lands lower than the fizz of
/// one played above its root.
const HONK_HZ: [f32; 3] = [1_500.0, 2_800.0, 4_500.0];

/// The corner of the dynamic shelf, and of the detector that drives it.
const SHELF_HZ: f32 = 5_000.0;

/// Where the air comes back — **above** the shelf's corner, or the two stages
/// would be one control undoing itself.
const AIR_HZ: f32 = 12_000.0;

/// The deepest each stage can go, in dB, at an amount of 1.
const MAX_SHELF_CUT_DB: f32 = 14.0;
const MAX_HONK_CUT_DB: f32 = 12.0;
const MAX_AIR_LIFT_DB: f32 = 6.0;

/// How much of an attack the softener can take off, as a gain.
const MAX_TRANSIENT_DUCK: f32 = 0.75;

/// A one-pole envelope follower.
#[derive(Debug, Clone, Copy, Default)]
struct Follower {
    value: f32,
}

impl Follower {
    /// Asymmetric on purpose: a detector that rises fast and falls slowly is
    /// what makes "how much of this is there" a stable number rather than one
    /// that tracks the waveform.
    fn feed(&mut self, level: f32, attack: f32, release: f32) -> f32 {
        let coeff = if level > self.value { attack } else { release };
        self.value += (level - self.value) * coeff;
        self.value
    }
}

/// The four-stage softener (TDD §13.5).
pub struct Soften {
    /// The detector's band-splitters: what the shelf listens to, and what each
    /// suppressor band listens to.
    shelf_detect: [SvfFilter; MAX_CHANNELS],
    honk_detect: [[SvfFilter; 3]; MAX_CHANNELS],
    /// The stages themselves.
    shelf: [SvfFilter; MAX_CHANNELS],
    honk: [[SvfFilter; 3]; MAX_CHANNELS],
    air: [SvfFilter; MAX_CHANNELS],
    /// Envelopes, shared across channels so the stereo image does not move
    /// with the material — the same reason the compressor is stereo-linked.
    shelf_env: Follower,
    honk_env: [Follower; 3],
    overall_env: Follower,
    fast: Follower,
    slow: Follower,
    sample_rate: f32,
}

impl Soften {
    pub fn new() -> Self {
        Self {
            shelf_detect: Default::default(),
            honk_detect: Default::default(),
            shelf: Default::default(),
            honk: Default::default(),
            air: Default::default(),
            shelf_env: Follower::default(),
            honk_env: [Follower::default(); 3],
            overall_env: Follower::default(),
            fast: Follower::default(),
            slow: Follower::default(),
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        for filter in self
            .shelf_detect
            .iter_mut()
            .chain(self.shelf.iter_mut())
            .chain(self.air.iter_mut())
        {
            filter.reset();
        }
        for channel in self.honk_detect.iter_mut().chain(self.honk.iter_mut()) {
            for filter in channel {
                filter.reset();
            }
        }
        self.shelf_env = Follower::default();
        self.honk_env = [Follower::default(); 3];
        self.overall_env = Follower::default();
        self.fast = Follower::default();
        self.slow = Follower::default();
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &SoftenConfig) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);

        let shelf_amount = config.shelf_amount.clamp(0.0, 1.0);
        let honk_amount = config.suppressor_amount.clamp(0.0, 1.0);
        let transient_amount = config.transient_amount.clamp(0.0, 1.0);
        let air_amount = config.air_restore_amount.clamp(0.0, 1.0);
        // Nothing asked for is nothing done, exactly: `every_stage_at_zero_is_
        // a_wire` is what makes the effect comparable against itself being off.
        if shelf_amount == 0.0 && honk_amount == 0.0 && transient_amount == 0.0 && air_amount == 0.0
        {
            return;
        }

        // Detector splitters: fixed, so they are built once a block.
        let shelf_probe =
            SvfFilter::coeffs(SvfMode::Highpass, SHELF_HZ, 0.707, 0.0, self.sample_rate);
        let honk_probe: [SvfCoeffs; 3] = std::array::from_fn(|band| {
            SvfFilter::coeffs(SvfMode::Bandpass, HONK_HZ[band], 1.4, 0.0, self.sample_rate)
        });

        // Envelope coefficients. The shelf and the suppressor want a *slow*
        // read of how much energy is in their band — this is a tone decision,
        // not a compressor — and the transient stage wants two speeds.
        let slow_attack = one_pole(20.0, self.sample_rate);
        let slow_release = one_pole(3.0, self.sample_rate);
        let fast_attack = one_pole(800.0, self.sample_rate);
        let fast_release = one_pole(80.0, self.sample_rate);
        let air_lift = MAX_AIR_LIFT_DB * air_amount;

        for frame in 0..frames {
            // What the detectors listen to: the loudest channel, so the two
            // sides get the same gain and the image stays put.
            let mut peak = 0.0f32;
            for channel in 0..used {
                peak = peak.max(channels[channel][frame].abs());
            }

            // --- how much top end is there, and how hot is each honk band
            let mut top = 0.0f32;
            let mut band_level = [0.0f32; 3];
            for channel in 0..used.min(1) {
                let sample = channels[channel][frame];
                top = self.shelf_detect[channel].process(sample, &shelf_probe).abs();
                for band in 0..3 {
                    band_level[band] = self.honk_detect[channel][band]
                        .process(sample, &honk_probe[band])
                        .abs();
                }
            }
            let top = self.shelf_env.feed(top, slow_attack, slow_release);
            let overall = self.overall_env.feed(peak, slow_attack, slow_release);

            // --- the transient softener: fast against slow
            let fast = self.fast.feed(peak, fast_attack, fast_release);
            let slow = self.slow.feed(peak, slow_attack, slow_release);
            // How far the fast envelope is above the slow one, 0..1. An
            // attack is exactly this and nothing else.
            let edge = ((fast - slow) / fast.max(1e-6)).clamp(0.0, 1.0);
            let duck = 1.0 - edge * transient_amount * MAX_TRANSIENT_DUCK;

            // --- the dynamic shelf: deeper the more top there is
            let shelf_db = -MAX_SHELF_CUT_DB * shelf_amount * saturating(top * 4.0);
            let shelf_coeffs = SvfFilter::coeffs(
                SvfMode::HighShelf,
                SHELF_HZ,
                0.707,
                shelf_db,
                self.sample_rate,
            );

            // --- the suppressor: each band ducks by how far it stands out
            let honk_coeffs: [SvfCoeffs; 3] = std::array::from_fn(|band| {
                let level = self.honk_env[band].feed(band_level[band], slow_attack, slow_release);
                // *Relative* to the whole signal, which is what makes it
                // adaptive: a band that is merely present is not honking, and
                // one that is most of the signal is.
                let prominence = (level / overall.max(1e-6)).clamp(0.0, 1.0);
                let db = -MAX_HONK_CUT_DB * honk_amount * saturating(level * 4.0) * prominence;
                SvfFilter::coeffs(SvfMode::Bell, HONK_HZ[band], 1.4, db, self.sample_rate)
            });

            let air_coeffs =
                SvfFilter::coeffs(SvfMode::HighShelf, AIR_HZ, 0.707, air_lift, self.sample_rate);

            for channel in 0..used {
                let mut sample = channels[channel][frame];
                if shelf_amount > 0.0 {
                    sample = self.shelf[channel].process(sample, &shelf_coeffs);
                }
                if honk_amount > 0.0 {
                    for band in 0..3 {
                        sample = self.honk[channel][band].process(sample, &honk_coeffs[band]);
                    }
                }
                if air_amount > 0.0 {
                    sample = self.air[channel].process(sample, &air_coeffs);
                }
                channels[channel][frame] = sample * duck;
            }
        }
    }
}

impl Default for Soften {
    fn default() -> Self {
        Self::new()
    }
}

/// A 0..1 curve that rises quickly and never quite reaches one.
///
/// What turns "how much energy is in this band" into "how much should this
/// stage do": linear in the level would mean a stage that does nothing at all
/// until the signal is loud, and everything the moment it is.
fn saturating(level: f32) -> f32 {
    let level = level.max(0.0);
    level / (1.0 + level)
}

/// A one-pole coefficient from a time constant expressed as a corner
/// frequency, which is how every other follower in this crate is written.
fn one_pole(hz: f32, sample_rate: f32) -> f32 {
    let w = std::f32::consts::TAU * hz.max(0.01) / sample_rate.max(1.0);
    (1.0 - (-w).exp()).clamp(0.0, 1.0)
}
