//! The oversampling seam (`docs/flopsynth-next.md` §4.1): an oscillator or
//! a filter's nonlinearity runs at two or four times the session's rate and
//! comes back down through a polyphase FIR, so what folds over Nyquist is
//! taken out rather than heard.
//!
//! What these tests hold is that the kernel is the one the module says it is
//! (a Hamming-windowed sinc, so the numbers in the table are checked against
//! their formula rather than trusted), that a decimator passes the band and
//! removes what is above it, and that an interpolator makes N samples of the
//! same tone without images.

use fontelle_dsp::{
    Decimator, Interpolator, OVERSAMPLE_TAPS, Oversampling, fft_in_place, oversample_kernel,
};

const SR: f32 = 48_000.0;

#[test]
fn oversampling_is_off_by_default_and_names_its_factors() {
    assert_eq!(Oversampling::default(), Oversampling::Off);
    assert!(Oversampling::Off.is_off());
    assert_eq!(
        Oversampling::ALL,
        [Oversampling::Off, Oversampling::X2, Oversampling::X4]
    );
    assert_eq!(
        Oversampling::ALL.map(Oversampling::factor),
        [1, 2, 4],
        "the factor is how many samples are rendered for one"
    );
    assert_eq!(
        Oversampling::ALL.map(Oversampling::label),
        ["Off", "2\u{d7}", "4\u{d7}"]
    );
    // By name, like every other chooser on a patch.
    assert_eq!(serde_json::to_string(&Oversampling::X4).unwrap(), "\"X4\"");
    assert_eq!(
        serde_json::from_str::<Oversampling>("\"X2\"").unwrap(),
        Oversampling::X2
    );
}

/// The formula the table in the module was generated from.
fn formula(factor: Oversampling) -> Vec<f32> {
    let n = factor.factor();
    let len = OVERSAMPLE_TAPS * n;
    let cutoff = 0.45 / n as f64;
    let mut taps: Vec<f64> = (0..len)
        .map(|l| {
            let centred = l as f64 - (len - 1) as f64 / 2.0;
            let x = 2.0 * cutoff * centred;
            let sinc = if x.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let window = 0.54 - 0.46 * (std::f64::consts::TAU * l as f64 / (len - 1) as f64).cos();
            2.0 * cutoff * sinc * window
        })
        .collect();
    let sum: f64 = taps.iter().sum();
    for tap in &mut taps {
        *tap /= sum;
    }
    taps.into_iter().map(|t| t as f32).collect()
}

#[test]
fn the_kernel_is_a_hamming_windowed_sinc_summing_to_one() {
    for factor in [Oversampling::X2, Oversampling::X4] {
        let n = factor.factor();
        let branches = oversample_kernel(factor);
        assert_eq!(branches.len(), n, "one polyphase branch per sub-sample");
        let expected = formula(factor);
        for (l, tap) in expected.iter().enumerate() {
            // Branch `p` holds every tap `p + j·N`.
            let got = branches[l % n][l / n];
            assert!(
                (got - tap).abs() < 1e-6,
                "{factor:?} tap {l}: table {got}, formula {tap}"
            );
        }
        let sum: f32 = branches.iter().flatten().sum();
        assert!((sum - 1.0).abs() < 1e-5, "{factor:?} sums to {sum}");
    }
}

/// Magnitude at `hz` of `samples` at `rate`, Blackman-Harris windowed, in dB
/// relative to a full-scale sine.
fn level_db(samples: &[f32], hz: f32, rate: f32) -> f32 {
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
    let bin = (hz / rate * n as f32).round() as usize;
    let mut peak = 0.0f32;
    for k in bin.saturating_sub(3)..=(bin + 3).min(n / 2 - 1) {
        peak = peak.max((re[k] * re[k] + im[k] * im[k]).sqrt());
    }
    // The window's coherent gain, so a full-scale sine reads 0 dB.
    let full_scale = 0.35875 * n as f32 / 2.0;
    20.0 * (peak / full_scale).max(1e-12).log10()
}

/// A bin-centred frequency near `target` for an `n`-point transform at `rate`.
fn on_grid(target: f32, n: usize, rate: f32) -> f32 {
    (target * n as f32 / rate).round() * rate / n as f32
}

