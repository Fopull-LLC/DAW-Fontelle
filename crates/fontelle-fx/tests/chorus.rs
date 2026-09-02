//! The chorus and ensemble's DSP (`docs/effects-catalogue.md` §2.4).
//!
//! Like the delay's and the reverb's, what these measure is the **wet only**:
//! `EffectNode` owns the blend, so nothing here should contain the signal
//! that made it. That makes the tests unusually direct — a chorus's output is
//! a sum of delayed copies of the input, so "how many copies", "how far back"
//! and "moving how fast" are all things an impulse or a tone can be asked.
//!
//! The two measurements worth explaining:
//!
//! - **Peaks in an impulse response** count the voices. Each voice is one
//!   read head, so an impulse comes out once per voice, and the positions of
//!   those copies are the delays the voices are sitting at.
//! - **Zero crossings** measure the pitch bend. A read head moving away from
//!   the write head stretches what it reads, and the amount is exactly
//!   `1 − dD/dt`. That is the whole reason a chorus sounds like more than one
//!   player, so it is measured rather than assumed.

use fontelle_fx::Chorus;
use fontelle_types::{ChorusConfig, ChorusMode};

const SR: f32 = 48_000.0;

/// Two seconds, so a half-hertz sweep has a full cycle inside it.
const FRAMES: usize = 96_000;

const BPM: f32 = 120.0;

fn ms(milliseconds: f32) -> usize {
    (milliseconds * SR / 1000.0).round() as usize
}

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn impulse(frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames];
    buffer[0] = 1.0;
    buffer
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

fn through(config: &ChorusConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut chorus = Chorus::new();
    chorus.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    chorus.process(&mut channels, config, BPM);
    (left, right)
}

fn both(config: &ChorusConfig, source: &[f32]) -> Vec<f32> {
    let (out, _) = through(config, source.to_vec(), source.to_vec());
    out
}

/// Where the copies of an impulse landed, in samples, one entry per run of
/// consecutive samples above `floor`.
fn copies(signal: &[f32], floor: f32) -> Vec<usize> {
    let mut found = Vec::new();
    let mut inside = false;
    let mut best = (0usize, 0.0f32);
    for (index, sample) in signal.iter().enumerate() {
        if sample.abs() > floor {
            if !inside || sample.abs() > best.1 {
                best = (index, sample.abs());
            }
            inside = true;
        } else if inside {
            found.push(best.0);
            inside = false;
            best = (0, 0.0);
        }
    }
    if inside {
        found.push(best.0);
    }
    found
}

/// The frequency of a tone, from the time between its first upward zero
/// crossing and its last.
///
/// Between the crossings rather than across the whole window, and with each
/// crossing's position interpolated between the two samples either side of
/// it: counting whole crossings in a fixed window quantises the answer to
/// `SR / window`, which at a hundred milliseconds is ten hertz — enough on
/// its own to make a chorus that is doing nothing look like one bending the
/// pitch by a percent.
fn frequency(signal: &[f32]) -> f32 {
    let mut first = None;
    let mut last = 0.0f32;
    let mut periods = 0;
    for (index, pair) in signal.windows(2).enumerate() {
        if pair[0] <= 0.0 && pair[1] > 0.0 {
            let at = index as f32 + -pair[0] / (pair[1] - pair[0]);
            match first {
                None => first = Some(at),
                Some(_) => periods += 1,
            }
            last = at;
        }
    }
    match first {
        Some(first) if periods > 0 => periods as f32 * SR / (last - first),
        _ => 0.0,
    }
}

/// A chorus with the modulation stopped, which is a bank of static delays —
/// the setting most of the structural claims are easiest to see at.
fn still() -> ChorusConfig {
    ChorusConfig {
        depth: 0.0,
        voices: 1,
        spread: 0.0,
        ..ChorusConfig::new()
    }
}

// ------------------------------------------------------------------ the wet

#[test]
fn what_comes_out_is_the_voices_and_not_the_signal_that_made_them() {
    // The rule every effect that sits *under* the track follows: the node
    // owns the dry, so an effect that mixed its own in would be blended
    // twice. Before the first copy has come round, there is nothing.
    let out = both(&still(), &sine(440.0, FRAMES));
    let first = ms(15.0 * 0.85) - 2;
    assert!(
        peak(&out[..first]) < 1e-6,
        "the dry signal is in the wet path"
    );
}

#[test]
fn one_voice_with_the_modulation_stopped_is_the_delay_it_says() {
    let source = sine(440.0, FRAMES);
    let out = both(&still(), &source);
    let delay = ms(15.0);
    for index in delay + 8..FRAMES {
        assert!(
            (out[index] - source[index - delay]).abs() < 1e-4,
            "frame {index} is not the signal delayed by fifteen milliseconds"
        );
    }
}

