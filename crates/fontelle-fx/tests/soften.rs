//! The soundfont-harshness effect (TDD §13.5).
//!
//! The one effect in this crate that exists because of what Fontelle *is*. A
//! sampled instrument played back at a pitch it was not recorded at, through a
//! filter an SF2 file specified in 1998, is harsh in four particular ways —
//! and a single low-pass fixes all four by throwing away the top of the
//! record, which is why "just turn the treble down" is not the answer. §13.5
//! names four stages, and this is a measurement of each of them:
//!
//! 1. A **dynamic high shelf** — a cut that deepens with how much high-end
//!    energy there actually is, so quiet passages are not darkened along with
//!    loud ones.
//! 2. An **adaptive resonance suppressor** over 1–6 kHz, the "honk" region a
//!    resampled sample sits in, ducking whichever band is hot rather than
//!    cutting all of them always.
//! 3. A **transient softener**, because the sharpest thing about a sampled
//!    attack is usually the sample's own edge.
//! 4. An **air restore** shelf, so the result reads as *smoothed* rather than
//!    merely darkened — which is the difference between this and the low-pass.
//!
//! Each stage is tested by the thing it is supposed to change, and — just as
//! importantly — by something it is supposed to leave alone. A stage that
//! cut everything would pass half of these.

use fontelle_fx::Soften;
use fontelle_types::{SoftenConfig, SoftenPreset};

const SR: f32 = 48_000.0;
const FRAMES: usize = 33_600;
const SETTLE: usize = 4_800;
const WINDOW: usize = 24_000;

fn sine(amplitude: f32, freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn through(config: &SoftenConfig, input: Vec<f32>) -> Vec<f32> {
    let mut soften = Soften::new();
    soften.prepare(SR);
    let mut left = input;
    let mut right = left.clone();
    soften.process(&mut [&mut left, &mut right], config);
    left
}

fn amplitude_at(samples: &[f32], freq: f32) -> f32 {
    let n = samples.len();
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, sample) in samples.iter().enumerate() {
        let phase = std::f64::consts::TAU * freq as f64 * i as f64 / SR as f64;
        re += *sample as f64 * phase.cos();
        im += *sample as f64 * phase.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / n as f64) as f32
}

/// What one stage does to a steady tone, as a gain.
fn gain_at(config: &SoftenConfig, amplitude: f32, freq: f32) -> f32 {
    let out = through(config, sine(amplitude, freq, FRAMES));
    amplitude_at(&out[SETTLE..SETTLE + WINDOW], freq) / amplitude
}

fn nothing() -> SoftenConfig {
    SoftenConfig {
        shelf_amount: 0.0,
        suppressor_amount: 0.0,
        transient_amount: 0.0,
        air_restore_amount: 0.0,
        mix: 1.0,
    }
}

#[test]
fn every_stage_at_zero_is_a_wire() {
    // The floor of all four knobs has to be *exactly* nothing, or there is no
    // setting at which the effect can be compared against itself being off.
    let input = sine(0.7, 1_000.0, FRAMES);
    let out = through(&nothing(), input.clone());
    let worst = out
        .iter()
        .zip(input.iter())
        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(worst < 1e-4, "a soften with every stage off moved {worst}");
}

// ------------------------------------------------------- the dynamic shelf

#[test]
fn the_shelf_takes_the_top_off_and_leaves_the_bottom_alone() {
    let mut config = nothing();
    config.shelf_amount = 1.0;
    let top = gain_at(&config, 0.8, 9_000.0);
    let bottom = gain_at(&config, 0.8, 200.0);
    assert!(top < 0.7, "9 kHz should come down; gain was {top}");
    assert!(
        bottom > 0.95,
        "200 Hz is not what a high shelf is for; gain was {bottom}"
    );
}

/// **Dynamic**, which is the word doing the work: the cut has to depend on how
/// much high end is there, or it is a tone control.
#[test]
fn the_shelf_cuts_a_loud_top_end_harder_than_a_quiet_one() {
    let mut config = nothing();
    config.shelf_amount = 1.0;
    let quiet = gain_at(&config, 0.02, 9_000.0);
    let loud = gain_at(&config, 0.9, 9_000.0);
    assert!(
        loud < quiet * 0.8,
        "a loud top end should be cut harder than a quiet one: {quiet} then {loud}"
    );
    assert!(
        quiet > 0.8,
        "and a quiet one should be left mostly alone; got {quiet}"
    );
}

// -------------------------------------------------- the resonance suppressor

#[test]
fn the_suppressor_ducks_the_honk_band_and_not_the_bass() {
    let mut config = nothing();
    config.suppressor_amount = 1.0;
    let honk = gain_at(&config, 0.9, 2_800.0);
    let bass = gain_at(&config, 0.9, 150.0);
    assert!(honk < 0.75, "2.8 kHz should duck; gain was {honk}");
    assert!(
        bass > 0.95,
        "the suppressor is a 1-6 kHz tool; 150 Hz gain was {bass}"
    );
}

