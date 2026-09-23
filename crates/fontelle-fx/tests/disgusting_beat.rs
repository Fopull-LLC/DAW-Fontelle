//! The machine: what a drawn curve does to a sound
//! (`docs/disgusting-beat-plan.md` §4, §10.2).
//!
//! Every claim here is measured rather than inspected, and the measures are
//! `common/mod.rs`'s. The two that are easy to get wrong:
//!
//! - **A pitch is measured inside a segment**, never across its edges: a
//!   window that straddles the moment the rate changes reads both rates and
//!   neither (memory `performance-events` — this project has been caught by
//!   it twice).
//! - **The memory starts empty.** A read from the past before anything has
//!   been written is a read of the clamp rule (§4.2), not of the curve, so
//!   every test that measures a curve primes the buffer first.

mod common;

use common::{SR, energy_at, peak, rms, sine};
use fontelle_fx::{DisgustingBeat, NoteInput};
use fontelle_types::{
    CurveShape, DisgustingBeatBank, DisgustingBeatConfig, DisgustingBeatGrid,
    DisgustingBeatLaneKind, DisgustingBeatLength, DisgustingBeatLook, DisgustingBeatPoint,
    DisgustingBeatQuality, DisgustingBeatSync, MusicalTime, PPQN,
};

const BPM: f32 = 120.0;
/// One bar of 4/4 at 120 bpm.
const BAR_SAMPLES: usize = 2 * SR as usize;

/// A bank with one lane drawn on scene 1 and everything else flat.
fn bank_with(
    kind: DisgustingBeatLaneKind,
    length: DisgustingBeatLength,
    points: &[DisgustingBeatPoint],
) -> DisgustingBeatBank {
    let mut bank = DisgustingBeatBank::new();
    let lane = bank.scenes[0].lane_mut(kind).unwrap();
    lane.length = length;
    lane.on = true;
    lane.points = points.to_vec();
    lane.tidy(kind);
    bank
}

fn grid_of(bank: &DisgustingBeatBank) -> DisgustingBeatGrid {
    DisgustingBeatGrid::from(bank)
}

/// Runs `frames` of `input` through a prepared DisgustingBeat, block by block,
/// with the song rolling from tick 0. Returns what came out.
fn render(
    disgusting_beat: &mut DisgustingBeat,
    config: &DisgustingBeatConfig,
    grid: &DisgustingBeatGrid,
    input: &[f32],
) -> Vec<f32> {
    render_from(disgusting_beat, config, grid, input, 0.0)
}

fn render_from(
    disgusting_beat: &mut DisgustingBeat,
    config: &DisgustingBeatConfig,
    grid: &DisgustingBeatGrid,
    input: &[f32],
    start_tick: f64,
) -> Vec<f32> {
    const BLOCK: usize = 128;
    let mut out = Vec::with_capacity(input.len());
    let mut music = MusicalTime::playing(BPM, SR);
    music.tick = start_tick;
    let mut written = 0;
    while written < input.len() {
        let frames = BLOCK.min(input.len() - written);
        let mut left = input[written..written + frames].to_vec();
        let mut right = left.clone();
        {
            let (a, b) = (&mut left[..], &mut right[..]);
            let mut channels: [&mut [f32]; 2] = [a, b];
            disgusting_beat.process(&mut channels, NoteInput::default(), grid, config, music);
        }
        out.extend_from_slice(&left);
        music.tick += music.ticks_per_sample * frames as f64;
        written += frames;
    }
    out
}

fn prepared() -> DisgustingBeat {
    let mut disgusting_beat = DisgustingBeat::new();
    disgusting_beat.prepare(SR);
    disgusting_beat
}

/// A straight ramp, so a frozen stretch is obvious: every sample is different
/// from every other one.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames).map(|i| i as f32 / frames as f32).collect()
}

#[test]
fn a_flat_curve_is_a_wire() {
    // Sample for sample, at every quality and every smoothing: somebody who
    // has just added one has not yet said what they want.
    let bank = DisgustingBeatBank::new();
    let grid = grid_of(&bank);
    for quality in DisgustingBeatQuality::ALL {
        for smooth in [0.0, 12.0, 50.0] {
            let mut config = DisgustingBeatConfig::new();
            config.quality = quality;
            config.smooth_ms = smooth;
            let mut disgusting_beat = prepared();
            let input = sine(440.0, 4096);
            let out = render(&mut disgusting_beat, &config, &grid, &input);
            for (i, (got, want)) in out.iter().zip(input.iter()).enumerate() {
                assert_eq!(
                    got, want,
                    "a fresh DisgustingBeat changed sample {i} at {quality:?}/{smooth} ms"
                );
            }
        }
    }
}

