//! The gate and expander's DSP (`docs/effects-catalogue.md` §2.1).
//!
//! Every test here measures a **gain over time** rather than a spectrum,
//! because that is what this effect is: a decision about whether the signal
//! is wanted, and a ramp on and off it. The shape of that ramp is the whole
//! product, so the tests are written as "what was the level of this tone
//! during this window", with the windows chosen so a failure says which stage
//! is wrong — the threshold, the hysteresis, the hold, or the smoothing.
//!
//! Three of them exist because their absence is a specific, nameable defect
//! rather than a shortfall: `hysteresis_stops_a_signal_at_the_threshold_from_
//! chattering` (the stutter), `hold_keeps_the_gate_open_through_a_dip` (the
//! halved decay), and `the_key_filter_deafens_the_detector_to_the_bass_without_
//! filtering_the_audio` (the kick that opens the hi-hat's gate).

use fontelle_fx::Gate;
use fontelle_types::{GATE_FLOOR_DB, GateConfig, MAX_GATE_RATIO};

const SR: f32 = 48_000.0;

/// Half a second. Long enough for a 100 ms release to finish inside it.
const FRAMES: usize = 24_000;

fn ms(milliseconds: f32) -> usize {
    (milliseconds * SR / 1000.0) as usize
}

/// A tone that starts at full amplitude rather than at zero — for the tests
/// that want the gate open from the very first sample, since a detector
/// starting from silence closes it for the fraction of a cycle a sine spends
/// climbing off zero.
fn cosine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).cos())
        .collect()
}

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
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

fn db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

fn from_db(decibels: f32) -> f32 {
    10f32.powf(decibels / 20.0)
}

fn through(config: &GateConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut gate = Gate::new();
    gate.prepare(SR);
    let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
    gate.process(&mut channels, None, config);
    (left, right)
}

/// The same signal on both sides, which is what most of these want.
fn both(config: &GateConfig, source: &[f32]) -> Vec<f32> {
    let (out, _) = through(config, source.to_vec(), source.to_vec());
    out
}

/// A gate set to gate: threshold where the caller says, everything else fast
/// enough that a test does not spend its life waiting.
fn gating(threshold_db: f32) -> GateConfig {
    GateConfig {
        threshold_db,
        attack_ms: 0.5,
        hold_ms: 0.0,
        release_ms: 10.0,
        hysteresis_db: 0.0,
        ..GateConfig::new()
    }
}

// ------------------------------------------------------------------ at rest

#[test]
fn a_fresh_gate_passes_everything_a_person_could_hear() {
    // The threshold opens at the bottom of its range, which is this effect's
    // "off": there is no material under −80 dBFS. See `GateConfig::new`.
    for level_db in [0.0, -20.0, -40.0, -60.0] {
        let source: Vec<f32> = sine(440.0, FRAMES)
            .iter()
            .map(|s| s * from_db(level_db))
            .collect();
        let out = both(&GateConfig::new(), &source);
        let moved = db(rms(&out) / rms(&source));
        assert!(
            moved.abs() < 0.01,
            "a fresh gate moved a {level_db} dB signal by {moved} dB"
        );
    }
}

#[test]
fn a_ratio_of_one_is_a_wire_at_any_threshold() {
    // The same rule the compressor's 1:1 follows: the ratio is what turns a
    // distance below the threshold into a gain, and one of it is no gain.
    let source: Vec<f32> = sine(440.0, FRAMES).iter().map(|s| s * 0.01).collect();
    let config = GateConfig {
        threshold_db: -6.0,
        ratio: 1.0,
        ..GateConfig::new()
    };
    let out = both(&config, &source);
    let moved = db(rms(&out) / rms(&source));
    assert!(moved.abs() < 0.01, "1:1 moved the signal by {moved} dB");
}

#[test]
fn silence_in_is_silence_out() {
    let (out_l, out_r) = through(&gating(-40.0), vec![0.0; FRAMES], vec![0.0; FRAMES]);
    assert_eq!(peak(&out_l), 0.0);
    assert_eq!(peak(&out_r), 0.0);
}

