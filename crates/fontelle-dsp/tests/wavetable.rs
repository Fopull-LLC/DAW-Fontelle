//! The wavetable bank (`docs/flopsynth-plan.md` §3.2).
//!
//! **No files.** Every table is generated from a spectrum recipe, so Flopsynth
//! works on a fresh install and a preset can never break a link — the drum
//! machine's position taken to the oscillators.
//!
//! What these tests hold is that the generation is *band-limited and honest*:
//! a mip level carries the harmonics it claims and cannot carry any above
//! them, frames interpolate linearly, the vowel table really has vowels in it,
//! and the whole bank fits in the memory §3.2 budgets for it.

use fontelle_dsp::{
    WAVETABLE_LEN, WAVETABLE_LEVELS, WavetableBank, WavetableId, wavetable_level_for,
};

const SR: f32 = 48_000.0;

/// A Hann-windowed DFT at one frequency, in linear magnitude.
///
/// A windowed *sum* rather than a sparse probe: the drum-kit memory's lesson
/// is that reading one bin of a rectangular transform reports leakage from its
/// neighbours as signal.
fn energy_at(samples: &[f32], cycles: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, s) in samples.iter().enumerate() {
        let t = i as f32 / n;
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * t).cos();
        let phase = std::f32::consts::TAU * cycles * t;
        re += s * window * phase.cos();
        im -= s * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

#[test]
fn every_table_builds_and_stays_inside_full_scale() {
    let bank = WavetableBank::new();
    for id in WavetableId::ALL {
        let table = bank.get(id);
        assert!(table.frame_count() >= 1, "{} has no frames", id.label());
        for frame in 0..table.frame_count() {
            for level in 0..WAVETABLE_LEVELS {
                let samples = table.level(frame, level);
                assert_eq!(
                    samples.len(),
                    WAVETABLE_LEN >> level,
                    "{}: frame {frame} level {level} is the wrong length",
                    id.label()
                );
                let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
                assert!(
                    peak <= 1.0,
                    "{}: frame {frame} level {level} peaks at {peak}, which clips \
                     before anything else in the voice gets a say",
                    id.label()
                );
                assert!(
                    samples.iter().all(|s| s.is_finite()),
                    "{}: frame {frame} level {level} has a non-finite sample in it",
                    id.label()
                );
            }
        }
    }
}

/// The mip pyramid's whole claim. A level of length `L` **cannot represent**
/// anything above harmonic `L/2` — that is arithmetic — so what has to be
/// tested is the other half: that it still carries the harmonics below that
/// faithfully, rather than having been decimated by throwing samples away.
///
/// A saw is the hardest case: every harmonic present, falling only as 1/n.
#[test]
fn a_coarser_level_keeps_the_harmonics_it_can_still_hold() {
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::Saw);
    let fine = table.level(0, 0);
    for level in 1..WAVETABLE_LEVELS {
        let samples = table.level(0, level);
        let limit = (samples.len() / 2) as f32;
        // Well inside the level's own band. A Hann window's mainlobe is a
        // couple of bins wide, so a harmonic near a short level's Nyquist
        // reads its own reflection as well as itself — that is the ruler
        // bending, not the table.
        for harmonic in [1.0f32, 2.0, 3.0] {
            if harmonic > limit / 4.0 {
                continue;
            }
            let coarse = energy_at(samples, harmonic);
            let reference = energy_at(fine, harmonic);
            assert!(
                (coarse - reference).abs() <= reference * 0.15 + 1e-4,
                "level {level} has {coarse} at harmonic {harmonic} where level 0 has \
                 {reference}: a mip level is a band-limit, not a decimation"
            );
        }
    }
}

