//! Flopsynth's oscillator (`docs/flopsynth-plan.md` §3.1).
//!
//! One description covers A, B, C, the Sub and the Noise. What these tests
//! hold is that each of its parts is *the thing it is named after*: unison at
//! one costs nothing and changes nothing, a warp at zero is the unwarped
//! table, FM makes sidebands where FM makes sidebands, and a random start
//! phase is random between notes and repeatable within a test.

use fontelle_dsp::{
    FilterRoute, SynthOsc, SynthSource, SynthState, Unison, UnisonMode, UnisonSpread, WarpMode,
    WavetableBank, WavetableId,
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
        // A patch's own table or recording is resolved by `WavetableSet`,
        // which is `fontelle-core`'s; nothing here builds one. The sample and
        // the string have suites of their own.
        SynthSource::User(_)
        | SynthSource::Sample(_)
        | SynthSource::Spectral(_)
        | SynthSource::String
        | SynthSource::Noise => None,
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
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
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
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
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
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
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

    // The spectral warps are a wire on a table (`WarpMode::is_spectral`):
    // their own test is `fontelle-dsp/tests/synth_spectral.rs`. Remap with
    // no curve drawn is a wire too, by design; its test is below.
    let described: Vec<(WarpMode, (f32, f32))> = WarpMode::ALL
        .iter()
        .filter(|m| **m != WarpMode::Off && !m.is_spectral() && !m.reads_a_curve())
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

/// The spectral source's three warps have no partials to work on in a
/// table, and are a wire there at any amount rather than a surprise.
#[test]
fn the_spectral_warps_are_a_wire_on_a_table() {
    let plain = render(&osc(WavetableId::Saw), 220.0, 4_096);
    for mode in WarpMode::ALL.iter().filter(|m| m.is_spectral()) {
        let mut warped = osc(WavetableId::Saw);
        warped.warp = *mode;
        warped.warp_amount = 1.0;
        let samples = render(&warped, 220.0, 4_096);
        assert!(
            plain
                .iter()
                .zip(&samples)
                .all(|(x, y)| (x - y).abs() < 1e-5),
            "{mode:?} on a table is a wire"
        );
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
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
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
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
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
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
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
            mode: UnisonMode::Classic,
            spread: UnisonSpread::Power,
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
            mode: UnisonMode::Classic,
            spread: UnisonSpread::Power,
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

// --- The table warps of phase 4 (`docs/flopsynth-next.md` §4.3) ---------

/// The harmonic content of a note at 220 Hz: the energy at the first
/// sixteen harmonics, the fundamental first.
fn harmonics(osc: &SynthOsc) -> [f32; 16] {
    let samples = render(osc, 220.0, 8_192);
    std::array::from_fn(|n| energy_at(&samples, 220.0 * (n + 1) as f32))
}

/// A spectral centroid over the harmonic grid, in harmonic numbers.
fn centroid(h: &[f32; 16]) -> f32 {
    let total: f32 = h.iter().sum();
    h.iter()
        .enumerate()
        .map(|(n, e)| (n + 1) as f32 * e / total.max(1e-9))
        .sum()
}

#[test]
fn the_phase_distortion_warp_adds_harmonics_to_a_sine() {
    // Casio's: the phase runs fast to a knee and slow after it, which on
    // a sine folds it towards a resonant saw — odd harmonics first.
    let mut pd = osc(WavetableId::Sine);
    pd.warp = WarpMode::PhaseDistortion;
    pd.warp_amount = 0.8;
    let h = harmonics(&pd);
    let plain = harmonics(&osc(WavetableId::Sine));
    assert!(h[0] > plain[0] * 0.5, "the fundamental stays: {h:?}");
    assert!(
        h[2] > plain[2] * 20.0 && h[2] > h[0] * 0.05,
        "a third harmonic appears: {h:?}"
    );
    assert!(
        centroid(&h) > centroid(&plain) * 1.5,
        "and the sine is brighter for it: {:.2} from {:.2}",
        centroid(&h),
        centroid(&plain)
    );
}

#[test]
fn the_formant_warp_keeps_the_pitch_and_moves_the_centroid_up() {
    // The cycle is read faster and held at its end, so the note's period
    // — and its fundamental — stay and the spectrum moves up.
    let mut formant = osc(WavetableId::Saw);
    formant.warp = WarpMode::Formant;
    formant.warp_amount = 0.6;
    let samples = render(&formant, 220.0, 8_192);
    let f0 = energy_at(&samples, 220.0);
    let below = energy_at(&samples, 220.0 / 2.0);
    assert!(
        f0 > below * 20.0,
        "the pitch is the note's: {f0} against {below} an octave under"
    );
    let h = harmonics(&formant);
    let plain = harmonics(&osc(WavetableId::Saw));
    // The formant sits where the cycle is read to: harmonics three to
    // eight carry more of the note, against its fundamental, than a saw's.
    let upper = |h: &[f32; 16]| h[2..8].iter().sum::<f32>() / h[0].max(1e-9);
    assert!(
        upper(&h) > upper(&plain) * 1.3,
        "the upper harmonics grew against the fundamental: {:.2} from {:.2}",
        upper(&h),
        upper(&plain)
    );
}

#[test]
fn the_flip_warp_adds_even_harmonics_to_a_sine() {
    // The second half of the cycle inverted: a sine becomes a rectified
    // shape, which is even harmonics.
    let mut flip = osc(WavetableId::Sine);
    flip.warp = WarpMode::Flip;
    flip.warp_amount = 1.0;
    let h = harmonics(&flip);
    let plain = harmonics(&osc(WavetableId::Sine));
    assert!(
        h[1] > plain[1] * 20.0 && h[1] > h[0] * 0.2,
        "a second harmonic appears: {h:?}"
    );
    assert!(h[1] > h[2], "even before odd: {h:?}");
}

#[test]
fn the_asym_warp_bends_the_two_halves_apart() {
    // The first half of the cycle bent one way and the second the other:
    // on a sine that is a lopsided wave, which is even harmonics, and
    // unlike Bend — which bends the whole cycle one way — its second
    // harmonic outweighs its third.
    let mut asym = osc(WavetableId::Sine);
    asym.warp = WarpMode::Asym;
    asym.warp_amount = 0.8;
    let h = harmonics(&asym);
    let plain = harmonics(&osc(WavetableId::Sine));
    assert!(h[1] > plain[1] * 20.0, "a second harmonic appears: {h:?}");
    let mut bend = osc(WavetableId::Sine);
    bend.warp = WarpMode::Bend;
    bend.warp_amount = 0.8;
    let b = harmonics(&bend);
    assert!(
        (h[1] / h[2]) > (b[1] / b[2]),
        "asym is more even than bend: {:.2} against {:.2}",
        h[1] / h[2],
        b[1] / b[2]
    );
}

#[test]
fn the_fm_noise_warp_spreads_the_line_into_a_band() {
    // FM from noise rather than from a layer: the line at the note
    // becomes a band round it, energy off the harmonic grid.
    let mut noisy = osc(WavetableId::Sine);
    noisy.warp = WarpMode::FmNoise;
    noisy.warp_amount = 0.5;
    let samples = render(&noisy, 220.0, 8_192);
    let plain = render(&osc(WavetableId::Sine), 220.0, 8_192);
    let off_grid = |s: &[f32]| energy_at(s, 220.0 * 1.5) + energy_at(s, 220.0 * 0.6);
    assert!(
        off_grid(&samples) > off_grid(&plain) * 20.0,
        "energy between the harmonics: {} from {}",
        off_grid(&samples),
        off_grid(&plain)
    );
    // And the same noise twice: seeded from the note.
    let again = render(&noisy, 220.0, 8_192);
    assert_eq!(samples, again);
}

#[test]
fn the_remap_warp_reads_the_phase_through_a_drawn_curve() {
    // A straight-line curve is a wire; a curve that is Bend's power law at
    // full is Bend at full, sample for sample.
    let mut remap = osc(WavetableId::Saw);
    remap.warp = WarpMode::Remap;
    remap.warp_amount = 1.0;
    let plain = render(&osc(WavetableId::Saw), 220.0, 4_096);
    let through = render(&remap, 220.0, 4_096);
    assert!(
        plain
            .iter()
            .zip(&through)
            .all(|(x, y)| (x - y).abs() < 2e-3),
        "the identity curve is a wire"
    );
    let mut bend = osc(WavetableId::Saw);
    bend.warp = WarpMode::Bend;
    bend.warp_amount = 1.0;
    let bent = render(&bend, 220.0, 4_096);
    // Bend at full is `t^4`; the curve's points are that law.
    remap.remap = std::array::from_fn(|i| {
        let t = i as f32 / (fontelle_dsp::REMAP_POINTS - 1) as f32;
        t.powi(4)
    });
    let curved = render(&remap, 220.0, 4_096);
    let error = bent
        .iter()
        .zip(&curved)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(error < 0.05, "the curve plays as bend does: worst {error}");
    // Half the amount is halfway between the curve and the line.
    remap.warp_amount = 0.5;
    let half = render(&remap, 220.0, 4_096);
    let toward = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>();
    assert!(
        toward(&half, &plain) < toward(&curved, &plain)
            && toward(&half, &curved) < toward(&plain, &curved)
    );
    // The curve is absent from the file while it is the line.
    let text = serde_json::to_string(&osc(WavetableId::Saw)).unwrap();
    assert!(!text.contains("remap"), "{text}");
    let text = serde_json::to_string(&remap).unwrap();
    assert!(text.contains("remap"));
    let back: SynthOsc = serde_json::from_str(&text).unwrap();
    assert_eq!(back, remap);
}

// --- Unison modes and spreads (`docs/flopsynth-next.md` §4.3) -----------

use fontelle_dsp::{unison_offset_cents, unison_offset_cents_in};

/// The line at `cents` from `f0` in a stack against the same in a single
/// voice: how much a stack put there.
fn stack_line(unison: Unison, cents: f32) -> (f32, f32) {
    let f0 = 220.0;
    let mut wide = osc(WavetableId::Sine);
    wide.unison = unison;
    let stack = render(&wide, f0, 32_768);
    let mut single = wide;
    single.unison.voices = 1;
    let one = render(&single, f0, 32_768);
    let hz = f0 * 2f32.powf(cents / 1200.0);
    (energy_at(&stack, hz), energy_at(&one, hz))
}

#[test]
fn the_octave_and_fifth_modes_put_the_side_voices_an_interval_up() {
    for (mode, cents) in [(UnisonMode::Octave, 1200.0), (UnisonMode::Fifth, 700.0)] {
        let unison = Unison {
            voices: 3,
            detune_cents: 0.0,
            blend: 1.0,
            width: 0.0,
            mode,
            spread: UnisonSpread::Power,
        };
        let (with, without) = stack_line(unison, cents);
        assert!(
            with > without * 20.0,
            "{mode:?}: a voice at {cents} cents: {with} against {without}"
        );
        // And the centre is still the note.
        let (centre, _) = stack_line(unison, 0.0);
        assert!(
            centre > with * 0.5,
            "{mode:?} keeps the centre: {centre} against {with}"
        );
    }
}

#[test]
fn a_chord_mode_plays_the_chords_intervals() {
    // A major chord over four voices: the note, its third, its fifth and
    // its octave.
    let unison = Unison {
        voices: 4,
        detune_cents: 0.0,
        blend: 1.0,
        width: 0.0,
        mode: UnisonMode::Chord(0),
        spread: UnisonSpread::Power,
    };
    for cents in [400.0, 700.0, 1200.0] {
        let (with, without) = stack_line(unison, cents);
        assert!(
            with > without * 20.0,
            "major has a voice at {cents}: {with}"
        );
    }
    let (minor_third, _) = stack_line(unison, 300.0);
    let (major_third, _) = stack_line(unison, 400.0);
    assert!(
        minor_third < major_third * 0.2,
        "and none at the minor third"
    );
    // Minor: the third moves down.
    let mut minor = unison;
    minor.mode = UnisonMode::Chord(1);
    let (third, _) = stack_line(minor, 300.0);
    assert!(
        third > major_third * 0.5,
        "minor has its third at 300: {third}"
    );
    assert!(UnisonMode::CHORDS.len() >= 4, "a few chords to choose from");
    for (index, (name, intervals)) in UnisonMode::CHORDS.iter().enumerate() {
        assert!(!name.is_empty() && !intervals.is_empty(), "chord {index}");
        assert_eq!(intervals[0], 0, "{name} starts on the note");
    }
}

#[test]
fn the_spreads_place_the_inner_voices_by_three_laws() {
    // Seven voices, a hundred cents: the outermost pair is at the knob
    // under every law; the inner two pairs sit where the law says.
    let outer = |spread: UnisonSpread| unison_offset_cents(6, 7, 100.0, spread);
    for spread in UnisonSpread::ALL {
        assert!(
            (outer(spread) - 100.0).abs() < 1e-3,
            "{spread:?} ends at the knob"
        );
        assert_eq!(
            unison_offset_cents(0, 7, 100.0, spread),
            0.0,
            "{spread:?}: the centre"
        );
        // Pairs: odd indices go one way, even the other.
        assert!(unison_offset_cents(1, 7, 100.0, spread) < 0.0);
        assert!(unison_offset_cents(2, 7, 100.0, spread) > 0.0);
    }
    let inner = |spread: UnisonSpread| {
        (
            unison_offset_cents(2, 7, 100.0, spread),
            unison_offset_cents(4, 7, 100.0, spread),
        )
    };
    let (l1, l2) = inner(UnisonSpread::Linear);
    assert!(
        (l1 - 33.3).abs() < 0.5 && (l2 - 66.7).abs() < 0.5,
        "linear: {l1} {l2}"
    );
    let (p1, p2) = inner(UnisonSpread::Power);
    assert!(p1 > l1 && p2 > l2, "power fans out: {p1} {p2}");
    let (h1, h2) = inner(UnisonSpread::Harmonic);
    assert!(h1 < l1 && h2 < l2, "harmonic bunches in: {h1} {h2}");
    // Power is what every stack was, so a preset's stack is its stack.
    assert_eq!(UnisonSpread::default(), UnisonSpread::Power);
    assert_eq!(UnisonMode::default(), UnisonMode::Classic);
    // Wide: the same pairs, twice as far out — the knob's detune is the
    // stack's middle rather than its edge.
    assert!(
        (unison_offset_cents(6, 7, 100.0, UnisonSpread::Power) * 2.0
            - unison_offset_cents_in(UnisonMode::Wide, 6, 7, 100.0, UnisonSpread::Power))
        .abs()
            < 1e-3
    );
    // The mode and the spread are absent from the file at their defaults.
    let text = serde_json::to_string(&Unison::default()).unwrap();
    assert!(!text.contains("mode") && !text.contains("spread"), "{text}");
}

// --- Noise kinds (`docs/flopsynth-next.md` §4.3) -------------------------

use fontelle_dsp::NoiseKind;

/// The spectral slope of a noise in dB an octave, over four octaves, on
/// bands of twenty-four bins each — a bin of noise is a random variable.
fn noise_slope(kind: NoiseKind, colour: f32, seed: u32) -> f32 {
    let config = SynthOsc {
        source: SynthSource::Noise,
        noise: kind,
        noise_colour: colour,
        ..SynthOsc::default()
    };
    let samples = render_seeded(&config, 440.0, 65_536, seed);
    let band = |centre: f32| {
        let count = 24;
        (0..count)
            .map(|i| energy_at(&samples, centre * (0.75 + 0.5 * i as f32 / count as f32)))
            .sum::<f32>()
            / count as f32
    };
    20.0 * (band(4_000.0).max(1e-9) / band(250.0).max(1e-9)).log10() / 4.0
}

#[test]
fn the_noise_kinds_have_their_own_slopes() {
    assert_eq!(NoiseKind::default(), NoiseKind::White);
    let white = noise_slope(NoiseKind::White, 0.0, 7);
    let pink = noise_slope(NoiseKind::Pink, 0.0, 7);
    let brown = noise_slope(NoiseKind::Brown, 0.0, 7);
    let blue = noise_slope(NoiseKind::Blue, 0.0, 7);
    assert!(white.abs() < 1.5, "white is flat: {white:.1} dB/oct");
    assert!(
        (pink - -3.0).abs() < 1.5,
        "pink falls three: {pink:.1} dB/oct"
    );
    assert!(
        (brown - -6.0).abs() < 1.5,
        "brown falls six: {brown:.1} dB/oct"
    );
    assert!(
        (blue - 3.0).abs() < 1.5,
        "blue rises three: {blue:.1} dB/oct"
    );
    // The colour knob is a tilt on top of whichever kind: pink at full
    // colour falls further than pink at none.
    let tilted = noise_slope(NoiseKind::Pink, 1.0, 7);
    assert!(
        tilted < pink - 2.0,
        "the tilt is on top: {tilted:.1} against {pink:.1}"
    );
    // And the file leaves the kind out at white.
    let text = serde_json::to_string(&SynthOsc::default()).unwrap();
    assert!(!text.contains("\"noise\""), "{text}");
}

#[test]
fn crackle_is_sparse_and_vinyl_has_its_rumble() {
    let render_kind = |kind: NoiseKind| {
        let config = SynthOsc {
            source: SynthSource::Noise,
            noise: kind,
            ..SynthOsc::default()
        };
        render_seeded(&config, 440.0, 48_000, 3)
    };
    let crackle = render_kind(NoiseKind::Crackle);
    let quiet = crackle.iter().filter(|s| s.abs() < 1e-4).count() as f32 / crackle.len() as f32;
    assert!(
        quiet > 0.9,
        "crackle is mostly nothing: {quiet:.2} of it is quiet"
    );
    let peak = crackle.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.3, "and the pops are pops: {peak}");
    let pops = crackle
        .windows(2)
        .filter(|w| w[0].abs() < 1e-4 && w[1].abs() > 0.05)
        .count();
    assert!(
        (20..=400).contains(&pops),
        "a few dozen pops a second: {pops}"
    );
    let vinyl = render_kind(NoiseKind::Vinyl);
    let rumble = |s: &[f32]| {
        (0..8)
            .map(|i| energy_at(s, 30.0 + 10.0 * i as f32))
            .sum::<f32>()
    };
    assert!(
        rumble(&vinyl) > rumble(&crackle) * 4.0,
        "vinyl rumbles under its crackle: {} against {}",
        rumble(&vinyl),
        rumble(&crackle)
    );
    let vinyl_pops = vinyl
        .windows(2)
        .filter(|w| (w[1] - w[0]).abs() > 0.1)
        .count();
    assert!(vinyl_pops > 10, "and crackles: {vinyl_pops}");
}
