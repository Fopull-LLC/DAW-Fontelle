//! Onsets (plan §2.7, P3).

use fontelle_analysis::onsets::{OnsetSettings, detect_onsets};
use fontelle_analysis::testsignals::noise;

const SR: u32 = 48_000;

#[test]
fn clicks_are_found_within_5ms() {
    // Clicks — short bursts — over a quiet hum, at uneven times.
    let mut x: Vec<f32> = (0..(4.0 * f64::from(SR)) as usize)
        .map(|i| (std::f64::consts::TAU * 110.0 * i as f64 / f64::from(SR)).sin() as f32 * 0.02)
        .collect();
    let burst = noise(SR, 0.01, 0.8);
    let at_seconds = [0.25, 0.71, 1.2, 1.33, 2.05, 2.9, 3.5];
    let at: Vec<usize> = at_seconds
        .iter()
        .map(|s| (s * f64::from(SR)) as usize)
        .collect();
    for &start in &at {
        for (k, b) in burst.iter().enumerate() {
            x[start + k] += b * (-(k as f32) / 100.0).exp();
        }
    }
    let found = detect_onsets(&x, SR, &OnsetSettings::default());
    assert_eq!(
        found.len(),
        at.len(),
        "found {:?}",
        found.iter().map(|o| o.sample).collect::<Vec<_>>()
    );
    let tolerance = (0.005 * f64::from(SR)) as i64;
    for (onset, want) in found.iter().zip(&at) {
        let error = onset.sample as i64 - *want as i64;
        assert!(
            error.abs() <= tolerance,
            "a click at {want} was found at {} ({:.2} ms off)",
            onset.sample,
            error as f64 * 1000.0 / f64::from(SR)
        );
        assert!(onset.strength > 0.0 && onset.strength <= 1.0);
    }
}

#[test]
fn a_drum_loop_has_an_onset_on_every_eighth() {
    let x = fontelle_analysis::testsignals::drum_loop(SR);
    let found = detect_onsets(&x, SR, &OnsetSettings::default());
    let eighth = 0.3 * f64::from(SR);
    for step in 0..16 {
        let want = (step as f64 * eighth) as i64;
        assert!(
            found
                .iter()
                .any(|o| (o.sample as i64 - want).abs() <= (0.01 * f64::from(SR)) as i64),
            "nothing found at step {step}: {:?}",
            found.iter().map(|o| o.sample).collect::<Vec<_>>()
        );
    }
    assert!(found.len() <= 20, "{} onsets in 16 hits", found.len());
}

#[test]
fn silence_has_no_onsets() {
    assert!(detect_onsets(&vec![0.0; 48_000], SR, &OnsetSettings::default()).is_empty());
    assert!(detect_onsets(&[], SR, &OnsetSettings::default()).is_empty());
}
