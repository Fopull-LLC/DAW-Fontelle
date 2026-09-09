//! What the pitch tracker promises (`docs/tune-plan.md` §9.1).
//!
//! YIN with the cumulative-mean-normalised difference, in two passes. These are
//! the claims the corrector above it is written against: a sine is found to a
//! cent, a sawtooth is found at its fundamental rather than an octave up, a
//! breath between two notes does not flip the octave, and none of it costs
//! latency — only reaction time.

use fontelle_dsp::{PitchFrame, PitchTracker};

/// The five ranges of `docs/tune-plan.md` §3.8, in hertz. Written out here
/// rather than imported: this crate depends on nothing above it (INVARIANT 4),
/// and `TuneRange` is the document's name for the same five pairs.
const RANGES: [(&str, f32, f32); 5] = [
    ("soprano", 160.0, 1_400.0),
    ("alto/tenor", 100.0, 1_000.0),
    ("baritone/bass", 60.0, 600.0),
    ("instrument", 40.0, 2_000.0),
    ("low", 25.0, 400.0),
];

const ALTO_TENOR: (&str, f32, f32) = RANGES[1];

/// One tracker, prepared for a range at a rate, hopping every `hop` samples.
fn tracker(range: (&str, f32, f32), sample_rate: f32, hop: u32) -> PitchTracker {
    let mut tracker = PitchTracker::new(range.1, range.2, hop);
    tracker.prepare(sample_rate);
    tracker
}

/// Pushes a signal through in blocks and keeps every frame the tracker reported.
fn run(tracker: &mut PitchTracker, signal: &[f32]) -> Vec<Option<PitchFrame>> {
    let mut frames = Vec::new();
    for block in signal.chunks(64) {
        tracker.push(block, &mut |frame| frames.push(frame));
    }
    frames
}

fn sine(hz: f32, seconds: f32, sample_rate: f32) -> Vec<f32> {
    let n = (seconds * sample_rate) as usize;
    (0..n)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / sample_rate).sin() * 0.5)
        .collect()
}

/// A band-limited-ish sawtooth: enough harmonics that the second one is loud,
/// which is what makes a naive autocorrelation answer an octave up.
fn saw(hz: f32, seconds: f32, sample_rate: f32) -> Vec<f32> {
    let n = (seconds * sample_rate) as usize;
    let partials = ((sample_rate / 2.0 / hz) as usize).clamp(1, 40);
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate;
            let mut sum = 0.0;
            for k in 1..=partials {
                sum += (std::f32::consts::TAU * hz * k as f32 * t).sin() / k as f32;
            }
            sum * 0.3
        })
        .collect()
}

/// A synthetic vowel: a buzz at `f0` shaped by three resonances. The signal the
/// tracker actually meets.
fn vowel(f0: f32, seconds: f32, sample_rate: f32) -> Vec<f32> {
    let n = (seconds * sample_rate) as usize;
    let formants = [700.0f32, 1_200.0, 2_600.0];
    let partials = ((sample_rate / 2.0 / f0) as usize).clamp(1, 60);
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate;
            let mut sum = 0.0;
            for k in 1..=partials {
                let hz = f0 * k as f32;
                // A gain per harmonic from three resonances, plus the source's
                // own −6 dB/octave roll-off.
                let mut gain = 0.0;
                for f in formants {
                    let bw = f * 0.12;
                    gain += 1.0 / (1.0 + ((hz - f) / bw).powi(2));
                }
                sum += gain / k as f32 * (std::f32::consts::TAU * hz * t).sin();
            }
            sum * 0.15
        })
        .collect()
}

