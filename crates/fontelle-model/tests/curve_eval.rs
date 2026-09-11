//! Evaluating a curve from a list of points that is already in order.
//!
//! The arrangement draws an automation clip's curve from a flattened list of
//! its points (a canvas may not see a `Project`, INVARIANT 2), and it has to
//! draw the *same* curve the audio thread hears. Two evaluators is a place for
//! them to disagree, so `AutomationData::value_at` is written over one shared
//! function that takes the sorted points, and that function is public.

use fontelle_model::{Arena, AutomationData, AutomationPoint, CurveShape, curve_value};
use fontelle_types::{PPQN, ParamAddress, Tick};

fn point(tick: Tick, value: f64, curve: CurveShape) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve,
        tension: 0.0,
    }
}

fn data_of(points: &[AutomationPoint]) -> AutomationData {
    let mut arena = Arena::default();
    // Inserted backwards, so the arena's own order is *not* time order and
    // the two evaluators only agree if the sorting is done where it should be.
    for p in points.iter().rev() {
        arena.insert(*p);
    }
    AutomationData {
        target: ParamAddress::new("mixer:1/gain"),
        points: arena,
    }
}

#[test]
fn the_shared_evaluator_and_the_clips_own_agree_on_every_shape() {
    for shape in [
        CurveShape::Linear,
        CurveShape::Exponential,
        CurveShape::Logarithmic,
        CurveShape::SCurve,
        CurveShape::Stepped,
        CurveShape::Hold,
    ] {
        let points = vec![
            point(0, 0.2, shape),
            point(PPQN, 0.9, CurveShape::Linear),
            point(PPQN * 3, 0.4, shape),
            point(PPQN * 4, 0.0, CurveShape::Linear),
        ];
        let data = data_of(&points);
        let mut sorted = points.clone();
        sorted.sort_by_key(|p| p.tick);
        for tick in (-PPQN..PPQN * 6).step_by(37) {
            assert_eq!(
                curve_value(&sorted, tick),
                data.value_at(tick),
                "{shape:?} at tick {tick}"
            );
        }
    }
}

#[test]
fn no_points_is_no_value() {
    assert_eq!(curve_value(&[], 0), None);
}

#[test]
fn a_straight_segment_is_read_off_exactly() {
    let sorted = [
        point(0, 0.0, CurveShape::Linear),
        point(PPQN * 2, 1.0, CurveShape::Linear),
    ];
    assert_eq!(curve_value(&sorted, PPQN), Some(0.5));
    assert_eq!(curve_value(&sorted, PPQN / 2), Some(0.25));
}