#[test]
fn a_freeze_repeats_the_frozen_window() {
    // The claim the whole effect rests on: a segment falling at one
    // lane-length per lane holds the sound still. Drawn as the second half of
    // a bar, so the first half is the wire and the join is visible.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = DisgustingBeatConfig::new();
    config.smooth_ms = 0.0;

    let mut disgusting_beat = prepared();
    let input = ramp(BAR_SAMPLES);
    let out = render(&mut disgusting_beat, &config, &grid, &input);

    // The first half is the wire.
    for (i, (got, want)) in out
        .iter()
        .zip(input.iter())
        .take(BAR_SAMPLES / 2)
        .enumerate()
    {
        assert!(
            (got - want).abs() < 1e-6,
            "the flat half moved at {i}: {got} vs {want}"
        );
    }
    // And the second half is the sample the freeze began on, all the way.
    let frozen = input[BAR_SAMPLES / 2];
    for (offset, got) in out[BAR_SAMPLES / 2 + 64..BAR_SAMPLES].iter().enumerate() {
        assert!(
            (got - frozen).abs() < 1e-4,
            "the freeze moved at {}: {got} should be {frozen}",
            BAR_SAMPLES / 2 + 64 + offset
        );
    }
}

#[test]
fn a_half_slope_segment_is_an_octave_down() {
    // rate = 1 + dv/dp, so a lane falling half a lane-length over its length
    // plays at half speed. Measured inside the segment, over a window that
    // does not touch either end of it.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();

    let mut disgusting_beat = prepared();
    // Three bars: the first two prime the memory, the third is measured.
    let input = sine(440.0, BAR_SAMPLES * 3);
    let out = render(&mut disgusting_beat, &config, &grid, &input);

    let window = &out[BAR_SAMPLES * 2 + BAR_SAMPLES / 4..BAR_SAMPLES * 2 + BAR_SAMPLES / 2];
    let at_440 = energy_at(window, 440.0);
    let at_220 = energy_at(window, 220.0);
    assert!(
        at_220 > at_440 * 4.0,
        "half speed should be an octave down: 220 Hz {at_220}, 440 Hz {at_440}"
    );
}

#[test]
fn a_steeper_slope_plays_it_backwards() {
    // rate = 1 + dv/dp, so −2 is reverse at the song's own speed — and that
    // is *half* a lane falling by one lane-length, not a whole lane falling
    // by two. The lane only reaches one lane-length (one freeze's worth), so
    // a reverse is drawn short rather than deep.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0, -1.0, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut disgusting_beat = prepared();

    // A rising ramp per bar: played backwards it falls.
    let input: Vec<f32> = (0..BAR_SAMPLES * 3)
        .map(|i| (i % BAR_SAMPLES) as f32 / BAR_SAMPLES as f32)
        .collect();
    let out = render(&mut disgusting_beat, &config, &grid, &input);

    // Inside the reversed half of the third bar.
    let from = BAR_SAMPLES * 2 + BAR_SAMPLES / 2 + 2_000;
    let window = &out[from..from + 20_000];
    let rising = window.windows(2).filter(|w| w[1] > w[0]).count();
    let falling = window.windows(2).filter(|w| w[1] < w[0]).count();
    assert!(
        falling > rising * 4,
        "a reversed ramp should mostly fall: {falling} down, {rising} up"
    );
}

#[test]
fn smoothing_takes_the_step_out_of_a_jump() {
    // A stepped curve moves the read head instantly, and that is a click. The
    // knob that fixes it has to be a knob you can hear working — so the test
    // asserts the step is *there* at zero as well.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, -0.25, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    // **Not a sine.** A 440 Hz sine read a quarter of a bar back at 120 bpm
    // is 220 whole periods earlier — the same sample, so the jump is
    // inaudible and the test measures nothing. A ramp has a different value
    // everywhere.
    let input = ramp(BAR_SAMPLES * 3);

    let step_of = |smooth_ms: f32| {
        let mut config = DisgustingBeatConfig::new();
        config.smooth_ms = smooth_ms;
        let mut disgusting_beat = prepared();
        let out = render(&mut disgusting_beat, &config, &grid, &input);
        // Around the jump in the third bar.
        let at = BAR_SAMPLES * 2 + BAR_SAMPLES / 2;
        out[at - 200..at + 200]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    };

    let hard = step_of(0.0);
    let soft = step_of(25.0);
    // The ramp moves by 3.5e-6 a sample; the jump is a quarter of a bar back
    // on a three-bar ramp, which is 0.083.
    assert!(hard > 0.05, "an unsmoothed jump should step: {hard}");
    assert!(
        soft < hard * 0.5,
        "smoothing should halve it at least: {soft} against {hard}"
    );
}

#[test]
fn the_volume_lane_silences_and_unity_is_untouched() {
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut disgusting_beat = prepared();
    let input = sine(440.0, BAR_SAMPLES);
    let out = render(&mut disgusting_beat, &config, &grid, &input);

    let open = rms(&out[1_000..BAR_SAMPLES / 2 - 1_000]);
    let shut = rms(&out[BAR_SAMPLES / 2 + 1_000..BAR_SAMPLES - 1_000]);
    let dry = rms(&input[1_000..BAR_SAMPLES / 2 - 1_000]);
    assert!(
        (open - dry).abs() < 1e-6,
        "unity should be untouched: {open} against {dry}"
    );
    assert!(shut < 1e-6, "zero should be silence: {shut}");
}

