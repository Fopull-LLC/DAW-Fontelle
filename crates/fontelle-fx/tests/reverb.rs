//! The reverb's DSP: a feedback delay network, measured as a decaying tail
//! (TDD §13.4).
//!
//! Algorithmic rather than convolution, which the stub this replaced already
//! said and which is worth restating as a *testable* difference: an FDN's
//! decay time, size and damping are knobs with closed-form effects on the
//! tail, so every test below is a measurement of the tail rather than a
//! comparison against a stored impulse response.
//!
//! Like the delay, and for the reason written at the top of `delay.rs`, what
//! comes out is the **tail only**. `EffectNode` owns the dry/wet blend.
//!
//! # What an FDN has to get right, and what these tests are watching for
//!
//! Three things go wrong in an FDN and only one of them is audible as an
//! obvious fault. A feedback matrix that is not orthogonal loses or gains
//! energy — `it_stays_bounded_at_the_longest_decay` catches the gaining half;
//! line lengths that share factors give a metallic tail rather than a dense
//! one; and damping applied outside the loop dulls the whole tail equally
//! instead of taking the top off it progressively, which is
//! `damping_shortens_the_top_end_more_than_the_bottom`.

use fontelle_fx::FdnReverb;
use fontelle_types::ReverbConfig;

const SR: f32 = 48_000.0;

/// Two seconds, so a long decay has somewhere to be measured.
const FRAMES: usize = 96_000;

fn silence(frames: usize) -> Vec<f32> {
    vec![0.0; frames]
}

fn impulse(frames: usize) -> Vec<f32> {
    let mut buffer = silence(frames);
    buffer[0] = 1.0;
    buffer
}

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

/// A short burst, so the tail is measured after the source has stopped.
///
/// **Faded in and out**, which is not decoration: a tone that starts and stops
/// abruptly is a tone plus two clicks, and a click is broadband. Measuring the
/// "8 kHz tail" of an unfaded burst measures the clicks, which is how a
/// damping test can come out saying damping does nothing.
fn burst(freq: f32, frames: usize, length: usize) -> Vec<f32> {
    let mut buffer = sine(freq, frames);
    buffer[length..].fill(0.0);
    let fade = (length / 4).max(1);
    for i in 0..fade {
        let window = 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / fade as f32).cos();
        buffer[i] *= window;
        buffer[length - 1 - i] *= window;
    }
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

fn ms(milliseconds: f32) -> usize {
    (milliseconds * SR / 1000.0) as usize
}

fn through(config: &ReverbConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut reverb = FdnReverb::new();
    reverb.prepare(SR);
    reverb.process(&mut [&mut left, &mut right], config);
    (left, right)
}

/// A medium hall with the damping open, which is the baseline every test
/// varies one knob away from.
fn hall() -> ReverbConfig {
    ReverbConfig {
        size: 0.5,
        decay_s: 2.0,
        damping_hz: 20_000.0,
        pre_delay_ms: 0.0,
        width: 1.0,
        mix: 1.0,
    }
}

#[test]
fn an_impulse_becomes_a_tail_that_outlasts_it() {
    let (left, right) = through(&hall(), impulse(FRAMES), impulse(FRAMES));
    let tail = rms(&left[ms(200.0)..ms(400.0)]);
    assert!(
        tail > 1e-4,
        "a click should still be ringing 200 ms later; got {tail}"
    );
    assert!(rms(&right[ms(200.0)..ms(400.0)]) > 1e-4, "on both sides");
}

#[test]
fn the_tail_decays_rather_than_holding() {
    let (left, _) = through(&hall(), impulse(FRAMES), impulse(FRAMES));
    let early = rms(&left[ms(100.0)..ms(200.0)]);
    let late = rms(&left[ms(1500.0)..ms(1900.0)]);
    assert!(
        late < early * 0.25,
        "a two-second decay should be well down after 1.5 s: {early} then {late}"
    );
}

#[test]
fn a_longer_decay_rings_longer() {
    // The knob's whole job, and the one an FDN gets wrong by scaling the
    // feedback gain without accounting for how long each line is: a short line
    // goes round more often per second than a long one, so one gain for all of
    // them gives a tail whose length depends on the size knob instead.
    let mut short = hall();
    short.decay_s = 0.4;
    let mut long = hall();
    long.decay_s = 8.0;

    let window = ms(1000.0)..ms(1200.0);
    let (short_out, _) = through(&short, impulse(FRAMES), impulse(FRAMES));
    let (long_out, _) = through(&long, impulse(FRAMES), impulse(FRAMES));

    assert!(
        rms(&long_out[window.clone()]) > rms(&short_out[window]) * 8.0,
        "an eight-second tail should dwarf a 0.4-second one a second in"
    );
}

#[test]
fn the_decay_time_is_roughly_what_it_says() {
    // RT60: the time to fall 60 dB. Within a factor of two is the useful
    // claim — an FDN's tail is not a single exponential — and it is enough to
    // catch a decay knob that is off by an order of magnitude, which is what a
    // feedback gain derived from the wrong logarithm looks like.
    let mut config = hall();
    config.decay_s = 2.0;
    let (left, _) = through(&config, impulse(FRAMES), impulse(FRAMES));

    let reference = rms(&left[ms(50.0)..ms(150.0)]);
    let at_two_seconds = rms(&left[ms(1900.0)..ms(2000.0)]);
    let fallen_db = 20.0 * (at_two_seconds / reference).max(1e-9).log10();
    assert!(
        (-90.0..-30.0).contains(&fallen_db),
        "a two-second RT60 should be somewhere near 60 dB down at two seconds, was {fallen_db} dB"
    );
}

