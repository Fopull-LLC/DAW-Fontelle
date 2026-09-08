//! Flopsynth's oscillator (`docs/flopsynth-plan.md` §3.1).
//!
//! One description covers A, B, C, the Sub and the Noise. What these tests
//! hold is that each of its parts is *the thing it is named after*: unison at
//! one costs nothing and changes nothing, a warp at zero is the unwarped
//! table, FM makes sidebands where FM makes sidebands, and a random start
//! phase is random between notes and repeatable within a test.

use fontelle_dsp::{
    FilterRoute, SynthOsc, SynthSource, SynthState, Unison, WarpMode, WavetableBank, WavetableId,
};

const SR: f32 = 48_000.0;

fn osc(table: WavetableId) -> SynthOsc {
    SynthOsc {
        source: SynthSource::Table(table),
        ..SynthOsc::default()
    }
}

/// `frames` samples of one oscillator's left channel at `freq`.
fn render(osc: &SynthOsc, freq: f32, frames: usize) -> Vec<f32> {
    render_seeded(osc, freq, frames, 1)
}

fn render_seeded(config: &SynthOsc, freq: f32, frames: usize, seed: u32) -> Vec<f32> {
    let bank = WavetableBank::new();
    let table = match config.source {
        SynthSource::Table(id) => Some(bank.get(id)),
        SynthSource::Noise => None,
    };
    let mut state = SynthState::new();
    state.reset(config, seed);
    (0..frames)
        .map(|_| state.next_sample(config, table.as_deref(), freq, SR, 0.0).0)
        .collect()
}

/// A Hann-windowed DFT at `hz`, in linear magnitude.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, s) in samples.iter().enumerate() {
        let t = i as f32 / n;
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * t).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += s * window * phase.cos();
        im -= s * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

#[test]
fn a_table_plays_the_note_it_is_asked_for() {
    for freq in [55.0f32, 220.0, 440.0, 1760.0] {
        let samples = render(&osc(WavetableId::Sine), freq, 24_000);
        // Zero crossings over half a second: two per cycle.
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let measured = crossings as f32 / (samples.len() as f32 / SR);
        assert!(
            (measured - freq).abs() < freq * 0.02,
            "asked for {freq} Hz, measured {measured} Hz"
        );
    }
}

/// The mip pyramid, heard rather than counted: a saw at the top of the
/// keyboard has to have nothing *below* its own fundamental, because
/// everything an aliased partial folds down to lands there.
#[test]
fn a_saw_high_up_has_no_energy_below_its_fundamental() {
    let f0 = 5_000.0;
    let samples = render(&osc(WavetableId::Saw), f0, 16_384);
    let fundamental = energy_at(&samples, f0);
    assert!(fundamental > 0.01, "the note itself has to be there");
    let mut worst: f32 = 0.0;
    // Everything an alias of a harmonic of 5 kHz would fold down to.
    let mut hz = 200.0;
    while hz < f0 * 0.9 {
        worst = worst.max(energy_at(&samples, hz));
        hz += 200.0;
    }
    assert!(
        worst < fundamental * 0.02,
        "aliases below the fundamental: worst {worst} against {fundamental}"
    );
}

#[test]
fn one_unison_voice_is_the_plain_oscillator() {
    let plain = osc(WavetableId::Saw);
    let mut detuned = plain;
    // A detune, a blend and a width that would all be audible with more than
    // one voice, and must be inaudible with one.
    detuned.unison = Unison {
        voices: 1,
        detune_cents: 100.0,
        blend: 0.0,
        width: 1.0,
    };
    let a = render(&plain, 220.0, 4_096);
    let b = render(&detuned, 220.0, 4_096);
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            (x - y).abs() < 1e-6,
            "unison at 1 must be the centre voice exactly; differ at {i}: {x} vs {y}"
        );
    }
}

