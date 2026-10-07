//! The vocoder: F0 found where it is, envelopes where the formants are,
//! and synthesis that lands on the pitch it is asked for.

mod common;

use common::{FS, cents, measure_f0, rms, vowel};
use fontelle_utau::wav;
use fontelle_utau::world::{self, AnalysisOptions, FRAME_PERIOD, Frames};

fn voiced_median(f0: &[f32]) -> f32 {
    let mut v: Vec<f32> = f0.iter().cloned().filter(|&f| f > 0.0).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

#[test]
fn f0_of_a_sung_vowel_is_found_to_a_few_cents() {
    for f in [110.0, 220.0, 440.0] {
        let x = vowel(f, 0.6, 'a');
        let f0 = world::estimate_f0(&x, FS, &AnalysisOptions::default());
        let expected = (x.len() as f64 / FS as f64 / FRAME_PERIOD) as usize;
        assert!(
            (f0.len() as i64 - expected as i64).abs() <= 2,
            "{}",
            f0.len()
        );
        let voiced = f0.iter().filter(|&&v| v > 0.0).count();
        assert!(
            voiced as f32 > 0.85 * f0.len() as f32,
            "{f}: {voiced}/{}",
            f0.len()
        );
        let m = voiced_median(&f0);
        assert!(cents(m, f).abs() < 3.0, "{f}: found {m}");
    }
}

#[test]
fn silence_and_noise_are_unvoiced() {
    let mut x = common::silence(0.2);
    x.extend(common::noise(0.3, 0.3, 7));
    let f0 = world::estimate_f0(&x, FS, &AnalysisOptions::default());
    let voiced = f0.iter().filter(|&&v| v > 0.0).count();
    assert!(voiced * 10 < f0.len(), "{voiced}/{}", f0.len());
}

#[test]
fn the_envelope_peaks_at_the_first_formant() {
    let x = vowel(220.0, 0.5, 'a');
    let a = world::analyze(&x, FS, &AnalysisOptions::default());
    assert_eq!(
        a.fft_size,
        world::fft_size_for(FS, AnalysisOptions::default().f0_floor)
    );
    let mid = a.frames() / 2;
    let sp = a.sp_frame(mid);
    let hz = |i: usize| i as f64 * FS as f64 / a.fft_size as f64;
    let (peak, _) = sp
        .iter()
        .enumerate()
        .filter(|(i, _)| hz(*i) < 2000.0 && hz(*i) > 200.0)
        .fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
    assert!((hz(peak) - 800.0).abs() < 250.0, "peak at {}", hz(peak));
    // A clean harmonic vowel is periodic below 4 kHz.
    let ap = a.ap_frame(mid);
    assert!(ap[1] < 0.3, "{ap:?}");
}

fn frames_of(a: &world::Analysis, f0_scale: f32) -> Frames {
    let bins = a.bins();
    let mut ap = vec![0.0; a.frames() * bins];
    for i in 0..a.frames() {
        world::expand_ap(
            a.ap_frame(i),
            a.fs,
            a.fft_size,
            &mut ap[i * bins..(i + 1) * bins],
        );
    }
    Frames {
        fft_size: a.fft_size,
        f0: a.f0.iter().map(|f| f * f0_scale).collect(),
        sp: a.sp.clone(),
        ap,
    }
}

#[test]
fn resynthesis_keeps_loudness_and_pitch() {
    let x = vowel(220.0, 0.6, 'a');
    let a = world::analyze(&x, FS, &AnalysisOptions::default());
    let y = world::synthesize(&frames_of(&a, 1.0), FS, 1, &|| false).unwrap();
    assert!((y.len() as i64 - x.len() as i64).abs() < (0.02 * FS as f64) as i64);
    let n = x.len().min(y.len());
    let (rx, ry) = (rms(&x[n / 4..3 * n / 4]), rms(&y[n / 4..3 * n / 4]));
    let db = 20.0 * (ry / rx).log10();
    eprintln!("resynthesis level {db:.2} dB");
    assert!(db.abs() < 2.0, "{db} dB");
    let f = measure_f0(&y, FS, 0.15, 0.45);
    assert!(cents(f, 220.0).abs() < 5.0, "{f}");
}

#[test]
fn a_shifted_pitch_is_sung_in_tune() {
    let x = vowel(220.0, 0.6, 'a');
    let a = world::analyze(&x, FS, &AnalysisOptions::default());
    for scale in [0.75f32, 1.5, 2.0] {
        let y = world::synthesize(&frames_of(&a, scale), FS, 1, &|| false).unwrap();
        let f = measure_f0(&y, FS, 0.15, 0.45);
        assert!(cents(f, 220.0 * scale).abs() < 5.0, "{scale}: {f}");
    }
}

#[test]
fn synthesis_is_deterministic_and_cancellable() {
    let x = vowel(220.0, 0.3, 'o');
    let a = world::analyze(&x, FS, &AnalysisOptions::default());
    let f = frames_of(&a, 1.0);
    assert_eq!(
        world::synthesize(&f, FS, 9, &|| false),
        world::synthesize(&f, FS, 9, &|| false)
    );
    assert!(world::synthesize(&f, FS, 9, &|| true).is_none());
}

#[test]
fn wav_files_read_back_and_resample() {
    let dir = common::temp_dir("wav");
    let x = vowel(220.0, 0.2, 'a');
    common::write_wav(&dir.join("a.wav"), &x, FS);
    let (y, rate) = wav::read_wav(&dir.join("a.wav")).unwrap();
    assert_eq!(rate, FS);
    assert_eq!(y.len(), x.len());
    assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-3));
    wav::write_wav(&dir.join("b.wav"), &y, 22_050).unwrap();
    assert_eq!(wav::read_wav(&dir.join("b.wav")).unwrap().1, 22_050);
    let r = wav::resample(&x, FS, 48_000);
    assert!((r.len() as f64 - x.len() as f64 * 48_000.0 / FS as f64).abs() < 2.0);
    let f = measure_f0(&r, 48_000, 0.05, 0.15);
    assert!(cents(f, 220.0).abs() < 3.0, "{f}");
}
