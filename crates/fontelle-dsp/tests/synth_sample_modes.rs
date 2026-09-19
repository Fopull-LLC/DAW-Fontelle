//! The **ways a sample source reads its recording** beyond once-through and a
//! forward loop: backwards, bouncing between the loop points, and as a cloud
//! of grains.
//!
//! > *"use flopsynths new sampling features to make a variety of new complex
//! > presets that can be experimental, synthy, modulating ... if you think we
//! > can expand the features any way to make our synth even more awesome and
//! > genuinely stand up to other synths like omnisphere and serum we should
//! > do that."* — Ty, 2026-09-16
//!
//! A recording played once is a sampler. What a *synthesiser* does with one
//! is read it in ways a tape head cannot: the grain cloud is the reason a
//! piano note can be a pad, and the reason a route on the start knob is a
//! sound that moves rather than a note that starts elsewhere. Each test
//! holds one property of one mode that the modes before it lack.

use fontelle_dsp::{
    Interpolation, SampleData, SampleLoop, SampleSettings, SynthInput, SynthOsc, SynthSource,
    SynthState,
};

const SR: f32 = 48_000.0;

fn tone(hz: f32, seconds: f32) -> Vec<f32> {
    let frames = (seconds * SR) as usize;
    (0..frames)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin() * 0.8)
        .collect()
}

/// A recording that rises from nothing to full over `seconds`: its shape is
/// its position, so where the head is can be read off the output.
fn ramp(seconds: f32) -> Vec<f32> {
    let frames = (seconds * SR) as usize;
    (0..frames).map(|i| i as f32 / frames as f32).collect()
}

fn osc(mode: SampleLoop) -> SynthOsc {
    SynthOsc {
        source: SynthSource::Sample(0),
        sample: SampleSettings {
            loop_mode: mode,
            ..SampleSettings::default()
        },
        ..SynthOsc::default()
    }
}

fn data(samples: &[f32], root_hz: f32) -> SampleData<'_> {
    SampleData {
        samples,
        sample_rate: SR,
        root_hz,
        interpolation: Interpolation::Normal,
    }
}

fn render_into(
    state: &mut SynthState,
    config: &SynthOsc,
    data: &SampleData<'_>,
    note_hz: f32,
    frames: usize,
) -> Vec<f32> {
    (0..frames)
        .map(|_| {
            state
                .next_sample_from(config, SynthInput::Sample(*data), note_hz, SR, 0.0)
                .0
        })
        .collect()
}

fn render(config: &SynthOsc, data: &SampleData<'_>, note_hz: f32, frames: usize) -> Vec<f32> {
    let mut state = SynthState::new();
    state.reset(config, 1);
    render_into(&mut state, config, data, note_hz, frames)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn zero_crossings_per_second(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    crossings as f32 / (samples.len() as f32 / SR)
}

/// A Hann-windowed DFT at `hz`.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

#[test]
fn the_ways_of_reading_are_five_and_each_has_a_word() {
    assert_eq!(
        SampleLoop::ALL,
        [
            SampleLoop::Off,
            SampleLoop::Forward,
            SampleLoop::PingPong,
            SampleLoop::Reverse,
            SampleLoop::Grains,
        ]
    );
    let labels: Vec<&str> = SampleLoop::ALL.iter().map(|m| m.label()).collect();
    assert_eq!(labels, ["Once", "Loop", "Bounce", "Reverse", "Grains"]);
}

/// Backwards: the recording's end is the note's start, and the note ends
/// where the recording began.
#[test]
fn a_reversed_one_shot_plays_the_recording_backwards_and_ends_at_its_start() {
    let sound = ramp(0.5);
    let data = data(&sound, 440.0);
    let out = render(&osc(SampleLoop::Reverse), &data, 440.0, 36_000);
    // Falling, not rising: what the recording's end has is what comes first.
    assert!(
        out[100] > 0.9 * std::f32::consts::FRAC_1_SQRT_2
            && out[100] > out[12_000]
            && out[12_000] > out[23_000],
        "backwards through a ramp the level falls: {} then {} then {}",
        out[100],
        out[12_000],
        out[23_000]
    );
    // And past the recording's own start there is nothing.
    let after = rms(&out[25_000..]);
    assert!(
        after < 1e-6,
        "a reversed one-shot ends at the recording's start: {after}"
    );
}

/// The start knob on a reversed read counts from the end, so a knob at rest
/// plays the whole recording backwards and a knob part way skips its tail.
#[test]
fn the_start_knob_of_a_reversed_read_counts_from_the_end() {
    let sound = ramp(0.5);
    let data = data(&sound, 440.0);
    let mut config = osc(SampleLoop::Reverse);
    config.position = 0.5;
    let out = render(&config, &data, 440.0, 1_000);
    let level = out[10] / std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (level - 0.5).abs() < 0.02,
        "halfway from the end of a ramp is half: {level}"
    );
}

