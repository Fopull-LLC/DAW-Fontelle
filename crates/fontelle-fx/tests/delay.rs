//! The delay's DSP, measured as an impulse and its repeats (TDD §13.4).
//!
//! The third effect, and the first one with **memory measured in seconds**.
//! An EQ's state is two numbers per section and a compressor's is one
//! envelope; a delay holds the last two seconds of the signal, which makes
//! `prepare` the place the buffer is sized and makes "does it click when the
//! time moves" a question worth a test.
//!
//! Every test here measures **where a peak landed** rather than what the
//! output sounds like, because that is the only question a delay answers: a
//! feedback path wired to the wrong side of a filter still produces plausible
//! echoes at the wrong times.
//!
//! # What comes out is the repeats, and only the repeats
//!
//! The delay writes the *wet* signal, exactly as the EQ writes the filtered
//! one: `EffectNode` owns the dry/wet blend for every effect (see
//! `fontelle-types/tests/effect_mix.rs`), so an effect that mixed its own dry
//! back in would be blended twice. That is why the tests below assert silence
//! before the first repeat — the dry signal is not this crate's to add.

use fontelle_fx::Delay;
use fontelle_types::{DEFAULT_BPM, DelayConfig};

const SR: f32 = 48_000.0;

/// A delay line long enough to hold three repeats of the times used here.
const FRAMES: usize = 48_000;

fn silence(frames: usize) -> Vec<f32> {
    vec![0.0; frames]
}

/// An impulse at sample zero, which is the signal every timing test uses: the
/// echo of a click is the click, so where it lands is unambiguous.
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

/// A tone that stops, so a repeat can be measured on its own rather than under
/// whatever is still being played.
fn burst(freq: f32, frames: usize, length: usize) -> Vec<f32> {
    let mut buffer = sine(freq, frames);
    buffer[length..].fill(0.0);
    buffer
}

/// Runs one buffer through a freshly prepared delay, returning `(left, right)`.
fn through(config: &DelayConfig, mut left: Vec<f32>, mut right: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
    let mut delay = Delay::new();
    delay.prepare(SR);
    delay.process(&mut [&mut left, &mut right], config, DEFAULT_BPM);
    (left, right)
}