#[test]
fn an_empty_bus_is_not_a_panic() {
    let mut gate = Gate::new();
    gate.prepare(SR);
    let mut channels: Vec<&mut [f32]> = Vec::new();
    gate.process(&mut channels, None, &GateConfig::new());
}

// --------------------------------------------------------------- the decision

#[test]
fn a_signal_under_the_threshold_is_taken_down_to_the_range() {
    let quiet: Vec<f32> = sine(440.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-30.0))
        .collect();
    let config = GateConfig {
        range_db: -24.0,
        ..gating(-12.0)
    };
    let out = both(&config, &quiet);
    // Past the release, which is where the gain has arrived.
    let settled = &out[ms(100.0)..];
    let moved = db(rms(settled) / rms(&quiet[ms(100.0)..]));
    assert!(
        (moved + 24.0).abs() < 0.5,
        "asked for a −24 dB range and got {moved} dB"
    );
}

#[test]
fn a_signal_over_the_threshold_is_left_alone() {
    let loud: Vec<f32> = sine(440.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-6.0))
        .collect();
    let out = both(&gating(-12.0), &loud);
    let settled = &out[ms(50.0)..];
    let moved = db(rms(settled) / rms(&loud[ms(50.0)..]));
    assert!(
        moved.abs() < 0.01,
        "an open gate moved the signal by {moved} dB"
    );
}

#[test]
fn the_ratio_is_how_steeply_the_gain_falls_away_below_the_threshold() {
    // An expander rather than a gate: at 2:1, six decibels under the
    // threshold is six more decibels down, and at 4:1 it is eighteen.
    let source: Vec<f32> = sine(440.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-26.0))
        .collect();
    for (ratio, expected) in [(2.0f32, -6.0f32), (4.0, -18.0)] {
        let config = GateConfig {
            ratio,
            range_db: GATE_FLOOR_DB,
            ..gating(-20.0)
        };
        let out = both(&config, &source);
        let settled = &out[ms(100.0)..];
        let moved = db(rms(settled) / rms(&source[ms(100.0)..]));
        assert!(
            (moved - expected).abs() < 0.5,
            "{ratio}:1 six dB under the threshold gave {moved} dB, not {expected}"
        );
    }
}

#[test]
fn the_range_is_a_floor_the_expander_cannot_go_below() {
    // What makes the same effect an expander that only ducks: however steep
    // the ratio, the gate never takes more off than the range says.
    let source: Vec<f32> = sine(440.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-60.0))
        .collect();
    let config = GateConfig {
        range_db: -6.0,
        ratio: MAX_GATE_RATIO,
        ..gating(-12.0)
    };
    let out = both(&config, &source);
    let settled = &out[ms(100.0)..];
    let moved = db(rms(settled) / rms(&source[ms(100.0)..]));
    assert!(
        (moved + 6.0).abs() < 0.2,
        "a −6 dB range let the gate reach {moved} dB"
    );
}

// ------------------------------------------------------------------ chatter

#[test]
fn hysteresis_stops_a_signal_at_the_threshold_from_chattering() {
    // A tone wobbling a decibel either side of the threshold. Without
    // hysteresis the gate opens and closes on every crossing, and the output
    // is amplitude-modulated at the wobble's rate; with it, the gate opens
    // once and stays open.
    let wobble_hz = 8.0;
    let source: Vec<f32> = (0..FRAMES)
        .map(|i| {
            let t = i as f32 / SR;
            let level_db = -20.0 + (std::f32::consts::TAU * wobble_hz * t).sin();
            (std::f32::consts::TAU * 440.0 * t).sin() * from_db(level_db)
        })
        .collect();

    let chattery = both(&gating(-20.0), &source);
    let steady = both(
        &GateConfig {
            hysteresis_db: 6.0,
            ..gating(-20.0)
        },
        &source,
    );

    // How much the level moves from one eighth of a wobble to the next: a
    // gate that is switching adds swings the source does not have.
    let swing = |signal: &[f32]| {
        let window = ms(1000.0 / wobble_hz / 8.0);
        let levels: Vec<f32> = signal[ms(100.0)..]
            .chunks(window)
            .map(|chunk| db(rms(chunk)))
            .collect();
        levels
            .windows(2)
            .fold(0.0f32, |m, pair| m.max((pair[1] - pair[0]).abs()))
    };

    let source_swing = swing(&source);
    assert!(
        swing(&chattery) > source_swing + 6.0,
        "without hysteresis the gate did not chatter, so this test proves nothing"
    );
    assert!(
        swing(&steady) < source_swing + 1.0,
        "hysteresis did not stop the chatter: {} dB of swing against the source's {source_swing}",
        swing(&steady)
    );
}

