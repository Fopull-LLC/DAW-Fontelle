//! What oversampling is for (`docs/flopsynth-next.md` §4.1): the warps, the
//! sync and the ladder's `tanh` all make harmonics past Nyquist, and without
//! it those fold back down as tones that are not the note's.
//!
//! `tests/synth_osc.rs` holds the mip pyramid's promise for a plain table
//! read (`a_saw_high_up_has_no_energy_below_its_fundamental`). This file asks
//! the same question of the four things a pyramid cannot help — FM at two
//! cycles, sync at 8×, quantise at four steps, the ladder driven 24 dB — and
//! holds two things about each: that **4× is at least 20 dB cleaner than
//! Off**, and that **Off is no worse than it was the day this file was
//! written** (the figures below, measured 2026-09-19), so the change is
//! opt-in and nobody's preset moved.
//!
//! "Alias" here is everything between 30 Hz and 20 kHz that is not within
//! four bins of a multiple of the note, as a ratio to what is.

use fontelle_dsp::{
    FilterModel, Oversampling, SynthFilter, SynthFilterSettings, SynthOsc, SynthSource, SynthState,
    WarpMode, WavetableBank, WavetableId, fft_in_place,
};

const SR: f32 = 48_000.0;
const FRAMES: usize = 16_384;

/// A bin-centred note near `target`, so the harmonics land on the grid.
fn on_grid(target: f32) -> f32 {
    (target * FRAMES as f32 / SR).round() * SR / FRAMES as f32
}

/// Off-grid energy over on-grid energy, in dB, between 30 Hz and 20 kHz.
fn alias_db(samples: &[f32], f0: f32) -> f32 {
    let n = samples.len();
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let t = std::f32::consts::TAU * i as f32 / n as f32;
            let w =
                0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; n];
    fft_in_place(&mut re, &mut im);
    let bin_hz = SR / n as f32;
    let f0_bins = f0 / bin_hz;
    let top = (20_000.0 / bin_hz) as usize;
    let (mut signal, mut alias) = (0.0f64, 0.0f64);
    for k in 8..top {
        let power = f64::from(re[k] * re[k] + im[k] * im[k]);
        let nearest = (k as f32 / f0_bins).round() * f0_bins;
        if (k as f32 - nearest).abs() <= 4.0 {
            signal += power;
        } else {
            alias += power;
        }
    }
    10.0 * (alias / signal.max(1e-30)).log10() as f32
}

/// `FRAMES` of `carrier`'s left channel at `f0`, fed by `modulator` at the
/// same note when there is one — the way the voice feeds a later layer to an
/// earlier one.
fn render(carrier: &SynthOsc, modulator: Option<&SynthOsc>, f0: f32) -> Vec<f32> {
    let bank = WavetableBank::new();
    let table = |osc: &SynthOsc| match osc.source {
        SynthSource::Table(id) => Some(bank.get(id)),
        _ => None,
    };
    let carrier_table = table(carrier);
    let modulator_table = modulator.and_then(table);
    let mut carrier_state = SynthState::new();
    carrier_state.reset(carrier, 1);
    let mut modulator_state = SynthState::new();
    if let Some(modulator) = modulator {
        modulator_state.reset(modulator, 2);
    }
    (0..FRAMES)
        .map(|_| {
            let fed = modulator.map_or(0.0, |modulator| {
                modulator_state
                    .next_sample(modulator, modulator_table.as_deref(), f0, SR, 0.0)
                    .0
            });
            carrier_state
                .next_sample(carrier, carrier_table.as_deref(), f0, SR, fed)
                .0
        })
        .collect()
}

fn table(id: WavetableId, quality: Oversampling) -> SynthOsc {
    SynthOsc {
        source: SynthSource::Table(id),
        quality,
        ..SynthOsc::default()
    }
}

/// Off must read no worse than `was`, and 4× at least `by` dB better than
/// Off.
fn hold(name: &str, off: f32, over: f32, was: f32, by: f32) {
    assert!(
        off <= was + 0.5,
        "{name}: Off reads {off:.1} dB of alias, was {was:.1} — the change is not opt-in"
    );
    assert!(
        over <= off - by,
        "{name}: 4\u{d7} reads {over:.1} dB against {off:.1} at Off — not {by} dB cleaner"
    );
}

/// Both halves of the pair at the same quality, the way the patch-wide
/// setting puts them: a modulator left at Off feeds the carrier its own
/// alias and one sample per frame, and the carrier at 4× can do nothing
/// about either (measured −32 dB against −29, 2026-09-19 — the reason the
/// setting is the patch's by default and the oscillator's by exception).
#[test]
fn fm_at_two_cycles_on_c7_is_cleaner_oversampled() {
    let f0 = on_grid(2_093.0);
    let pair = |quality| {
        let carrier = SynthOsc {
            warp: WarpMode::Fm,
            warp_amount: 1.0,
            modulator: Some(1),
            ..table(WavetableId::Sine, quality)
        };
        let modulator = table(WavetableId::Sine, quality);
        alias_db(&render(&carrier, Some(&modulator), f0), f0)
    };
    let off = pair(Oversampling::Off);
    let over = pair(Oversampling::X4);
    hold("FM", off, over, -29.0, 20.0);
}

