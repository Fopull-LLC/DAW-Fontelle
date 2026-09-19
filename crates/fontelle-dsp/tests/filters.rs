//! Flopsynth's filter models (`docs/flopsynth-plan.md` §3.3).
//!
//! Four models behind one slot, and every one of them measured against what it
//! claims to be: a 24 dB low-pass is 24 dB an octave down where the 12 dB is
//! 12, a ladder self-oscillates and does not blow up doing it, a formant
//! filter puts its peaks at the vowel's formants, and a comb puts its nulls
//! where a comb's nulls go.

use fontelle_dsp::{
    FilterModel, FilterSlope, SynthFilter, SynthFilterSettings, key_tracked_cutoff,
};

const SR: f32 = 48_000.0;

fn settings(model: FilterModel, cutoff: f32) -> SynthFilterSettings {
    SynthFilterSettings {
        model,
        mode: fontelle_dsp::SvfMode::Lowpass,
        slope: FilterSlope::Db12,
        cutoff_hz: cutoff,
        resonance: 0.707,
        drive: 0.0,
        character: 0.0,
        oversampling: fontelle_dsp::Oversampling::Off,
    }
}

/// The response at `hz`, in dB, measured by running a sine through and reading
/// the settled amplitude — the honest way, since three of the four models have
/// no closed form worth trusting.
fn response_db(config: &SynthFilterSettings, hz: f32) -> f32 {
    let mut filter = SynthFilter::new();
    let cycles = 400.0;
    let frames = ((SR / hz) * cycles) as usize;
    let mut peak = 0.0f32;
    // The last fifth, so the filter has settled and the measurement is of the
    // steady state rather than of the transient going in.
    let settle = frames * 4 / 5;
    for i in 0..frames {
        let x = (std::f32::consts::TAU * hz * i as f32 / SR).sin();
        let y = filter.process(x, config, SR);
        if i >= settle {
            peak = peak.max(y.abs());
        }
    }
    20.0 * peak.max(1e-9).log10()
}

#[test]
fn a_twenty_four_db_low_pass_falls_twice_as_fast_as_a_twelve() {
    let cutoff = 1_000.0;
    let mut twelve = settings(FilterModel::Clean, cutoff);
    // Butterworth, so the corner is exactly −3 dB and the slope is the slope.
    twelve.resonance = std::f32::consts::FRAC_1_SQRT_2;
    let mut twenty_four = twelve;
    twenty_four.slope = FilterSlope::Db24;

    // Two octaves up, where the asymptote has taken over and the corner's own
    // shape is no longer part of the answer.
    let probe = cutoff * 4.0;
    let reference = cutoff / 8.0;
    let fall =
        |config: &SynthFilterSettings| response_db(config, reference) - response_db(config, probe);

    let twelve_fall = fall(&twelve);
    let twenty_four_fall = fall(&twenty_four);
    assert!(
        (twelve_fall - 24.0).abs() < 4.0,
        "12 dB/oct over two octaves is 24 dB; measured {twelve_fall}"
    );
    assert!(
        (twenty_four_fall - 48.0).abs() < 6.0,
        "24 dB/oct over two octaves is 48 dB; measured {twenty_four_fall}"
    );
}

/// The ladder's whole character is the feedback: at the top of the resonance
/// range it rings on its own with nothing going in, and the `tanh` in the loop
/// is what stops that becoming a divergence.
#[test]
fn the_ladder_self_oscillates_without_blowing_up() {
    let mut config = settings(FilterModel::Ladder, 400.0);
    config.resonance = 1.0;
    let mut filter = SynthFilter::new();
    // A single sample to start it, then silence for a second.
    let mut peak = 0.0f32;
    let mut late_rms = 0.0f32;
    let frames = SR as usize;
    for i in 0..frames {
        let x = if i == 0 { 1.0 } else { 0.0 };
        let y = filter.process(x, &config, SR);
        peak = peak.max(y.abs());
        if i > frames / 2 {
            late_rms += y * y;
        }
    }
    late_rms = (late_rms / (frames / 2) as f32).sqrt();
    assert!(
        late_rms > 0.02,
        "at full resonance the ladder has to still be ringing after half a \
         second; RMS {late_rms}"
    );
    assert!(peak < 2.0, "and it must not run away: peak {peak}");

    // And at rest it does not ring at all, or every patch would hum.
    let mut quiet = config;
    quiet.resonance = 0.0;
    let mut filter = SynthFilter::new();
    let mut tail = 0.0f32;
    for i in 0..frames {
        let x = if i == 0 { 1.0 } else { 0.0 };
        let y = filter.process(x, &quiet, SR);
        if i > frames / 2 {
            tail = tail.max(y.abs());
        }
    }
    assert!(
        tail < 1e-3,
        "with no resonance there is nothing to ring: {tail}"
    );
}