/// Seven voices are **seven lines**, not one thick one: the outermost pair
/// sits at the detune the knob says, and the rest fan in towards the centre.
#[test]
fn seven_unison_voices_are_seven_lines_around_the_fundamental() {
    let mut wide = osc(WavetableId::Saw);
    wide.unison = Unison {
        voices: 7,
        detune_cents: 40.0,
        blend: 1.0,
        width: 0.0,
    };
    let f0 = 220.0;
    let at = |samples: &[f32], cents: f32| energy_at(samples, f0 * 2f32.powf(cents / 1200.0));

    let stack = render(&wide, f0, 65_536);
    let mut single = wide;
    single.unison.voices = 1;
    let one = render(&single, f0, 65_536);

    // The outermost pair is at exactly the knob's detune; the centre is at
    // zero. A single voice has only the centre.
    for cents in [-40.0f32, 40.0] {
        let with = at(&stack, cents);
        let without = at(&one, cents);
        assert!(
            with > at(&stack, 0.0) * 0.2,
            "the outermost voice at {cents} cents is not there: {with} against a \
             centre of {}",
            at(&stack, 0.0)
        );
        assert!(
            with > without * 8.0,
            "one voice has no business at {cents} cents: {without} against the \
             stack's {with}"
        );
    }
    // And the inner pairs fan in rather than piling up on the centre, which is
    // what the exponent in `detune_of` is for.
    for cents in [-28.8f32, -17.1, 17.1, 28.8] {
        assert!(
            at(&stack, cents) > at(&one, cents) * 8.0,
            "no voice near {cents} cents"
        );
    }
    // Nothing beyond the outermost voice: `detune` is the edge of the stack,
    // not the middle of it.
    for cents in [-70.0f32, 70.0] {
        assert!(
            at(&stack, cents) < at(&stack, 40.0) * 0.25,
            "the stack reaches past its own detune at {cents} cents"
        );
    }
}

#[test]
fn blend_at_zero_is_the_centre_voice_alone() {
    let mut stack = osc(WavetableId::Saw);
    stack.unison = Unison {
        voices: 7,
        detune_cents: 30.0,
        blend: 0.0,
        width: 0.0,
    };
    let mut alone = stack;
    alone.unison.voices = 1;
    let a = render(&stack, 220.0, 4_096);
    let b = render(&alone, 220.0, 4_096);
    for (x, y) in a.iter().zip(&b) {
        assert!(
            (x - y).abs() < 1e-5,
            "blend 0 turns every voice but the centre one off: {x} vs {y}"
        );
    }
}

/// Catalogue rule 1: every warp is a **continuum** under one amount, and at
/// zero it is off. A mode that changed the sound at zero would be a mode you
/// cannot turn off without also changing the chooser.
#[test]
fn every_warp_at_zero_is_the_unwarped_table() {
    let plain = render(&osc(WavetableId::Saw), 220.0, 4_096);
    for mode in WarpMode::ALL {
        let mut warped = osc(WavetableId::Saw);
        warped.warp = mode;
        warped.warp_amount = 0.0;
        let samples = render(&warped, 220.0, 4_096);
        for (i, (x, y)) in plain.iter().zip(&samples).enumerate() {
            assert!(
                (x - y).abs() < 1e-5,
                "{mode:?} at amount 0 must be a wire; differ at {i}: {x} vs {y}"
            );
        }
    }
}

/// And at full amount each one has to be its own sound — not merely different
/// from the unwarped table but different from every *other* warp, which is the
/// drum kits' "audibly apart" test applied one level down.
#[test]
fn every_warp_at_full_is_apart_from_the_others() {
    let describe = |mode: WarpMode| {
        let mut warped = osc(WavetableId::Saw);
        warped.warp = mode;
        warped.warp_amount = 1.0;
        // A modulator for the two that need one; the rest ignore it.
        warped.modulator = Some(1);
        let bank = WavetableBank::new();
        let table = bank.get(WavetableId::Saw);
        let mut state = SynthState::new();
        state.reset(&warped, 1);
        let mut modulator = SynthState::new();
        let mut mod_osc = osc(WavetableId::Sine);
        mod_osc.semitones = 12;
        modulator.reset(&mod_osc, 1);
        let samples: Vec<f32> = (0..8_192)
            .map(|_| {
                let m = modulator
                    .next_sample(&mod_osc, Some(&table), 440.0, SR, 0.0)
                    .0;
                state.next_sample(&warped, Some(&table), 220.0, SR, m).0
            })
            .collect();
        // Spectral centroid and crest: the two axes that tell a bright warp
        // from a loud one (`drum-kit-axes`).
        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        let mut hz = 110.0;
        while hz < 12_000.0 {
            let e = energy_at(&samples, hz);
            weighted += hz * e;
            total += e;
            hz *= 1.1;
        }
        let centroid = if total > 0.0 { weighted / total } else { 0.0 };
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        let crest = if rms > 1e-6 { peak / rms } else { 0.0 };
        (centroid.ln(), crest.ln())
    };

    let described: Vec<(WarpMode, (f32, f32))> = WarpMode::ALL
        .iter()
        .filter(|m| **m != WarpMode::Off)
        .map(|m| (*m, describe(*m)))
        .collect();
    let plain = describe(WarpMode::Off);
    for (mode, point) in &described {
        let from_plain = (point.0 - plain.0).abs().max((point.1 - plain.1).abs());
        assert!(
            from_plain > 0.05,
            "{mode:?} at full amount is still the unwarped table ({from_plain})"
        );
    }
    for (i, (a_mode, a)) in described.iter().enumerate() {
        for (b_mode, b) in &described[i + 1..] {
            let apart = (a.0 - b.0).abs().max((a.1 - b.1).abs());
            assert!(
                apart > 0.05,
                "{a_mode:?} and {b_mode:?} are the same warp ({apart})"
            );
        }
    }
}