/// Bouncing between the loop points reverses direction at each rather than
/// jumping back, so a recording with no seam-friendly cycle loops without
/// a seam at all: a ramp goes up, then down, then up, and never steps.
#[test]
fn a_bouncing_loop_turns_round_at_each_end_and_never_jumps() {
    let sound = ramp(0.5);
    let data = data(&sound, 440.0);
    let mut bounce = osc(SampleLoop::PingPong);
    bounce.sample.loop_start = 0.2;
    bounce.sample.loop_end = 0.8;
    let bounced = render(&bounce, &data, 440.0, 96_000);
    let jump = bounced
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(jump < 1e-3, "a bouncing loop on a ramp never jumps: {jump}");
    // It reaches both ends: the top of the loop and the bottom of it.
    let late = &bounced[48_000..];
    let top = late.iter().cloned().fold(0.0f32, f32::max) / std::f32::consts::FRAC_1_SQRT_2;
    let bottom = late.iter().cloned().fold(f32::MAX, f32::min) / std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (top - 0.8).abs() < 0.02 && (bottom - 0.2).abs() < 0.02,
        "it turns round at the loop points: {bottom} .. {top}"
    );
    // Still going two seconds in, and going *down* some of the time.
    let late = &bounced[80_000..];
    assert!(
        rms(late) > 0.1,
        "a bouncing loop keeps sounding: {}",
        rms(late)
    );
    let falling = late.windows(2).filter(|w| w[1] < w[0]).count();
    let rising = late.windows(2).filter(|w| w[1] > w[0]).count();
    assert!(
        falling > rising / 3 && rising > falling / 3,
        "a bounce spends time going each way: {rising} up, {falling} down"
    );
}

/// The grain cloud: a note held past the recording's end is still the note,
/// because grains keep landing at the start knob.
#[test]
fn grains_keep_a_short_recording_alive_at_the_note() {
    let sound = tone(440.0, 0.25);
    let data = data(&sound, 440.0);
    let mut config = osc(SampleLoop::Grains);
    config.position = 0.3;
    config.sample.spray = 0.0;
    let out = render(&config, &data, 440.0, 96_000);
    let late = &out[72_000..];
    assert!(
        rms(late) > 0.25,
        "grains from a quarter-second recording still sound at a second and a half: {}",
        rms(late)
    );
    let measured = zero_crossings_per_second(late);
    assert!(
        (measured - 440.0).abs() < 6.0,
        "and they are the note: {measured} Hz"
    );
    // An octave up, an octave up.
    let up = render(&config, &data, 880.0, 48_000);
    let measured = zero_crossings_per_second(&up[24_000..]);
    assert!(
        (measured - 880.0).abs() < 12.0,
        "an octave over the root is 880: {measured} Hz"
    );
}

/// The start knob is read as it moves: grains spawned after the knob turns
/// land where it points, so a route on it *scans* the recording.
#[test]
fn grains_follow_the_start_knob_as_it_moves() {
    let mut sound = tone(440.0, 0.5);
    sound.extend(tone(880.0, 0.5));
    let data = data(&sound, 440.0);
    let mut config = osc(SampleLoop::Grains);
    config.position = 0.2;
    config.sample.spray = 0.0;
    config.sample.grain_ms = 40.0;
    let mut state = SynthState::new();
    state.reset(&config, 1);
    let first = render_into(&mut state, &config, &data, 440.0, 24_000);
    config.position = 0.7;
    let second = render_into(&mut state, &config, &data, 440.0, 24_000);
    let before = zero_crossings_per_second(&first[12_000..]);
    let after = zero_crossings_per_second(&second[12_000..]);
    assert!(
        (before - 440.0).abs() < 8.0,
        "pointing at the first half, the grains are 440: {before}"
    );
    assert!(
        (after - 880.0).abs() < 16.0,
        "pointed at the second half, the grains are 880 within a few grains: {after}"
    );
}

