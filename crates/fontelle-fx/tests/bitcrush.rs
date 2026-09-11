//! The bitcrusher's DSP: quantisation and decimation (TDD §13.4,
//! `docs/effects-catalogue.md` §3.2).
//!
//! Two destructions of the signal that are usually confused and are not the
//! same thing:
//!
//! - **Bit depth** quantises the *amplitude*. Every sample is rounded to one of
//!   `2^bits` levels, which adds an error signal correlated with the material —
//!   that correlation is what makes low-bit audio sound gritty rather than
//!   merely noisy, and it is what dither trades away for hiss.
//! - **Rate** quantises *time*. The signal is held for a whole number of input
//!   samples, which is a sample-and-hold, and its images fold back down the
//!   spectrum as aliases. **That aliasing is the point** — a bitcrusher whose
//!   decimation is band-limited is a low-pass filter — so the anti-alias
//!   option is off by default and exists for the times you want the grit
//!   without the ring modulation.
//!
//! *"There are lots of types of bitcrush."* What makes the types is **how**
//! the amplitude is rounded (to nearest, toward zero, on a logarithmic grid),
//! **how** the time is held (held flat, ramped, or dropped), and what the
//! level is when it happens. Each of those is measured here by the thing
//! that distinguishes it from the others, because a quantiser wired to the
//! wrong power of two still produces plausible-sounding rubbish.

use fontelle_fx::Bitcrush;
use fontelle_types::{BitcrushConfig, BitcrushPreset, Decimation, Dither, Quantiser};

const SR: f32 = 48_000.0;
/// Long enough that the analysed window below is a whole number of cycles of
/// every frequency used here, so a rectangular-windowed bin does not leak.
const FRAMES: usize = 33_600;
const SETTLE: usize = 4_800;
const WINDOW: usize = 24_000;

fn sine(amplitude: f32, freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

/// A slow rise from 0 to 1 over `frames`: every sample distinct, so a held
/// run is exactly as long as the hold and nothing else.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames).map(|i| i as f32 / frames as f32).collect()
}

fn through(config: &BitcrushConfig, input: Vec<f32>) -> Vec<f32> {
    let mut crush = Bitcrush::new();
    crush.prepare(SR);
    let mut left = input;
    let mut right = left.clone();
    crush.process(&mut [&mut left, &mut right], config);
    left
}

/// Transparent: full depth, full rate, no dither, every new stage at rest.
fn clean() -> BitcrushConfig {
    BitcrushConfig::new()
}