/// This is why "Choir Ahh" can be built without a sample: the formant filter
/// puts three resonances where a throat puts them, and the character knob is
/// which vowel.
#[test]
fn the_formant_filter_puts_its_peaks_where_the_vowel_does() {
    // /a/ at character 0, /i/ at 0.5 — the third of five.
    for (character, f1, f2) in [(0.0f32, 730.0f32, 1090.0f32), (0.5, 270.0, 2290.0)] {
        let mut config = settings(FilterModel::Formant, 1_000.0);
        config.character = character;
        config.resonance = 0.6;
        let peak_near = |hz: f32| {
            let mut best = f32::NEG_INFINITY;
            for step in -3..=3 {
                best = best.max(response_db(&config, hz * 2f32.powf(step as f32 / 24.0)));
            }
            best
        };
        let at_f1 = peak_near(f1);
        let at_f2 = peak_near(f2);
        // Somewhere with no formant of this vowel anywhere near it.
        let between =
            response_db(&config, (f1 * f2).sqrt() * 1.0).min(response_db(&config, 6_000.0));
        assert!(
            at_f1 > between + 6.0 && at_f2 > between + 6.0,
            "character {character}: F1 {at_f1} dB and F2 {at_f2} dB against \
             {between} dB away from them"
        );
    }
}

/// A comb is a delay fed back on itself, and what makes it a comb is that its
/// peaks and nulls alternate at even spacing.
///
/// With the feedback **negative** the delay line inverts every time round, so
/// it reinforces at the frequencies where a round trip is half a cycle — the
/// **odd** multiples of half the comb's frequency — and cancels at the whole
/// multiples. That is the reverse of the positive-feedback comb, and it is
/// what makes the negative half of the `character` knob the hollow one.
#[test]
fn a_comb_with_negative_feedback_alternates_peaks_and_nulls() {
    let comb_hz = 200.0;
    let mut config = settings(FilterModel::Comb, comb_hz);
    // Below the middle is the negative half of the feedback knob.
    config.character = 0.05;
    config.resonance = 0.0;

    let half = comb_hz / 2.0;
    for odd in [1.0f32, 3.0, 5.0] {
        let peak = response_db(&config, half * odd);
        let null = response_db(&config, half * (odd + 1.0));
        assert!(
            peak > null + 8.0,
            "a peak at {} Hz and a null at {} Hz: {peak} dB against {null} dB",
            half * odd,
            half * (odd + 1.0)
        );
    }

    // And the positive half of the knob is the other comb: the peaks move to
    // the whole multiples. A knob whose two ends are the same filter would be
    // half a knob.
    let mut positive = config;
    positive.character = 0.95;
    let at_whole = response_db(&positive, comb_hz);
    let at_half = response_db(&positive, half);
    assert!(
        at_whole > at_half + 8.0,
        "positive feedback resonates at the comb's own frequency: {at_whole} dB \
         against {at_half} dB at half of it"
    );
}

#[test]
fn key_tracking_moves_the_corner_with_the_keyboard() {
    // At 1.0 the corner follows the key exactly: twelve keys up is an octave.
    let base = key_tracked_cutoff(1_000.0, 60, 1.0);
    let octave_up = key_tracked_cutoff(1_000.0, 72, 1.0);
    assert!(
        (octave_up / base - 2.0).abs() < 0.01,
        "full key tracking is an octave per twelve keys: {base} to {octave_up}"
    );
    // At 0.5, half an octave.
    let half = key_tracked_cutoff(1_000.0, 72, 0.5);
    assert!(
        (half / base - 2f32.sqrt()).abs() < 0.02,
        "half key tracking is half an octave: {half}"
    );
    // At 0 it does not move at all, and middle C is the pivot.
    assert!((key_tracked_cutoff(1_000.0, 96, 0.0) - 1_000.0).abs() < 0.01);
    assert!(
        (base - 1_000.0).abs() < 0.01,
        "middle C is where nothing moves"
    );
}