#[test]
fn the_tone_lane_at_zero_depth_is_bit_identical() {
    // An off filter is off, not a filter at a neutral setting: a state
    // variable running flat still moves the last bit, and "nothing drawn
    // here" has to mean nothing.
    let bank = bank_with(
        DisgustingBeatLaneKind::Tone,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, -1.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0, 1.0, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = DisgustingBeatConfig::new();
    config.tone = 0.0;
    let mut disgusting_beat = prepared();
    let input = sine(440.0, 4096);
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    assert_eq!(out, input, "a tone depth of zero is not a filter");

    config.tone = 1.0;
    let mut disgusting_beat = prepared();
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    assert!(out != input, "and a depth of one is");
}

#[test]
fn the_read_clamps_to_the_history_it_has() {
    // After a reset there is nothing behind the write head. A curve asking
    // for half a bar back would play silence for half a bar, which is a thing
    // people report as a bug — so it plays the live signal until the memory
    // has caught up.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[DisgustingBeatPoint::new(0.0, -0.5, CurveShape::Stepped)],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut disgusting_beat = prepared();
    let input = sine(440.0, 8_192);
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    assert!(
        rms(&out[..4_096]) > 0.2,
        "an empty memory should play live, not silence: {}",
        rms(&out[..4_096])
    );
}

#[test]
fn looking_ahead_reports_its_latency_and_delays_by_it() {
    let mut config = DisgustingBeatConfig::new();
    assert_eq!(
        config.latency_samples(BPM, 4, SR),
        0,
        "off is a zero-latency insert"
    );
    config.look = DisgustingBeatLook::Beat;
    let beat = config.latency_samples(BPM, 4, SR);
    assert_eq!(beat, SR as u32 / 2, "a beat at 120 bpm is half a second");
    config.look = DisgustingBeatLook::Bar;
    assert_eq!(config.latency_samples(BPM, 4, SR), SR as u32 * 2);

    // And a flat curve under lookahead is that delay, exactly: the graph
    // compensates it, so what it must *be* is a pure delay.
    config.look = DisgustingBeatLook::Beat;
    let grid = grid_of(&DisgustingBeatBank::new());
    let mut disgusting_beat = prepared();
    let mut input = vec![0.0f32; BAR_SAMPLES];
    input[100] = 1.0;
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    let landed = out
        .iter()
        .position(|s| s.abs() > 0.5)
        .expect("the impulse went somewhere");
    assert!(
        (landed as i64 - (100 + beat as i64)).abs() <= 1,
        "the impulse should land a beat late, at {}, not {landed}",
        100 + beat as i64
    );
}

#[test]
fn a_positive_offset_under_lookahead_plays_it_early() {
    // The half of the axis the thing this is modelled on cannot reach: with a
    // beat of lookahead and a quarter of a bar of *forward* offset, the two
    // cancel and the impulse comes out where it went in.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[DisgustingBeatPoint::new(0.0, 0.25, CurveShape::Stepped)],
    );
    let grid = grid_of(&bank);
    let mut config = DisgustingBeatConfig::new();
    config.look = DisgustingBeatLook::Beat;
    let mut disgusting_beat = prepared();
    let mut input = vec![0.0f32; BAR_SAMPLES];
    input[SR as usize] = 1.0;
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    let landed = out
        .iter()
        .position(|s| s.abs() > 0.5)
        .expect("the impulse went somewhere");
    assert!(
        (landed as i64 - SR as i64).abs() <= 2,
        "a beat of lookahead against a beat of forward offset is a wire: {landed} against {}",
        SR as i64
    );
}

#[test]
fn the_pattern_is_where_the_song_is() {
    // Bar 1 and bar 37 get the same part of the curve. This is what the tick
    // on the transport snapshot is for, and the property a
    // `position_sample × bpm` derivation loses at the first tempo change.
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let input = sine(440.0, BAR_SAMPLES);

    let mut first = prepared();
    let from_bar_1 = render_from(&mut first, &config, &grid, &input, 0.0);
    let mut later = prepared();
    let from_bar_37 = render_from(
        &mut later,
        &config,
        &grid,
        &input,
        (PPQN * 4 * 36) as f64, // bar 37 of 4/4
    );

    let shut_early = rms(&from_bar_1[BAR_SAMPLES / 2 + 1_000..BAR_SAMPLES - 1_000]);
    let shut_later = rms(&from_bar_37[BAR_SAMPLES / 2 + 1_000..BAR_SAMPLES - 1_000]);
    assert!(shut_early < 1e-6 && shut_later < 1e-6, "both bars gate");
    let open_early = rms(&from_bar_1[1_000..BAR_SAMPLES / 2 - 1_000]);
    let open_later = rms(&from_bar_37[1_000..BAR_SAMPLES / 2 - 1_000]);
    assert!((open_early - open_later).abs() < 1e-6, "and both bars open");
}

