//! Reading a value off an automation clip (TDD §12).
//!
//! `AutomationData` has been in the document since the format was written and
//! nothing has ever evaluated it. This is the half that turns points into a
//! number, and §12.2's two rules — the ones the TDD says FL leaves implicit
//! and this project must not — are the sharp end of it:
//!
//! 1. **Overlapping clips on the same target: the later one wins**, for the
//!    duration of the overlap. Not blended.
//! 2. **Outside a clip's bounds the parameter holds its last automated
//!    value.** It does not snap back to where the knob was.
//!
//! Both are decisions somebody will otherwise make by accident, differently,
//! in two places.

use fontelle_model::Arena;
use fontelle_model::{
    AutomationData, AutomationPoint, Clip, ClipSource, CurveShape, Lane, Project, automation_at,
};
use fontelle_types::{PPQN, ParamAddress, ParamTarget, Tick};

fn point(tick: Tick, value: f64, curve: CurveShape) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve,
        tension: 0.0,
    }
}

fn clip_of(points: Vec<AutomationPoint>) -> AutomationData {
    let mut arena = Arena::default();
    for p in points {
        arena.insert(p);
    }
    AutomationData {
        target: ParamAddress::new("transport/tempo"),
        points: arena,
    }
}

// ------------------------------------------------------------ one clip

#[test]
fn a_clip_with_no_points_has_no_value_to_give() {
    // Not zero, which would slam every automated parameter to its minimum the
    // moment somebody made an empty clip.
    assert_eq!(clip_of(Vec::new()).value_at(0), None);
}

#[test]
fn one_point_holds_its_value_everywhere() {
    let data = clip_of(vec![point(PPQN, 0.75, CurveShape::Linear)]);
    assert_eq!(data.value_at(0), Some(0.75));
    assert_eq!(data.value_at(PPQN), Some(0.75));
    assert_eq!(data.value_at(PPQN * 10), Some(0.75));
}

#[test]
fn before_the_first_point_the_value_is_the_first_points() {
    // A curve that started somewhere else would be a value nobody drew.
    let data = clip_of(vec![
        point(PPQN * 4, 0.25, CurveShape::Linear),
        point(PPQN * 8, 0.75, CurveShape::Linear),
    ]);
    assert_eq!(data.value_at(0), Some(0.25));
}

#[test]
fn after_the_last_point_the_value_is_the_last_points() {
    let data = clip_of(vec![
        point(0, 0.25, CurveShape::Linear),
        point(PPQN * 4, 0.75, CurveShape::Linear),
    ]);
    assert_eq!(data.value_at(PPQN * 100), Some(0.75));
}

#[test]
fn a_linear_segment_is_a_straight_line_between_its_ends() {
    let data = clip_of(vec![
        point(0, 0.0, CurveShape::Linear),
        point(PPQN * 4, 1.0, CurveShape::Linear),
    ]);
    assert_eq!(data.value_at(0), Some(0.0));
    assert_eq!(data.value_at(PPQN * 2), Some(0.5));
    assert_eq!(data.value_at(PPQN * 4), Some(1.0));
}

#[test]
fn points_are_read_in_time_order_however_they_were_added() {
    // A person draws points wherever they like; the arena keeps insertion
    // order. Reading them unsorted gives a curve that jumps about.
    let data = clip_of(vec![
        point(PPQN * 4, 1.0, CurveShape::Linear),
        point(0, 0.0, CurveShape::Linear),
        point(PPQN * 2, 0.25, CurveShape::Linear),
    ]);
    assert_eq!(data.value_at(PPQN), Some(0.125));
    assert_eq!(data.value_at(PPQN * 3), Some(0.625));
}

#[test]
fn a_stepped_segment_holds_until_the_next_point() {
    // What a rhythmic automation lane is made of, and the shape a sample-rate
    // interpolator would smear away.
    let data = clip_of(vec![
        point(0, 0.2, CurveShape::Stepped),
        point(PPQN * 4, 0.8, CurveShape::Linear),
    ]);
    assert_eq!(data.value_at(PPQN * 2), Some(0.2));
    assert_eq!(data.value_at(PPQN * 4 - 1), Some(0.2));
    assert_eq!(data.value_at(PPQN * 4), Some(0.8));
}