/// **Adaptive**: it ducks the band that is hot, not every band always. A tone
/// well inside the range but *quiet* should come through nearly untouched.
#[test]
fn the_suppressor_leaves_a_band_that_is_not_honking() {
    let mut config = nothing();
    config.suppressor_amount = 1.0;
    let quiet = gain_at(&config, 0.02, 2_800.0);
    let loud = gain_at(&config, 0.9, 2_800.0);
    assert!(
        quiet > loud * 1.2,
        "a quiet band should keep more of itself: {quiet} against {loud}"
    );
}

// -------------------------------------------------- the transient softener

#[test]
fn the_transient_softener_takes_the_edge_off_an_attack() {
    // A tone that starts abruptly. The softener should pull the first few
    // milliseconds down and leave what follows alone — a stage that cut the
    // whole note would be a compressor with a bad release.
    let attack = |amount: f32| {
        let mut config = nothing();
        config.transient_amount = amount;
        let out = through(&config, sine(0.9, 500.0, FRAMES));
        let edge = out[..480].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let body = out[12_000..24_000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        (edge, body)
    };
    let (open_edge, open_body) = attack(0.0);
    let (soft_edge, soft_body) = attack(1.0);

    assert!(
        soft_edge < open_edge * 0.8,
        "the attack should be softened: {open_edge} then {soft_edge}"
    );
    assert!(
        soft_body > open_body * 0.9,
        "and the note it starts should still be there: {open_body} then {soft_body}"
    );
}

// -------------------------------------------------------- the air restore

/// The stage that makes this different from turning the treble down.
#[test]
fn air_restore_gives_the_very_top_back() {
    let mut config = nothing();
    config.air_restore_amount = 1.0;
    let air = gain_at(&config, 0.4, 14_000.0);
    assert!(air > 1.1, "the air shelf should lift 14 kHz; gain was {air}");
    let middle = gain_at(&config, 0.4, 1_000.0);
    assert!(
        (middle - 1.0).abs() < 0.1,
        "and leave the midrange alone; gain was {middle}"
    );
}

#[test]
fn the_shelf_and_the_air_restore_are_not_the_same_shelf() {
    // Together they should be *smoothed*: the harsh region down, the very top
    // back. If both stages hit the same band the effect is a tone control that
    // undoes itself.
    let mut config = nothing();
    config.shelf_amount = 1.0;
    config.air_restore_amount = 1.0;
    let harsh = gain_at(&config, 0.9, 6_000.0);
    let air = gain_at(&config, 0.9, 15_000.0);
    assert!(
        harsh < air,
        "the harsh region should end up below the air: {harsh} against {air}"
    );
}

// -------------------------------------------------------------- the presets

#[test]
fn the_presets_are_ordered_by_how_much_they_do() {
    let gentle = SoftenConfig::from_preset(SoftenPreset::Gentle);
    let standard = SoftenConfig::from_preset(SoftenPreset::Standard);
    let aggressive = SoftenConfig::from_preset(SoftenPreset::Aggressive);
    let total = |c: SoftenConfig| c.shelf_amount + c.suppressor_amount + c.transient_amount;
    assert!(total(gentle) < total(standard));
    assert!(total(standard) < total(aggressive));
}

/// The fourth preset is not a point on that line: a rompler's harshness is a
/// *resonance* problem more than a treble one, so it leans on the suppressor.
#[test]
fn the_rompler_preset_leans_on_the_suppressor() {
    let rompler = SoftenConfig::from_preset(SoftenPreset::VintageRompler);
    assert!(rompler.suppressor_amount > rompler.shelf_amount);
    assert!(rompler.air_restore_amount > 0.0, "and it gives the top back");
}

#[test]
fn every_preset_is_inside_the_range_its_knobs_have() {
    for preset in SoftenPreset::ALL {
        let config = SoftenConfig::from_preset(preset);
        for amount in [
            config.shelf_amount,
            config.suppressor_amount,
            config.transient_amount,
            config.air_restore_amount,
        ] {
            assert!((0.0..=1.0).contains(&amount), "{preset:?} is out of range");
        }
    }
}

// ------------------------------------------------------------- and it holds

#[test]
fn it_stays_finite_on_anything_it_is_given() {
    let mut config = nothing();
    config.shelf_amount = 1.0;
    config.suppressor_amount = 1.0;
    config.transient_amount = 1.0;
    config.air_restore_amount = 1.0;
    let loud: Vec<f32> = (0..FRAMES)
        .map(|i| ((i as f32 * 12.9898).sin() * 43_758.547).fract() * 6.0 - 3.0)
        .collect();
    let out = through(&config, loud);
    assert!(out.iter().all(|s| s.is_finite()));
}

#[test]
fn reset_forgets_the_filters() {
    let mut config = nothing();
    config.shelf_amount = 1.0;
    config.suppressor_amount = 1.0;
    let mut soften = Soften::new();
    soften.prepare(SR);
    let mut left = sine(1.0, 3_000.0, 4_800);
    let mut right = left.clone();
    soften.process(&mut [&mut left, &mut right], &config);

    soften.reset();
    let mut left = vec![0.0; 4_800];
    let mut right = vec![0.0; 4_800];
    soften.process(&mut [&mut left, &mut right], &config);
    assert!(
        left.iter().all(|s| s.abs() < 1e-6),
        "a reset soften still had signal in it"
    );
}