// --------------------------------------------------------------------- hold

#[test]
fn hold_keeps_the_gate_open_through_a_dip() {
    // A drum's decay dips under the threshold long before the drum has
    // stopped. With no hold the release starts on the first dip and the tail
    // is cut in half.
    let dip_at = ms(50.0);
    // Long enough that the detector's own peak hold has expired well inside
    // it — otherwise this would be measuring `DETECT_HOLD_MS` rather than the
    // knob.
    let dip_for = ms(120.0);
    let source: Vec<f32> = sine(440.0, FRAMES)
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let level = if (dip_at..dip_at + dip_for).contains(&i) {
                from_db(-40.0)
            } else {
                from_db(-6.0)
            };
            s * level
        })
        .collect();

    let config = GateConfig {
        hold_ms: 0.0,
        release_ms: 5.0,
        ..gating(-20.0)
    };
    let cut = both(&config, &source);
    let held = both(
        &GateConfig {
            hold_ms: 300.0,
            ..config
        },
        &source,
    );

    let window = dip_at + ms(70.0)..dip_at + dip_for;
    let expected = rms(&source[window.clone()]);
    assert!(
        db(rms(&cut[window.clone()]) / expected) < -20.0,
        "with no hold the dip should have been gated"
    );
    assert!(
        db(rms(&held[window]) / expected).abs() < 0.5,
        "the hold did not carry the gate across the dip"
    );
}

// ------------------------------------------------------------- the envelope

#[test]
fn attack_and_release_are_the_times_they_say() {
    // A tone that starts loud and stops. The gate opens over the attack and
    // closes over the release, and "the time" is the usual convention: how
    // long to cover about 63 % of the distance in dB.
    let onset = ms(100.0);
    let offset = ms(200.0);
    let source: Vec<f32> = sine(2_000.0, FRAMES)
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if (onset..offset).contains(&i) {
                *s * 0.5
            } else {
                0.0
            }
        })
        .collect();

    let config = GateConfig {
        attack_ms: 10.0,
        hold_ms: 0.0,
        release_ms: 50.0,
        range_db: -60.0,
        ..gating(-30.0)
    };
    let out = both(&config, &source);

    // One attack after the onset the gain should be about 63 % of the way up
    // from the range, in dB.
    let at = |frame: usize| {
        let window = ms(2.0);
        db(rms(&out[frame..frame + window]) / rms(&source[frame..frame + window]))
    };
    let opened = at(onset + ms(10.0));
    assert!(
        (opened - (-60.0 * (1.0 - 0.632))).abs() < 6.0,
        "one attack in, the gate was {opened} dB down"
    );
    let open = at(onset + ms(80.0));
    assert!(open.abs() < 0.5, "the gate never fully opened: {open} dB");
}

// ---------------------------------------------------------------- the key

