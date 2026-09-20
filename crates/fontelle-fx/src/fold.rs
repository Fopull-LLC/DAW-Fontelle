//! The wavefolder (`docs/flopsynth-next.md` §4.5): a signal driven past
//! full scale is folded back on itself rather than clipped, so the
//! harmonics keep coming as the drive goes up instead of flattening into a
//! square. The West Coast sound; the distortion's `Fold` curve is one
//! fold, this one keeps going.
//!
//! # The fold
//!
//! Two folds, blended by `smooth`. The **triangle** fold reflects the
//! signal back into ±1 every time it crosses the edge — a crease, whose
//! harmonics go all the way up. The **sine** fold is `sin(v·π/2)`, which
//! folds the same way (a ramp through it comes back down) with a round
//! corner and far fewer partials. `symmetry` is a bias added before the
//! fold, which is where the even harmonics come from: a symmetric fold of
//! a symmetric wave has none.
//!
//! No oversampling here: the effect is zero-latency by §4.5's rule, and a
//! crease at full drive on a bus aliases the way every folder on a bus
//! does. The synth's own oscillators fold with oversampling
//! (`docs/flopsynth-next.md` §4.1) where a voice can afford it.

use fontelle_types::FoldConfig;

pub struct Fold {
    sample_rate: f32,
}

impl Fold {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub fn reset(&mut self) {}

    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &FoldConfig) {
        let drive = config.drive_db.clamp(0.0, 40.0);
        let output = 10f32.powf(config.output_db.clamp(-24.0, 24.0) / 20.0);
        if drive <= 0.0 && config.symmetry == 0.0 {
            // **Exactly** the input, as a drive knob at the bottom of its
            // travel has to be.
            if output != 1.0 {
                for channel in channels.iter_mut() {
                    for sample in channel.iter_mut() {
                        *sample *= output;
                    }
                }
            }
            return;
        }
        for channel in channels.iter_mut() {
            for sample in channel.iter_mut() {
                *sample = fold_curve_at(*sample, config) * output;
            }
        }
    }
}

/// The folder's transfer at `x`, for whatever wants to draw it rather than
/// run it — the same arithmetic the block loop does.
pub fn fold_curve_at(x: f32, config: &FoldConfig) -> f32 {
    let drive = config.drive_db.clamp(0.0, 40.0);
    if drive <= 0.0 && config.symmetry == 0.0 {
        return x;
    }
    let v = x * 10f32.powf(drive / 20.0) + config.symmetry.clamp(-1.0, 1.0);
    let creased = triangle_fold(v);
    let round = (v * std::f32::consts::FRAC_PI_2).sin();
    creased + (round - creased) * config.smooth.clamp(0.0, 1.0)
}

/// `v` reflected back into ±1 as many times as it takes: the triangle wave
/// of period 4 that is `v` on −1..=1.
fn triangle_fold(v: f32) -> f32 {
    let t = (v + 1.0).rem_euclid(4.0);
    if t < 2.0 { t - 1.0 } else { 3.0 - t }
}

impl Default for Fold {
    fn default() -> Self {
        Self::new()
    }
}