#[test]
fn a_decimator_passes_the_band_and_removes_what_is_above_it() {
    const FRAMES: usize = 8_192;
    for factor in [Oversampling::X2, Oversampling::X4] {
        let n = factor.factor();
        let over = SR * n as f32;
        let tone = on_grid(1_000.0, FRAMES, SR);
        // Above the session's Nyquist, so without the filter it would fold to
        // an audible place — 40 kHz lands at 8 kHz.
        let folding = on_grid(if n == 2 { 40_000.0 } else { 70_000.0 }, FRAMES, SR);
        let mut out = Vec::with_capacity(FRAMES);
        let mut decimator = Decimator::default();
        let mut block = [0.0f32; 4];
        for frame in 0..FRAMES {
            for (sub, slot) in block.iter_mut().enumerate().take(n) {
                let t = (frame * n + sub) as f32 / over;
                *slot = (std::f32::consts::TAU * tone * t).sin()
                    + (std::f32::consts::TAU * folding * t).sin();
            }
            out.push(decimator.decimate(&block[..n], factor));
        }
        let kept = level_db(&out, tone, SR);
        assert!(
            kept.abs() < 0.1,
            "{factor:?}: the 1 kHz tone came through at {kept} dB"
        );
        let fold_to = (folding % SR).min(SR - folding % SR);
        let leaked = level_db(&out, fold_to, SR);
        assert!(
            leaked < -50.0,
            "{factor:?}: {folding} Hz folded to {fold_to} Hz at {leaked} dB"
        );
    }
}

#[test]
fn the_decimators_passband_is_flat_to_the_top_of_hearing() {
    const FRAMES: usize = 8_192;
    for factor in [Oversampling::X2, Oversampling::X4] {
        let n = factor.factor();
        let over = SR * n as f32;
        for (hz, within) in [(10_000.0, 0.1), (15_000.0, 0.1), (18_000.0, 1.0)] {
            let tone = on_grid(hz, FRAMES, SR);
            let mut decimator = Decimator::default();
            let mut block = [0.0f32; 4];
            let out: Vec<f32> = (0..FRAMES)
                .map(|frame| {
                    for (sub, slot) in block.iter_mut().enumerate().take(n) {
                        let t = (frame * n + sub) as f32 / over;
                        *slot = (std::f32::consts::TAU * tone * t).sin();
                    }
                    decimator.decimate(&block[..n], factor)
                })
                .collect();
            let db = level_db(&out, tone, SR);
            assert!(
                db.abs() < within,
                "{factor:?}: {hz} Hz reads {db} dB, wanted within {within}"
            );
        }
    }
}

#[test]
fn an_interpolator_makes_n_samples_of_the_same_tone_without_images() {
    const FRAMES: usize = 4_096;
    for factor in [Oversampling::X2, Oversampling::X4] {
        let n = factor.factor();
        let over = SR * n as f32;
        let tone = on_grid(1_000.0, FRAMES, SR);
        let mut interpolator = Interpolator::default();
        let mut out = Vec::with_capacity(FRAMES * n);
        let mut block = [0.0f32; 4];
        for frame in 0..FRAMES {
            let x = (std::f32::consts::TAU * tone * frame as f32 / SR).sin();
            interpolator.interpolate(x, factor, &mut block);
            out.extend_from_slice(&block[..n]);
        }
        let kept = level_db(&out, tone, over);
        assert!(kept.abs() < 0.1, "{factor:?}: the tone reads {kept} dB");
        // Zero-stuffing puts an image of the tone at every multiple of the
        // session's rate; the kernel is what takes them out.
        let image = level_db(&out, SR - tone, over);
        assert!(
            image < -50.0,
            "{factor:?}: the image at {} Hz reads {image} dB",
            SR - tone
        );
    }
}

#[test]
fn the_state_is_copy_and_a_default_is_silence() {
    fn is_copy<T: Copy + Default>() {}
    is_copy::<Decimator>();
    is_copy::<Interpolator>();
    let mut decimator = Decimator::default();
    assert_eq!(decimator.decimate(&[0.0, 0.0], Oversampling::X2), 0.0);
    let mut block = [1.0f32; 4];
    Interpolator::default().interpolate(0.0, Oversampling::X4, &mut block);
    assert_eq!(block, [0.0; 4]);
}
