//! The plumbing insert (`docs/effects-catalogue.md` §2.5): gain, pan, width,
//! polarity, mutes, the mono-maker and the rumble filter.
//!
//! Ten controls that each do one obvious thing, which makes the interesting
//! tests the ones about **exactness** and **order** rather than about
//! character. A utility that is 0.4 dB out is worse than useless — it is the
//! effect a person reaches for when they want to know what the level is — so
//! the level tests below are held to a hundredth of a decibel rather than to
//! "about right", and the wire test is sample-for-sample.
//!
//! One ordering is a decision rather than an accident, and
//! `a_flipped_side_summed_to_mono_is_a_null` is the test that would fail if it
//! were swapped: the polarity switches run **before** the width knob, which is
//! what makes the standard null test work. The other pair a person might worry
//! about — width against the mono-maker — turns out to commute, because both
//! are operations on the one side signal and one of them is a scalar;
//! `a_widened_bass_is_still_summed` is the claim that matters there, and it
//! holds whichever way round they are written.

use fontelle_fx::Utility;
use fontelle_types::UtilityConfig;

const SR: f32 = 48_000.0;

/// A quarter of a second: long enough for a 60 Hz tone to have fifteen
/// cycles in it, which is what the mono-maker's low end needs.
const FRAMES: usize = 12_000;

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn silence(frames: usize) -> Vec<f32> {
    vec![0.0; frames]
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn mean(samples: &[f32]) -> f32 {
    samples.iter().sum::<f32>() / samples.len() as f32
}

/// The tail of a buffer, past whatever the filters needed to settle.
fn settled(samples: &[f32]) -> &[f32] {
    &samples[samples.len() / 2..]
}

fn through(config: &UtilityConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut utility = Utility::new();
    utility.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    utility.process(&mut channels, config);
    (left, right)
}

fn db(gain: f32) -> f32 {
    20.0 * gain.log10()
}

// ------------------------------------------------------------------ a wire

#[test]
fn a_fresh_utility_is_a_wire_sample_for_sample() {
    // Not "close to a wire". Somebody who has just added this insert has said
    // nothing about what they want, and the one thing a gain-staging tool
    // must never do is change the level before it is asked to.
    let left = sine(220.0, FRAMES);
    let right = sine(330.0, FRAMES);
    let (out_l, out_r) = through(&UtilityConfig::new(), left.clone(), right.clone());
    assert_eq!(out_l, left);
    assert_eq!(out_r, right);
}

// ------------------------------------------------------------------- level

#[test]
fn gain_moves_the_level_by_the_decibels_it_says() {
    for asked in [-24.0, -12.0, -6.0, 6.0, 12.0, 24.0] {
        let config = UtilityConfig {
            gain_db: asked,
            ..UtilityConfig::new()
        };
        let source = sine(1_000.0, FRAMES);
        let (out, _) = through(&config, source.clone(), source.clone());
        let moved = db(rms(&out) / rms(&source));
        assert!(
            (moved - asked).abs() < 0.01,
            "asked for {asked} dB and got {moved}"
        );
    }
}

#[test]
fn pan_is_a_balance_that_leaves_the_centre_at_unity() {
    // A constant-power law would take 3 dB off a centred signal the moment
    // the insert was added, which is exactly the thing rule 2 forbids: what
    // arrives here is already a stereo bus, not a mono source being placed.
    let source = sine(1_000.0, FRAMES);
    let centred = UtilityConfig::new();
    let (out_l, out_r) = through(&centred, source.clone(), source.clone());
    assert!((rms(&out_l) - rms(&source)).abs() < 1e-6);
    assert!((rms(&out_r) - rms(&source)).abs() < 1e-6);

    let hard_right = UtilityConfig {
        pan: 1.0,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&hard_right, source.clone(), source.clone());
    assert_eq!(peak(&out_l), 0.0, "hard right still has a left side");
    assert!(
        (rms(&out_r) - rms(&source)).abs() < 1e-6,
        "the side panned toward should not gain"
    );

    let half_left = UtilityConfig {
        pan: -0.5,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&half_left, source.clone(), source.clone());
    assert!((rms(&out_l) - rms(&source)).abs() < 1e-6, "the near side moved");
    assert!((rms(&out_r) - rms(&source) * 0.5).abs() < 1e-6, "the far side is not halved");
}

// ------------------------------------------------------------------ stereo

#[test]
fn width_at_one_hundred_is_the_wire_and_at_zero_is_mono() {
    let left = sine(220.0, FRAMES);
    let right = sine(330.0, FRAMES);

    let (out_l, out_r) = through(&UtilityConfig::new(), left.clone(), right.clone());
    assert_eq!(out_l, left, "100 % width is not the signal that arrived");
    assert_eq!(out_r, right);

    let mono = UtilityConfig {
        width: 0.0,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&mono, left.clone(), right.clone());
    for (index, (l, r)) in out_l.iter().zip(out_r.iter()).enumerate() {
        assert!((l - r).abs() < 1e-6, "frame {index} is still stereo");
        let sum = (left[index] + right[index]) * 0.5;
        assert!((l - sum).abs() < 1e-6, "mono is not the sum at frame {index}");
    }
}

#[test]
fn width_widens_the_sides_and_leaves_the_middle_where_it_was() {
    // Two signals, one purely in the middle and one purely in the sides, so
    // that what the knob does to each is a number rather than an impression.
    let tone = sine(1_000.0, FRAMES);
    let wide = UtilityConfig {
        width: 2.0,
        ..UtilityConfig::new()
    };

    let (out_l, _) = through(&wide, tone.clone(), tone.clone());
    assert!(
        (rms(&out_l) - rms(&tone)).abs() < 1e-6,
        "a centred signal moved when the width did"
    );

    let flipped: Vec<f32> = tone.iter().map(|s| -s).collect();
    let (out_l, out_r) = through(&wide, tone.clone(), flipped.clone());
    assert!(
        (rms(&out_l) - rms(&tone) * 2.0).abs() < 1e-4,
        "200 % did not double the side"
    );
    for (l, r) in out_l.iter().zip(out_r.iter()) {
        assert!((l + r).abs() < 1e-5, "a pure side signal gained a middle");
    }
}

#[test]
fn swapping_the_channels_puts_each_side_where_the_other_was() {
    let left = sine(220.0, FRAMES);
    let right = sine(330.0, FRAMES);
    let config = UtilityConfig {
        swap: true,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, left.clone(), right.clone());
    for index in 0..FRAMES {
        assert!((out_l[index] - right[index]).abs() < 1e-6, "frame {index}");
        assert!((out_r[index] - left[index]).abs() < 1e-6, "frame {index}");
    }
}

// -------------------------------------------------------------- mono-maker

#[test]
fn the_mono_maker_sums_the_bass_and_leaves_the_top_wide() {
    // A pure side signal at each end of the spectrum: the low one should very
    // nearly vanish (a side that is gone *is* mono) and the high one should
    // survive.
    let config = UtilityConfig {
        mono_below_hz: 200.0,
        ..UtilityConfig::new()
    };
    for (freq, kept) in [(60.0f32, false), (4_000.0f32, true)] {
        let tone = sine(freq, FRAMES);
        let flipped: Vec<f32> = tone.iter().map(|s| -s).collect();
        let (out_l, out_r) = through(&config, tone.clone(), flipped);
        let side: Vec<f32> = out_l
            .iter()
            .zip(out_r.iter())
            .map(|(l, r)| (l - r) * 0.5)
            .collect();
        let survived = rms(settled(&side)) / rms(&tone);
        if kept {
            assert!(
                survived > 0.98,
                "{freq} Hz is above the corner and lost {:.1} dB of its width",
                -db(survived)
            );
        } else {
            assert!(
                survived < 0.1,
                "{freq} Hz is below the corner and kept {:.0} % of its width",
                survived * 100.0
            );
        }
    }
}

#[test]
fn the_mono_maker_is_out_of_the_path_at_the_bottom_of_its_knob() {
    let tone = sine(40.0, FRAMES);
    let flipped: Vec<f32> = tone.iter().map(|s| -s).collect();
    let (out_l, out_r) = through(&UtilityConfig::new(), tone.clone(), flipped.clone());
    assert_eq!(out_l, tone);
    assert_eq!(out_r, flipped);
}

#[test]
fn a_widened_bass_is_still_summed() {
    // The two knobs a person turns together on a bus. Width does not defeat
    // the mono-maker, because both act on the same side signal and one of
    // them is a number — but a mono-maker written as a sum of the *middle*,
    // or one applied to the channels rather than the sides, would fail this.
    let tone = sine(60.0, FRAMES);
    let flipped: Vec<f32> = tone.iter().map(|s| -s).collect();
    let config = UtilityConfig {
        width: 2.0,
        mono_below_hz: 200.0,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, tone.clone(), flipped);
    let side: Vec<f32> = out_l
        .iter()
        .zip(out_r.iter())
        .map(|(l, r)| (l - r) * 0.5)
        .collect();
    assert!(
        rms(settled(&side)) / rms(&tone) < 0.2,
        "a widened bass is not being summed"
    );
}

// ---------------------------------------------------------------- channels

#[test]
fn inverting_one_side_flips_that_side_and_only_that_side() {
    let left = sine(220.0, FRAMES);
    let right = sine(330.0, FRAMES);
    let config = UtilityConfig {
        invert_left: true,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, left.clone(), right.clone());
    for index in 0..FRAMES {
        assert!((out_l[index] + left[index]).abs() < 1e-6, "frame {index}");
    }
    assert_eq!(out_r, right, "the right side moved with the left");
}

#[test]
fn a_flipped_side_summed_to_mono_is_a_null() {
    // Invert before width, which is what makes the standard null test work:
    // two copies of the same take, one flipped, summed to mono, is silence
    // if and only if they are the same take.
    let tone = sine(1_000.0, FRAMES);
    let config = UtilityConfig {
        invert_left: true,
        width: 0.0,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, tone.clone(), tone.clone());
    assert!(peak(&out_l) < 1e-6, "the null left {} behind", peak(&out_l));
    assert!(peak(&out_r) < 1e-6);
}

#[test]
fn muting_one_side_silences_it_and_leaves_the_other() {
    let left = sine(220.0, FRAMES);
    let right = sine(330.0, FRAMES);
    let config = UtilityConfig {
        mute_left: true,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, left, right.clone());
    assert_eq!(peak(&out_l), 0.0);
    assert_eq!(out_r, right);
}

#[test]
fn the_switches_name_the_channel_that_arrives_and_the_swap_happens_after() {
    // The order the module doc states, measured: mute L then swap is "drop
    // what came in on the left and move the right across", which leaves the
    // right output silent.
    let left = sine(220.0, FRAMES);
    let right = sine(330.0, FRAMES);
    let config = UtilityConfig {
        mute_left: true,
        swap: true,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, left, right.clone());
    assert_eq!(out_l, right, "the right signal did not move across");
    assert_eq!(peak(&out_r), 0.0, "the muted channel came back");
}

// ------------------------------------------------------------------ output

#[test]
fn the_dc_filter_takes_an_offset_out_and_leaves_the_music() {
    let tone = sine(1_000.0, FRAMES);
    let offset: Vec<f32> = tone.iter().map(|s| s * 0.5 + 0.4).collect();

    let (out, _) = through(&UtilityConfig::new(), offset.clone(), offset.clone());
    assert!(
        (mean(&out) - 0.4).abs() < 0.01,
        "the filter is in the path at the bottom of its knob"
    );

    let config = UtilityConfig {
        dc_hz: 30.0,
        ..UtilityConfig::new()
    };
    let (out, _) = through(&config, offset.clone(), offset.clone());
    let tail = settled(&out);
    assert!(mean(tail).abs() < 0.005, "the offset survived: {}", mean(tail));
    assert!(
        (rms(tail) - rms(settled(&tone)) * 0.5).abs() < 0.01,
        "the tone did not survive"
    );
}

#[test]
fn the_rumble_filter_is_a_filter_at_the_top_of_its_knob() {
    // The same control, further up, is what takes footsteps off an acoustic
    // guitar: a real slope, not a DC blocker with a different label.
    let config = UtilityConfig {
        dc_hz: 200.0,
        ..UtilityConfig::new()
    };
    let low = sine(50.0, FRAMES);
    let high = sine(2_000.0, FRAMES);
    let (out_low, _) = through(&config, low.clone(), low.clone());
    let (out_high, _) = through(&config, high.clone(), high.clone());
    assert!(
        db(rms(settled(&out_low)) / rms(&low)) < -20.0,
        "50 Hz is two octaves under a 200 Hz corner and should be well down"
    );
    assert!(
        db(rms(settled(&out_high)) / rms(&high)).abs() < 0.5,
        "2 kHz is a decade above the corner and should be untouched"
    );
}

// ------------------------------------------------------------------- edges

#[test]
fn a_mono_bus_keeps_its_level_and_its_switches() {
    // Nothing in the graph hands an insert one channel today, but an effect
    // that indexed `channels[1]` unconditionally would be a panic waiting for
    // the day something does.
    let mut utility = Utility::new();
    utility.prepare(SR);
    let mut buffer = sine(1_000.0, FRAMES);
    let source = buffer.clone();
    let config = UtilityConfig {
        gain_db: 6.0,
        width: 0.0,
        invert_left: true,
        ..UtilityConfig::new()
    };
    {
        let mut channels: Vec<&mut [f32]> = vec![&mut buffer];
        utility.process(&mut channels, &config);
    }
    let moved = db(rms(&buffer) / rms(&source));
    assert!((moved - 6.0).abs() < 0.01, "the gain moved by {moved} dB");
    let gain = 10f32.powf(6.0 / 20.0);
    for index in 0..FRAMES {
        assert!(
            (buffer[index] + source[index] * gain).abs() < 1e-5,
            "frame {index} was not flipped"
        );
    }
}

#[test]
fn an_empty_bus_is_not_a_panic() {
    let mut utility = Utility::new();
    utility.prepare(SR);
    let mut channels: Vec<&mut [f32]> = Vec::new();
    utility.process(&mut channels, &UtilityConfig::new());
}

#[test]
fn silence_in_is_silence_out_whatever_the_switches_say() {
    let config = UtilityConfig {
        gain_db: 24.0,
        width: 2.0,
        mono_below_hz: 300.0,
        dc_hz: 100.0,
        invert_right: true,
        swap: true,
        ..UtilityConfig::new()
    };
    let (out_l, out_r) = through(&config, silence(FRAMES), silence(FRAMES));
    assert_eq!(peak(&out_l), 0.0);
    assert_eq!(peak(&out_r), 0.0);
}