#[test]
fn the_lanes_run_at_their_own_lengths() {
    // A three-beat volume lane under a four-beat time lane is a twelve-beat
    // pattern out of two simple curves — the thing one grid for both cannot
    // do.
    let mut bank = DisgustingBeatBank::new();
    {
        let lane = bank.scenes[0]
            .lane_mut(DisgustingBeatLaneKind::Volume)
            .unwrap();
        lane.length = DisgustingBeatLength::ThreeBeats;
        lane.on = true;
        lane.points = vec![
            DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        ];
        lane.tidy(DisgustingBeatLaneKind::Volume);
    }
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut disgusting_beat = prepared();
    let input = sine(440.0, BAR_SAMPLES * 3);
    let out = render(&mut disgusting_beat, &config, &grid, &input);

    // A beat is half a second; the lane is three of them, so it gates from
    // 1.5 s into each 3 s cycle. At 3.0 s it is open again.
    let beat = SR as usize / 2;
    let quiet = rms(&out[beat * 2 + 1_000..beat * 3 - 1_000]);
    let loud = rms(&out[beat * 3 + 1_000..beat * 4 - 1_000]);
    assert!(
        quiet < 1e-6,
        "the second half of the three-beat lane: {quiet}"
    );
    assert!(loud > 0.2, "and the first half of the next one: {loud}");
}

#[test]
fn a_stopped_transport_still_runs_the_pattern() {
    // Somebody auditioning an insert with the transport stopped has to hear
    // it stutter, or they conclude it is broken before they ever press play.
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut disgusting_beat = prepared();

    let input = sine(440.0, BAR_SAMPLES);
    let mut out = Vec::new();
    let music = MusicalTime::stopped(BPM, SR); // tick never advances
    let mut written = 0;
    while written < input.len() {
        let frames = 128.min(input.len() - written);
        let mut left = input[written..written + frames].to_vec();
        let mut right = left.clone();
        {
            let mut channels: [&mut [f32]; 2] = [&mut left[..], &mut right[..]];
            disgusting_beat.process(&mut channels, NoteInput::default(), &grid, &config, music);
        }
        out.extend_from_slice(&left);
        written += frames;
    }

    assert!(rms(&out[1_000..BAR_SAMPLES / 2 - 1_000]) > 0.2, "opens");
    assert!(
        rms(&out[BAR_SAMPLES / 2 + 1_000..BAR_SAMPLES - 1_000]) < 1e-6,
        "and shuts, with a transport that never moved"
    );
}

#[test]
fn swing_moves_the_offbeat_and_leaves_the_downbeat() {
    // Swing leans on the *eighths*, so the gate that shows it has to be
    // drawn on one: points on the quarter grid sit exactly on the beats,
    // which is where swing by definition changes nothing.
    let points = [
        DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
        DisgustingBeatPoint::new(0.125, 0.0, CurveShape::Stepped),
        DisgustingBeatPoint::new(0.25, 1.0, CurveShape::Stepped),
        DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
    ];
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &points,
    );
    let grid = grid_of(&bank);
    // A gate is measured on a signal that is never zero of its own accord —
    // looking for the first silent sample of a *sine* finds its next zero
    // crossing, which is what the first draft of this test did.
    let input = vec![1.0f32; BAR_SAMPLES];

    let edges_of = |swing: f32| {
        let mut config = DisgustingBeatConfig::new();
        config.swing = swing;
        let mut disgusting_beat = prepared();
        let out = render(&mut disgusting_beat, &config, &grid, &input);
        // Where the gate **crosses** half open, not where it jumps by half
        // in one sample: the control lanes ramp across a step (0.17 ms), so
        // an edge is eight samples of 0.125 rather than one of 1.0.
        (1..out.len())
            .filter(|i| (out[*i - 1] > 0.5) != (out[*i] > 0.5))
            .collect::<Vec<_>>()
    };

    let straight = edges_of(0.0);
    let swung = edges_of(1.0);
    assert_eq!(straight.len(), swung.len(), "the same gates, moved");

    // The eighth-note gate shuts an eighth in when straight and a third of a
    // beat later at full swing: the triplet feel.
    let eighth = BAR_SAMPLES / 8;
    assert!(
        straight[0].abs_diff(eighth) < 100,
        "straight, the offbeat is on the eighth: {}",
        straight[0]
    );
    assert!(
        swung[0] > straight[0] + eighth / 4,
        "swing should push the offbeat later: {} against {}",
        swung[0],
        straight[0]
    );

    // And the beat itself does not move, at any swing: that is what
    // separates swing from a pattern drawn late.
    let beat = BAR_SAMPLES / 4;
    assert!(
        swung[1].abs_diff(beat) < 100,
        "the beat should stay where it is: {} against {beat}",
        swung[1]
    );
}

#[test]
fn high_quality_keeps_more_of_the_top_than_normal() {
    // What the two kernels differ by, measured rather than assumed. A read
    // at a fractional position is an interpolation, and a four-point Hermite
    // rolls the top off; the eight-point sinc is flat much further up. Read
    // a 12 kHz tone at half speed and the 6 kHz it becomes is the measure.
    //
    // It is *not* an aliasing test: reading a pure tone faster than the song
    // moves it, and if the move puts it past Nyquist no kernel can save it —
    // that needs a filter before the decimation, which is a different
    // feature (and not one this has).
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = DisgustingBeatConfig::new();
    let input = sine(16_000.0, BAR_SAMPLES * 3);

    let mut kept_by = |quality: DisgustingBeatQuality| {
        config.quality = quality;
        let mut disgusting_beat = prepared();
        let out = render(&mut disgusting_beat, &config, &grid, &input);
        let window = &out[BAR_SAMPLES * 2 + 10_000..BAR_SAMPLES * 2 + 40_000];
        energy_at(window, 8_000.0)
    };

    let normal = kept_by(DisgustingBeatQuality::Normal);
    let high = kept_by(DisgustingBeatQuality::High);
    assert!(
        high > normal * 1.05,
        "the sinc should keep more of it than the Hermite: {high} against {normal}"
    );
}