#[test]
fn silence_in_is_silence_out() {
    let (out_l, out_r) = through(&ChorusConfig::new(), vec![0.0; FRAMES], vec![0.0; FRAMES]);
    assert_eq!(peak(&out_l), 0.0);
    assert_eq!(peak(&out_r), 0.0);
}

#[test]
fn an_empty_bus_is_not_a_panic() {
    let mut chorus = Chorus::new();
    chorus.prepare(SR);
    let mut channels: Vec<&mut [f32]> = Vec::new();
    chorus.process(&mut channels, &ChorusConfig::new(), BPM);
}

// ------------------------------------------------------------------- voices

#[test]
fn each_voice_is_a_copy_of_its_own() {
    // The count, measured: one impulse in, one copy out per voice, and no two
    // of them in the same place — which is the claim the per-voice centres
    // exist to make true. See the module doc on `chorus.rs`.
    for voices in 1..=4 {
        let config = ChorusConfig {
            voices,
            ..still()
        };
        let out = both(&config, &impulse(FRAMES));
        let found = copies(&out, 0.01);
        assert_eq!(
            found.len(),
            voices as usize,
            "{voices} voices produced {} copies at {found:?}",
            found.len()
        );
    }
}

#[test]
fn the_voices_sit_around_the_delay_the_knob_says() {
    // Spread across the knob rather than piled on it, and centred on it: the
    // number a person set is still the middle of what they hear.
    let config = ChorusConfig {
        voices: 4,
        delay_ms: 20.0,
        ..still()
    };
    let out = both(&config, &impulse(FRAMES));
    let found = copies(&out, 0.01);
    assert_eq!(found.len(), 4);
    let middle = (found[0] + found[3]) as f32 / 2.0;
    assert!(
        (middle - ms(20.0) as f32).abs() < ms(0.2) as f32,
        "the voices are centred on {middle} samples, not on the knob's {}",
        ms(20.0)
    );
    assert!(
        found[3] - found[0] > ms(4.0),
        "four voices are all in the same place"
    );
}

#[test]
fn more_voices_is_never_louder() {
    // Summed and divided by the count, so that turning the voices up is a
    // change of thickness rather than a change of level. It is allowed to be
    // quieter — copies at different delays cancel each other in places, and
    // that cancellation *is* the comb.
    let source = sine(440.0, FRAMES);
    let one = rms(&both(&ChorusConfig { voices: 1, ..still() }, &source)[ms(50.0)..]);
    for voices in 2..=4 {
        let many = rms(&both(&ChorusConfig { voices, ..still() }, &source)[ms(50.0)..]);
        assert!(
            many <= one * 1.01,
            "{voices} voices came out louder than one: {many} against {one}"
        );
    }
}

// -------------------------------------------------------------- the sweep

#[test]
fn depth_bends_the_pitch_of_what_comes_out() {
    // A read head moving away from the write head stretches what it reads and
    // one moving toward it compresses it, so a chorus's copies are flat while
    // the sweep is going out and sharp while it is coming back. That is the
    // sound; a "chorus" that only combed would be a phaser.
    let config = ChorusConfig {
        voices: 1,
        delay_ms: 30.0,
        depth: 1.0,
        rate_hz: 0.5,
        spread: 0.0,
        ..ChorusConfig::new()
    };
    let out = both(&config, &sine(1_000.0, FRAMES));

    // The LFO starts at zero and rising, so the first quarter of its cycle is
    // the head moving away: flat. Half a cycle later it is coming back.
    let flat = frequency(&out[ms(100.0)..ms(200.0)]);
    let sharp = frequency(&out[ms(1_100.0)..ms(1_200.0)]);
    assert!(
        flat < 970.0,
        "the outward sweep did not flatten the pitch: {flat} Hz"
    );
    assert!(
        sharp > 1_030.0,
        "the return did not sharpen it: {sharp} Hz"
    );
}

#[test]
fn no_depth_is_no_bend() {
    let out = both(&still(), &sine(1_000.0, FRAMES));
    let heard = frequency(&out[ms(100.0)..ms(200.0)]);
    assert!((heard - 1_000.0).abs() < 2.0, "a still chorus bent to {heard} Hz");
}

#[test]
fn the_rate_is_how_often_the_sweep_comes_round() {
    // One LFO cycle per `1 / rate`, measured as the output repeating itself:
    // a chorus is periodic with its own LFO.
    let config = ChorusConfig {
        voices: 1,
        rate_hz: 2.0,
        depth: 1.0,
        spread: 0.0,
        ..ChorusConfig::new()
    };
    let out = both(&config, &sine(1_000.0, FRAMES));
    let period = ms(500.0);
    let first = &out[period..period * 2];
    let second = &out[period * 2..period * 3];
    let difference: f32 = first
        .iter()
        .zip(second.iter())
        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(
        difference < 0.02,
        "one LFO period later the chorus was somewhere else: {difference}"
    );
}