#[test]
fn sync_at_eight_times_on_c7_is_cleaner_oversampled() {
    let f0 = on_grid(2_093.0);
    let case = |quality| SynthOsc {
        warp: WarpMode::Sync,
        warp_amount: 1.0,
        ..table(WavetableId::Saw, quality)
    };
    let off = alias_db(&render(&case(Oversampling::Off), None, f0), f0);
    let over = alias_db(&render(&case(Oversampling::X4), None, f0), f0);
    hold("Sync", off, over, -18.5, 20.0);
}

/// Ten decibels, not the twenty the plan asked of every case. A quantised
/// phase is a stair, and a stair's edges are steps: the part of their
/// spectrum past the *oversampled* Nyquist folds at the render and no
/// decimator can reach it afterwards, so a stair gains about six decibels
/// per doubling of the rate however the table is read (measured: −18.4,
/// −25.1, −31.2 dB at Off, 2×, 4×, the same on a sine as on a saw). The
/// other ten would take a band-limited step at each edge, which is warp
/// work (`docs/flopsynth-next.md` §4.3), not oversampling.
#[test]
fn quantise_at_four_steps_on_c6_is_cleaner_oversampled() {
    let f0 = on_grid(1_046.5);
    // 64 steps at the bottom of the knob, 2 at the top: four is 60/62 up.
    let case = |quality| SynthOsc {
        warp: WarpMode::Quantise,
        warp_amount: 60.0 / 62.0,
        ..table(WavetableId::Saw, quality)
    };
    let off = alias_db(&render(&case(Oversampling::Off), None, f0), f0);
    let over = alias_db(&render(&case(Oversampling::X4), None, f0), f0);
    hold("Quantise", off, over, -18.4, 10.0);
}

#[test]
fn two_times_is_between_off_and_four() {
    let f0 = on_grid(2_093.0);
    let case = |quality| SynthOsc {
        warp: WarpMode::Sync,
        warp_amount: 1.0,
        ..table(WavetableId::Saw, quality)
    };
    let off = alias_db(&render(&case(Oversampling::Off), None, f0), f0);
    let twice = alias_db(&render(&case(Oversampling::X2), None, f0), f0);
    let four = alias_db(&render(&case(Oversampling::X4), None, f0), f0);
    assert!(twice < off - 6.0, "2\u{d7} {twice:.1} against Off {off:.1}");
    assert!(four < twice, "4\u{d7} {four:.1} against 2\u{d7} {twice:.1}");
}

/// An oscillator at Off is **the same samples** it was: the oversampled path
/// is a different branch, not a filter everything now goes through.
#[test]
fn off_is_bit_for_bit_the_plain_oscillator() {
    let f0 = on_grid(440.0);
    let plain = SynthOsc {
        warp: WarpMode::Sync,
        warp_amount: 0.5,
        ..table(WavetableId::Saw, Oversampling::Off)
    };
    let a = render(&plain, None, f0);
    let b = render(&plain, None, f0);
    assert_eq!(a, b);
    // And an oversampled one is not — it is a different render, delayed by
    // the kernel's half length, so the two cannot be the same samples.
    let over = SynthOsc {
        quality: Oversampling::X4,
        ..plain
    };
    assert_ne!(render(&over, None, f0), a);
}

/// The ladder at drive 24 dB with its loop saturating: the oscillator's
/// oversampling is applied around the filter's nonlinearity too, from the
/// filter's own setting.
#[test]
fn the_ladder_driven_hard_is_cleaner_oversampled() {
    let f0 = on_grid(1_046.5);
    // A pure sine in, so what comes out off the grid is the filter's alone.
    let sine: Vec<f32> = (0..FRAMES)
        .map(|i| (std::f32::consts::TAU * f0 * i as f32 / SR).sin())
        .collect();
    // Resonance short of self-oscillation, so what is measured is the
    // aliasing and not the ring — a ladder oscillating at 12 kHz is a tone
    // off the note's grid at either rate.
    let case = |oversampling| SynthFilterSettings {
        model: FilterModel::Ladder,
        cutoff_hz: 12_000.0,
        resonance: 0.7,
        drive: 1.0,
        character: 1.0,
        oversampling,
        ..SynthFilterSettings::default()
    };
    let run = |settings: SynthFilterSettings| -> Vec<f32> {
        let mut filter = SynthFilter::new();
        sine.iter()
            .map(|s| filter.process(*s, &settings, SR))
            .collect()
    };
    let off = alias_db(&run(case(Oversampling::Off)), f0);
    let over = alias_db(&run(case(Oversampling::X4)), f0);
    hold("Ladder", off, over, -36.2, 20.0);
}

/// A clean filter has no nonlinearity but the drive; at drive 0 it is
/// linear, and running it oversampled would be cost for nothing. The
/// oversampling is the ladder's.
#[test]
fn a_clean_filter_ignores_the_oversampling() {
    let f0 = on_grid(440.0);
    let sine: Vec<f32> = (0..2_048)
        .map(|i| (std::f32::consts::TAU * f0 * i as f32 / SR).sin())
        .collect();
    let run = |oversampling| -> Vec<f32> {
        let mut filter = SynthFilter::new();
        let settings = SynthFilterSettings {
            cutoff_hz: 2_000.0,
            oversampling,
            ..SynthFilterSettings::default()
        };
        sine.iter()
            .map(|s| filter.process(*s, &settings, SR))
            .collect()
    };
    assert_eq!(run(Oversampling::Off), run(Oversampling::X4));
}
