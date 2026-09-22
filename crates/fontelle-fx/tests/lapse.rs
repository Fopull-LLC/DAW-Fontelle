//! The machine: what a drawn curve does to a sound
//! (`docs/lapse-plan.md` §4, §10.2).
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
use fontelle_fx::{Lapse, NoteInput};
use fontelle_types::{
    CurveShape, LapseBank, LapseConfig, LapseGrid, LapseLaneKind, LapseLength, LapseLook,
    LapsePoint, LapseQuality, LapseSync, MusicalTime, PPQN,
};

const BPM: f32 = 120.0;
/// One bar of 4/4 at 120 bpm.
const BAR_SAMPLES: usize = 2 * SR as usize;

/// A bank with one lane drawn on scene 1 and everything else flat.
fn bank_with(kind: LapseLaneKind, length: LapseLength, points: &[LapsePoint]) -> LapseBank {
    let mut bank = LapseBank::new();
    let lane = bank.scenes[0].lane_mut(kind).unwrap();
    lane.length = length;
    lane.on = true;
    lane.points = points.to_vec();
    lane.tidy(kind);
    bank
}

fn grid_of(bank: &LapseBank) -> LapseGrid {
    LapseGrid::from(bank)
}

/// Runs `frames` of `input` through a prepared Lapse, block by block, with the
/// song rolling from tick 0. Returns what came out.
fn render(lapse: &mut Lapse, config: &LapseConfig, grid: &LapseGrid, input: &[f32]) -> Vec<f32> {
    render_from(lapse, config, grid, input, 0.0)
}

fn render_from(
    lapse: &mut Lapse,
    config: &LapseConfig,
    grid: &LapseGrid,
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
            lapse.process(&mut channels, NoteInput::default(), grid, config, music);
        }
        out.extend_from_slice(&left);
        music.tick += music.ticks_per_sample * frames as f64;
        written += frames;
    }
    out
}