#[test]
fn the_key_filter_deafens_the_detector_to_the_bass_without_filtering_the_audio() {
    // The hi-hat mic that hears the kick. A loud low tone under a quiet high
    // one: with the key open the gate opens on the low tone and both come
    // through; with the key high-passed the gate stays shut.
    let low: Vec<f32> = sine(60.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-6.0))
        .collect();
    let high: Vec<f32> = sine(6_000.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-40.0))
        .collect();
    let mixed: Vec<f32> = low.iter().zip(high.iter()).map(|(l, h)| l + h).collect();

    let open_key = gating(-20.0);
    let filtered_key = GateConfig {
        key_hp_hz: 1_000.0,
        ..open_key
    };

    let heard = both(&open_key, &mixed);
    let deaf = both(&filtered_key, &mixed);
    let window = ms(100.0)..;

    assert!(
        db(rms(&heard[window.clone()]) / rms(&mixed[window.clone()])).abs() < 0.5,
        "the unfiltered key should have opened the gate"
    );
    assert!(
        db(rms(&deaf[window.clone()]) / rms(&mixed[window.clone()])) < -20.0,
        "the key filter did not deafen the detector to the bass"
    );

    // And the filter is on the decision, not on the audio: with the gate held
    // open by a loud enough signal, the low tone comes through untouched.
    let loud_high: Vec<f32> = sine(6_000.0, FRAMES).iter().map(|s| s * 0.5).collect();
    let both_tones: Vec<f32> = low
        .iter()
        .zip(loud_high.iter())
        .map(|(l, h)| l + h)
        .collect();
    let out = both(&filtered_key, &both_tones);
    let settled = ms(100.0)..;
    let moved = db(rms(&out[settled.clone()]) / rms(&both_tones[settled]));
    assert!(
        moved.abs() < 0.1,
        "the key filter reached the audio: {moved} dB"
    );
}

// ---------------------------------------------------------------- lookahead

#[test]
fn lookahead_opens_the_gate_before_the_transient_arrives() {
    // A hard onset with an attack slow enough to be heard eating it. With the
    // look-ahead set past the attack, the decision has already been made when
    // the transient comes out of the delay line.
    let onset = ms(100.0);
    let source: Vec<f32> = sine(2_000.0, FRAMES)
        .iter()
        .enumerate()
        .map(|(i, s)| if i >= onset { *s * 0.5 } else { 0.0 })
        .collect();

    let config = GateConfig {
        // Five of these fit inside the longest look-ahead the knob offers,
        // which is what "already open" means for a one-pole that never quite
        // arrives.
        attack_ms: 2.0,
        range_db: -60.0,
        lookahead_ms: 0.0,
        ..gating(-30.0)
    };
    let ahead = GateConfig {
        lookahead_ms: 10.0,
        ..config
    };

    let late = both(&config, &source);
    let early = both(&ahead, &source);

    // The first two milliseconds of the note, against what it should have
    // been. The delayed one is compared against the same two milliseconds of
    // source, since the look-ahead moves the audio by exactly its own length.
    let window = ms(2.0);
    let late_level = db(rms(&late[onset..onset + window]) / rms(&source[onset..onset + window]));
    let shifted = onset + ms(10.0);
    let early_level =
        db(rms(&early[shifted..shifted + window]) / rms(&source[onset..onset + window]));

    assert!(
        late_level < -6.0,
        "without look-ahead the attack should have eaten the front: {late_level} dB"
    );
    assert!(
        early_level > -1.0,
        "look-ahead did not save the transient: {early_level} dB"
    );
}

#[test]
fn a_gate_wide_open_with_lookahead_is_the_signal_delayed_and_nothing_else() {
    let source = cosine(440.0, FRAMES);
    let config = GateConfig {
        lookahead_ms: 5.0,
        ..GateConfig::new()
    };
    let out = both(&config, &source);
    let delay = ms(5.0);
    for index in delay..FRAMES {
        assert!(
            (out[index] - source[index - delay]).abs() < 1e-6,
            "frame {index} is not the delayed signal"
        );
    }
}

// ---------------------------------------------------------------- the image

#[test]
fn the_decision_is_stereo_linked() {
    // A gate that closed one side of an overhead pair and not the other would
    // move the kit. One channel loud, the other quiet: both stay open.
    let loud: Vec<f32> = sine(440.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-6.0))
        .collect();
    let quiet: Vec<f32> = sine(660.0, FRAMES)
        .iter()
        .map(|s| s * from_db(-40.0))
        .collect();
    let (out_l, out_r) = through(&gating(-20.0), loud.clone(), quiet.clone());
    let window = ms(100.0)..;
    assert!(db(rms(&out_l[window.clone()]) / rms(&loud[window.clone()])).abs() < 0.1);
    assert!(
        db(rms(&out_r[window.clone()]) / rms(&quiet[window])).abs() < 0.1,
        "the quiet side was gated on its own"
    );
}
