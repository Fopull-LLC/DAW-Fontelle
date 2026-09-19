//! A drawn LFO shape (`docs/flopsynth-next.md` §3.4): up to sixty-four
//! points across one cycle, each segment bent by a tension, played smooth
//! or as steps. One function says what the shape is worth at a phase —
//! the picture and, when Phase 3 wires it, the voice read the same one —
//! and a patch without a shape writes nothing new.

use fontelle_types::{LfoPoint, LfoShape, LfoShapeMode, LfoWave, MAX_LFO_POINTS};

fn ramp() -> LfoShape {
    LfoShape {
        points: vec![
            LfoPoint {
                x: 0.0,
                y: -1.0,
                tension: 0.0,
            },
            LfoPoint {
                x: 1.0,
                y: 1.0,
                tension: 0.0,
            },
        ],
        grid: 8,
        mode: LfoShapeMode::Smooth,
    }
}

#[test]
fn a_shape_is_read_between_its_points_and_a_tension_bends_the_segment() {
    let shape = ramp();
    assert!((shape.value(0.0) + 1.0).abs() < 1e-6);
    assert!((shape.value(0.5)).abs() < 1e-6, "{}", shape.value(0.5));
    // A cycle: the phase 1.0 is the phase 0.0 again, and just short of it
    // the shape is at its last point.
    assert!((shape.value(0.999) - 1.0).abs() < 1e-2);
    assert!((shape.value(1.0) - shape.value(0.0)).abs() < 1e-6);
    // Past the end it wraps, like a wave.
    assert!((shape.value(1.25) - shape.value(0.25)).abs() < 1e-6);
    // Bent: positive tension is slow to leave, then fast; negative the
    // mirror. The ends are where they were.
    let mut bent = ramp();
    bent.points[0].tension = 0.8;
    assert!(
        bent.value(0.5) < 0.0 - 0.2,
        "slow to leave: {}",
        bent.value(0.5)
    );
    assert!((bent.value(0.0) + 1.0).abs() < 1e-6 && (bent.value(0.999) - 1.0).abs() < 1e-2);
    bent.points[0].tension = -0.8;
    assert!(
        bent.value(0.5) > 0.2,
        "fast off the mark: {}",
        bent.value(0.5)
    );
    // Between the last point and the first, the cycle closes: a shape that
    // ends high and starts low falls across the wrap.
    let mut open = ramp();
    open.points[1].x = 0.75;
    let across = open.value(0.875);
    assert!(across > -1.0 && across < 1.0, "{across}");
}

#[test]
fn a_stepped_shape_holds_each_point_until_the_next() {
    let mut shape = ramp();
    shape.mode = LfoShapeMode::Step;
    assert!((shape.value(0.0) + 1.0).abs() < 1e-6);
    assert!(
        (shape.value(0.49) + 1.0).abs() < 1e-6,
        "held: {}",
        shape.value(0.49)
    );
    assert!((shape.value(0.999) + 1.0).abs() < 1e-6);
}

#[test]
fn a_shape_is_made_from_a_wave_and_never_holds_more_than_sixty_four_points() {
    let from_sine = LfoShape::from_wave(LfoWave::Sine, 16);
    assert_eq!(from_sine.points.len(), 16);
    for point in &from_sine.points {
        assert!((point.y - LfoWave::Sine.value(point.x)).abs() < 1e-5);
    }
    let too_many = LfoShape::from_wave(LfoWave::Triangle, 500);
    assert_eq!(too_many.points.len(), MAX_LFO_POINTS);
    // A point added past the cap is refused; one within lands in x order.
    let mut shape = ramp();
    assert!(shape.add_point(0.5, 0.0));
    assert_eq!(shape.points.len(), 3);
    assert!((shape.points[1].x - 0.5).abs() < 1e-6);
    let mut full = LfoShape::from_wave(LfoWave::Sine, MAX_LFO_POINTS);
    assert!(!full.add_point(0.123, 0.0));
    assert_eq!(full.points.len(), MAX_LFO_POINTS);
    // The first and the last cannot be taken out — a cycle needs its ends
    // — and a middle one can.
    assert!(!shape.remove_point(0));
    assert!(shape.remove_point(1));
    assert_eq!(shape.points.len(), 2);
    // The factory shapes each have a name and a few points, and the
    // thumbnails' function reads them as any shape.
    let presets = LfoShape::presets();
    assert!(presets.len() >= 6);
    for (name, shape) in &presets {
        assert!(
            !name.is_empty() && shape.points.len() >= 2 && shape.points.len() <= MAX_LFO_POINTS
        );
    }
}

#[test]
fn the_shape_is_absent_from_the_file_unless_drawn() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Carrier {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shape: Option<LfoShape>,
    }
    let none = serde_json::to_string(&Carrier { shape: None }).unwrap();
    assert_eq!(none, "{}");
    let some = serde_json::to_string(&Carrier {
        shape: Some(ramp()),
    })
    .unwrap();
    let back: Carrier = serde_json::from_str(&some).unwrap();
    assert_eq!(back.shape, Some(ramp()));
}
