//! Trim, fades and gain (plan §2.7, P3).

use fontelle_analysis::edit::{
    FadeShape, apply_fades, apply_gain_db, ms_to_samples, sound_bounds, trim,
};

#[test]
fn every_fade_shape_runs_from_nothing_to_whole() {
    for shape in [
        FadeShape::Linear,
        FadeShape::EqualPower,
        FadeShape::Exponential,
    ] {
        assert!(shape.gain(0.0).abs() < 1e-6);
        assert!((shape.gain(1.0) - 1.0).abs() < 1e-6);
        let mut last = 0.0;
        for i in 1..=100 {
            let g = shape.gain(i as f32 / 100.0);
            assert!(g >= last, "{shape:?} goes down");
            last = g;
        }
    }
    // Equal power: the two sides of a crossfade sum to one in power.
    let s = FadeShape::EqualPower;
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        assert!((s.gain(t).powi(2) + s.gain(1.0 - t).powi(2) - 1.0).abs() < 1e-5);
    }
}

#[test]
fn fades_touch_only_their_ends() {
    let mut x = vec![1.0f32; 1000];
    apply_fades(&mut x, 100, 200, FadeShape::Linear);
    assert_eq!(x[0], 0.0);
    assert!(x[50] > 0.4 && x[50] < 0.6);
    assert!(x[100..800].iter().all(|v| *v == 1.0));
    assert!(x[999] < 0.01);
    // Fades longer than the buffer meet in the middle without panicking.
    let mut short = vec![1.0f32; 10];
    apply_fades(&mut short, 100, 100, FadeShape::EqualPower);
    assert!(short.iter().all(|v| (0.0..=1.0).contains(v)));
}

#[test]
fn trim_clamps_and_gain_is_in_decibels() {
    let x: Vec<f32> = (0..10).map(|i| i as f32).collect();
    assert_eq!(trim(&x, 2, 5), &[2.0, 3.0, 4.0]);
    assert_eq!(trim(&x, 8, 50), &[8.0, 9.0]);
    assert!(trim(&x, 7, 3).is_empty());
    let mut y = vec![1.0f32; 4];
    apply_gain_db(&mut y, -6.0206);
    assert!((y[0] - 0.5).abs() < 1e-4);
    assert_eq!(ms_to_samples(10.0, 48_000), 480);
}

#[test]
fn sound_bounds_find_where_the_sound_is() {
    let mut x = vec![0.0001f32; 1000];
    x[200] = 0.5;
    x[700] = -0.5;
    assert_eq!(sound_bounds(&x, -40.0), Some((200, 701)));
    assert_eq!(sound_bounds(&vec![0.0; 100], -60.0), None);
}