#[test]
fn pre_delay_holds_the_tail_off() {
    // The gap between the sound and the room answering it, which is what makes
    // a big reverb sit behind a vocal instead of on top of it.
    let mut config = hall();
    config.pre_delay_ms = 100.0;
    let (left, _) = through(&config, impulse(FRAMES), impulse(FRAMES));
    assert!(
        peak(&left[..ms(90.0)]) < 1e-5,
        "something arrived before the pre-delay was up"
    );
    assert!(
        rms(&left[ms(150.0)..ms(400.0)]) > 1e-4,
        "and the tail should arrive after it"
    );
}

#[test]
fn nothing_arrives_at_the_moment_the_sound_was_made() {
    // Even with no pre-delay: the shortest line in the network is the first
    // reflection, and a room whose first reflection is at zero is not a room,
    // it is the dry signal leaking through.
    let (left, _) = through(&hall(), impulse(FRAMES), impulse(FRAMES));
    assert!(
        peak(&left[..ms(5.0)]) < 0.05,
        "the dry impulse leaked into the tail"
    );
}

#[test]
fn a_bigger_room_answers_later() {
    let mut small = hall();
    small.size = 0.0;
    let mut big = hall();
    big.size = 1.0;

    let first_arrival = |config: &ReverbConfig| {
        let (left, _) = through(config, impulse(FRAMES), impulse(FRAMES));
        left.iter()
            .position(|s| s.abs() > 1e-3)
            .expect("something should arrive")
    };
    assert!(
        first_arrival(&big) > first_arrival(&small) * 2,
        "a large room's first reflection should be well after a small one's"
    );
}

#[test]
fn damping_shortens_the_top_end_more_than_the_bottom() {
    // Air absorbs treble, and a room without this sounds like a metal tank.
    // Inside the feedback loop, so each trip round takes a little more off —
    // a filter on the output would dull the first reflection as much as the
    // last, which is a tone control rather than a room.
    let mut open = hall();
    open.decay_s = 4.0;
    let mut damped = open;
    damped.damping_hz = 800.0;

    let window = ms(600.0)..ms(1000.0);
    let treble = |config: &ReverbConfig| {
        let (left, _) = through(config, burst(8_000.0, FRAMES, ms(50.0)), silence(FRAMES));
        rms(&left[window.clone()])
    };
    let bass = |config: &ReverbConfig| {
        let (left, _) = through(config, burst(200.0, FRAMES, ms(50.0)), silence(FRAMES));
        rms(&left[window.clone()])
    };

    let treble_loss = treble(&damped) / treble(&open).max(1e-9);
    let bass_loss = bass(&damped) / bass(&open).max(1e-9);
    assert!(
        treble_loss < bass_loss * 0.5,
        "damping should cost the 8 kHz tail far more than the 200 Hz one: \
         {treble_loss} against {bass_loss}"
    );
}

#[test]
fn width_at_zero_is_the_same_tail_on_both_sides() {
    let mut config = hall();
    config.width = 0.0;
    let (left, right) = through(&config, impulse(FRAMES), silence(FRAMES));
    let difference = left
        .iter()
        .zip(right.iter())
        .fold(0.0f32, |m, (l, r)| m.max((l - r).abs()));
    assert!(difference < 1e-5, "a mono tail differs by {difference}");
}

#[test]
fn a_wide_tail_is_not_the_same_on_both_sides() {
    // The other half of the claim: a width knob that does nothing at 100 % is
    // a width knob that does nothing.
    let (left, right) = through(&hall(), impulse(FRAMES), silence(FRAMES));
    let difference = left
        .iter()
        .zip(right.iter())
        .fold(0.0f32, |m, (l, r)| m.max((l - r).abs()));
    assert!(difference > 1e-3, "a stereo tail should differ across it");
}

#[test]
fn it_stays_bounded_at_the_longest_decay() {
    // The feedback matrix has to be energy-preserving. One that is not either
    // dies immediately or runs away, and running away is the failure that
    // reaches the speakers.
    let mut config = hall();
    config.decay_s = 20.0;
    config.size = 1.0;
    let noise: Vec<f32> = (0..FRAMES)
        .map(|i| ((i as f32 * 12.9898).sin() * 43_758.547).fract() * 2.0 - 1.0)
        .collect();
    let (left, right) = through(&config, noise.clone(), noise);
    assert!(
        left.iter().chain(right.iter()).all(|s| s.is_finite()),
        "the network produced a non-finite sample"
    );
    assert!(peak(&left) < 8.0, "the network ran away to {}", peak(&left));
}

#[test]
fn reset_forgets_the_room() {
    let mut reverb = FdnReverb::new();
    reverb.prepare(SR);
    let config = hall();
    let mut left = impulse(FRAMES);
    let mut right = impulse(FRAMES);
    reverb.process(&mut [&mut left, &mut right], &config);

    reverb.reset();
    let mut left = silence(FRAMES);
    let mut right = silence(FRAMES);
    reverb.process(&mut [&mut left, &mut right], &config);
    assert!(peak(&left) < 1e-6, "a reset reverb still had a tail in it");
}