#[test]
fn every_curve_shape_does_something_and_they_are_not_all_the_same_thing() {
    // Catalogue rule 1, by name. Two shapes that sound alike are one shape.
    let mut rendered = Vec::new();
    for shape in CurveShape::ALL {
        let bank = bank_with(
            DisgustingBeatLaneKind::Volume,
            DisgustingBeatLength::Bar,
            // Three points, because `Hold` and `Stepped` are the same shape
            // until there is a *second* later point for `Hold` to ignore.
            &[
                DisgustingBeatPoint::new(0.0, 1.0, shape),
                DisgustingBeatPoint::new(0.5, 0.25, CurveShape::Linear),
                DisgustingBeatPoint::new(0.75, 1.0, CurveShape::Linear),
            ],
        );
        let grid = grid_of(&bank);
        let config = DisgustingBeatConfig::new();
        let mut disgusting_beat = prepared();
        let input = sine(440.0, BAR_SAMPLES);
        rendered.push((shape, render(&mut disgusting_beat, &config, &grid, &input)));
    }
    for (a, (shape_a, out_a)) in rendered.iter().enumerate() {
        for (shape_b, out_b) in rendered.iter().skip(a + 1) {
            let difference: f32 = out_a
                .iter()
                .zip(out_b.iter())
                .map(|(x, y)| (x - y).abs())
                .sum::<f32>()
                / out_a.len() as f32;
            assert!(
                difference > 1e-3,
                "{shape_a:?} and {shape_b:?} are the same curve: {difference}"
            );
        }
    }
}

#[test]
fn the_scene_is_what_plays() {
    let mut bank = DisgustingBeatBank::new();
    for (index, level) in [(0usize, 1.0), (1, 0.5), (2, 0.0)] {
        let lane = bank.scenes[index]
            .lane_mut(DisgustingBeatLaneKind::Volume)
            .unwrap();
        lane.on = true;
        lane.points = vec![DisgustingBeatPoint::new(0.0, level, CurveShape::Stepped)];
    }
    let grid = grid_of(&bank);
    let input = sine(440.0, 4_096);
    for (scene, expected) in [(0u8, 1.0f32), (1, 0.5), (2, 0.0)] {
        let mut config = DisgustingBeatConfig::new();
        config.scene = scene;
        let mut disgusting_beat = prepared();
        let out = render(&mut disgusting_beat, &config, &grid, &input);
        let got = rms(&out[1_000..]) / rms(&input[1_000..]);
        assert!(
            (got - expected).abs() < 0.02,
            "scene {scene} should play at {expected}, got {got}"
        );
    }
}

#[test]
fn a_note_picks_a_scene_when_the_chooser_says_so() {
    let mut bank = DisgustingBeatBank::new();
    let lane = bank.scenes[2]
        .lane_mut(DisgustingBeatLaneKind::Volume)
        .unwrap();
    lane.on = true;
    lane.points = vec![DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped)];
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new(); // notes: select
    let input = sine(440.0, 4_096);

    // D, in any octave, is the third scene — which is silent.
    let mut disgusting_beat = prepared();
    let mut out = Vec::new();
    let music = MusicalTime::playing(BPM, SR);
    let notes = NoteInput {
        last: Some(62),
        mask: 1 << 2,
        bend_cents: 0.0,
        ons: 1,
    };
    let mut written = 0;
    while written < input.len() {
        let frames = 128.min(input.len() - written);
        let mut left = input[written..written + frames].to_vec();
        let mut right = left.clone();
        {
            let mut channels: [&mut [f32]; 2] = [&mut left[..], &mut right[..]];
            disgusting_beat.process(&mut channels, notes, &grid, &config, music);
        }
        out.extend_from_slice(&left);
        written += frames;
    }
    assert!(rms(&out[1_000..]) < 1e-6, "D chose the silent scene");
}

#[test]
fn the_depth_knob_dials_the_whole_thing_back() {
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &[DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped)],
    );
    let grid = grid_of(&bank);
    let input = sine(440.0, 4_096);
    let mut config = DisgustingBeatConfig::new();
    config.volume = 0.5;
    let mut disgusting_beat = prepared();
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    let got = rms(&out[1_000..]) / rms(&input[1_000..]);
    assert!(
        (got - 0.5).abs() < 0.02,
        "half depth on a lane at silence is half volume: {got}"
    );
}

