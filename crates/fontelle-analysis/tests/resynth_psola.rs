//! Moving a sung note so it still sounds sung (`docs/analyze-musically-plan.md`
//! §2.4, §4 P2): offline TD-PSOLA on real pitch marks, driven by the edited
//! contour, only the edited span resynthesised and spliced back.
//!
//! Every signal is synthesised (`fontelle_analysis::testsignals` and the
//! vowel below); nothing here reads or writes audio. The numbers each test
//! measures are printed, for `PROGRESS.md`.

use fontelle_analysis::mono::{PyinParams, pyin, segment};
use fontelle_analysis::render::render_edits;
use fontelle_analysis::resynth::Psola;
use fontelle_analysis::testsignals::{drifting_vibrato_note, sung_curve};
use fontelle_types::PitchEdit;

const SR: u32 = 44_100;

fn frames(seconds: f64) -> i64 {
    (seconds * f64::from(SR)).round() as i64
}

fn moved(from: f64, to: f64, cents: f32) -> PitchEdit {
    PitchEdit {
        shift_cents: cents,
        ..PitchEdit::none((frames(from), frames(to)))
    }
}

/// The median pitch (MIDI cents) pYIN hears between `from` and `to`.
fn heard(audio: &[f32], from: f64, to: f64) -> f32 {
    let track = pyin(audio, SR, &PyinParams::default());
    let mut cents: Vec<f32> = (0..track.len())
        .filter(|i| track.time(*i) >= from && track.time(*i) < to)
        .filter_map(|i| track.cents(i))
        .collect();
    assert!(!cents.is_empty(), "nothing voiced between {from} and {to}");
    cents.sort_by(f32::total_cmp);
    cents[cents.len() / 2]
}

/// A sung A3 thirty cents flat, Q'd up to A3: it lands within 3 cents.
#[test]
fn a_30_cent_flat_note_moved_lands_within_3_cents() {
    let audio = sung_curve(SR, 1.8, 0.3, 1.5, |_| 5700.0 - 30.0);
    let before = heard(&audio, 0.5, 1.3);
    let rendered = render_edits(&audio, 1, SR, &[moved(0.3, 1.5, 30.0)], &Psola);
    let after = heard(&rendered.audio, 0.5, 1.3);
    println!("30-cent flat note: heard at {before:.2} before, {after:.2} after (target 5700)");
    assert!((before - 5670.0).abs() < 3.0, "the source reads {before}");
    assert!((after - 5700.0).abs() < 3.0, "moved to {after}");
}

/// Two notes, the second moved: every sample outside what was re-rendered
/// is the source's own, bit for bit, and with no edits all of it is.
#[test]
fn unedited_samples_are_bit_identical() {
    let audio = sung_curve(SR, 2.4, 0.2, 2.2, |t| if t < 1.0 { 5700.0 } else { 5900.0 });
    let untouched = render_edits(&audio, 1, SR, &[], &Psola);
    assert_eq!(untouched.audio, audio);
    assert!(untouched.spans.is_empty());

    let rendered = render_edits(&audio, 1, SR, &[moved(1.25, 2.2, -150.0)], &Psola);
    assert_eq!(rendered.audio.len(), audio.len());
    assert_eq!(rendered.spans.len(), 1);
    let span = rendered.spans[0].clone();
    // Re-rendered: the note and a little padding, no more.
    assert!(span.start >= frames(1.25 - 0.06) as usize, "{span:?}");
    assert!(span.end <= frames(2.2 + 0.06) as usize, "{span:?}");
    let mut changed = 0usize;
    for (i, (a, b)) in audio.iter().zip(&rendered.audio).enumerate() {
        if span.contains(&i) {
            changed += usize::from(a.to_bits() != b.to_bits());
        } else {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "sample {i} outside {span:?} changed"
            );
        }
    }
    assert!(changed > span.len() / 2, "the moved note changed");
}

/// A windowed-sinc high-pass at 8 kHz (Blackman, 255 taps: 74 dB down in
/// the stop band), so a voice's partials, all under 3 kHz here, are gone
/// and a click — broadband — is what is left.
fn high_pass(audio: &[f32]) -> Vec<f32> {
    let taps = 255usize;
    let fc = 8_000.0 / f64::from(SR);
    let m = (taps - 1) as f64;
    let mut h: Vec<f64> = (0..taps)
        .map(|n| {
            let x = n as f64 - m / 2.0;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (std::f64::consts::TAU * fc * x).sin() / (std::f64::consts::PI * x)
            };
            let w = 0.42 - 0.5 * (std::f64::consts::TAU * n as f64 / m).cos()
                + 0.08 * (2.0 * std::f64::consts::TAU * n as f64 / m).cos();
            -sinc * w
        })
        .collect();
    h[taps / 2] += 1.0;
    (0..audio.len())
        .map(|i| {
            let mut acc = 0.0f64;
            for (k, c) in h.iter().enumerate() {
                if let Some(x) = (i + taps / 2).checked_sub(k).and_then(|j| audio.get(j)) {
                    acc += c * f64::from(*x);
                }
            }
            acc as f32
        })
        .collect()
}