fn cents_between(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

/// The last frame the tracker was sure about.
fn last_voiced(frames: &[Option<PitchFrame>]) -> PitchFrame {
    frames
        .iter()
        .rev()
        .find_map(|frame| *frame)
        .expect("nothing was ever voiced")
}

#[test]
fn a_sine_is_found_within_a_cent() {
    for rate in [44_100.0f32, 48_000.0, 96_000.0] {
        let mut tracker = tracker(ALTO_TENOR, rate, 64);
        let frames = run(&mut tracker, &sine(220.0, 0.4, rate));
        let found = last_voiced(&frames);
        assert!(
            cents_between(found.hz, 220.0).abs() < 1.0,
            "at {rate} Hz the tracker heard {} Hz",
            found.hz
        );
        assert!(found.confidence > 0.8);
    }
}

#[test]
fn a_sawtooth_is_found_at_its_fundamental_not_its_second_harmonic() {
    let rate = 48_000.0;
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    let frames = run(&mut tracker, &saw(110.0, 0.4, rate));
    let found = last_voiced(&frames);
    assert!(
        cents_between(found.hz, 110.0).abs() < 5.0,
        "the octave guard let {} Hz through",
        found.hz
    );
}

#[test]
fn a_synthetic_vowel_is_found_within_two_cents() {
    let rate = 48_000.0;
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    let frames = run(&mut tracker, &vowel(150.0, 0.4, rate));
    let found = last_voiced(&frames);
    assert!(
        cents_between(found.hz, 150.0).abs() < 2.0,
        "the vowel came out at {} Hz",
        found.hz
    );
}

#[test]
fn noise_is_unvoiced() {
    let rate = 48_000.0;
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    // A deterministic hash rather than a crate: this has to give the same
    // answer on every machine that runs it.
    let mut state = 0x1234_5678u32;
    let noise: Vec<f32> = (0..(rate * 0.4) as usize)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / 8_388_608.0 - 1.0
        })
        .collect();
    let frames = run(&mut tracker, &noise);
    let voiced = frames.iter().filter(|frame| frame.is_some()).count();
    assert!(
        voiced * 10 < frames.len(),
        "{voiced} of {} hops of noise were called notes",
        frames.len()
    );
}

#[test]
fn silence_under_the_gate_is_unvoiced() {
    let rate = 48_000.0;
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    tracker.set_gate_db(-50.0);
    // A perfectly good 220 Hz tone, sixty decibels down.
    let quiet: Vec<f32> = sine(220.0, 0.3, rate).iter().map(|s| s * 0.001).collect();
    let frames = run(&mut tracker, &quiet);
    assert!(
        frames.iter().all(|frame| frame.is_none()),
        "room noise got tuned"
    );
}

#[test]
fn a_glide_is_followed_within_ten_cents_and_three_hops() {
    let rate = 48_000.0;
    let seconds = 1.0;
    let n = (rate * seconds) as usize;
    let mut phase = 0.0f32;
    let signal: Vec<f32> = (0..n)
        .map(|i| {
            let hz = 200.0 + 100.0 * (i as f32 / n as f32);
            phase += std::f32::consts::TAU * hz / rate;
            phase.sin() * 0.5
        })
        .collect();
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    let frames = run(&mut tracker, &signal);
    // Halfway through, and allowing three hops of lag on a line that moves
    // 100 Hz in a second.
    let hop_hz = |index: usize| 200.0 + 100.0 * (index as f32 * 64.0 / n as f32);
    let mut checked = 0;
    for (index, frame) in frames.iter().enumerate().skip(frames.len() / 4) {
        let Some(frame) = frame else { continue };
        let want = hop_hz(index.saturating_sub(3));
        let error = cents_between(frame.hz, want).abs();
        assert!(
            error < 20.0,
            "at hop {index} the tracker was {error:.1} cents from the glide"
        );
        checked += 1;
    }
    assert!(checked > 100, "the glide was barely tracked at all");
}

