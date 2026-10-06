//! Offline pYIN and note segmentation (plan §2.3, §4 P1).

use fontelle_analysis::mono::{F0Track, PyinParams, pyin, segment};
use fontelle_analysis::testsignals;

fn track(audio: &[f32], sample_rate: u32) -> F0Track {
    pyin(audio, sample_rate, &PyinParams::default())
}

#[test]
fn pyin_tracks_a_sung_glissando_within_10_cents() {
    let sr = 44_100;
    let t = track(&testsignals::sung_glide(sr), sr);
    let (mut inside, mut total, mut voiced) = (0, 0, 0);
    let mut worst = 0.0f64;
    for i in 0..t.len() {
        let time = t.time(i);
        // Clear of the attack and the release.
        if !(0.35..=1.75).contains(&time) {
            continue;
        }
        total += 1;
        let Some(cents) = t.cents(i) else { continue };
        voiced += 1;
        let error = (f64::from(cents) - testsignals::sung_glide_cents(time - 0.3)).abs();
        worst = worst.max(error);
        if error <= 10.0 {
            inside += 1;
        }
    }
    assert!(voiced * 100 >= total * 98, "voiced {voiced} of {total}");
    assert!(
        inside * 100 >= voiced * 98,
        "{inside} of {voiced} within 10 cents, worst {worst:.1}"
    );
}

#[test]
fn a_saw_is_found_at_its_fundamental_not_an_octave_up() {
    let sr = 44_100;
    let audio: Vec<f32> = (0..sr as usize)
        .map(|i| {
            let t = i as f32 / sr as f32;
            (1..=30)
                .map(|k| (std::f32::consts::TAU * 110.0 * k as f32 * t).sin() / k as f32)
                .sum::<f32>()
                * 0.2
        })
        .collect();
    let t = track(&audio, sr);
    let middle: Vec<f32> = (40..t.len() - 40).filter_map(|i| t.cents(i)).collect();
    assert!(middle.len() > 100);
    let a2 = 4500.0;
    assert!(
        middle.iter().all(|c| (c - a2).abs() < 10.0),
        "{:?}",
        &middle[..10]
    );
}

#[test]
fn silence_and_noise_are_unvoiced() {
    let sr = 44_100;
    let quiet = track(&vec![0.0; sr as usize], sr);
    assert!(quiet.len() > 150);
    assert!((0..quiet.len()).all(|i| quiet.cents(i).is_none()));
    let noisy = track(&testsignals::noise(sr, 1.0, 0.3), sr);
    let voiced = (0..noisy.len())
        .filter(|&i| noisy.cents(i).is_some())
        .count();
    assert!(
        voiced * 10 < noisy.len(),
        "{voiced} of {} voiced in noise",
        noisy.len()
    );
    let mean = noisy.voicing.iter().sum::<f32>() / noisy.len() as f32;
    assert!(mean < 0.3, "mean voicing {mean} in noise");
}

#[test]
fn a_sung_melody_segments_into_its_notes() {
    let sr = 44_100;
    let fixture = testsignals::vibrato_melody(sr);
    let notes = segment(&track(&fixture.samples, sr));
    let got: Vec<String> = notes
        .iter()
        .map(|n| {
            format!(
                "{:.3}-{:.3} {} ({:.0})",
                n.start_time,
                n.end_time,
                n.midi(),
                n.centre
            )
        })
        .collect();
    assert_eq!(notes.len(), fixture.notes.len(), "{got:?}");
    for (n, e) in notes.iter().zip(&fixture.notes) {
        assert_eq!(n.midi(), e.midi, "{got:?}");
        assert!(
            (n.start_time - e.start).abs() <= 0.030,
            "onset of {}: {got:?}",
            e.midi
        );
        assert!(
            (n.end_time - e.end).abs() <= 0.060,
            "end of {}: {got:?}",
            e.midi
        );
        // The vibrato is symmetric, so the centre is the note.
        assert!(
            (n.centre - f32::from(e.midi) * 100.0).abs() < 8.0,
            "centre of {}: {got:?}",
            e.midi
        );
    }
}

#[test]
fn drift_and_vibrato_are_separated() {
    let sr = 44_100;
    let t = track(&testsignals::drifting_vibrato_note(sr), sr);
    let notes = segment(&t);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let note = &notes[0];
    assert_eq!(note.midi(), 57);
    // The middle 60 % of a linear 60-cent rise has its median halfway up.
    assert!((note.centre - 5730.0).abs() < 8.0, "centre {}", note.centre);
    assert_eq!(note.drift.len(), note.end - note.first);
    assert_eq!(note.vibrato.len(), note.end - note.first);

    let n = note.drift.len();
    let (mut drift_err, mut vib_err, mut vib_power, mut count) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    // The middle 80 %: a filter's edges are guesses.
    for k in n / 10..n - n / 10 {
        let time = t.time(note.first + k) - 0.25;
        let true_drift = testsignals::drift_cents(time) - f64::from(note.centre);
        let true_vib = testsignals::vibrato_cents(time);
        drift_err += (f64::from(note.drift[k]) - true_drift).powi(2);
        vib_err += (f64::from(note.vibrato[k]) - true_vib).powi(2);
        vib_power += f64::from(note.vibrato[k]).powi(2);
        count += 1.0;
    }
    let drift_rms = (drift_err / count).sqrt();
    let vib_rms = (vib_err / count).sqrt();
    let vib_depth = (vib_power / count).sqrt() * std::f64::consts::SQRT_2;
    assert!(drift_rms < 5.0, "drift off by {drift_rms:.1} cents RMS");
    assert!(vib_rms < 8.0, "vibrato off by {vib_rms:.1} cents RMS");
    assert!(
        (vib_depth - 30.0).abs() < 5.0,
        "vibrato depth {vib_depth:.1}"
    );
}