/// And the arithmetic half, said out loud: the pyramid has to reach far enough
/// down that even the top of the keyboard has a level with no partial above
/// Nyquist. Seven levels would stop at sixteen harmonics and alias at A8.
#[test]
fn the_level_chosen_for_a_pitch_keeps_every_harmonic_under_nyquist() {
    for f0 in [27.5f32, 110.0, 440.0, 1760.0, 7040.0] {
        let level = wavetable_level_for(f0, SR);
        let top_harmonic = ((WAVETABLE_LEN >> level) / 2) as f32;
        assert!(
            f0 * top_harmonic <= SR * 0.5,
            "at {f0} Hz level {level} would put harmonic {top_harmonic} at {} Hz, \
             above Nyquist",
            f0 * top_harmonic
        );
        // And not needlessly dull: the finer level next to it would have
        // aliased, which is what makes this the *smallest* usable k.
        if level > 0 {
            let finer = ((WAVETABLE_LEN >> (level - 1)) / 2) as f32;
            assert!(
                f0 * finer > SR * 0.45,
                "at {f0} Hz level {level} throws away harmonics it could have kept"
            );
        }
    }
}

#[test]
fn frame_interpolation_at_a_half_is_the_mean_of_its_neighbours() {
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::AnalogMorph);
    assert!(
        table.frame_count() >= 2,
        "a morphing table with one frame is not morphing"
    );
    let width = 1.0 / (table.frame_count() - 1) as f32;
    for phase in [0.0f32, 0.13, 0.37, 0.5, 0.81] {
        let a = table.read(0.0, phase, 0);
        let b = table.read(width, phase, 0);
        let middle = table.read(width * 0.5, phase, 0);
        assert!(
            (middle - (a + b) * 0.5).abs() < 1e-4,
            "halfway between two frames must be their mean: got {middle}, expected {}",
            (a + b) * 0.5
        );
    }
}

/// The Vowel table is what makes "choir ahhs" reachable without a sample, and
/// the claim is spectral rather than aesthetic: frame 0 is an /a/, so its
/// energy peaks where /a/'s first two formants are and not between them.
#[test]
fn the_vowel_tables_first_frame_has_the_formants_of_an_a() {
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::Vowel);
    let samples = table.level(0, 0);
    // One cycle read as a 100 Hz note, so harmonic n is n × 100 Hz and the two
    // formants are far enough apart to be separate partials.
    let energy_near = |hz: f32| {
        let centre = hz / 100.0;
        ((centre - 1.5).max(1.0) as usize..=(centre + 1.5) as usize)
            .map(|h| energy_at(samples, h as f32))
            .fold(0.0f32, f32::max)
    };
    // /a/: F1 730 Hz, F2 1090 Hz (Peterson & Barney, the standard table).
    let at_formants = energy_near(730.0).min(energy_near(1090.0));
    let between = energy_near(2600.0);
    assert!(
        at_formants > between * 3.0,
        "frame 0 of the Vowel table has to sound like an /a/: {at_formants} at the \
         formants against {between} away from them"
    );
}

/// The chip tables are first-class, not a novelty (`fopull-llc-and-ty`), so
/// the NES triangle is the hardware's: a 4-bit DAC counting 0..15 and back,
/// which is sixteen distinct levels and thirty-two steps.
#[test]
fn the_nes_triangle_is_a_four_bit_staircase() {
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::NesTriangle);
    let samples = table.level(0, 0);
    let mut levels: Vec<i32> = samples
        .iter()
        .map(|s| (s * 10_000.0).round() as i32)
        .collect();
    levels.sort_unstable();
    levels.dedup();
    assert_eq!(
        levels.len(),
        16,
        "the NES triangle's DAC is four bits; got {} distinct levels",
        levels.len()
    );
}

#[test]
fn a_second_get_returns_the_same_table_rather_than_building_it_again() {
    let bank = WavetableBank::new();
    let first = bank.get(WavetableId::Saw);
    let second = bank.get(WavetableId::Saw);
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "the bank builds lazily and caches; a second get that rebuilt would \
         allocate in whatever asked for it"
    );
}

#[test]
fn the_whole_bank_fits_in_its_memory_budget() {
    let bank = WavetableBank::new();
    let bytes: usize = WavetableId::ALL
        .iter()
        .map(|id| bank.get(*id).bytes())
        .sum();
    let megabytes = bytes as f32 / (1024.0 * 1024.0);
    assert!(
        megabytes < 24.0,
        "§3.2 budgets the whole bank at under 24 MB; it is {megabytes:.1} MB"
    );
}