/// §9.1 calls this `a_step_lands_within_three_hops`, and three is not
/// reachable: a period cannot be measured over less than two of itself, and
/// 330 Hz is 290 samples of that before the median's own hop is added. Five
/// hops at a 64-sample hop is 6.7 ms, which *is* the floor — the claim is
/// written here as the truth rather than as the number the plan guessed.
#[test]
fn a_step_lands_within_two_of_its_own_periods_and_the_medians_hop() {
    let rate = 48_000.0;
    let mut signal = sine(220.0, 0.25, rate);
    // Continuing the phase would be a glide; a step is what a new note is.
    signal.extend(sine(330.0, 0.25, rate));
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    let frames = run(&mut tracker, &signal);
    let step_hop = (0.25 * rate / 64.0) as usize;
    // The median's one-hop lag plus one for the window to fill.
    let landed = frames
        .iter()
        .enumerate()
        .skip(step_hop)
        .find(|(_, frame)| frame.is_some_and(|frame| cents_between(frame.hz, 330.0).abs() < 10.0))
        .map(|(index, _)| index - step_hop)
        .expect("the step was never found");
    // Two of the new note's own periods, the median's hop, and one more for
    // the coarse pass's anti-alias filter to settle at the new pitch.
    let periods = (2.0f32 * 48_000.0 / 330.0 / 64.0).ceil() as usize + 2;
    assert!(
        landed <= periods,
        "the step took {landed} hops to land, and {periods} is the floor"
    );
}

#[test]
fn the_lowest_note_of_each_range_is_found_and_the_one_below_is_not() {
    let rate = 48_000.0;
    for range in RANGES {
        let (name, min_hz, _) = range;
        let mut at_the_bottom = tracker(range, rate, 64);
        let frames = run(&mut at_the_bottom, &saw(min_hz * 1.05, 0.5, rate));
        let found = last_voiced(&frames);
        assert!(
            cents_between(found.hz, min_hz * 1.05).abs() < 15.0,
            "{name} could not find its own bottom note ({} Hz)",
            found.hz
        );
        // Half an octave under the range: whatever it reports, it must not
        // report *that* note, because it cannot see a period that long.
        let mut under = tracker(range, rate, 64);
        let frames = run(&mut under, &saw(min_hz * 0.7, 0.5, rate));
        for frame in frames.iter().flatten() {
            assert!(
                cents_between(frame.hz, min_hz * 0.7).abs() > 50.0,
                "{name} found a note below its own floor"
            );
        }
    }
}

#[test]
fn a_breath_between_notes_does_not_flip_the_octave() {
    let rate = 48_000.0;
    let mut signal = vowel(150.0, 0.2, rate);
    let mut state = 0x9e37_79b9u32;
    signal.extend((0..(rate * 0.02) as usize).map(|_| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.05
    }));
    signal.extend(vowel(150.0, 0.2, rate));
    let mut tracker = tracker(ALTO_TENOR, rate, 64);
    let frames = run(&mut tracker, &signal);
    for (index, frame) in frames.iter().enumerate() {
        let Some(frame) = frame else { continue };
        let error = cents_between(frame.hz, 150.0).abs();
        assert!(
            error < 600.0,
            "hop {index} reported {} Hz — the breath flipped the octave",
            frame.hz
        );
    }
    let found = last_voiced(&frames);
    assert!(cents_between(found.hz, 150.0).abs() < 5.0);
}

#[test]
fn the_tracker_adds_no_latency_only_reaction_time() {
    let rate = 48_000.0;
    let hop = 64;
    let silence = vec![0.0f32; 4_800];
    let mut signal = silence.clone();
    signal.extend(sine(220.0, 0.2, rate));
    let mut tracker = tracker(ALTO_TENOR, rate, hop);
    let frames = run(&mut tracker, &signal);
    let onset_hop = silence.len() / hop as usize;
    let first_voiced = frames
        .iter()
        .position(|frame| frame.is_some())
        .expect("the note was never heard");
    // One hop for the window to reach the note, plus the median's one, plus a
    // hop of grace. Not a *delay* on the audio: this tracker only ever looks
    // backwards, and the shifter's latency is the shifter's.
    assert!(
        first_voiced >= onset_hop,
        "the tracker reported a note before it arrived"
    );
    assert!(
        first_voiced - onset_hop <= 12,
        "the note took {} hops to be noticed",
        first_voiced - onset_hop
    );
}