/// The grains are staggered so their windows add to a constant: a cloud of
/// a steady tone is a steady tone, not a tremolo at the grain rate.
#[test]
fn evenly_staggered_grains_are_a_steady_level() {
    let sound = tone(440.0, 1.0);
    let data = data(&sound, 440.0);
    let mut config = osc(SampleLoop::Grains);
    config.position = 0.3;
    config.sample.spray = 0.0;
    config.sample.grain_ms = 50.0;
    let out = render(&config, &data, 440.0, 48_000);
    let windows: Vec<f32> = out[4_800..].chunks(2_400).map(rms).collect();
    let loudest = windows.iter().cloned().fold(0.0f32, f32::max);
    let quietest = windows.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        quietest > loudest * 0.7,
        "a cloud of a steady tone is steady: {quietest} .. {loudest}"
    );
    assert!(loudest > 0.3, "and it is not quiet: {loudest}");
}

/// Spray: how far from the start knob each grain may land. At nothing every
/// grain is the same moment of the recording; at full they come from
/// anywhere in it.
#[test]
fn spray_scatters_the_grains_across_the_recording() {
    let mut sound = tone(440.0, 0.5);
    sound.extend(tone(880.0, 0.5));
    let data = data(&sound, 440.0);
    let mut tight = osc(SampleLoop::Grains);
    tight.position = 0.2;
    tight.sample.spray = 0.0;
    let mut wide = tight;
    wide.sample.spray = 1.0;
    let a = render(&tight, &data, 440.0, 48_000);
    let b = render(&wide, &data, 440.0, 48_000);
    let octave_share = |out: &[f32]| energy_at(out, 880.0) / energy_at(out, 440.0).max(1e-9);
    assert!(
        octave_share(&a) < 0.1,
        "no spray, pointed at the 440 half: nothing of the 880 half, {}",
        octave_share(&a)
    );
    assert!(
        octave_share(&b) > 0.3,
        "full spray reaches the other half: {}",
        octave_share(&b)
    );
}

/// Grain length is how quickly the cloud follows the knob: a grain in the
/// air keeps reading where it landed until its window closes, so short
/// grains are at the knob's new place in a few milliseconds and long ones
/// take their length to get there.
#[test]
fn short_grains_follow_the_knob_at_once_and_long_ones_take_their_time() {
    let mut sound = tone(440.0, 0.5);
    sound.extend(tone(880.0, 0.5));
    let data = data(&sound, 440.0);
    let octave_share = |grain_ms: f32| {
        let mut config = osc(SampleLoop::Grains);
        config.position = 0.2;
        config.sample.spray = 0.0;
        config.sample.grain_ms = grain_ms;
        let mut state = SynthState::new();
        state.reset(&config, 1);
        let _ = render_into(&mut state, &config, &data, 440.0, 24_000);
        config.position = 0.7;
        let after = render_into(&mut state, &config, &data, 440.0, 4_800);
        // The window from 50 to 100 ms after the knob moved.
        let window = &after[2_400..];
        energy_at(window, 880.0) / energy_at(window, 440.0).max(1e-9)
    };
    let (short, long) = (octave_share(8.0), octave_share(300.0));
    assert!(
        short > 4.0,
        "fifty milliseconds after the knob moved, eight-millisecond grains are at the octave: {short}"
    );
    assert!(
        long < 0.5,
        "and three-hundred-millisecond grains are still mostly where they were: {long}"
    );
}

/// The settings a recording had before these modes existed still read as
/// they did — a file that says only `loop_mode` and the points loads with
/// the new knobs at their defaults, and a default block is still left out.
#[test]
fn the_new_knobs_default_and_stay_out_of_an_unchanged_file() {
    let old = r#"{"loop_mode":"Forward","loop_start":0.25,"loop_end":0.75}"#;
    let settings: SampleSettings = serde_json::from_str(old).expect("an old block loads");
    assert_eq!(settings.loop_mode, SampleLoop::Forward);
    assert_eq!(settings.loop_start, 0.25);
    assert_eq!(settings.grain_ms, SampleSettings::default().grain_ms);
    assert_eq!(settings.spray, SampleSettings::default().spray);
    assert_eq!(settings.zone, None);
    let osc = SynthOsc {
        source: SynthSource::Sample(0),
        ..SynthOsc::default()
    };
    let text = serde_json::to_string(&osc).unwrap();
    assert!(
        !text.contains("grain_ms"),
        "an oscillator on the default settings writes no sample block: {text}"
    );
}