#[test]
fn every_table_has_a_label_and_a_family_and_no_two_share_a_name() {
    let mut names: Vec<&str> = WavetableId::ALL.iter().map(|id| id.label()).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names.len(),
        total,
        "a table's name is INVARIANT 7's the moment a patch saves naming it"
    );
    assert!(total >= 36, "§3.2's first-ship bank is thirty-six tables");
    for id in WavetableId::ALL {
        assert!(!id.label().is_empty());
        assert!(!id.family().is_empty());
        // Serde by name, so a saved patch is readable and a variant reordered
        // in code does not silently become a different table.
        let text = serde_json::to_string(&id).expect("a table id serialises");
        let back: WavetableId = serde_json::from_str(&text).expect("and reads back");
        assert_eq!(back, id);
    }
}

/// Two tables that sound the same are one table with two names — the drum
/// kits' lesson (`drum-kit-axes`), applied before the bank grows.
#[test]
fn no_two_tables_are_the_same_wave() {
    let bank = WavetableBank::new();
    let shapes: Vec<(WavetableId, Vec<f32>)> = WavetableId::ALL
        .iter()
        .map(|id| {
            let table = bank.get(*id);
            // The middle frame, which is where a morphing table differs most
            // from the static one it starts at.
            let position = 0.5;
            (
                *id,
                (0..256)
                    .map(|i| table.read(position, i as f32 / 256.0, 0))
                    .collect(),
            )
        })
        .collect();
    // **One pair is allowed to be the same wave**, and saying so is more
    // honest than pretending otherwise: the NES pulse channel at 50 % duty
    // *is* a square, and that is the fact the Chip family exists to record.
    // It earns its row by being where somebody looking for it will look, not
    // by sounding different from `Square` — which it does not, and cannot.
    // Every other pair has to be its own sound.
    let allowed = |a: WavetableId, b: WavetableId| {
        matches!(
            (a, b),
            (WavetableId::Square, WavetableId::NesPulse50)
                | (WavetableId::NesPulse50, WavetableId::Square)
        )
    };
    for (i, (a_id, a)) in shapes.iter().enumerate() {
        for (b_id, b) in &shapes[i + 1..] {
            if allowed(*a_id, *b_id) {
                continue;
            }
            let difference: f32 =
                a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32;
            assert!(
                difference > 0.02,
                "{} and {} are the same wave (mean difference {difference})",
                a_id.label(),
                b_id.label()
            );
        }
    }
}

/// Reading a table costs two frame lookups, and most of the time one of them
/// is worth nothing (`docs/flopsynth-plan.md` §10's first named lever).
///
/// A position knob that is not being swept sits **on** a frame, which is the
/// ordinary case: every preset that does not modulate position, and every one
/// that does between its moves. The blend there is exactly zero, so the second
/// read contributes exactly nothing and skipping it is not an approximation.
///
/// The two halves this holds: on a frame, one read gives the same answer as
/// two; between frames, both are still read.
#[test]
fn a_position_that_sits_on_a_frame_is_that_frame() {
    let bank = fontelle_dsp::WavetableBank::new();
    // A morphing table, so there is something to be between.
    let table = bank.get(fontelle_dsp::WavetableId::AnalogMorph);
    assert!(table.frame_count() > 1, "this test needs a morphing table");

    let frames = table.frame_count();
    for frame in 0..frames {
        let position = frame as f32 / (frames - 1) as f32;
        for step in 0..16 {
            let phase = step as f32 / 16.0;
            let read = table.read(position, phase, 0);
            let direct = table.read_frame(frame, phase, 0);
            assert!(
                (read - direct).abs() < 1e-6,
                "frame {frame} at phase {phase}: {read} against {direct}"
            );
        }
    }
}