/// Where the moved span is spliced back, nothing broadband: the high-passed
/// output stays under -60 dBFS within 15 ms of each seam, two semitones up.
#[test]
fn the_splice_has_no_click() {
    let audio = sung_curve(SR, 2.4, 0.2, 2.2, |t| if t < 1.0 { 5700.0 } else { 5900.0 });
    let rendered = render_edits(&audio, 1, SR, &[moved(1.25, 2.0, 200.0)], &Psola);
    assert_eq!(rendered.seams.len(), 2, "{:?}", rendered.seams);
    let hp = high_pass(&rendered.audio);
    let reach = frames(0.015) as usize;
    for seam in &rendered.seams {
        let around = &hp[seam.saturating_sub(reach)..(seam + reach).min(hp.len())];
        let peak = around.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let db = 20.0 * peak.max(1e-12).log10();
        println!("seam at {seam}: high-passed peak {db:.1} dBFS");
        assert!(db < -60.0, "a click at {seam}: {db:.1} dBFS");
    }
    // And the source's own there, for scale: the filter passes nothing of
    // the voice, so what is measured above is the splice's.
    let source = high_pass(&audio);
    for seam in &rendered.seams {
        let around = &source[seam.saturating_sub(reach)..(seam + reach).min(source.len())];
        let peak = around.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(20.0 * peak.max(1e-12).log10() < -60.0);
    }
}

/// A note with a steady vibrato, moved two semitones: the vibrato comes out
/// as deep and as fast as it went in, and the centre moved by 200 cents.
#[test]
fn vibrato_survives_a_move() {
    let audio = drifting_vibrato_note(SR);
    let rendered = render_edits(&audio, 1, SR, &[moved(0.25, 2.25, 200.0)], &Psola);
    let depth = |audio: &[f32]| {
        let notes = segment(&pyin(audio, SR, &PyinParams::default()));
        let note = notes
            .iter()
            .max_by(|a, b| (a.end - a.first).cmp(&(b.end - b.first)))
            .expect("a note")
            .clone();
        let n = note.vibrato.len();
        let middle = &note.vibrato[n / 5..n - n / 5];
        let rms = (middle.iter().map(|v| v * v).sum::<f32>() / middle.len() as f32).sqrt();
        (rms, note.centre)
    };
    let (before, centre_before) = depth(&audio);
    let (after, centre_after) = depth(&rendered.audio);
    println!(
        "vibrato RMS {before:.1} ct before, {after:.1} ct after; centre {centre_before:.1} -> {centre_after:.1}"
    );
    assert!(before > 15.0, "the source has its vibrato: {before}");
    assert!(
        (after / before - 1.0).abs() < 0.2,
        "vibrato {before:.1} ct became {after:.1} ct"
    );
    assert!(
        (centre_after - centre_before - 200.0).abs() < 5.0,
        "moved {} cents",
        centre_after - centre_before
    );
}

/// A vowel: pulses at `f0` through two resonances, F1 700 Hz and F2
/// 1220 Hz, as an /a/.
fn vowel(f0: f64, seconds: f64) -> Vec<f32> {
    let sr = f64::from(SR);
    let n = (seconds * sr) as usize;
    let mut source = vec![0.0f64; n];
    let mut phase = 0.0f64;
    for (i, s) in source.iter_mut().enumerate() {
        phase += f0 / sr;
        if phase >= 1.0 {
            phase -= 1.0;
            *s = 1.0;
        }
        let _ = i;
    }
    let resonate = |x: &[f64], f: f64, bw: f64| -> Vec<f64> {
        let r = (-std::f64::consts::PI * bw / sr).exp();
        let a1 = -2.0 * r * (std::f64::consts::TAU * f / sr).cos();
        let a2 = r * r;
        let g = 1.0 - r;
        let (mut y1, mut y2) = (0.0, 0.0);
        x.iter()
            .map(|v| {
                let y = g * v - a1 * y1 - a2 * y2;
                y2 = y1;
                y1 = y;
                y
            })
            .collect()
    };
    let a = resonate(&source, 700.0, 90.0);
    let b = resonate(&source, 1220.0, 110.0);
    let mixed: Vec<f64> = a.iter().zip(&b).map(|(a, b)| a + 0.6 * b).collect();
    let peak = mixed.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    // 10 ms in and out.
    let edge = (0.01 * sr) as usize;
    mixed
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let env = (i.min(n - 1 - i) as f64 / edge as f64).min(1.0);
            (0.4 * v / peak * env) as f32
        })
        .collect()
}