fn prepared() -> Lapse {
    let mut lapse = Lapse::new();
    lapse.prepare(SR);
    lapse
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
    let bank = LapseBank::new();
    let grid = grid_of(&bank);
    for quality in LapseQuality::ALL {
        for smooth in [0.0, 12.0, 50.0] {
            let mut config = LapseConfig::new();
            config.quality = quality;
            config.smooth_ms = smooth;
            let mut lapse = prepared();
            let input = sine(440.0, 4096);
            let out = render(&mut lapse, &config, &grid, &input);
            for (i, (got, want)) in out.iter().zip(input.iter()).enumerate() {
                assert_eq!(
                    got, want,
                    "a fresh Lapse changed sample {i} at {quality:?}/{smooth} ms"
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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 0.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Linear),
            LapsePoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = LapseConfig::new();
    config.smooth_ms = 0.0;

    let mut lapse = prepared();
    let input = ramp(BAR_SAMPLES);
    let out = render(&mut lapse, &config, &grid, &input);

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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 0.0, CurveShape::Linear),
            LapsePoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();

    let mut lapse = prepared();
    // Three bars: the first two prime the memory, the third is measured.
    let input = sine(440.0, BAR_SAMPLES * 3);
    let out = render(&mut lapse, &config, &grid, &input);

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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 0.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Linear),
            LapsePoint::new(1.0, -1.0, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
    let mut lapse = prepared();

    // A rising ramp per bar: played backwards it falls.
    let input: Vec<f32> = (0..BAR_SAMPLES * 3)
        .map(|i| (i % BAR_SAMPLES) as f32 / BAR_SAMPLES as f32)
        .collect();
    let out = render(&mut lapse, &config, &grid, &input);

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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 0.0, CurveShape::Stepped),
            LapsePoint::new(0.5, -0.25, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    // **Not a sine.** A 440 Hz sine read a quarter of a bar back at 120 bpm
    // is 220 whole periods earlier — the same sample, so the jump is
    // inaudible and the test measures nothing. A ramp has a different value
    // everywhere.
    let input = ramp(BAR_SAMPLES * 3);

    let step_of = |smooth_ms: f32| {
        let mut config = LapseConfig::new();
        config.smooth_ms = smooth_ms;
        let mut lapse = prepared();
        let out = render(&mut lapse, &config, &grid, &input);
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
        LapseLaneKind::Volume,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 1.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
    let mut lapse = prepared();
    let input = sine(440.0, BAR_SAMPLES);
    let out = render(&mut lapse, &config, &grid, &input);

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
        LapseLaneKind::Tone,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, -1.0, CurveShape::Linear),
            LapsePoint::new(1.0, 1.0, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = LapseConfig::new();
    config.tone = 0.0;
    let mut lapse = prepared();
    let input = sine(440.0, 4096);
    let out = render(&mut lapse, &config, &grid, &input);
    assert_eq!(out, input, "a tone depth of zero is not a filter");

    config.tone = 1.0;
    let mut lapse = prepared();
    let out = render(&mut lapse, &config, &grid, &input);
    assert!(out != input, "and a depth of one is");
}

#[test]
fn the_read_clamps_to_the_history_it_has() {
    // After a reset there is nothing behind the write head. A curve asking
    // for half a bar back would play silence for half a bar, which is a thing
    // people report as a bug — so it plays the live signal until the memory
    // has caught up.
    let bank = bank_with(
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[LapsePoint::new(0.0, -0.5, CurveShape::Stepped)],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
    let mut lapse = prepared();
    let input = sine(440.0, 8_192);
    let out = render(&mut lapse, &config, &grid, &input);
    assert!(
        rms(&out[..4_096]) > 0.2,
        "an empty memory should play live, not silence: {}",
        rms(&out[..4_096])
    );
}

#[test]
fn looking_ahead_reports_its_latency_and_delays_by_it() {
    let mut config = LapseConfig::new();
    assert_eq!(
        config.latency_samples(BPM, 4, SR),
        0,
        "off is a zero-latency insert"
    );
    config.look = LapseLook::Beat;
    let beat = config.latency_samples(BPM, 4, SR);
    assert_eq!(beat, SR as u32 / 2, "a beat at 120 bpm is half a second");
    config.look = LapseLook::Bar;
    assert_eq!(config.latency_samples(BPM, 4, SR), SR as u32 * 2);

    // And a flat curve under lookahead is that delay, exactly: the graph
    // compensates it, so what it must *be* is a pure delay.
    config.look = LapseLook::Beat;
    let grid = grid_of(&LapseBank::new());
    let mut lapse = prepared();
    let mut input = vec![0.0f32; BAR_SAMPLES];
    input[100] = 1.0;
    let out = render(&mut lapse, &config, &grid, &input);
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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[LapsePoint::new(0.0, 0.25, CurveShape::Stepped)],
    );
    let grid = grid_of(&bank);
    let mut config = LapseConfig::new();
    config.look = LapseLook::Beat;
    let mut lapse = prepared();
    let mut input = vec![0.0f32; BAR_SAMPLES];
    input[SR as usize] = 1.0;
    let out = render(&mut lapse, &config, &grid, &input);
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
        LapseLaneKind::Volume,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 1.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
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
    let mut bank = LapseBank::new();
    {
        let lane = bank.scenes[0].lane_mut(LapseLaneKind::Volume).unwrap();
        lane.length = LapseLength::ThreeBeats;
        lane.on = true;
        lane.points = vec![
            LapsePoint::new(0.0, 1.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Stepped),
        ];
        lane.tidy(LapseLaneKind::Volume);
    }
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
    let mut lapse = prepared();
    let input = sine(440.0, BAR_SAMPLES * 3);
    let out = render(&mut lapse, &config, &grid, &input);

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
        LapseLaneKind::Volume,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 1.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
    let mut lapse = prepared();

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
            lapse.process(&mut channels, NoteInput::default(), &grid, &config, music);
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
        LapsePoint::new(0.0, 1.0, CurveShape::Stepped),
        LapsePoint::new(0.125, 0.0, CurveShape::Stepped),
        LapsePoint::new(0.25, 1.0, CurveShape::Stepped),
        LapsePoint::new(0.5, 0.0, CurveShape::Stepped),
    ];
    let bank = bank_with(LapseLaneKind::Volume, LapseLength::Bar, &points);
    let grid = grid_of(&bank);
    // A gate is measured on a signal that is never zero of its own accord —
    // looking for the first silent sample of a *sine* finds its next zero
    // crossing, which is what the first draft of this test did.
    let input = vec![1.0f32; BAR_SAMPLES];

    let edges_of = |swing: f32| {
        let mut config = LapseConfig::new();
        config.swing = swing;
        let mut lapse = prepared();
        let out = render(&mut lapse, &config, &grid, &input);
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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 0.0, CurveShape::Linear),
            LapsePoint::new(1.0, -0.5, CurveShape::Linear),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = LapseConfig::new();
    let input = sine(16_000.0, BAR_SAMPLES * 3);

    let mut kept_by = |quality: LapseQuality| {
        config.quality = quality;
        let mut lapse = prepared();
        let out = render(&mut lapse, &config, &grid, &input);
        let window = &out[BAR_SAMPLES * 2 + 10_000..BAR_SAMPLES * 2 + 40_000];
        energy_at(window, 8_000.0)
    };

    let normal = kept_by(LapseQuality::Normal);
    let high = kept_by(LapseQuality::High);
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
            LapseLaneKind::Volume,
            LapseLength::Bar,
            // Three points, because `Hold` and `Stepped` are the same shape
            // until there is a *second* later point for `Hold` to ignore.
            &[
                LapsePoint::new(0.0, 1.0, shape),
                LapsePoint::new(0.5, 0.25, CurveShape::Linear),
                LapsePoint::new(0.75, 1.0, CurveShape::Linear),
            ],
        );
        let grid = grid_of(&bank);
        let config = LapseConfig::new();
        let mut lapse = prepared();
        let input = sine(440.0, BAR_SAMPLES);
        rendered.push((shape, render(&mut lapse, &config, &grid, &input)));
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
    let mut bank = LapseBank::new();
    for (index, level) in [(0usize, 1.0), (1, 0.5), (2, 0.0)] {
        let lane = bank.scenes[index].lane_mut(LapseLaneKind::Volume).unwrap();
        lane.on = true;
        lane.points = vec![LapsePoint::new(0.0, level, CurveShape::Stepped)];
    }
    let grid = grid_of(&bank);
    let input = sine(440.0, 4_096);
    for (scene, expected) in [(0u8, 1.0f32), (1, 0.5), (2, 0.0)] {
        let mut config = LapseConfig::new();
        config.scene = scene;
        let mut lapse = prepared();
        let out = render(&mut lapse, &config, &grid, &input);
        let got = rms(&out[1_000..]) / rms(&input[1_000..]);
        assert!(
            (got - expected).abs() < 0.02,
            "scene {scene} should play at {expected}, got {got}"
        );
    }
}

#[test]
fn a_note_picks_a_scene_when_the_chooser_says_so() {
    let mut bank = LapseBank::new();
    let lane = bank.scenes[2].lane_mut(LapseLaneKind::Volume).unwrap();
    lane.on = true;
    lane.points = vec![LapsePoint::new(0.0, 0.0, CurveShape::Stepped)];
    let grid = grid_of(&bank);
    let config = LapseConfig::new(); // notes: select
    let input = sine(440.0, 4_096);

    // D, in any octave, is the third scene — which is silent.
    let mut lapse = prepared();
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
            lapse.process(&mut channels, notes, &grid, &config, music);
        }
        out.extend_from_slice(&left);
        written += frames;
    }
    assert!(rms(&out[1_000..]) < 1e-6, "D chose the silent scene");
}