#[test]
fn a_hold_segment_keeps_the_value_past_the_next_point_too() {
    // `Hold` is not `Stepped`: stepped jumps at the next point, hold ignores
    // it. It is how a lane says "leave this alone from here on".
    let data = clip_of(vec![
        point(0, 0.3, CurveShape::Hold),
        point(PPQN * 4, 0.9, CurveShape::Linear),
    ]);
    assert_eq!(data.value_at(PPQN * 2), Some(0.3));
    assert_eq!(data.value_at(PPQN * 6), Some(0.3));
}

#[test]
fn the_curved_shapes_start_and_end_where_the_linear_one_does() {
    // Every shape is an interpolation between the same two points, so they can
    // only differ in the middle. A shape that missed its own endpoint would
    // make a jump at every point it touched.
    for shape in [
        CurveShape::Linear,
        CurveShape::Exponential,
        CurveShape::Logarithmic,
        CurveShape::SCurve,
    ] {
        let data = clip_of(vec![
            point(0, 0.2, shape),
            point(PPQN * 4, 0.8, CurveShape::Linear),
        ]);
        assert_eq!(data.value_at(0), Some(0.2), "{shape:?} at its start");
        assert_eq!(data.value_at(PPQN * 4), Some(0.8), "{shape:?} at its end");
    }
}

#[test]
fn exponential_and_logarithmic_bend_opposite_ways() {
    let half = PPQN * 2;
    let ends = |shape| {
        clip_of(vec![
            point(0, 0.0, shape),
            point(PPQN * 4, 1.0, CurveShape::Linear),
        ])
        .value_at(half)
        .unwrap()
    };
    let exponential = ends(CurveShape::Exponential);
    let logarithmic = ends(CurveShape::Logarithmic);
    assert!(
        exponential < 0.5 - 0.05,
        "exponential starts slow: {exponential}"
    );
    assert!(
        logarithmic > 0.5 + 0.05,
        "logarithmic starts fast: {logarithmic}"
    );
}

#[test]
fn an_s_curve_is_flat_at_both_ends_and_steep_in_the_middle() {
    let data = clip_of(vec![
        point(0, 0.0, CurveShape::SCurve),
        point(PPQN * 4, 1.0, CurveShape::Linear),
    ]);
    let quarter = data.value_at(PPQN).unwrap();
    let middle = data.value_at(PPQN * 2).unwrap();
    assert!((middle - 0.5).abs() < 0.01, "symmetric about its middle");
    assert!(quarter < 0.25, "and eased in: {quarter}");
}

#[test]
fn a_value_never_leaves_the_zero_to_one_range_it_is_stored_in() {
    // §12.1 says normalised 0..1. A curve shape that overshot would drive a
    // parameter past its own range, and the clamp is meant to be the
    // parameter's business rather than every curve's.
    let data = clip_of(vec![
        point(0, 0.0, CurveShape::SCurve),
        point(PPQN * 4, 1.0, CurveShape::Linear),
    ]);
    for tick in 0..(PPQN * 4) {
        let value = data.value_at(tick).unwrap();
        assert!(
            (0.0..=1.0).contains(&value),
            "at {tick} the value was {value}"
        );
    }
}

// --------------------------------------------------- the two §12.2 rules

