//! The curve vocabulary, moved down from `fontelle-model` and given the bend
//! its `tension` field has been waiting for (`docs/disgusting-beat-plan.md` §3.5).
//!
//! It moved because the audio thread reads these shapes now — DisgustingBeat's
//! lanes are drawn in them — and `fontelle-engine` may not see
//! `fontelle-model` (INVARIANT 4). `fontelle-model` re-exports the name, so
//! nothing above it changed.

use fontelle_types::CurveShape;

/// The one property every shape has to keep: it interpolates between the
/// *same* two points, so a shape that missed its own endpoint would put a
/// jump at every point it touched.
#[test]
fn every_shape_starts_at_zero_and_ends_at_one() {
    for shape in [
        CurveShape::Linear,
        CurveShape::Exponential,
        CurveShape::Logarithmic,
        CurveShape::SCurve,
    ] {
        for tension in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert!(
                shape.eased(0.0, tension).abs() < 1e-9,
                "{shape:?} at tension {tension} does not start at zero"
            );
            assert!(
                (shape.eased(1.0, tension) - 1.0).abs() < 1e-9,
                "{shape:?} at tension {tension} does not end at one"
            );
        }
    }
}

#[test]
fn every_shape_is_monotone() {
    // A curve that doubled back would draw one way and sound another: the
    // window reads the same function the audio thread does.
    for shape in [
        CurveShape::Linear,
        CurveShape::Exponential,
        CurveShape::Logarithmic,
        CurveShape::SCurve,
    ] {
        for tension in [-0.9, -0.3, 0.0, 0.3, 0.9] {
            let mut last = -1.0;
            for step in 0..=64 {
                let value = shape.eased(step as f64 / 64.0, tension);
                assert!(
                    value >= last - 1e-9,
                    "{shape:?} at tension {tension} goes backwards at {step}"
                );
                last = value;
            }
        }
    }
}

#[test]
fn tension_zero_is_exactly_the_unbent_shape() {
    // The automation lane stores a tension on every point and has never set
    // one. If zero were not exactly today's answer, every automation clip in
    // every project would change shape the day this landed.
    for shape in [
        CurveShape::Linear,
        CurveShape::Exponential,
        CurveShape::Logarithmic,
        CurveShape::SCurve,
    ] {
        for step in 0..=32 {
            let t = step as f64 / 32.0;
            let expected = match shape {
                CurveShape::Linear => t,
                CurveShape::Exponential => t * t,
                CurveShape::Logarithmic => 1.0 - (1.0 - t) * (1.0 - t),
                CurveShape::SCurve => t * t * (3.0 - 2.0 * t),
                _ => unreachable!(),
            };
            assert!(
                (shape.eased(t, 0.0) - expected).abs() < 1e-12,
                "{shape:?} at {t} moved"
            );
        }
    }
}

#[test]
fn tension_bends_the_shape_both_ways() {
    // The claim the gesture makes: dragging a segment up puts it above the
    // unbent line the whole way, and down puts it below.
    let flat = CurveShape::Linear.eased(0.5, 0.0);
    let up = CurveShape::Linear.eased(0.5, 0.7);
    let down = CurveShape::Linear.eased(0.5, -0.7);
    assert!(up > flat + 0.05, "a positive tension lifts it: {up}");
    assert!(down < flat - 0.05, "a negative one drops it: {down}");

    for step in 1..32 {
        let t = step as f64 / 32.0;
        assert!(CurveShape::Linear.eased(t, 0.7) > CurveShape::Linear.eased(t, 0.0));
        assert!(CurveShape::Linear.eased(t, -0.7) < CurveShape::Linear.eased(t, 0.0));
    }
}

#[test]
fn the_flat_shapes_hold_and_only_one_of_them_freezes() {
    assert!(CurveShape::Stepped.holds());
    assert!(CurveShape::Hold.holds());
    assert!(CurveShape::Hold.freezes(), "Hold is the full stop");
    assert!(
        !CurveShape::Stepped.freezes(),
        "a staircase jumps at the next point"
    );
    assert!(!CurveShape::Linear.holds());
}

#[test]
fn a_tension_past_the_ends_is_clamped_rather_than_wild() {
    // Reachable from a drag that ran off the top of the lane.
    for tension in [-9.0, 9.0, f32::NAN] {
        let value = CurveShape::Linear.eased(0.5, tension);
        assert!(
            value.is_finite() && (0.0..=1.0).contains(&value),
            "tension {tension} gave {value}"
        );
    }
}