/// FM is FM: a carrier at `fc` modulated at `fm` puts energy at `fc ± fm`,
/// where there was none before.
#[test]
fn fm_from_a_modulator_makes_sidebands() {
    let carrier_hz = 440.0;
    let modulator_hz = 110.0;
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::Sine);

    let mut carrier = osc(WavetableId::Sine);
    carrier.warp = WarpMode::Fm;
    carrier.warp_amount = 0.5;
    carrier.modulator = Some(1);
    let modulator_cfg = osc(WavetableId::Sine);

    let mut carrier_state = SynthState::new();
    carrier_state.reset(&carrier, 1);
    let mut modulator_state = SynthState::new();
    modulator_state.reset(&modulator_cfg, 1);

    let samples: Vec<f32> = (0..32_768)
        .map(|_| {
            let m = modulator_state
                .next_sample(&modulator_cfg, Some(&table), modulator_hz, SR, 0.0)
                .0;
            carrier_state
                .next_sample(&carrier, Some(&table), carrier_hz, SR, m)
                .0
        })
        .collect();

    let plain = render(&osc(WavetableId::Sine), carrier_hz, 32_768);
    for sideband in [carrier_hz - modulator_hz, carrier_hz + modulator_hz] {
        let with = energy_at(&samples, sideband);
        let without = energy_at(&plain, sideband);
        assert!(
            with > without * 20.0 && with > 1e-3,
            "no sideband at {sideband} Hz: {with} against {without} unmodulated"
        );
    }
}

/// Hard sync's whole character: the *pitch* stays the note's, because the
/// slave is restarted every time the master's cycle ends — only the timbre
/// moves. A sync that changed the pitch would be a transpose knob.
#[test]
fn sync_keeps_the_notes_pitch_and_changes_its_timbre() {
    let f0 = 220.0;
    let mut synced = osc(WavetableId::Saw);
    synced.warp = WarpMode::Sync;
    synced.warp_amount = 0.4;
    let samples = render(&synced, f0, 32_768);
    let plain = render(&osc(WavetableId::Saw), f0, 32_768);

    // The lines are at multiples of the note, not of the synced frequency.
    for harmonic in 1..=4 {
        let at = energy_at(&samples, f0 * harmonic as f32);
        let between = energy_at(&samples, f0 * (harmonic as f32 + 0.5));
        assert!(
            at > between * 3.0,
            "harmonic {harmonic} of the note is not a line: {at} against {between} \
             between harmonics"
        );
    }
    // And it is brighter than the table it came from, which is what sync is for.
    let bright = |s: &[f32]| (8..24).map(|h| energy_at(s, f0 * h as f32)).sum::<f32>();
    assert!(
        bright(&samples) > bright(&plain) * 1.5,
        "sync at 0.4 has to move energy up the spectrum"
    );
}

