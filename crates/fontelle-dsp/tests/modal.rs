//! The modal resonator bank — what makes a drum sound like a drum.
//!
//! A struck membrane does not ring at one frequency. It rings at a set of
//! **inharmonic** modes, each with its own decay, and the ratios between them
//! are what the ear reads as "a drum of this shape" rather than "a sine with
//! an envelope on it". One sine with an exponential envelope is a beep; the
//! same envelope over five modes at 1.00, 1.59, 2.14, 2.30 and 2.92 is a tom.
//!
//! What is measured here is that the bank rings at the frequencies it was
//! given, that each mode decays at its own rate, and that it is stable — a
//! resonator that grows instead of decaying takes a mix out with it.

use fontelle_dsp::{MAX_MODES, ModalBank, ModalMode};

const RATE: f32 = 48_000.0;

/// The strongest frequency in a signal, by a windowed DFT over a band.
fn peak_hz(signal: &[f32], from: f32, to: f32) -> f32 {
    let n = signal.len();
    let windowed: Vec<f32> = signal
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n - 1).max(1) as f32).cos();
            s * w
        })
        .collect();
    let mut best = (0.0f32, from);
    let mut hz = from;
    while hz <= to {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        let w = std::f32::consts::TAU * hz / RATE;
        for (i, s) in windowed.iter().enumerate() {
            let a = w * i as f32;
            re += s * a.cos();
            im -= s * a.sin();
        }
        let mag = (re * re + im * im).sqrt();
        if mag > best.0 {
            best = (mag, hz);
        }
        hz += 0.5;
    }
    best.1
}

fn rms(signal: &[f32]) -> f32 {
    if signal.is_empty() {
        return 0.0;
    }
    (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt()
}

/// One mode, struck once, rings at the frequency it was given.
#[test]
fn a_single_mode_rings_at_its_own_frequency() {
    let mut bank = ModalBank::new();
    bank.set(
        &[ModalMode {
            ratio: 1.0,
            decay: 1.0,
            gain: 1.0,
        }],
        220.0,
        0.5,
        RATE,
    );
    let out: Vec<f32> = (0..4800)
        .map(|i| bank.next_sample(if i == 0 { 1.0 } else { 0.0 }))
        .collect();
    let found = peak_hz(&out, 150.0, 320.0);
    assert!(
        (found - 220.0).abs() < 6.0,
        "a mode tuned to 220 Hz rang at {found:.1}"
    );
}

/// Five modes at a membrane's ratios put energy at every one of them —
/// which is the whole difference between a drum and a beep.
#[test]
fn a_membranes_modes_are_all_present_and_inharmonic() {
    let ratios = [1.0f32, 1.593, 2.135, 2.295, 2.917];
    let modes: Vec<ModalMode> = ratios
        .iter()
        .map(|ratio| ModalMode {
            ratio: *ratio,
            decay: 1.0,
            gain: 1.0,
        })
        .collect();
    let mut bank = ModalBank::new();
    bank.set(&modes, 120.0, 0.6, RATE);
    let out: Vec<f32> = (0..9600)
        .map(|i| bank.next_sample(if i == 0 { 1.0 } else { 0.0 }))
        .collect();
    for ratio in ratios {
        let want = 120.0 * ratio;
        let found = peak_hz(&out, want - 12.0, want + 12.0);
        assert!(
            (found - want).abs() < 5.0,
            "the mode at {want:.0} Hz rang at {found:.1}"
        );
    }
}

/// A mode with a shorter `decay` dies sooner. Modes that all died together
/// would be one filter with extra steps.
#[test]
fn a_mode_with_a_shorter_decay_dies_sooner() {
    let render = |decay: f32| -> Vec<f32> {
        let mut bank = ModalBank::new();
        bank.set(
            &[ModalMode {
                ratio: 1.0,
                decay,
                gain: 1.0,
            }],
            200.0,
            0.5,
            RATE,
        );
        (0..24000)
            .map(|i| bank.next_sample(if i == 0 { 1.0 } else { 0.0 }))
            .collect()
    };
    let short = render(0.25);
    let long = render(1.0);
    let tail = 12000..24000;
    assert!(
        rms(&long[tail.clone()]) > rms(&short[tail]) * 4.0,
        "the two decays are the same length"
    );
}

/// **Stable.** A resonator whose pole slips outside the unit circle grows
/// without bound and takes the mix with it. Every combination this can be
/// asked for has to stay finite, including the ones a hand-edited project
/// could produce.
#[test]
fn every_mode_is_stable_however_it_is_asked_for() {
    for (base, decay, ratio) in [
        (20.0f32, 20.0f32, 1.0f32),
        (18_000.0, 20.0, 3.0),
        (440.0, 0.0, 1.0),
        (440.0, -1.0, 1.0),
        (440.0, 1.0, 0.0),
        (440.0, 1.0, 400.0),
        (f32::NAN, 1.0, 1.0),
        (440.0, f32::NAN, 1.0),
        (440.0, 1.0, f32::NAN),
    ] {
        let mut bank = ModalBank::new();
        bank.set(
            &[ModalMode {
                ratio,
                decay,
                gain: 1.0,
            }],
            base,
            1.0,
            RATE,
        );
        let mut peak = 0.0f32;
        for i in 0..48_000 {
            let out = bank.next_sample(if i == 0 { 1.0 } else { 0.0 });
            assert!(
                out.is_finite(),
                "({base}, {decay}, {ratio}) went non-finite"
            );
            peak = peak.max(out.abs());
        }
        assert!(peak < 100.0, "({base}, {decay}, {ratio}) grew to {peak}");
    }
}

/// A bank takes at most [`MAX_MODES`] and quietly ignores the rest, rather
/// than panicking on a project file that asked for more.
#[test]
fn a_bank_holds_the_modes_it_can_and_no_more() {
    let many: Vec<ModalMode> = (0..MAX_MODES + 6)
        .map(|i| ModalMode {
            ratio: 1.0 + i as f32 * 0.4,
            decay: 1.0,
            gain: 1.0,
        })
        .collect();
    let mut bank = ModalBank::new();
    bank.set(&many, 150.0, 0.5, RATE);
    let out: Vec<f32> = (0..4800)
        .map(|i| bank.next_sample(if i == 0 { 1.0 } else { 0.0 }))
        .collect();
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(rms(&out) > 0.0, "it made no sound at all");
}

/// An untouched bank is silent: voices come out of a pool.
#[test]
fn a_bank_nobody_set_is_silent() {
    let mut bank = ModalBank::new();
    for _ in 0..1000 {
        assert_eq!(bank.next_sample(1.0), 0.0);
    }
}