#[test]
fn nothing_it_makes_is_louder_than_what_went_in() {
    // A moving read head crossfading two copies of the same sound can sum,
    // and a curve that doubled the level would be a curve that clipped the
    // master. Every shape, at full depth, against the peak of the input.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.25, -0.1, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, -0.2, CurveShape::Linear),
            DisgustingBeatPoint::new(0.75, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut disgusting_beat = prepared();
    let input = sine(440.0, BAR_SAMPLES * 2);
    let out = render(&mut disgusting_beat, &config, &grid, &input);
    assert!(
        peak(&out) <= peak(&input) * 1.02,
        "a crossfade should not add level: {} against {}",
        peak(&out),
        peak(&input)
    );
}

#[test]
fn the_free_clock_starts_where_playback_did() {
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = DisgustingBeatConfig::new();
    config.sync = DisgustingBeatSync::Free;
    let mut disgusting_beat = prepared();
    let input = sine(440.0, BAR_SAMPLES);
    // Starting mid-bar: `song` would gate immediately, `free` opens first.
    let out = render_from(
        &mut disgusting_beat,
        &config,
        &grid,
        &input,
        (PPQN * 2) as f64,
    );
    assert!(
        rms(&out[1_000..BAR_SAMPLES / 4]) > 0.2,
        "a free clock opens where playback began"
    );
}

#[test]
fn the_trail_is_where_the_read_head_has_been() {
    // The window drew where the head *is* and nothing of where it has been,
    // which is the one picture that explains what a curve does. The trail is
    // one number per memory bucket: how far behind the write head the read
    // was when that bucket was written. A wire is a flat zero; a freeze walks
    // away from the present at one sample per sample.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = DisgustingBeatConfig::new();
    config.smooth_ms = 0.0;

    let mut disgusting_beat = prepared();
    let input = ramp(BAR_SAMPLES);
    let _ = render(&mut disgusting_beat, &config, &grid, &input);

    let trail = disgusting_beat.trail().to_vec();
    let newest = disgusting_beat.newest_bucket();
    let buckets = trail.len();
    // Oldest first, the way the window reads it.
    let ordered: Vec<f32> = (0..buckets)
        .map(|step| trail[(newest + 1 + step) % buckets])
        .collect();
    // Only the half that was actually written in this bar carries anything;
    // the memory is twelve seconds and the bar is two.
    let written = buckets / 6;
    let run = &ordered[buckets - written..];
    let half = run.len() / 2;
    for (index, age) in run[..half].iter().enumerate() {
        assert!(
            *age < 1.0,
            "the wire half is not live at {index}: {age} buckets behind"
        );
    }
    // And the freeze walks away: the last bucket of the bar is most of half a
    // bar behind the present, and every step is further back than the one
    // before it.
    let last = *run.last().expect("a bar of trail");
    assert!(
        last > half as f32 * 0.8,
        "the freeze did not walk away: {last} buckets behind after half a bar"
    );
    for pair in run[half + 2..].windows(2) {
        assert!(
            pair[1] >= pair[0] - 0.5,
            "the trail went forwards during a freeze: {pair:?}"
        );
    }
}

/// Where the read head was, decoded from the output of a ramp: a ramp's value
/// *is* its own sample index, so what comes out says where it was read from.
fn read_at(out: &[f32], sample: usize, frames: usize) -> f64 {
    out[sample] as f64 * frames as f64
}

#[test]
fn a_steep_segment_scrubs_instead_of_jamming_the_crossfade() {
    // **A steep line is a scrub and has to be heard as one.** The crossfade
    // decides there is a discontinuity when the read position moves more than
    // half a millisecond in a sample, and a segment steep enough to do that
    // *every* sample re-arms it every sample — which is anything past about
    // 25x, and a point dragged a whole lane-length across one 1/32 division
    // is 32x. Written because that looked certain to jam the fade and play
    // the line back at normal speed; it does not, because the outgoing head
    // is re-seated to the curve each time, so the scrub survives one sample
    // late. This is here to keep it that way — the failure it would catch is
    // silent, and a drawn line that does nothing is the worst kind.
    //
    // Drawn here: half a lane-length dropped across a sixty-fourth of it, so
    // the read head runs backwards at thirty-one times speed.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(1.0 / 64.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();

    let mut fx = prepared();
    let frames = BAR_SAMPLES * 2;
    let input = ramp(frames);
    let out = render(&mut fx, &config, &grid, &input);

    // Inside the steep stretch of the second bar, which is 1/64 of a bar =
    // 1500 samples long. Read the head early in it and late in it.
    let early = BAR_SAMPLES + 200;
    let late = BAR_SAMPLES + 1300;
    let moved = read_at(&out, late, frames) - read_at(&out, early, frames);
    let elapsed = (late - early) as f64;
    // Thirty-one times backwards over 1100 samples is about 34 000 samples of
    // memory. Anything forwards at all means the fade jammed and the drawn
    // line was thrown away.
    assert!(
        moved < -20.0 * elapsed,
        "the steep segment did not scrub: the read head moved {moved:.0} samples \
         over {elapsed:.0}, which is {:.1}x. It should be about -31x.",
        moved / elapsed
    );
}

#[test]
fn a_read_that_touches_the_write_head_does_not_spike() {
    // **The crunch.** Ty, on the first release: *"lots of crunchyness in the
    // sounds"*. The interpolator is a four-point Hermite, so it wants two
    // samples either side of where it is reading — and the ring only has
    // samples up to the write head. Filling the taps past it with **zeros**
    // puts a cliff under the kernel, and a cubic through a cliff overshoots:
    // one sample at 0.664 in a stretch where every neighbour is 0.625, every
    // time the read head touches live at a fractional position.
    //
    // Which is often. Any segment with a rate above one is a read closing on
    // the write head, and it arrives there at whatever fraction it likes:
    // *Double-time*, *Speed Up*, *Forward*, *Chirp* and *Time Melt* all did
    // it twice a bar. The taps are held at the edge now, which is what every
    // resampler does at the end of a buffer.
    //
    // Drawn here: a quarter of a lane-length climbed back over half of it, so
    // the read runs at 1.5x and meets the write head exactly at the halfway
    // point.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, -0.25, CurveShape::Linear),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut fx = prepared();
    let frames = BAR_SAMPLES * 3;
    let input = ramp(frames);
    let out = render(&mut fx, &config, &grid, &input);

    // A ramp read at any steady rate is a straight line, so every kink in the
    // output is the effect's own. The rate changes once a bar, which is a
    // kink of one sample's worth of ramp; a spike is thousands of times that.
    let tail = &out[BAR_SAMPLES..];
    let (mut worst, mut at) = (0.0f32, 0usize);
    for (i, w) in tail.windows(3).enumerate() {
        let kink = (w[0] - 2.0 * w[1] + w[2]).abs();
        if kink > worst {
            worst = kink;
            at = i;
        }
    }
    let one_sample = 1.0 / frames as f32;
    assert!(
        worst < one_sample * 4.0,
        "the read spiked at sample {at}: a kink of {worst}, where one sample \
         of the ramp is {one_sample}. The interpolator is reading off the end \
         of what has been written."
    );
}

#[test]
fn every_preset_in_the_bank_plays_without_a_click() {
    // **The sweep.** Ty: *"please make sure it works with no issues, covering
    // any edge cases or weird interactions."* Seventy rows, every scene of
    // every kit, each one played for four bars — and the thing being looked
    // for is a discontinuity nobody asked for.
    //
    // A **ramp** goes in, because a ramp's value is its own sample index: the
    // output *is* the read position, and reading it back says exactly where
    // the head was. A drawn jump then shows as the crossfade walking across
    // over its own twelve milliseconds; anything that moves the head a
    // thousand samples between two output samples is a click, and there is
    // nowhere for one to hide.
    //
    // This found two faults and would have found both before release. The
    // read touching the write head at a fractional position spiked on five
    // rows twice a bar, and every peak over unity in the bank was the same
    // fault seen from the other side.
    use fontelle_types::DisgustingBeatFactoryPreset as Row;
    let frames = BAR_SAMPLES * 4;
    let input = ramp(frames);
    let loudest = sine(1000.0, frames);
    for row in Row::ALL {
        let (config, mut bank) = row.build();
        // The **time lane alone**: a gate's edge is a discontinuity by
        // design — it is what a gate *is* — and it would drown out the thing
        // being looked for. `a_gate_edge_is_ramped_not_stepped` is the volume
        // lane's own guard.
        for scene in bank.scenes.iter_mut() {
            for kind in [
                DisgustingBeatLaneKind::Volume,
                DisgustingBeatLaneKind::Tone,
                DisgustingBeatLaneKind::Pan,
            ] {
                if let Some(lane) = scene.lane_mut(kind) {
                    lane.on = false;
                }
            }
        }
        let grid = grid_of(&bank);
        let mut fx = prepared();
        let out = render(&mut fx, &config, &grid, &input);
        // The last two bars: the first two are the memory filling, where a
        // curve reaching for history that does not exist yet is answered by
        // the clamp rather than by the drawing.
        let tail = &out[BAR_SAMPLES * 2..];
        for (index, pair) in tail.windows(2).enumerate() {
            let moved = (pair[1] - pair[0]).abs() * frames as f32;
            assert!(
                moved < 2_000.0,
                "{} ({}) moved the read head {moved:.0} samples between two \
                 output samples, {index} into the third bar. A jump the \
                 crossfade covered walks across in twelve milliseconds; this \
                 is a step, and a step is a click.",
                row.name,
                row.category
            );
        }

        // And nothing it does is louder than what went in. Two copies of one
        // sound under a fade can sum, and an interpolator reading off the end
        // of the memory overshoots — both come out here.
        let mut fx = prepared();
        let out = render(&mut fx, &config, &grid, &loudest);
        assert!(
            peak(&out[BAR_SAMPLES..]) <= 1.001,
            "{} peaks at {:.3} on a full-scale tone",
            row.name,
            peak(&out[BAR_SAMPLES..])
        );
    }
}

#[test]
fn a_gate_edge_is_ramped_not_stepped() {
    // The volume lane's own guard, and the reason the sweep above can leave
    // the lane out: a gate's edge *is* a discontinuity, so it has to be a
    // deliberate one. A lane stepping 1 to 0 between two samples is a jump at
    // full scale — a click on every edge, and every preset in the *Gate*
    // category is made of nothing else. Eight samples is 0.17 ms, still far
    // too fast to read as a fade.
    let bank = bank_with(
        DisgustingBeatLaneKind::Volume,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Stepped),
            DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut fx = prepared();
    let input = vec![1.0f32; BAR_SAMPLES * 2];
    let out = render(&mut fx, &config, &grid, &input);

    let tail = &out[BAR_SAMPLES..];
    let steepest = tail
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        steepest < 0.2,
        "the gate edge is a step of {steepest}, not a ramp"
    );
    // And it is still a gate: all the way open and all the way shut.
    assert!(
        peak(&tail[..BAR_SAMPLES / 4]) > 0.99,
        "the gate never opened"
    );
    assert!(
        peak(&tail[BAR_SAMPLES * 3 / 4..]) < 0.01,
        "the gate never shut"
    );
}

