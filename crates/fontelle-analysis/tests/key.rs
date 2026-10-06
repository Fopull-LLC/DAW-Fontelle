//! Key, scale and their confidence (plan §2.6, §4 P1).

use fontelle_analysis::key::{PitchWeight, detect_key, key_confidence};
use fontelle_analysis::testsignals::key_material;
use fontelle_types::KeyScale;

fn line(notes: &[(u8, f32)]) -> Vec<PitchWeight> {
    notes
        .iter()
        .map(|&(midi, seconds)| PitchWeight {
            midi,
            seconds,
            weight: 0.9,
        })
        .collect()
}

#[test]
fn c_major_scale_is_c_major() {
    let scale = line(&[
        (60, 1.0),
        (62, 0.5),
        (64, 0.5),
        (65, 0.5),
        (67, 0.8),
        (69, 0.5),
        (71, 0.5),
        (72, 1.0),
        (48, 2.0),
    ]);
    let reading = detect_key(&scale).unwrap();
    assert_eq!(reading.key, KeyScale::new(0, "major"), "{reading:?}");
    assert_eq!(reading.relative, KeyScale::new(9, "natural-minor"));
    assert!(reading.confidence > 0.6, "{reading:?}");
}

#[test]
fn a_minor_reports_its_relative() {
    let tune = line(&[
        (57, 1.0),
        (60, 0.5),
        (64, 0.8),
        (62, 0.4),
        (60, 0.4),
        (59, 0.4),
        (57, 1.2),
        (64, 0.6),
        (65, 0.4),
        (64, 0.4),
        (62, 0.4),
        (60, 0.4),
        (59, 0.5),
        (57, 1.5),
        (45, 3.0),
        (40, 1.5),
    ]);
    let reading = detect_key(&tune).unwrap();
    assert_eq!(
        reading.key,
        KeyScale::new(9, "natural-minor"),
        "{reading:?}"
    );
    assert_eq!(reading.relative, KeyScale::new(0, "major"));
    assert_eq!(
        reading.alternatives.first(),
        Some(&KeyScale::new(0, "major")),
        "{reading:?}"
    );
    assert!(reading.tonic_confidence > 0.5, "{reading:?}");
}

#[test]
fn a_minor_with_a_raised_sixth_reads_dorian() {
    // A minor's material, every F an F♯.
    let tune = line(&[
        (57, 1.0),
        (60, 0.5),
        (64, 0.8),
        (66, 0.6),
        (64, 0.4),
        (62, 0.4),
        (60, 0.4),
        (57, 1.2),
        (66, 0.5),
        (67, 0.4),
        (66, 0.4),
        (64, 0.6),
        (59, 0.5),
        (57, 1.5),
        (45, 3.0),
    ]);
    let reading = detect_key(&tune).unwrap();
    assert_eq!(reading.key, KeyScale::new(9, "dorian"), "{reading:?}");
}

#[test]
fn every_key_is_found_from_its_material() {
    for root in 0..12u8 {
        for minor in [false, true] {
            let notes = key_material(
                u64::from(root) * 2 + u64::from(minor) + 1,
                root,
                minor,
                0.0,
                48,
            );
            let reading = detect_key(&notes).unwrap();
            let want = if minor {
                KeyScale::new(root, "natural-minor")
            } else {
                KeyScale::new(root, "major")
            };
            assert!(
                reading.key == want || reading.relative == want,
                "{} found as {}",
                want.label(),
                reading.key.label()
            );
        }
    }
}

#[test]
fn confidence_falls_as_the_material_gets_chromatic() {
    let mean = |chromatic: f32| {
        let mut sum = 0.0;
        for seed in 0..40u64 {
            let root = (seed % 12) as u8;
            let notes = key_material(1000 + seed, root, seed % 2 == 1, chromatic, 32);
            sum += detect_key(&notes).unwrap().confidence;
        }
        sum / 40.0
    };
    let levels: Vec<f32> = [0.0, 0.2, 0.4, 0.6, 0.8].iter().map(|&c| mean(c)).collect();
    for pair in levels.windows(2) {
        assert!(pair[1] < pair[0], "{levels:?}");
    }
    assert!(levels[0] > 0.8, "{levels:?}");
    assert!(levels[4] < 0.4, "{levels:?}");
}

#[test]
fn calibration_is_monotone() {
    // In each of its four inputs, holding the others.
    let margins = [0.0f32, 0.02, 0.05, 0.1, 0.2, 0.4];
    let bests = [0.2f32, 0.4, 0.6, 0.8, 0.95];
    let masses = [0.5f32, 2.0, 8.0, 30.0, 120.0];
    let insides = [0.4f32, 0.6, 0.8, 0.9, 1.0];
    let rising = |ps: &[f32]| {
        ps.windows(2).all(|w| w[1] >= w[0]) && ps.iter().all(|p| (0.0..=1.0).contains(p))
    };
    for &a in &margins {
        for &b in &bests {
            for &m in &masses {
                for &i in &insides {
                    let at = |a, b, m, i| key_confidence(a, b, m, i);
                    assert!(
                        rising(&margins.map(|x| at(x, b, m, i))),
                        "margin at {b} {m} {i}"
                    );
                    assert!(
                        rising(&bests.map(|x| at(a, x, m, i))),
                        "best at {a} {m} {i}"
                    );
                    assert!(
                        rising(&masses.map(|x| at(a, b, x, i))),
                        "mass at {a} {b} {i}"
                    );
                    assert!(
                        rising(&insides.map(|x| at(a, b, m, x))),
                        "inside at {a} {b} {m}"
                    );
                }
            }
        }
    }
}

#[test]
fn nothing_has_no_key() {
    assert!(detect_key(&[]).is_none());
    assert!(detect_key(&line(&[(60, 0.0)])).is_none());
}