#[test]
fn a_position_between_frames_is_still_a_blend_of_both() {
    let bank = fontelle_dsp::WavetableBank::new();
    let table = bank.get(fontelle_dsp::WavetableId::AnalogMorph);
    let frames = table.frame_count();
    let step = 1.0 / (frames - 1) as f32;
    // Halfway between the first two frames, where the two differ most.
    let mut apart = 0.0f32;
    for i in 0..64 {
        let phase = i as f32 / 64.0;
        let a = table.read_frame(0, phase, 0);
        let b = table.read_frame(1, phase, 0);
        let middle = table.read(step * 0.5, phase, 0);
        apart = apart.max((a - b).abs());
        assert!(
            (middle - (a + b) * 0.5).abs() < 1e-5,
            "phase {phase}: {middle} is not halfway between {a} and {b}"
        );
    }
    assert!(apart > 0.01, "the two frames should differ somewhere");
}

/// One harmonic of a single-cycle table, read exactly: a level-0 frame is one
/// period of the wave, so a rectangular transform lands each partial in its
/// own bin with nothing leaking in from the ones beside it — which a Hann
/// window would do, and which is the wrong tool for finding a *null*.
fn harmonic(samples: &[f32], h: usize) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, s) in samples.iter().enumerate() {
        let phase = std::f32::consts::TAU * h as f32 * i as f32 / n;
        re += s * phase.cos();
        im -= s * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

#[test]
fn the_struck_table_is_a_string_hit_an_eighth_of_the_way_along() {
    // A hammer landing a fraction β along a string cannot excite the modes
    // that have a node there: at β = 1/8 — where a piano's hammers land —
    // the eighth partial and its multiples are silent, and the ones either
    // side of them fall away in a lobe. That comb is what a piano's spectrum
    // has and a saw's does not, and it is the reason every synth piano
    // built on a saw sounds like a synth.
    //
    // The position knob is the **hammer's hardness**, not where it lands: a
    // felt hammer is soft, and how soft depends on how hard it was thrown —
    // a pianissimo excites little above the lobe and a fortissimo rings
    // partials in the twenties. That is the axis velocity has to move, so
    // it is the axis the knob is.
    let bank = WavetableBank::new();
    let table = bank.get(WavetableId::Struck);
    let (soft, mid, hard) = (table.level(0, 0), table.level(6, 0), table.level(12, 0));
    assert_eq!(table.frame_count(), 13);

    // The comb is the string's, so it is there at every hardness.
    for (name, frame) in [("soft", soft), ("mid", mid), ("hard", hard)] {
        let one = harmonic(frame, 1);
        for h in [8, 16, 24] {
            assert!(
                harmonic(frame, h) < one * 0.02,
                "{name}: partial {h} should be a node when the hammer lands at an eighth: \
                 {} against a fundamental of {one}",
                harmonic(frame, h)
            );
        }
    }
    // The roll-off between the nodes, from **both** sides. A struck string's
    // displacement goes as `sin(πhβ)/h²`, so the second partial is six or
    // seven decibels under the first and the third about eleven: present,
    // and not level with it. The upper bound is the claim that matters —
    // at `/h` the second partial comes out level with the fundamental, and
    // that spectrum is a clavinet's rather than a piano's.
    let one = harmonic(mid, 1);
    let (two, three) = (harmonic(mid, 2) / one, harmonic(mid, 3) / one);
    assert!(
        (0.35..0.60).contains(&two),
        "the second partial is {two:.3} of the fundamental"
    );
    assert!(
        (0.18..0.45).contains(&three),
        "the third partial is {three:.3} of the fundamental"
    );

    // The felt: a soft hammer's partials above the lobe go down far faster
    // than a saw's do — the twentieth is at least ten times further under
    // the fundamental than in a saw — and a hard hammer's do not.
    let saw = bank.get(WavetableId::Saw);
    let saw_level = saw.level(0, 0);
    let saw_ratio = harmonic(saw_level, 20) / harmonic(saw_level, 1);
    let soft_ratio = harmonic(soft, 20) / harmonic(soft, 1);
    let hard_ratio = harmonic(hard, 20) / harmonic(hard, 1);
    assert!(
        soft_ratio < saw_ratio / 10.0,
        "a soft hammer's twentieth partial is {soft_ratio:.4} of its fundamental; a saw's is {saw_ratio:.4}"
    );
    assert!(
        hard_ratio > soft_ratio * 10.0,
        "a hard hammer should ring the twentieth: hard {hard_ratio:.4}, soft {soft_ratio:.4}"
    );
}