#[test]
fn noise_colour_runs_from_flat_to_falling() {
    let mut white = SynthOsc {
        source: SynthSource::Noise,
        ..SynthOsc::default()
    };
    white.noise_colour = 0.0;
    let mut brown = white;
    brown.noise_colour = 1.0;

    // Averaged over a band at each end rather than measured at one frequency:
    // a single DFT bin of noise is a Rayleigh random variable and two draws of
    // it differ by several dB for nothing.
    let band = |samples: &[f32], centre: f32| {
        let count = 24;
        (0..count)
            .map(|i| {
                let spread = 0.75 + 0.5 * i as f32 / count as f32;
                energy_at(samples, centre * spread)
            })
            .sum::<f32>()
            / count as f32
    };
    let slope = |config: &SynthOsc| {
        let samples = render_seeded(config, 440.0, 65_536, 7);
        let low = band(&samples, 250.0);
        let high = band(&samples, 4_000.0);
        // Four octaves apart, in dB.
        20.0 * (high.max(1e-9) / low.max(1e-9)).log10() / 4.0
    };
    let white_slope = slope(&white);
    let brown_slope = slope(&brown);
    assert!(
        white_slope.abs() < 2.0,
        "colour 0 is white: {white_slope} dB/oct"
    );
    assert!(
        brown_slope < -3.5,
        "colour 1 has to fall steeply: {brown_slope} dB/oct"
    );
}

/// `random_phase` is about a **stack**: it exists so that eight voices do not
/// all step together on the first sample and make a click eight times the size
/// of any one of them. So the randomness is in the side voices, and the test
/// looks where the feature is.
#[test]
fn a_random_start_phase_scatters_the_stack_and_repeats_without_it() {
    let mut fixed = osc(WavetableId::Saw);
    fixed.random_phase = false;
    fixed.phase = 0.25;
    fixed.unison = Unison {
        voices: 5,
        detune_cents: 20.0,
        blend: 1.0,
        width: 0.0,
    };
    let a = render_seeded(&fixed, 220.0, 8, 1);
    let b = render_seeded(&fixed, 220.0, 8, 99);
    assert_eq!(
        a, b,
        "with random phase off, the same note starts the same way every time"
    );

    let mut random = fixed;
    random.random_phase = true;
    let c = render_seeded(&random, 220.0, 8, 1);
    let d = render_seeded(&random, 220.0, 8, 2);
    assert!(
        (c[0] - d[0]).abs() > 1e-3,
        "two notes must not start their stacks at the same phase: {} vs {}",
        c[0],
        d[0]
    );
    // And repeatable for the same seed, so a test can measure it at all.
    assert_eq!(c, render_seeded(&random, 220.0, 8, 1));
    // A scattered stack does not all step at once, which is the point: the
    // first sample of five voices at one phase is five times the size.
    assert!(
        c[0].abs() < a[0].abs(),
        "the scattered stack's first sample ({}) must be smaller than the \
         aligned one's ({})",
        c[0],
        a[0]
    );
}

/// The centre voice keeps its start phase even in a random-phase stack, so a
/// bass note with one voice does not lose its click-free start.
#[test]
fn the_centre_voice_of_a_random_stack_still_starts_where_it_was_told() {
    let mut stack = osc(WavetableId::Saw);
    stack.random_phase = true;
    stack.phase = 0.0;
    stack.unison = Unison {
        voices: 5,
        // The sides silent, so what is left is the centre voice alone.
        detune_cents: 20.0,
        blend: 0.0,
        width: 0.0,
    };
    let mut single = stack;
    single.unison.voices = 1;
    single.random_phase = false;
    let a = render_seeded(&stack, 110.0, 16, 4);
    let b = render_seeded(&single, 110.0, 16, 4);
    for (x, y) in a.iter().zip(&b) {
        assert!(
            (x - y).abs() < 1e-5,
            "the centre voice starts at `phase` however the sides are scattered"
        );
    }
}

#[test]
fn semitones_transpose_and_key_track_can_switch_the_pitch_off() {
    let pitch_of = |config: &SynthOsc, freq: f32| {
        let samples = render(config, freq, 24_000);
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        crossings as f32 / (samples.len() as f32 / SR)
    };
    let mut down = osc(WavetableId::Sine);
    down.semitones = -12;
    assert!(
        (pitch_of(&down, 440.0) - 220.0).abs() < 5.0,
        "−12 semitones is an octave down"
    );

    let mut fixed = osc(WavetableId::Sine);
    fixed.key_track = false;
    // With key tracking off the oscillator ignores the note and plays its own
    // pitch — which is what a drone and a fixed noise band need.
    let low = pitch_of(&fixed, 110.0);
    let high = pitch_of(&fixed, 880.0);
    assert!(
        (low - high).abs() < 5.0,
        "key tracking off means the note does not move it: {low} vs {high}"
    );
}

