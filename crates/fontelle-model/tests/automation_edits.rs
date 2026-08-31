//! Drawing on an automation clip (TDD §12.1, §12.4).
//!
//! `automation.rs` is the half that reads a curve. This is the half that makes
//! one: the commands behind clicking to add a point, dragging it, deleting it,
//! and changing the shape of the segment after it.
//!
//! Every one is a `Command` through `History` (INVARIANT 9), and the drag
//! merges the way every other continuous gesture in this project does — a
//! point dragged across a bar is one undo entry, not sixty.

use fontelle_model::Arena;
use fontelle_model::{
    AddAutomationPoint, AutomationData, AutomationPoint, Clip, ClipSource, Command, CurveShape,
    History, Lane, MoveAutomationPoints, Project, RemoveAutomationPoints, SetPointCurve,
};
use fontelle_types::{ClipId, PPQN, ParamAddress, PointId, Tick};

fn point(tick: Tick, value: f64) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: CurveShape::Linear,
        tension: 0.0,
    }
}

/// A project with one automation clip holding `points`.
fn fixture(points: Vec<AutomationPoint>) -> (Project, ClipId) {
    let mut project = Project::new("edits");
    let lane = project.lanes.insert(Lane {
        name: "auto".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
    });
    let mut arena = Arena::default();
    for p in points {
        arena.insert(p);
    }
    let clip = project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Automation(AutomationData {
            target: ParamAddress::new("mixer:5/gain"),
            points: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    (project, clip)
}

fn points_of(project: &Project, clip: ClipId) -> Vec<(Tick, f64)> {
    let ClipSource::Automation(data) = &project.clips[clip].source else {
        panic!("not an automation clip")
    };
    let mut out: Vec<(Tick, f64)> = data.points.iter().map(|(_, p)| (p.tick, p.value)).collect();
    out.sort_by_key(|(tick, _)| *tick);
    out
}

fn ids_of(project: &Project, clip: ClipId) -> Vec<PointId> {
    let ClipSource::Automation(data) = &project.clips[clip].source else {
        panic!("not an automation clip")
    };
    data.points.keys().collect()
}

// ------------------------------------------------------------------ adding

#[test]
fn a_point_can_be_added_where_it_was_clicked() {
    let (mut project, clip) = fixture(Vec::new());
    AddAutomationPoint::new(clip, point(PPQN, 0.4))
        .apply(&mut project)
        .unwrap();
    assert_eq!(points_of(&project, clip), vec![(PPQN, 0.4)]);
}

#[test]
fn the_id_of_a_new_point_comes_back_so_the_drag_can_continue() {
    // The same handshake drawing a note has: click and drag are one gesture,
    // and the second half cannot start until it knows what the first made.
    let (mut project, clip) = fixture(Vec::new());
    let mut command = AddAutomationPoint::new(clip, point(PPQN, 0.4));
    command.apply(&mut project).unwrap();
    assert!(command.id().is_some());
}

#[test]
fn undoing_an_add_takes_the_point_away_again() {
    let (mut project, clip) = fixture(Vec::new());
    let mut history = History::new();
    history
        .apply(
            Box::new(AddAutomationPoint::new(clip, point(PPQN, 0.4))),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    assert!(points_of(&project, clip).is_empty());
}

#[test]
fn a_value_outside_zero_to_one_is_refused_rather_than_stored() {
    // §12.1 says normalised. A point outside that would drive a parameter past
    // its own range, and the clamp belongs at the door.
    let (mut project, clip) = fixture(Vec::new());
    assert!(
        AddAutomationPoint::new(clip, point(PPQN, 1.5))
            .apply(&mut project)
            .is_err()
    );
    assert!(
        AddAutomationPoint::new(clip, point(PPQN, -0.1))
            .apply(&mut project)
            .is_err()
    );
}

#[test]
fn adding_to_a_clip_that_is_not_automation_is_refused() {
    let mut project = Project::new("notes");
    let lane = project.lanes.insert(Lane {
        name: "l".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
    });
    let channel = project.channels.insert(fontelle_model::Channel {
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        pan: 0.0,
        muted: false,
        soloed: false,
    });
    let clip = project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN,
        source: ClipSource::Notes(fontelle_model::NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    assert!(
        AddAutomationPoint::new(clip, point(0, 0.5))
            .apply(&mut project)
            .is_err()
    );
}

// ------------------------------------------------------------------ moving

#[test]
fn a_point_moves_by_the_deltas_it_is_given() {
    // Deltas relative to the previous step of the same drag, like every other
    // drag in this project — which is what lets one gesture merge into one
    // history entry.
    let (mut project, clip) = fixture(vec![point(PPQN, 0.4)]);
    let ids = ids_of(&project, clip);
    MoveAutomationPoints::new(clip, ids, PPQN, 0.2)
        .apply(&mut project)
        .unwrap();
    assert_eq!(
        points_of(&project, clip),
        vec![(PPQN * 2, 0.6000000000000001)]
    );
}

#[test]
fn a_point_cannot_be_dragged_off_either_end_of_its_range() {
    let (mut project, clip) = fixture(vec![point(PPQN, 0.9)]);
    let ids = ids_of(&project, clip);
    MoveAutomationPoints::new(clip, ids.clone(), 0, 0.5)
        .apply(&mut project)
        .unwrap();
    assert_eq!(points_of(&project, clip)[0].1, 1.0);
    MoveAutomationPoints::new(clip, ids, 0, -5.0)
        .apply(&mut project)
        .unwrap();
    assert_eq!(points_of(&project, clip)[0].1, 0.0);
}

#[test]
fn a_point_cannot_be_dragged_before_the_clip_starts() {
    let (mut project, clip) = fixture(vec![point(PPQN, 0.5)]);
    let ids = ids_of(&project, clip);
    MoveAutomationPoints::new(clip, ids, -PPQN * 10, 0.0)
        .apply(&mut project)
        .unwrap();
    assert_eq!(points_of(&project, clip)[0].0, 0);
}

#[test]
fn a_whole_drag_is_one_undo_entry() {
    let (mut project, clip) = fixture(vec![point(PPQN, 0.4)]);
    let ids = ids_of(&project, clip);
    let mut history = History::new();
    let depth = history.depth();
    for _ in 0..5 {
        history
            .apply(
                Box::new(MoveAutomationPoints::new(clip, ids.clone(), 10, 0.02)),
                &mut project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), depth + 1);

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(
        points_of(&project, clip),
        vec![(PPQN, 0.4)],
        "and undoing it goes back to before the drag, not to its middle"
    );
}

#[test]
fn two_drags_separated_by_a_gesture_break_are_two_entries() {
    let (mut project, clip) = fixture(vec![point(PPQN, 0.4)]);
    let ids = ids_of(&project, clip);
    let mut history = History::new();
    history
        .apply(
            Box::new(MoveAutomationPoints::new(clip, ids.clone(), 10, 0.0)),
            &mut project,
        )
        .unwrap();
    history.break_gesture();
    history
        .apply(
            Box::new(MoveAutomationPoints::new(clip, ids, 10, 0.0)),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(points_of(&project, clip)[0].0, PPQN + 10);
}

#[test]
fn several_points_move_together() {
    let (mut project, clip) = fixture(vec![point(0, 0.2), point(PPQN, 0.4)]);
    let ids = ids_of(&project, clip);
    MoveAutomationPoints::new(clip, ids, PPQN, 0.1)
        .apply(&mut project)
        .unwrap();
    let moved = points_of(&project, clip);
    assert_eq!(moved[0].0, PPQN);
    assert_eq!(moved[1].0, PPQN * 2);
}

// ---------------------------------------------------------------- removing

#[test]
fn a_point_can_be_deleted_and_put_back() {
    let (mut project, clip) = fixture(vec![point(0, 0.2), point(PPQN, 0.4)]);
    let ids = ids_of(&project, clip);
    let mut history = History::new();
    history
        .apply(
            Box::new(RemoveAutomationPoints::new(clip, vec![ids[0]])),
            &mut project,
        )
        .unwrap();
    assert_eq!(points_of(&project, clip).len(), 1);

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(points_of(&project, clip).len(), 2);
    assert_eq!(
        ids_of(&project, clip).len(),
        2,
        "and with the same ids, because the command above it in the history \
         refers to them"
    );
}

// ----------------------------------------------------------------- shaping

#[test]
fn a_segments_shape_can_be_changed() {
    let (mut project, clip) = fixture(vec![point(0, 0.0), point(PPQN * 4, 1.0)]);
    let ids = ids_of(&project, clip);
    SetPointCurve::new(clip, vec![ids[0]], CurveShape::Stepped)
        .apply(&mut project)
        .unwrap();

    let ClipSource::Automation(data) = &project.clips[clip].source else {
        panic!()
    };
    let first = data.points.iter().min_by_key(|(_, p)| p.tick).unwrap().1;
    assert_eq!(first.curve, CurveShape::Stepped);
    // And it is audible in the curve, which is the point of the command.
    assert_eq!(data.value_at(PPQN * 2), Some(0.0));
}

#[test]
fn undoing_a_shape_change_restores_each_points_own_shape() {
    // Two points that disagree, which a single restored value gets wrong.
    let (mut project, clip) = fixture(vec![point(0, 0.0), point(PPQN * 2, 0.5)]);
    let ClipSource::Automation(data) = &mut project.clips[clip].source else {
        panic!()
    };
    let ids: Vec<PointId> = data.points.keys().collect();
    data.points[ids[1]].curve = CurveShape::SCurve;

    let mut history = History::new();
    history
        .apply(
            Box::new(SetPointCurve::new(clip, ids.clone(), CurveShape::Stepped)),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();

    let ClipSource::Automation(data) = &project.clips[clip].source else {
        panic!()
    };
    assert_eq!(data.points[ids[0]].curve, CurveShape::Linear);
    assert_eq!(data.points[ids[1]].curve, CurveShape::SCurve);
}