#[test]
fn every_drawn_scene_in_the_bank_actually_changes_the_sound() {
    // **A preset that does nothing is the worst kind of bug**, because it has
    // no symptom: it sounds like a wire, and a wire sounds fine. *Push* and
    // *Rushed* were drawn above the line — reading a future that only
    // look-ahead buys — so the clamp ate every sample of them and they
    // shipped doing nothing whatever. One beat of *Drunk* went the same way.
    //
    // `no_factory_row_reads_a_future_it_has_not_got` stops that particular
    // way of being a wire. This stops all the others: every scene anybody
    // drew has to be audibly different from the same knobs with nothing on
    // them.
    use fontelle_types::DisgustingBeatFactoryPreset as Row;
    // A **ramp**, not a tone. A steady sine is invariant under a shift of a
    // whole number of its own periods, and a quarter bar at 120 bpm is
    // exactly two hundred and twenty cycles of 440 Hz — so *Quarter Roll*,
    // which replays the first quarter of the bar four times, came out of a
    // sine bit for bit unchanged. Every sample of a ramp is different from
    // every other one, which is the whole reason it is the probe here.
    let input = ramp(BAR_SAMPLES * 3);
    for row in Row::ALL {
        let (mut config, bank) = row.build();
        for scene in 0..bank.scenes.len() {
            if bank.scenes[scene].is_flat() {
                continue; // a kit's first scene is *off*, on purpose
            }
            config.scene = scene as u8;
            let mut drawn = prepared();
            let with = render(&mut drawn, &config, &grid_of(&bank), &input);
            // The same knobs, including whatever latency they carry, with
            // nothing drawn: anything else would measure the look-ahead.
            let empty = fontelle_types::DisgustingBeatBank::new();
            let mut plain = prepared();
            let without = render(&mut plain, &config, &grid_of(&empty), &input);

            // In **samples of read position**, which is the unit the drawing
            // is in: the ramp climbs the whole way over three bars, so a
            // difference of one part in 288 000 is one sample of offset.
            // Amplitude would have been the wrong ruler — *Swing 16* pushes
            // the offbeats by twenty-four milliseconds, which is a whole
            // groove and four thousandths of a ramp.
            let frames = input.len() as f32;
            let moved = with
                .iter()
                .zip(&without)
                .skip(BAR_SAMPLES)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max)
                * frames;
            assert!(
                moved > 48.0,
                "{} scene {} is a wire: it moves the sound by {moved:.1} samples, \
                 which is under a millisecond",
                row.name,
                scene + 1
            );
        }
    }
}