/// A project with automation clips on one lane, each `(start, length, value)`
/// holding a single flat point.
fn project_with(clips: &[(Tick, Tick, f64)]) -> Project {
    let mut project = Project::new("automation");
    let lane = project.lanes.insert(Lane {
        name: "auto".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    for (start, length, value) in clips {
        let mut points = Arena::default();
        points.insert(point(0, *value, CurveShape::Linear));
        project.clips.insert(Clip {
            lane,
            start: *start,
            length: *length,
            source: ClipSource::Automation(AutomationData {
                target: ParamTarget::Tempo.address(),
                points,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });
    }
    project
}

#[test]
fn a_parameter_with_no_automation_reads_nothing() {
    let project = Project::new("bare");
    assert_eq!(
        automation_at(&project, &ParamTarget::Tempo.address(), 0),
        None
    );
}

#[test]
fn inside_a_clip_the_parameter_takes_the_clips_value() {
    let project = project_with(&[(PPQN * 4, PPQN * 4, 0.6)]);
    let target = ParamTarget::Tempo.address();
    assert_eq!(automation_at(&project, &target, PPQN * 6), Some(0.6));
}

#[test]
fn before_any_clip_starts_the_parameter_is_not_automated_yet() {
    // Rule 2 is about *after*: a clip cannot reach back in time and hold a
    // value before it exists.
    let project = project_with(&[(PPQN * 4, PPQN * 4, 0.6)]);
    let target = ParamTarget::Tempo.address();
    assert_eq!(automation_at(&project, &target, 0), None);
}

#[test]
fn after_a_clip_ends_the_parameter_holds_the_value_it_left() {
    // §12.2 rule 2, and the one people get wrong: it does **not** snap back to
    // where the knob was sitting.
    let project = project_with(&[(0, PPQN * 4, 0.6)]);
    let target = ParamTarget::Tempo.address();
    assert_eq!(automation_at(&project, &target, PPQN * 100), Some(0.6));
}

#[test]
fn where_two_clips_overlap_the_later_one_wins() {
    // §12.2 rule 1, stated out loud. Additive blending was considered and
    // rejected as confusing, and this is the test that keeps somebody from
    // quietly implementing it.
    let project = project_with(&[(0, PPQN * 8, 0.2), (PPQN * 4, PPQN * 8, 0.9)]);
    let target = ParamTarget::Tempo.address();
    assert_eq!(automation_at(&project, &target, PPQN * 2), Some(0.2));
    assert_eq!(
        automation_at(&project, &target, PPQN * 6),
        Some(0.9),
        "in the overlap, the later clip"
    );
    assert_eq!(automation_at(&project, &target, PPQN * 10), Some(0.9));
}

#[test]
fn later_means_later_in_time_not_later_in_the_document() {
    // The order clips happen to sit in the arena is the order they were drawn,
    // which is not the order they play.
    let project = project_with(&[(PPQN * 4, PPQN * 8, 0.9), (0, PPQN * 8, 0.2)]);
    let target = ParamTarget::Tempo.address();
    assert_eq!(automation_at(&project, &target, PPQN * 6), Some(0.9));
}

#[test]
fn a_muted_clip_automates_nothing() {
    let mut project = project_with(&[(0, PPQN * 8, 0.6)]);
    for (_, clip) in project.clips.iter_mut() {
        clip.muted = true;
    }
    let target = ParamTarget::Tempo.address();
    assert_eq!(automation_at(&project, &target, PPQN * 2), None);
}

#[test]
fn a_clip_aimed_at_another_parameter_is_not_this_ones_business() {
    let mut project = project_with(&[(0, PPQN * 8, 0.6)]);
    for (_, clip) in project.clips.iter_mut() {
        if let ClipSource::Automation(data) = &mut clip.source {
            data.target = ParamAddress::new("mixer:7/gain");
        }
    }
    assert_eq!(
        automation_at(&project, &ParamTarget::Tempo.address(), PPQN * 2),
        None
    );
    assert_eq!(
        automation_at(&project, &ParamAddress::new("mixer:7/gain"), PPQN * 2),
        Some(0.6)
    );
}

#[test]
fn a_note_clip_on_the_same_lane_is_ignored() {
    // Automation is a clip type (§12.1), which means a lane can hold both.
    let mut project = project_with(&[(0, PPQN * 8, 0.6)]);
    let lane = project.lanes.keys().next().unwrap();
    let channel = project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
    });
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 8,
        source: ClipSource::Notes(fontelle_model::NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    assert_eq!(
        automation_at(&project, &ParamTarget::Tempo.address(), PPQN * 2),
        Some(0.6)
    );
}

#[test]
fn every_automated_target_in_a_project_can_be_listed() {
    // What the compiler needs to know what to emit, and what the timeline
    // needs to know which lanes to draw.
    let mut project = project_with(&[(0, PPQN * 4, 0.5), (PPQN * 8, PPQN * 4, 0.5)]);
    let lane = project.lanes.keys().next().unwrap();
    let mut points = Arena::default();
    points.insert(point(0, 0.5, CurveShape::Linear));
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Automation(AutomationData {
            target: ParamAddress::new("mixer:7/gain"),
            points,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    let mut targets = fontelle_model::automated_targets(&project);
    targets.sort();
    assert_eq!(
        targets,
        vec![
            ParamAddress::new("mixer:7/gain"),
            ParamTarget::Tempo.address(),
        ],
        "each target once, however many clips aim at it"
    );
}