/// The spectral envelope's peaks between `bands`, from the averaged
/// spectrum of `audio`, smoothed in the cepstrum (liftered under half the
/// shorter period, so harmonics are not what is read).
fn formants(audio: &[f32], f0_max: f64, bands: &[(f64, f64)]) -> Vec<f64> {
    use fontelle_dsp::fft_in_place;
    let n = 4096usize;
    let hop = n / 4;
    let mut power = vec![0.0f64; n];
    let mut frames = 0;
    let mut at = audio.len() / 5;
    while at + n < audio.len() * 4 / 5 {
        let mut re: Vec<f32> = (0..n)
            .map(|k| {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / n as f32).cos();
                audio[at + k] * w
            })
            .collect();
        let mut im = vec![0.0f32; n];
        fft_in_place(&mut re, &mut im);
        for k in 0..n {
            power[k] += f64::from(re[k] * re[k] + im[k] * im[k]);
        }
        frames += 1;
        at += hop;
    }
    assert!(frames > 0);
    let mut re: Vec<f32> = power
        .iter()
        .map(|p| ((p / frames as f64) + 1e-12).ln() as f32)
        .collect();
    let mut im = vec![0.0f32; n];
    fft_in_place(&mut re, &mut im);
    let lifter = (0.5 * f64::from(SR) / f0_max) as usize;
    for k in 0..n {
        let q = k.min(n - k);
        if q > lifter {
            re[k] = 0.0;
            im[k] = 0.0;
        }
    }
    // The inverse, through the forward transform of the conjugate.
    for v in im.iter_mut() {
        *v = -*v;
    }
    fft_in_place(&mut re, &mut im);
    let bin = f64::from(SR) / n as f64;
    bands
        .iter()
        .map(|(lo, hi)| {
            let (a, b) = ((lo / bin) as usize, (hi / bin) as usize);
            let best = (a..b).max_by(|x, y| re[*x].total_cmp(&re[*y])).unwrap();
            best as f64 * bin
        })
        .collect()
}

/// A vowel moved up three semitones keeps its vowel: both formants within
/// 5 % of where they were. PSOLA keeps the envelope by construction — each
/// grain is a period of the voice through its own vocal tract.
#[test]
fn formant_peaks_stay_within_5_percent() {
    let audio = vowel(130.0, 1.4);
    let rendered = render_edits(&audio, 1, SR, &[moved(0.05, 1.35, 300.0)], &Psola);
    let pitch = heard(&rendered.audio, 0.4, 1.0) - heard(&audio, 0.4, 1.0);
    let bands = [(450.0, 950.0), (1000.0, 1500.0)];
    let before = formants(&audio, 160.0, &bands);
    let after = formants(&rendered.audio, 160.0, &bands);
    println!(
        "vowel up {pitch:.1} ct: F1 {:.0} -> {:.0} Hz, F2 {:.0} -> {:.0} Hz",
        before[0], after[0], before[1], after[1]
    );
    assert!((pitch - 300.0).abs() < 10.0, "moved {pitch} cents");
    for (b, a) in before.iter().zip(&after) {
        assert!(
            (a / b - 1.0).abs() < 0.05,
            "a formant moved from {b:.0} to {a:.0} Hz"
        );
    }
}

/// Stereo: one mark schedule for both sides, so the image holds — the
/// channels stay as alike as they went in.
#[test]
fn both_channels_move_together() {
    let mono = sung_curve(SR, 1.4, 0.2, 1.2, |_| 5650.0);
    let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s * 0.5]).collect();
    let rendered = render_edits(&stereo, 2, SR, &[moved(0.2, 1.2, 50.0)], &Psola);
    assert_eq!(rendered.audio.len(), stereo.len());
    for frame in rendered.audio.chunks(2) {
        assert!((frame[1] - frame[0] * 0.5).abs() < 1e-5);
    }
    let left: Vec<f32> = rendered.audio.iter().step_by(2).copied().collect();
    assert!((heard(&left, 0.4, 1.0) - 5700.0).abs() < 3.0);
}

/// Plan §3.10's budget: one note re-rendered in well under the time a
/// preview has (debounced 120 ms). Printed; the bound is loose for a busy
/// machine.
#[test]
fn one_note_renders_quickly() {
    let audio = sung_curve(SR, 4.0, 0.2, 3.8, |t| {
        5700.0 + if t > 2.0 { 200.0 } else { 0.0 }
    });
    let started = std::time::Instant::now();
    let rendered = render_edits(&audio, 1, SR, &[moved(0.5, 1.1, 100.0)], &Psola);
    let took = started.elapsed();
    println!("one 0.6 s note re-rendered in {took:?}");
    assert_eq!(rendered.spans.len(), 1);
    assert!(took.as_millis() < 150, "{took:?}");
}