fn worst_difference(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

/// The lengths of every run of identical samples in `samples`.
fn run_lengths(samples: &[f32]) -> Vec<usize> {
    let mut runs = Vec::new();
    let mut run = 1;
    for pair in samples.windows(2) {
        if pair[0] == pair[1] {
            run += 1;
        } else {
            runs.push(run);
            run = 1;
        }
    }
    runs.push(run);
    runs
}

// --------------------------------------------------------- the old claims

#[test]
fn at_its_defaults_it_is_very_nearly_a_wire() {
    // An effect somebody has just added must not commit them to a sound. At
    // sixteen bits the quantisation step is 1/32768, which is below anything
    // audible and below what this asserts.
    let input = sine(0.8, 220.0, FRAMES);
    let out = through(&clean(), input.clone());
    let worst = worst_difference(&out, &input);
    assert!(
        worst < 1e-3,
        "a fresh bitcrusher changed the signal by {worst}"
    );
}

#[test]
fn lowering_the_bit_depth_puts_the_signal_on_steps() {
    // The claim, stated as the thing that is actually true of the output:
    // three bits is eight levels, so the whole waveform is drawn from a very
    // small set of distinct values.
    let mut config = clean();
    config.bits = 3.0;
    let out = through(&config, sine(1.0, 220.0, FRAMES));

    let mut levels: Vec<i64> = out.iter().map(|s| (s * 1e6) as i64).collect();
    levels.sort_unstable();
    levels.dedup();
    assert!(
        levels.len() <= 16,
        "three bits should leave at most sixteen distinct values, left {}",
        levels.len()
    );
    assert!(
        levels.len() > 2,
        "and more than two, or it is not a quantiser, it is a comparator"
    );
}

#[test]
fn more_bits_is_a_smaller_step() {
    // Monotonic in the knob, which is what makes it a knob: the error a
    // quantiser adds has to fall as the depth rises.
    let error_at = |bits: f32| {
        let mut config = clean();
        config.bits = bits;
        let input = sine(0.9, 220.0, FRAMES);
        let out = through(&config, input.clone());
        worst_difference(&out, &input)
    };
    let coarse = error_at(3.0);
    let finer = error_at(6.0);
    assert!(
        finer < coarse * 0.5,
        "doubling the bits should shrink the step: {coarse} then {finer}"
    );
}

#[test]
fn lowering_the_rate_holds_each_sample() {
    // A sample-and-hold at a quarter of the rate holds every value for four
    // input samples. Counted rather than described: a decimator off by one is
    // still a decimator and still sounds like one.
    let mut config = clean();
    config.rate_hz = SR / 4.0;
    let out = through(&config, sine(1.0, 220.0, FRAMES));

    // The longest run of identical samples, away from the turning points where
    // a smooth signal repeats a value on its own.
    let longest = run_lengths(&out[100..2_000]).into_iter().max().unwrap();
    assert_eq!(
        longest, 4,
        "a quarter-rate hold should repeat each value four times, held {longest}"
    );
}

#[test]
fn the_full_rate_holds_nothing() {
    let out = through(&clean(), sine(1.0, 220.0, FRAMES));
    let repeats = out[100..2_000].windows(2).filter(|p| p[0] == p[1]).count();
    assert!(
        repeats < 20,
        "at full rate nothing should be held; {repeats} samples repeated"
    );
}

#[test]
fn decimation_aliases_and_that_is_the_point() {
    // A 6 kHz tone held at 8 kHz folds back to 2 kHz. **This is the feature**,
    // not a defect: a bitcrusher whose decimation is band-limited is a
    // low-pass filter with extra steps.
    let mut config = clean();
    config.rate_hz = 8_000.0;
    let out = through(&config, sine(0.8, 6_000.0, FRAMES));
    assert!(
        amplitude_at(&out[SETTLE..SETTLE + WINDOW], 2_000.0) > 0.05,
        "the fold-down should be plainly there"
    );
}

#[test]
fn anti_alias_takes_the_fold_down_away() {
    // The other half, for when the grit is wanted and the ring modulation is
    // not. Off by default — see the module comment.
    let mut config = clean();
    config.rate_hz = 8_000.0;
    let raw = amplitude_at(
        &through(&config, sine(0.8, 6_000.0, FRAMES))[SETTLE..SETTLE + WINDOW],
        2_000.0,
    );
    config.anti_alias = true;
    let filtered = amplitude_at(
        &through(&config, sine(0.8, 6_000.0, FRAMES))[SETTLE..SETTLE + WINDOW],
        2_000.0,
    );
    assert!(
        filtered < raw * 0.5,
        "band-limiting should cut the alias: {raw} then {filtered}"
    );
}

/// **Dither carries a signal the quantiser cannot see.**
///
/// The obvious test — "the output lands on more distinct values" — is wrong,
/// and writing it first is how that got noticed: dither does *not* add levels.
/// The output still lands on the quantiser's grid. What it changes is *which*
/// level gets chosen, by making the choice depend on a random offset rather
/// than only on the signal, so the error stops being correlated with the
/// material.
///
/// The consequence you can actually measure is this one: a tone quieter than
/// half a quantisation step rounds to the same level every sample and
/// disappears completely. With dither it survives — not sample by sample, but
/// in the average, which is what a listener hears. It is the reason dither
/// exists, and every kind of dither in the chooser has to do it.
#[test]
fn every_dither_carries_a_signal_too_quiet_for_the_quantiser_to_see() {
    let mut config = clean();
    // Four bits is a step of 2/15; a tenth of that is far below the rounding
    // threshold.
    config.bits = 4.0;
    let quiet = sine(0.013, 100.0, FRAMES);

    let plain = through(&config, quiet.clone());
    assert!(
        plain.iter().all(|s| s.abs() < 1e-9),
        "without dither a tone this quiet rounds away to nothing, as it should"
    );

    for dither in [Dither::Rectangular, Dither::Triangular, Dither::Shaped] {
        config.dither = dither;
        let dithered = through(&config, quiet.clone());
        assert!(
            dithered.iter().any(|s| s.abs() > 1e-9),
            "{dither:?}: with dither something comes out"
        );

        // And what comes out is *the tone*, not just noise: averaged over each
        // half of a cycle, the dithered output leans the way the input does.
        let period = (SR / 100.0) as usize;
        let mean =
            |from: usize, len: usize| dithered[from..from + len].iter().sum::<f32>() / len as f32;
        let (up, down) = (
            mean(SETTLE, period / 2),
            mean(SETTLE + period / 2, period / 2),
        );
        assert!(
            up > down,
            "{dither:?}: the dithered output should still follow the tone: {up} against {down}"
        );
    }
}

#[test]
fn it_never_leaves_the_rails() {
    // Quantisation rounds, and rounding at full scale can round *up*. A
    // bitcrusher that returned 1.03 would clip whatever came after it. Every
    // quantiser, every dither, every decimator, at two bits, with the input
    // gain all the way up.
    for quantiser in Quantiser::ALL {
        for dither in Dither::ALL {
            for decimation in Decimation::ALL {
                let mut config = clean();
                config.bits = 2.0;
                config.input_db = 24.0;
                config.quantiser = quantiser;
                config.dither = dither;
                config.decimation = decimation;
                config.rate_hz = 6_000.0;
                config.jitter = 0.5;
                let out = through(&config, sine(1.0, 220.0, FRAMES));
                assert!(
                    out.iter().all(|s| s.is_finite() && s.abs() <= 1.001),
                    "{quantiser:?}/{dither:?}/{decimation:?} left the rails"
                );
            }
        }
    }
}

#[test]
fn reset_forgets_what_it_was_holding() {
    let mut config = clean();
    config.rate_hz = 1_000.0;
    let mut crush = Bitcrush::new();
    crush.prepare(SR);
    let mut left = sine(1.0, 220.0, 4_800);
    let mut right = left.clone();
    crush.process(&mut [&mut left, &mut right], &config);

    crush.reset();
    let mut left = vec![0.0; 4_800];
    let mut right = vec![0.0; 4_800];
    crush.process(&mut [&mut left, &mut right], &config);
    assert!(
        left.iter().all(|s| s.abs() < 1e-6),
        "a reset crusher was still holding the last note's sample"
    );
}

// ------------------------------------------------------------ the depth

#[test]
fn the_input_gain_reaches_the_quantiser() {
    // A quantiser is a level-dependent effect: what four bits does to a
    // signal depends entirely on how loud the signal is when it gets there.
    // A tone too quiet to register at 0 dB registers at +24.
    let mut config = clean();
    config.bits = 4.0;
    let quiet = sine(0.013, 100.0, FRAMES);
    let silent = through(&config, quiet.clone());
    assert!(
        silent.iter().all(|s| s.abs() < 1e-9),
        "too quiet to see, at 0 dB"
    );
    config.input_db = 24.0;
    let heard = through(&config, quiet.clone());
    assert!(
        heard[SETTLE..].iter().any(|s| s.abs() > 0.05),
        "at +24 dB the same tone reaches the grid"
    );
    // And a loud one driven past the rails clips rather than overflowing:
    // most of it sits on the grid's top level. Which is **not** 1.0 — a
    // four-bit grid has fifteen steps across -1..1 with one at zero, so its
    // top level is 14/15 — hence the top is measured, not assumed.
    let hot = through(&config, sine(0.5, 220.0, FRAMES));
    assert!(hot.iter().all(|s| s.abs() <= 1.001));
    let top = hot[SETTLE..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let flat = hot[SETTLE..]
        .iter()
        .filter(|s| s.abs() >= top - 1e-6)
        .count();
    assert!(
        flat > (FRAMES - SETTLE) / 3,
        "a signal driven into the quantiser sits on its top level ({top}); {flat} samples did"
    );
}

#[test]
fn truncation_drops_what_rounding_would_keep() {
    // Toward zero rather than to nearest: anything smaller than one whole
    // step falls to silence, which is the gating an 8-bit sample player does
    // to the tail of every note.
    let mut config = clean();
    config.bits = 3.0;
    // Eight levels over -1..1 is a step of 2/7. A tone at 0.8 of a step is
    // over the rounding threshold and under the truncation one.
    let step = 2.0 / 7.0;
    let tone = sine(0.8 * step, 220.0, FRAMES);
    config.quantiser = Quantiser::Round;
    let rounded = through(&config, tone.clone());
    assert!(
        rounded.iter().any(|s| s.abs() > 1e-6),
        "rounding should keep a tone that crosses half a step"
    );
    config.quantiser = Quantiser::Truncate;
    let truncated = through(&config, tone.clone());
    assert!(
        truncated.iter().all(|s| s.abs() < 1e-6),
        "truncation should drop a tone that never reaches a whole step"
    );
}

#[test]
fn mu_law_keeps_quiet_detail_and_still_lands_on_few_levels() {
    // A logarithmic grid: fine near zero and coarse near full scale, which is
    // how telephony got speech through eight bits. A tone that rounds away on
    // the linear grid survives on this one — and a loud one is still drawn
    // from `2^bits` values, because it is still a quantiser.
    let mut config = clean();
    config.bits = 4.0;
    let quiet = sine(0.013, 100.0, FRAMES);
    config.quantiser = Quantiser::Round;
    assert!(
        through(&config, quiet.clone())
            .iter()
            .all(|s| s.abs() < 1e-9)
    );
    config.quantiser = Quantiser::MuLaw;
    let out = through(&config, quiet.clone());
    assert!(
        amplitude_at(&out[SETTLE..SETTLE + WINDOW], 100.0) > 0.005,
        "mu-law should keep the quiet tone; got {}",
        amplitude_at(&out[SETTLE..SETTLE + WINDOW], 100.0)
    );

    let loud = through(&config, sine(1.0, 220.0, FRAMES));
    let mut levels: Vec<i64> = loud.iter().map(|s| (s * 1e5) as i64).collect();
    levels.sort_unstable();
    levels.dedup();
    assert!(
        levels.len() <= 16,
        "four bits of mu-law is still sixteen values; got {}",
        levels.len()
    );
}

#[test]
fn every_quantiser_at_sixteen_bits_is_very_nearly_a_wire() {
    // The chooser must not be a tone control at full depth: whichever grid
    // is chosen, sixteen bits of it is below hearing.
    let input = sine(0.8, 220.0, FRAMES);
    for quantiser in Quantiser::ALL {
        let mut config = clean();
        config.quantiser = quantiser;
        let worst = worst_difference(&through(&config, input.clone()), &input);
        assert!(
            worst < 2e-3,
            "{quantiser:?} at sixteen bits changed the signal by {worst}"
        );
    }
}

#[test]
fn shaped_dither_moves_the_noise_up_out_of_the_way() {
    // Error feedback pushes the quantisation noise toward the top of the
    // band, where it is least audible. Measured where it should have gone
    // *from*: the low end of the error spectrum is quieter than plain
    // triangular dither leaves it.
    let input = sine(0.5, 100.0, FRAMES);
    let low_noise = |dither: Dither| {
        let mut config = clean();
        config.bits = 4.0;
        config.dither = dither;
        let out = through(&config, input.clone());
        let error: Vec<f32> = out[SETTLE..SETTLE + WINDOW]
            .iter()
            .zip(input[SETTLE..SETTLE + WINDOW].iter())
            .map(|(a, b)| a - b)
            .collect();
        (1..=10)
            .map(|k| amplitude_at(&error, 200.0 * k as f32))
            .sum::<f32>()
    };
    let plain = low_noise(Dither::Triangular);
    let shaped = low_noise(Dither::Shaped);
    assert!(
        shaped < plain * 0.5,
        "shaped dither should leave less noise at the bottom: {plain} then {shaped}"
    );
}

// ------------------------------------------------------------- the rate

#[test]
fn linear_decimation_ramps_between_held_values() {
    // A sampler with interpolation: the same values at the same rate, joined
    // by lines instead of steps. Smoother, darker, and no stair to be found.
    let mut hold = clean();
    hold.rate_hz = SR / 4.0;
    let mut linear = hold;
    linear.decimation = Decimation::Linear;
    let input = sine(1.0, 220.0, FRAMES);
    let stepped = through(&hold, input.clone());
    let ramped = through(&linear, input.clone());
    let biggest_step = |samples: &[f32]| {
        samples[100..2_000]
            .windows(2)
            .fold(0.0f32, |m, pair| m.max((pair[1] - pair[0]).abs()))
    };
    assert!(
        biggest_step(&ramped) < biggest_step(&stepped) * 0.5,
        "a ramp should have no big steps: {} against the hold's {}",
        biggest_step(&ramped),
        biggest_step(&stepped)
    );
    assert!(
        worst_difference(&ramped, &input) > 0.01,
        "and it is still a decimator, not a wire"
    );
}

#[test]
fn drop_decimation_leaves_silence_between_samples() {
    // The held value plays once and the rest of the period is nothing:
    // sparse, comb-like, and unlike either of the other two.
    let mut config = clean();
    config.rate_hz = SR / 4.0;
    config.decimation = Decimation::Drop;
    let out = through(&config, sine(1.0, 220.0, FRAMES));
    let zeros = out[100..2_000].iter().filter(|s| **s == 0.0).count();
    assert!(
        (1_300..=1_450).contains(&zeros),
        "three of every four samples should be silence; {zeros} of 1900 were"
    );
    assert!(
        out[100..2_000].iter().any(|s| s.abs() > 0.5),
        "and the fourth is the signal"
    );
}

#[test]
fn jitter_makes_the_hold_uneven() {
    // An unstable clock. Without it every hold is the same length; with it
    // they vary, and the sound stops being a pitched buzz at the rate.
    let mut steady = clean();
    steady.rate_hz = SR / 8.0;
    let mut shaky = steady;
    shaky.jitter = 1.0;
    // The slice can start and end part way through a hold, so the first
    // and last runs are not the clock's and are left out.
    let inner = |samples: &[f32]| {
        let runs = run_lengths(samples);
        let mut inner = runs[1..runs.len() - 1].to_vec();
        inner.sort_unstable();
        inner.dedup();
        inner
    };
    let lengths = inner(&through(&steady, ramp(FRAMES))[100..4_100]);
    assert_eq!(
        lengths,
        [8],
        "a steady clock holds for exactly eight every time"
    );

    let lengths = inner(&through(&shaky, ramp(FRAMES))[100..4_100]);
    assert!(
        lengths.len() >= 3,
        "a jittered clock should hold for different lengths; saw {lengths:?}"
    );
}

// ----------------------------------------------------------- the output

#[test]
fn the_post_filter_takes_the_top_off_after_everything() {
    let mut config = clean();
    config.post_lp_hz = 1_000.0;
    let out = through(&config, sine(0.8, 8_000.0, FRAMES));
    let level = amplitude_at(&out[SETTLE..SETTLE + WINDOW], 8_000.0);
    assert!(
        level < 0.8 * 0.2,
        "an 8 kHz tone should not survive a 1 kHz post filter; got {level}"
    );
}

#[test]
fn the_output_gain_moves_the_level() {
    let mut config = clean();
    config.output_db = -12.0;
    let out = through(&config, sine(0.8, 1_000.0, FRAMES));
    let level = amplitude_at(&out[SETTLE..SETTLE + WINDOW], 1_000.0);
    assert!(
        (level / 0.8 - 0.251).abs() < 0.01,
        "-12 dB should be a quarter of the amplitude; got {}",
        level / 0.8
    );
}

#[test]
fn every_preset_is_audibly_a_bitcrush() {
    let input = sine(0.5, 1_000.0, FRAMES);
    for preset in BitcrushPreset::ALL {
        let config = BitcrushConfig::from_preset(preset);
        let out = through(&config, input.clone());
        assert!(
            worst_difference(&out[SETTLE..], &input[SETTLE..]) > 0.02,
            "{preset:?} left a 1 kHz tone as it was"
        );
        assert!(
            out.iter().all(|s| s.is_finite()),
            "{preset:?} is not finite"
        );
    }
}

/// One DFT bin, as `distortion.rs` does and for the same reason.
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