#[test]
fn a_loop_wrap_does_not_click() {
    // The commonest thing anybody does with this: put it on a two-bar loop
    // and hold the play button. Every wrap is a **discontinuity in the song's
    // position** — the tick jumps backwards by the loop's length — and the
    // offset the lane reports jumps with it. That is a jump like any other
    // and the crossfade has to take it, or the effect clicks once a loop
    // forever.
    let bank = bank_with(
        DisgustingBeatLaneKind::Time,
        DisgustingBeatLength::Bar,
        &[
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(0.5, -0.4, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = DisgustingBeatConfig::new();
    let mut fx = prepared();

    const BLOCK: usize = 128;
    let frames = BAR_SAMPLES * 4;
    let input = ramp(frames);
    let mut music = MusicalTime::playing(BPM, SR);
    let mut out = Vec::with_capacity(frames);
    let mut written = 0;
    // Two bars, then back to the top, twice over: a loop.
    let loop_ticks = music.ticks_per_sample * (BAR_SAMPLES * 2) as f64;
    while written < frames {
        let take = BLOCK.min(frames - written);
        let mut left = input[written..written + take].to_vec();
        let mut right = left.clone();
        {
            let (a, b) = (&mut left[..], &mut right[..]);
            let mut channels: [&mut [f32]; 2] = [a, b];
            fx.process(&mut channels, NoteInput::default(), &grid, &config, music);
        }
        out.extend_from_slice(&left);
        music.tick += music.ticks_per_sample * take as f64;
        if music.tick >= loop_ticks {
            music.tick -= loop_ticks;
        }
        written += take;
    }

    // The wraps land at two bars and four; look at the whole of the second
    // pass, which contains one.
    let tail = &out[BAR_SAMPLES * 2 + 1..];
    let worst = tail
        .windows(2)
        .map(|w| (w[1] - w[0]).abs() * frames as f32)
        .fold(0.0f32, f32::max);
    assert!(
        worst < 2_000.0,
        "the loop wrap moved the read head {worst:.0} samples between two \
         output samples \u{2014} the crossfade did not cover it"
    );
}