#[test]
fn the_depth_knob_dials_the_whole_thing_back() {
    let bank = bank_with(
        LapseLaneKind::Volume,
        LapseLength::Bar,
        &[LapsePoint::new(0.0, 0.0, CurveShape::Stepped)],
    );
    let grid = grid_of(&bank);
    let input = sine(440.0, 4_096);
    let mut config = LapseConfig::new();
    config.volume = 0.5;
    let mut lapse = prepared();
    let out = render(&mut lapse, &config, &grid, &input);
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
        LapseLaneKind::Time,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 0.0, CurveShape::Stepped),
            LapsePoint::new(0.25, -0.1, CurveShape::Stepped),
            LapsePoint::new(0.5, -0.2, CurveShape::Linear),
            LapsePoint::new(0.75, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let config = LapseConfig::new();
    let mut lapse = prepared();
    let input = sine(440.0, BAR_SAMPLES * 2);
    let out = render(&mut lapse, &config, &grid, &input);
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
        LapseLaneKind::Volume,
        LapseLength::Bar,
        &[
            LapsePoint::new(0.0, 1.0, CurveShape::Stepped),
            LapsePoint::new(0.5, 0.0, CurveShape::Stepped),
        ],
    );
    let grid = grid_of(&bank);
    let mut config = LapseConfig::new();
    config.sync = LapseSync::Free;
    let mut lapse = prepared();
    let input = sine(440.0, BAR_SAMPLES);
    // Starting mid-bar: `song` would gate immediately, `free` opens first.
    let out = render_from(&mut lapse, &config, &grid, &input, (PPQN * 2) as f64);
    assert!(
        rms(&out[1_000..BAR_SAMPLES / 4]) > 0.2,
        "a free clock opens where playback began"
    );
}
