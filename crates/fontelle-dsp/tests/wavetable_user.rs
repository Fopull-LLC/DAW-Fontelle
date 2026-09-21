//! A table somebody drew (`docs/flopsynth-next.md` §4.3), through the
//! same pyramid the bank's tables get: a drawn square is the bank's
//! `Square` within the mip levels — the same harmonics at the finest
//! level and the same band-limiting up the pyramid, so a drawn wave at the
//! top of the keyboard aliases no more than a recipe's.

use fontelle_dsp::{WAVETABLE_LEN, WAVETABLE_LEVELS, Wavetable, WavetableId, wavetables};

fn spectrum(points: &[f32], harmonics: usize) -> Vec<f32> {
    let n = points.len() as f32;
    (1..=harmonics)
        .map(|h| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in points.iter().enumerate() {
                let phase = std::f32::consts::TAU * h as f32 * i as f32 / n;
                re += s * phase.cos();
                im -= s * phase.sin();
            }
            (re * re + im * im).sqrt() * 2.0 / n
        })
        .collect()
}

/// One cycle of `table` at `level`, read the way the oscillator reads it.
fn cycle(table: &Wavetable, level: usize) -> Vec<f32> {
    (0..WAVETABLE_LEN)
        .map(|i| table.read(0.0, i as f32 / WAVETABLE_LEN as f32, level))
        .collect()
}

#[test]
fn a_drawn_square_is_the_banks_square_within_the_mip_levels() {
    let drawn: Vec<f32> = (0..WAVETABLE_LEN)
        .map(|i| if i < WAVETABLE_LEN / 2 { 1.0 } else { -1.0 })
        .collect();
    let user = Wavetable::from_samples(&drawn, 1);
    let bank = wavetables().get(WavetableId::Square);
    for level in 0..WAVETABLE_LEVELS {
        let (a, b) = (
            spectrum(&cycle(&user, level), 32),
            spectrum(&cycle(&bank, level), 32),
        );
        for (h, (x, y)) in a.iter().zip(&b).enumerate() {
            // Within a decibel wherever either has anything, and both
            // silent where the level has taken the harmonic out.
            let loud = x.max(*y) > 0.02;
            if loud {
                let db = 20.0 * (x / y.max(1e-9)).log10();
                assert!(
                    db.abs() < 1.0,
                    "level {level} harmonic {}: {db:.2} dB",
                    h + 1
                );
            } else {
                assert!(
                    *x < 0.02 && *y < 0.02,
                    "level {level} harmonic {}: {x} {y}",
                    h + 1
                );
            }
        }
    }
}