/// Where the largest sample in `range` sits, and how big it is.
fn peak_in(samples: &[f32], range: std::ops::Range<usize>) -> (usize, f32) {
    samples[range.clone()]
        .iter()
        .enumerate()
        .map(|(i, s)| (i + range.start, s.abs()))
        .fold((range.start, 0.0), |best, next| {
            if next.1 > best.1 { next } else { best }
        })
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// A delay that repeats once and does nothing else to the signal: no feedback,
/// the damping filter wide open, no saturation.
///
/// The baseline every timing test wants, because each of those three would
/// otherwise smear the very peak being located.
fn plain(time_ms: f32) -> DelayConfig {
    DelayConfig {
        time_ms,
        sync: false,
        division: fontelle_types::NoteDivision::Eighth,
        feedback: 0.0,
        damping_hz: 20_000.0,
        drive: 0.0,
        ping_pong: false,
        mix: 1.0,
    }
}

/// A hundred milliseconds is 4800 samples at 48 kHz — a round number, which
/// makes an off-by-one in the read pointer visible rather than plausible.
const TIME_MS: f32 = 100.0;
const TIME_SAMPLES: usize = 4_800;

#[test]
fn an_impulse_comes_back_one_delay_time_later() {
    let (left, _) = through(&plain(TIME_MS), impulse(FRAMES), silence(FRAMES));
    let (at, level) = peak_in(&left, 1..FRAMES);
    assert!(
        at.abs_diff(TIME_SAMPLES) <= 2,
        "the repeat should land at {TIME_SAMPLES}, landed at {at}"
    );
    assert!(level > 0.9, "and arrive at full level; got {level}");
}

#[test]
fn nothing_comes_out_before_the_first_repeat() {
    // The dry signal is `EffectNode`'s to blend, not this crate's to add — see
    // the module comment. A delay that passed its input through would be mixed
    // in twice and a fully dry insert would still be audible.
    let (left, _) = through(&plain(TIME_MS), impulse(FRAMES), silence(FRAMES));
    let early = peak(&left[..TIME_SAMPLES - 4]);
    assert!(
        early < 1e-4,
        "the input leaked into the wet path; got {early}"
    );
}

#[test]
fn feedback_sends_it_round_again() {
    let mut config = plain(TIME_MS);
    config.feedback = 0.5;
    let (left, _) = through(&config, impulse(FRAMES), silence(FRAMES));

    for repeat in 1..=3 {
        let centre = TIME_SAMPLES * repeat;
        let (at, level) = peak_in(&left, centre - 40..centre + 40);
        let expected = 0.5f32.powi(repeat as i32 - 1);
        assert!(
            at.abs_diff(centre) <= 2,
            "repeat {repeat} should land at {centre}, landed at {at}"
        );
        assert!(
            (level - expected).abs() < 0.1,
            "repeat {repeat} should be {expected}, was {level}"
        );
    }
}

#[test]
fn no_feedback_means_exactly_one_repeat() {
    let (left, _) = through(&plain(TIME_MS), impulse(FRAMES), silence(FRAMES));
    let second = peak(&left[TIME_SAMPLES + 100..]);
    assert!(second < 1e-3, "a second repeat appeared at {second}");
}

#[test]
fn damping_takes_the_top_off_each_repeat_and_not_the_first() {
    // In the feedback path, which is where a delay's damping belongs: the
    // first repeat is what was played, and each one after it has been round
    // the filter once more. A damping filter on the output instead would dull
    // the first repeat too, which is a low-pass with extra steps.
    let mut open = plain(TIME_MS);
    open.feedback = 0.7;
    let mut dull = open;
    dull.damping_hz = 1_000.0;

    // A burst shorter than one delay time, so each window holds one repeat and
    // nothing else. A continuous tone would put the *fresh input* into every
    // window — the line always contains what is being played into it — and the
    // measurement would be of the input, which no amount of damping changes.
    let tone = burst(8_000.0, FRAMES, 2_000);
    let (bright_out, _) = through(&open, tone.clone(), silence(FRAMES));
    let (dull_out, _) = through(&dull, tone, silence(FRAMES));

    let first = TIME_SAMPLES..TIME_SAMPLES + 2_000;
    let later = TIME_SAMPLES * 3..TIME_SAMPLES * 3 + 2_000;

    assert!(
        (peak(&bright_out[first.clone()]) - peak(&dull_out[first])).abs() < 0.05,
        "the first repeat has not been through the filter yet"
    );
    assert!(
        peak(&dull_out[later.clone()]) < peak(&bright_out[later]) * 0.5,
        "an 8 kHz tone should not survive three trips through a 1 kHz filter"
    );
}

#[test]
fn ping_pong_puts_each_repeat_on_the_other_side() {
    let mut config = plain(TIME_MS);
    config.feedback = 0.7;
    config.ping_pong = true;
    // Only the left channel is played, so a repeat on the right can only have
    // come from the crossed feedback path.
    let (left, right) = through(&config, impulse(FRAMES), silence(FRAMES));

    let odd = peak_in(&left, TIME_SAMPLES - 40..TIME_SAMPLES + 40).1;
    let odd_other = peak_in(&right, TIME_SAMPLES - 40..TIME_SAMPLES + 40).1;
    assert!(
        odd > 0.9 && odd_other < 0.05,
        "the first repeat is on the left"
    );

    let even = peak_in(&right, TIME_SAMPLES * 2 - 40..TIME_SAMPLES * 2 + 40).1;
    let even_other = peak_in(&left, TIME_SAMPLES * 2 - 40..TIME_SAMPLES * 2 + 40).1;
    assert!(
        even > 0.5 && even_other < 0.05,
        "the second is on the right; got {even} against {even_other}"
    );
}

#[test]
fn without_ping_pong_a_repeat_stays_where_it_was_played() {
    let mut config = plain(TIME_MS);
    config.feedback = 0.7;
    let (_, right) = through(&config, impulse(FRAMES), silence(FRAMES));
    assert!(
        peak(&right) < 1e-4,
        "an unping-ponged delay must not move the image"
    );
}

#[test]
fn moving_the_time_while_it_runs_does_not_click() {
    // A delay time is a knob somebody drags, and a read pointer that jumped to
    // the new position would splice two unrelated points of the signal
    // together — which is a click, at full scale, every time the knob moves.
    let mut delay = Delay::new();
    delay.prepare(SR);
    const BLOCK: usize = 512;
    let tone = sine(200.0, FRAMES);

    let mut config = plain(200.0);
    let mut worst = 0.0f32;
    let mut previous = 0.0;
    for block in 0..40 {
        if block == 20 {
            config.time_ms = 350.0;
        }
        let start = block * BLOCK;
        let mut left = tone[start..start + BLOCK].to_vec();
        let mut right = left.clone();
        delay.process(&mut [&mut left, &mut right], &config, DEFAULT_BPM);
        for sample in left {
            worst = worst.max((sample - previous).abs());
            previous = sample;
        }
    }
    // A 200 Hz sine moves 0.026 per sample; a spliced discontinuity would be
    // most of full scale.
    assert!(
        worst < 0.1,
        "the output stepped by {worst} on a time change"
    );
}

#[test]
fn a_hard_driven_feedback_loop_stays_bounded() {
    // Saturation in the feedback path is what makes a delay's repeats sit
    // down rather than pile up, and it is also what keeps a feedback of 0.95
    // from being an oscillator.
    let mut config = plain(TIME_MS);
    config.feedback = 0.95;
    config.drive = 1.0;
    let (left, right) = through(&config, sine(440.0, FRAMES), sine(440.0, FRAMES));
    assert!(
        left.iter().chain(right.iter()).all(|s| s.is_finite()),
        "the loop produced a non-finite sample"
    );
    assert!(peak(&left) < 4.0, "the loop ran away to {}", peak(&left));
}

#[test]
fn reset_forgets_the_tail() {
    let mut config = plain(TIME_MS);
    config.feedback = 0.8;
    let mut delay = Delay::new();
    delay.prepare(SR);
    let mut left = impulse(FRAMES / 2);
    let mut right = silence(FRAMES / 2);
    delay.process(&mut [&mut left, &mut right], &config, DEFAULT_BPM);

    delay.reset();
    let mut left = silence(FRAMES / 2);
    let mut right = silence(FRAMES / 2);
    delay.process(&mut [&mut left, &mut right], &config, DEFAULT_BPM);
    assert!(
        peak(&left) < 1e-6,
        "a reset delay is silent until something is played into it"
    );
}

#[test]
fn a_time_longer_than_the_line_is_clamped_rather_than_believed() {
    // The spec clamps this long before the DSP sees it; the DSP not trusting
    // it anyway is what keeps a config built by hand from indexing past the
    // buffer.
    let mut config = plain(10_000.0);
    config.feedback = 0.0;
    let (left, _) = through(&config, impulse(FRAMES), silence(FRAMES));
    assert!(left.iter().all(|s| s.is_finite()));
}

// ------------------------------------------------------- and in note values

/// A synced delay's repeat lands on the note value, at the tempo it was given.
///
/// The timing tests above all set a time in milliseconds; this is the same
/// measurement made through the other half of the control. The DSP does not
/// know what a dotted eighth is — `DelayConfig::effective_time_ms` does, which
/// is where it belongs, because what a note value *is* is a document fact.
#[test]
fn a_synced_repeat_lands_on_the_beat() {
    use fontelle_types::NoteDivision;

    // A quarter note at 120 bpm is half a second: 24 000 samples.
    let mut config = plain(TIME_MS);
    config.sync = true;
    config.division = NoteDivision::Quarter;

    let mut delay = Delay::new();
    delay.prepare(SR);
    let mut left = impulse(FRAMES);
    let mut right = silence(FRAMES);
    delay.process(&mut [&mut left, &mut right], &config, 120.0);

    let (at, level) = peak_in(&left, 1..FRAMES);
    assert!(
        at.abs_diff(24_000) <= 2,
        "a quarter at 120 bpm should land at 24000, landed at {at}"
    );
    assert!(level > 0.9, "and at full level; got {level}");
}

#[test]
fn the_same_note_value_moves_with_the_tempo() {
    use fontelle_types::NoteDivision;

    // Which is the whole point of syncing: the setting is musical, so it has
    // to follow the song rather than the clock.
    let mut config = plain(TIME_MS);
    config.sync = true;
    config.division = NoteDivision::Eighth;

    let landing = |bpm: f32| {
        let mut delay = Delay::new();
        delay.prepare(SR);
        let mut left = impulse(FRAMES);
        let mut right = silence(FRAMES);
        delay.process(&mut [&mut left, &mut right], &config, bpm);
        peak_in(&left, 1..FRAMES).0
    };

    // An eighth is a quarter of a second at 120, and a sixth at 180.
    assert!(landing(120.0).abs_diff(12_000) <= 2);
    assert!(landing(180.0).abs_diff(8_000) <= 2);
}

#[test]
fn an_unsynced_delay_is_unmoved_by_the_tempo_it_is_handed() {
    let config = plain(TIME_MS);
    for bpm in [60.0, 120.0, 200.0] {
        let mut delay = Delay::new();
        delay.prepare(SR);
        let mut left = impulse(FRAMES);
        let mut right = silence(FRAMES);
        delay.process(&mut [&mut left, &mut right], &config, bpm);
        assert!(
            peak_in(&left, 1..FRAMES).0.abs_diff(TIME_SAMPLES) <= 2,
            "a free delay moved when the tempo did"
        );
    }
}

#[test]
fn a_tempo_change_while_it_runs_glides_rather_than_clicking() {
    use fontelle_types::NoteDivision;

    // A synced delay follows a tempo change, and it has to arrive there the
    // same way the time knob does — see `moving_the_time_while_it_runs_does_
    // not_click`, which this is the tempo-driven half of.
    let mut delay = Delay::new();
    delay.prepare(SR);
    const BLOCK: usize = 512;
    let tone = sine(200.0, FRAMES);

    let mut config = plain(400.0);
    config.sync = true;
    config.division = NoteDivision::Quarter;

    let mut worst = 0.0f32;
    let mut previous = 0.0;
    for block in 0..40 {
        let bpm = if block < 20 { 120.0 } else { 75.0 };
        let start = block * BLOCK;
        let mut left = tone[start..start + BLOCK].to_vec();
        let mut right = left.clone();
        delay.process(&mut [&mut left, &mut right], &config, bpm);
        for sample in left {
            worst = worst.max((sample - previous).abs());
            previous = sample;
        }
    }
    assert!(
        worst < 0.1,
        "the output stepped by {worst} on a tempo change"
    );
}