#[test]
fn ensemble_voices_never_come_back_into_step() {
    // The difference between the two modes, as a measurement rather than as a
    // description: a chorus repeats every LFO cycle (the test above), and an
    // ensemble does not, because its voices are at rates that do not divide
    // each other.
    let config = ChorusConfig {
        voices: 3,
        mode: ChorusMode::Ensemble,
        rate_hz: 2.0,
        depth: 1.0,
        spread: 0.0,
        ..ChorusConfig::new()
    };
    let out = both(&config, &sine(1_000.0, FRAMES));
    let period = ms(500.0);
    let first = &out[period..period * 2];
    let second = &out[period * 2..period * 3];
    let difference: f32 = first
        .iter()
        .zip(second.iter())
        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(
        difference > 0.3,
        "the ensemble repeated after one cycle of the knob's rate: {difference}"
    );
}

#[test]
fn a_synced_chorus_takes_its_rate_from_the_tempo() {
    // Rule 5: anything measured in time follows the song. One cycle per
    // division, so a chorus synced to a whole note at 120 bpm sweeps every
    // two seconds.
    let config = ChorusConfig {
        sync: true,
        division: fontelle_types::NoteDivision::Quarter,
        ..ChorusConfig::new()
    };
    // A quarter note at 120 bpm is half a second, so the sweep is at 2 Hz.
    assert!((config.effective_rate_hz(120.0) - 2.0).abs() < 1e-4);
    assert!((config.effective_rate_hz(60.0) - 1.0).abs() < 1e-4);
    // And the unsynced rate is untouched by the tempo.
    let free = ChorusConfig { sync: false, ..config };
    assert_eq!(free.effective_rate_hz(60.0), free.rate_hz);
}

// ------------------------------------------------------------------ stereo

#[test]
fn spread_is_what_makes_the_two_sides_different() {
    let source = sine(1_000.0, FRAMES);
    let narrow = ChorusConfig {
        spread: 0.0,
        depth: 1.0,
        ..ChorusConfig::new()
    };
    let (left, right) = through(&narrow, source.clone(), source.clone());
    assert_eq!(left, right, "a mono signal came out of a mono chorus stereo");

    let wide = ChorusConfig { spread: 1.0, ..narrow };
    let (left, right) = through(&wide, source.clone(), source.clone());
    let difference = left
        .iter()
        .zip(right.iter())
        .skip(ms(100.0))
        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(
        difference > 0.2,
        "full spread left the two sides the same: {difference}"
    );
}

// ---------------------------------------------------------------- feedback

#[test]
fn feedback_sends_the_voices_round_again() {
    let config = ChorusConfig {
        voices: 1,
        feedback: 0.7,
        ..still()
    };
    let out = both(&config, &impulse(FRAMES));
    let found = copies(&out, 0.01);
    assert!(
        found.len() > 4,
        "feedback produced {} copies, which is not a loop",
        found.len()
    );
    // Each one quieter than the last, which is what makes it a loop and not
    // an oscillator.
    let first = out[found[0]].abs();
    let second = out[found[1]].abs();
    assert!(second < first, "the second pass was not quieter");
}

#[test]
fn negative_feedback_is_the_other_comb() {
    // The same loop with its sign inverted: the repeats alternate, which puts
    // the comb's teeth in the gaps of the positive one. A different sound,
    // and the reason the knob is signed rather than a percentage.
    let config = ChorusConfig {
        voices: 1,
        feedback: -0.7,
        ..still()
    };
    let out = both(&config, &impulse(FRAMES));
    let found = copies(&out, 0.01);
    assert!(found.len() > 4);
    assert!(
        out[found[0]] * out[found[1]] < 0.0,
        "the repeats did not alternate in sign"
    );
}

// -------------------------------------------------------------------- tone

#[test]
fn the_tone_control_takes_the_top_off_the_voices() {
    let low = sine(200.0, FRAMES);
    let high = sine(8_000.0, FRAMES);
    let open = ChorusConfig {
        tone_hz: 20_000.0,
        ..still()
    };
    let dark = ChorusConfig {
        tone_hz: 1_000.0,
        ..still()
    };
    let window = ms(100.0)..;
    let kept = rms(&both(&dark, &low)[window.clone()]) / rms(&both(&open, &low)[window.clone()]);
    let lost = rms(&both(&dark, &high)[window.clone()]) / rms(&both(&open, &high)[window]);
    assert!(kept > 0.9, "the tone control took the bottom too: {kept}");
    assert!(lost < 0.2, "8 kHz survived a 1 kHz tone control: {lost}");
}