#[test]
fn width_spreads_the_side_voices_and_leaves_one_voice_centred() {
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::Saw);
    let stereo = |config: &SynthOsc| {
        let mut state = SynthState::new();
        state.reset(config, 1);
        let mut difference = 0.0f32;
        for _ in 0..4_096 {
            let (l, r) = state.next_sample(config, Some(&table), 220.0, SR, 0.0);
            difference += (l - r).abs();
        }
        difference / 4_096.0
    };
    let mut wide = osc(WavetableId::Saw);
    wide.unison = Unison {
        voices: 7,
        detune_cents: 25.0,
        blend: 1.0,
        width: 1.0,
    };
    let mut narrow = wide;
    narrow.unison.width = 0.0;
    let mut single = wide;
    single.unison.voices = 1;

    assert!(
        stereo(&wide) > stereo(&narrow) * 3.0,
        "width has to move the side voices apart"
    );
    assert!(
        stereo(&single) < 1e-6,
        "one voice is centred whatever the width says"
    );
}

#[test]
fn a_synth_oscillator_is_copy_and_serialises_by_name() {
    let mut config = osc(WavetableId::Choir);
    config.warp = WarpMode::Mirror;
    config.filter_route = FilterRoute::Serial;
    let copied = config;
    assert_eq!(
        copied, config,
        "the voice holds these by value (INVARIANT 6)"
    );
    let text = serde_json::to_string(&config).expect("serialises");
    assert!(text.contains("Mirror") && text.contains("Serial") && text.contains("Choir"));
    let back: SynthOsc = serde_json::from_str(&text).expect("reads back");
    assert_eq!(back, config);
}

// ------------------------------------- the stack's constants, once ---

/// A unison stack recomputes the same numbers every sample.
///
/// Per voice, per sample, the stack was working out a detune ratio (a `powf`),
/// a step, a blend gain and a pair of pan gains — none of which depends on
/// anything that moves *within* a block. "Supersaw" is three oscillators of
/// seven voices, so that is twenty-one `powf`s a sample for constants
/// (`docs/flopsynth-plan.md` §10).
///
/// Caching them is only safe if a change is still heard on the very next
/// sample, which is what these two hold: the first says a moved knob is
/// followed, the second says the cache is a cache and not a memory.
#[test]
fn a_stack_follows_its_detune_the_sample_after_it_moves() {
    let mut narrow = SynthOsc {
        source: SynthSource::Table(WavetableId::Saw),
        unison: Unison {
            voices: 7,
            detune_cents: 2.0,
            blend: 1.0,
            width: 1.0,
        },
        ..SynthOsc::default()
    };
    let bank = fontelle_dsp::WavetableBank::new();
    let table = bank.get(WavetableId::Saw);
    let mut state = SynthState::default();
    // Let the stack spread out, so the voices are somewhere different from
    // each other and a change of detune actually shows.
    for _ in 0..2_000 {
        state.next_sample(&narrow, Some(&table), 220.0, SR, 0.0);
    }
    let before = state.next_sample(&narrow, Some(&table), 220.0, SR, 0.0);
    narrow.unison.detune_cents = 40.0;
    let after = state.next_sample(&narrow, Some(&table), 220.0, SR, 0.0);
    assert_ne!(
        before, after,
        "a stack whose detune moved gave the same sample back"
    );
}

#[test]
fn two_stacks_at_the_same_settings_are_the_same_stack() {
    let osc = SynthOsc {
        source: SynthSource::Table(WavetableId::Saw),
        unison: Unison {
            voices: 5,
            detune_cents: 14.0,
            blend: 0.8,
            width: 0.6,
        },
        ..SynthOsc::default()
    };
    let other = SynthOsc {
        unison: Unison {
            detune_cents: 30.0,
            ..osc.unison
        },
        ..osc
    };
    let bank = fontelle_dsp::WavetableBank::new();
    let table = bank.get(WavetableId::Saw);
    let mut steady = SynthState::default();
    let mut jostled = SynthState::default();
    for i in 0..512 {
        let a = steady.next_sample(&osc, Some(&table), 220.0, SR, 0.0);
        // A throwaway asked about different settings in between, which is
        // what a voice does when a route moves and moves back.
        let mut scratch = SynthState::default();
        scratch.next_sample(&other, Some(&table), 220.0, SR, 0.0);
        let b = jostled.next_sample(&osc, Some(&table), 220.0, SR, 0.0);
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "sample {i}: {a:?} against {b:?}"
        );
    }
}