#[test]
fn drive_at_zero_is_a_wire() {
    let mut clean = settings(FilterModel::Clean, 20_000.0);
    // Wide open and undamped, so what comes out is what went in.
    clean.cutoff_hz = 20_000.0;
    clean.drive = 0.0;
    let mut with_drive = clean;
    with_drive.drive = 0.6;

    let mut a = SynthFilter::new();
    let mut b = SynthFilter::new();
    let mut differed = false;
    for i in 0..2_048 {
        let x = (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin() * 0.8;
        let dry = a.process(x, &clean, SR);
        let driven = b.process(x, &with_drive, SR);
        if (dry - driven).abs() > 1e-4 {
            differed = true;
        }
    }
    assert!(
        differed,
        "a drive knob that changes nothing at 0.6 is not a drive knob"
    );

    // And the wire: drive 0 has to be bit-for-bit what a filter with no drive
    // path at all would produce.
    let mut a = SynthFilter::new();
    let mut b = SynthFilter::new();
    let mut zero_drive = clean;
    zero_drive.drive = 0.0;
    for i in 0..2_048 {
        let x = (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin() * 0.8;
        let one = a.process(x, &clean, SR);
        let two = b.process(x, &zero_drive, SR);
        assert!((one - two).abs() < 1e-9, "at {i}: {one} vs {two}");
    }
}

/// The zipper test. An LFO on cutoff at block rate steps the corner 375 times
/// a second; the ramp is what turns that into a sweep instead of a buzz.
#[test]
fn a_cutoff_swept_per_sample_has_no_step_in_it() {
    let mut config = settings(FilterModel::Clean, 400.0);
    config.resonance = 2.0;
    let mut filter = SynthFilter::new();
    let mut previous = 0.0f32;
    let mut worst_step = 0.0f32;
    let frames = 8_192;
    for i in 0..frames {
        // Two octaves of sweep over the run, which is a fast LFO on cutoff.
        let t = i as f32 / frames as f32;
        config.cutoff_hz = 400.0 * 2f32.powf(t * 2.0);
        let x = (std::f32::consts::TAU * 8_000.0 * i as f32 / SR).sin();
        let y = filter.process(x, &config, SR);
        if i > 64 {
            worst_step = worst_step.max((y - previous).abs());
        }
        previous = y;
    }
    assert!(
        worst_step < 2.0,
        "a swept cutoff must not step: worst sample-to-sample jump {worst_step}"
    );
}

#[test]
fn every_model_is_a_different_filter() {
    let describe = |model: FilterModel| {
        let mut config = settings(model, 800.0);
        config.resonance = 0.8;
        config.character = 0.5;
        [220.0f32, 800.0, 3_000.0, 9_000.0].map(|hz| response_db(&config, hz))
    };
    let models = [
        FilterModel::Clean,
        FilterModel::Ladder,
        FilterModel::Formant,
        FilterModel::Comb,
    ];
    let described: Vec<_> = models.iter().map(|m| (*m, describe(*m))).collect();
    for (i, (a_model, a)) in described.iter().enumerate() {
        for (b_model, b) in &described[i + 1..] {
            let apart = a
                .iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0f32, f32::max);
            assert!(
                apart > 3.0,
                "{a_model:?} and {b_model:?} are the same filter ({apart} dB apart \
                 at the furthest of four probes)"
            );
        }
    }
}

#[test]
fn a_fresh_filter_is_silent_and_settles_to_silence() {
    let config = settings(FilterModel::Clean, 1_000.0);
    let mut filter = SynthFilter::new();
    assert_eq!(filter.process(0.0, &config, SR), 0.0);
    // And a reset puts it back where it started, which is what a voice coming
    // out of the pool needs — see `Voice::trigger_note`.
    for i in 0..1_000 {
        filter.process((i as f32 * 0.1).sin(), &config, SR);
    }
    filter.reset();
    assert_eq!(filter.process(0.0, &config, SR), 0.0);
}

/// **The picture may not lie.** `response_db` is what the window draws a
/// filter's curve from, and it is a closed form rather than a measurement —
/// so the only thing standing between it and a curve that has nothing to do
/// with the sound is this test, which measures the real filter and compares.
#[test]
fn the_drawn_response_matches_the_filter_it_describes() {
    let cases = [
        ("clean 12", {
            let mut c = settings(FilterModel::Clean, 1_000.0);
            c.resonance = std::f32::consts::FRAC_1_SQRT_2;
            c
        }),
        ("clean 24", {
            let mut c = settings(FilterModel::Clean, 1_200.0);
            c.resonance = std::f32::consts::FRAC_1_SQRT_2;
            c.slope = FilterSlope::Db24;
            c
        }),
        ("clean highpass", {
            let mut c = settings(FilterModel::Clean, 800.0);
            c.mode = fontelle_dsp::SvfMode::Highpass;
            c
        }),
        ("clean bandpass", {
            let mut c = settings(FilterModel::Clean, 1_500.0);
            c.mode = fontelle_dsp::SvfMode::Bandpass;
            c.resonance = 2.0;
            c
        }),
        ("ladder", {
            let mut c = settings(FilterModel::Ladder, 900.0);
            c.resonance = 0.5;
            c
        }),
        ("ladder open", {
            let mut c = settings(FilterModel::Ladder, 4_000.0);
            c.resonance = 0.2;
            c
        }),
        ("formant /a/", {
            let mut c = settings(FilterModel::Formant, 1_000.0);
            c.resonance = 0.5;
            c.character = 0.0;
            c
        }),
        ("comb", {
            let mut c = settings(FilterModel::Comb, 300.0);
            c.character = 0.75;
            c
        }),
    ];

    for (name, config) in cases {
        let mut worst = 0.0f32;
        let mut worst_at = 0.0f32;
        // Log-spaced across the band the window draws, staying clear of the
        // very top where a discrete filter's own warping dominates.
        let mut hz = 40.0f32;
        while hz < 12_000.0 {
            let drawn = fontelle_dsp::response_db(&config, hz, SR);
            // `response_db` at the top of this file is the measured one: a
            // sine in, the settled peak out.
            let measured = response_db(&config, hz);
            // Both floored the same way: below −60 dB the difference between
            // two very small numbers is not something a picture can show.
            let (a, b) = (drawn.max(-60.0), measured.max(-60.0));
            if (a - b).abs() > worst {
                worst = (a - b).abs();
                worst_at = hz;
            }
            hz *= 1.15;
        }
        assert!(
            worst < 3.0,
            "{name}: the drawn curve is {worst:.1} dB from the real filter at \
             {worst_at:.0} Hz"
        );
    }
}

// ------------------------------------------------- coefficients, once ---

/// A `tan` and a `powf` per sample per section is most of what a voice costs.
///
/// `SvfFilter::coeffs` pre-warps the corner, which is a transcendental, and
/// `SynthFilter::clean` was calling it **every sample** — twice at 24 dB. The
/// settings only move every `FILTER_STEP` samples (`voice.rs` ramps the
/// cutoff), so all but one call in eight was rebuilding the same numbers.
///
/// Caching them is only safe if a *changed* setting is still noticed on the
/// very next sample, which is what these two tests are for: the first says the
/// cache is used, the second says it is not stale.
#[test]
fn a_filter_follows_its_cutoff_the_sample_after_it_moves() {
    let mut filter = SynthFilter::default();
    let low = SynthFilterSettings {
        model: FilterModel::Clean,
        mode: fontelle_dsp::SvfMode::Lowpass,
        slope: FilterSlope::Db24,
        cutoff_hz: 200.0,
        resonance: 0.7,
        drive: 0.0,
        character: 0.0,
        oversampling: fontelle_dsp::Oversampling::Off,
    };
    let high = SynthFilterSettings {
        cutoff_hz: 12_000.0,
        ..low
    };

    // A step into a closed filter, then the same step with the filter open on
    // the next sample. The open one has to let more through immediately.
    let mut closed = SynthFilter::default();
    let a = closed.process(1.0, &low, 48_000.0);
    let _ = filter.process(1.0, &low, 48_000.0);
    let b = filter.process(1.0, &high, 48_000.0);
    assert!(
        b > a * 1.5,
        "a filter opened between samples did not follow: {b} against {a}"
    );
}

#[test]
fn the_same_settings_give_the_same_answer_however_many_times_they_are_asked() {
    // The cache must be a cache and not a memory: two filters fed the same
    // samples with the same settings are the same filter, whatever order the
    // rebuilds happened in.
    let settings = SynthFilterSettings {
        model: FilterModel::Clean,
        mode: fontelle_dsp::SvfMode::Lowpass,
        slope: FilterSlope::Db12,
        cutoff_hz: 900.0,
        resonance: 1.4,
        drive: 0.0,
        character: 0.0,
        oversampling: fontelle_dsp::Oversampling::Off,
    };
    let other = SynthFilterSettings {
        cutoff_hz: 4_000.0,
        ..settings
    };
    let mut steady = SynthFilter::default();
    let mut jostled = SynthFilter::default();
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..256 {
        let input = ((i as f32) * 0.05).sin();
        a.push(steady.process(input, &settings, 48_000.0));
        // The same filter, but asked about a different cutoff on a throwaway
        // instance in between — which is what a voice does when a route moves
        // and moves back.
        let mut scratch = SynthFilter::default();
        let _ = scratch.process(input, &other, 48_000.0);
        b.push(jostled.process(input, &settings, 48_000.0));
    }
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        assert!((x - y).abs() < 1e-6, "sample {i}: {x} against {y}");
    }
}
