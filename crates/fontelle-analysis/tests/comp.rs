//! Comping (plan P5).

use fontelle_analysis::comp::{CompSpan, comp, default_crossfade};
use fontelle_analysis::edit::FadeShape;

const SR: u32 = 48_000;

fn sine(len: usize, hz: f64, phase: f64) -> Vec<f32> {
    (0..len)
        .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(SR) + phase).sin() as f32 * 0.5)
        .collect()
}

fn largest_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_comp_of_two_takes_crossfades_without_a_click() {
    let len = SR as usize;
    // The same line, played a quarter-cycle apart: a hard cut between them
    // is a jump of 0.7 at the seam.
    let a = sine(len, 220.0, 0.0);
    let b = sine(len, 220.0, std::f64::consts::FRAC_PI_2);
    let seam = 24_000;
    let spans = [
        CompSpan {
            take: 0,
            start: 0,
            end: seam,
        },
        CompSpan {
            take: 1,
            start: seam,
            end: len,
        },
    ];
    let fade = default_crossfade(SR);
    assert_eq!(fade, 480);
    for shape in [FadeShape::EqualPower, FadeShape::Linear] {
        let out = comp(&[&a, &b], &spans, len, fade, shape);
        assert_eq!(out.len(), len);
        let natural = largest_step(&a).max(largest_step(&b));
        let at_seam = largest_step(&out[seam - fade..seam + fade]);
        assert!(
            at_seam <= 1.5 * natural,
            "{shape:?}: a step of {at_seam} at the seam against {natural} in the takes"
        );
        // Outside the crossfade each side is its own take, exactly.
        assert_eq!(&out[..seam - fade / 2], &a[..seam - fade / 2]);
        assert_eq!(&out[seam + fade / 2..], &b[seam + fade / 2..]);
    }
    // And a hard cut would have clicked: the test can tell.
    let cut = comp(&[&a, &b], &spans, len, 0, FadeShape::Linear);
    assert!(largest_step(&cut[seam - 2..seam + 2]) > 5.0 * largest_step(&a));
}

#[test]
fn uncovered_frames_and_short_takes_are_silence() {
    let a = vec![1.0f32; 100];
    let spans = [CompSpan {
        take: 0,
        start: 50,
        end: 200,
    }];
    let out = comp(&[&a], &spans, 300, 0, FadeShape::Linear);
    assert!(out[..50].iter().all(|v| *v == 0.0));
    assert!(out[50..100].iter().all(|v| *v == 1.0));
    assert!(out[100..].iter().all(|v| *v == 0.0));
    // A span naming a take that is not there is silence, not a panic.
    let out = comp(
        &[&a],
        &[CompSpan {
            take: 3,
            start: 0,
            end: 10,
        }],
        10,
        0,
        FadeShape::Linear,
    );
    assert!(out.iter().all(|v| *v == 0.0));
}
