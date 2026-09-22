//! The bank, the grid it realises into, and the edits that change it
//! (`docs/lapse-plan.md` §6, §10.3).
//!
//! The generic parameter tests cover the config the moment the kind is in
//! `EffectKind::ALL`. What is here is the rules that are this effect's own.

use fontelle_types::{
    CurveShape, EffectConfig, EffectKind, LAPSE_LANES, LAPSE_POINTS, LAPSE_SCENES, LapseBank,
    LapseConfig, LapseEdit, LapseGrid, LapseLaneKind, LapseLength, LapsePoint,
};

fn bank() -> LapseBank {
    LapseBank::new()
}

#[test]
fn a_fresh_bank_is_twelve_flat_scenes() {
    let bank = bank();
    assert_eq!(bank.scenes.len(), LAPSE_SCENES);
    for scene in &bank.scenes {
        assert_eq!(scene.lanes.len(), LAPSE_LANES);
        assert!(scene.is_flat(), "and every one of them is a wire");
    }
    // Time and volume are drawn; tone and pan wait to be asked for.
    let scene = &bank.scenes[0];
    assert!(scene.lane(LapseLaneKind::Time).unwrap().on);
    assert!(scene.lane(LapseLaneKind::Volume).unwrap().on);
    assert!(!scene.lane(LapseLaneKind::Tone).unwrap().on);
    assert!(!scene.lane(LapseLaneKind::Pan).unwrap().on);
}

#[test]
fn a_fresh_lapse_config_is_a_wire() {
    let config = LapseConfig::new();
    assert_eq!(config.mix, 1.0, "fully wet: it replaces the signal");
    assert_eq!(config.scene, 0);
    assert_eq!(config.tone, 0.0, "the two extra lanes are off");
    assert_eq!(config.pan, 0.0);
    assert_eq!(config.output_db, 0.0);
    assert_eq!(
        EffectConfig::new(EffectKind::Lapse),
        EffectConfig::Lapse(config)
    );
}

#[test]
fn every_edit_undoes_itself() {
    // The algebra: `apply` hands back the inverse of what it just did, so the
    // command has nothing to work out and no second copy of these rules.
    let edits = [
        LapseEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: LapsePoint::new(0.5, -0.25, CurveShape::Linear),
        },
        LapseEdit::SetCurve {
            scene: 0,
            lane: 1,
            index: 0,
            curve: CurveShape::SCurve,
        },
        LapseEdit::SetTension {
            scene: 0,
            lane: 1,
            index: 0,
            tension: 0.4,
        },
        LapseEdit::SetLength {
            scene: 0,
            lane: 0,
            length: LapseLength::TwoBars,
        },
        LapseEdit::SetLaneOn {
            scene: 0,
            lane: 2,
            on: true,
        },
        LapseEdit::RenameScene {
            scene: 3,
            name: "Baby scratch".to_string(),
        },
    ];
    for edit in edits {
        let mut subject = bank();
        let before = subject.clone();
        let undo = subject
            .apply(&edit)
            .unwrap_or_else(|| panic!("{edit:?} changed nothing"));
        assert_ne!(subject, before, "{edit:?} did nothing");
        subject.apply(&undo).expect("the inverse applies");
        assert_eq!(subject, before, "{edit:?} did not undo itself");
    }
}

#[test]
fn a_move_and_a_remove_undo_themselves_too() {
    // These two need a point to exist first, so they get their own case.
    let mut subject = bank();
    subject
        .apply(&LapseEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: LapsePoint::new(0.5, -0.25, CurveShape::Linear),
        })
        .unwrap();
    let before = subject.clone();

    let undo = subject
        .apply(&LapseEdit::MovePoint {
            scene: 0,
            lane: 0,
            index: 1,
            to: (0.75, -0.5),
        })
        .expect("it moved");
    subject.apply(&undo).unwrap();
    assert_eq!(subject, before, "a move undoes itself");

    let undo = subject
        .apply(&LapseEdit::RemovePoint {
            scene: 0,
            lane: 0,
            index: 1,
        })
        .expect("it went");
    subject.apply(&undo).unwrap();
    assert_eq!(subject, before, "a remove undoes itself");
}

#[test]
fn an_edit_that_changes_nothing_is_refused() {
    // So Ctrl+Z never walks back through edits that never happened.
    let mut subject = bank();
    assert!(
        subject
            .apply(&LapseEdit::SetLaneOn {
                scene: 0,
                lane: 0,
                on: true,
            })
            .is_none(),
        "the time lane is already on"
    );
    assert!(
        subject
            .apply(&LapseEdit::RenameScene {
                scene: 0,
                name: String::new(),
            })
            .is_none(),
        "and it is already unnamed"
    );
    assert!(
        subject
            .apply(&LapseEdit::SetLength {
                scene: 0,
                lane: 0,
                length: LapseLength::Bar,
            })
            .is_none(),
        "and already a bar long"
    );
    // A scene that does not exist is refused rather than panicking.
    assert!(
        subject
            .apply(&LapseEdit::ClearLane { scene: 99, lane: 0 })
            .is_none()
    );
}

#[test]
fn points_stay_sorted_and_no_two_share_a_place() {
    let mut subject = bank();
    for at in [0.75, 0.25, 0.5] {
        subject
            .apply(&LapseEdit::AddPoint {
                scene: 0,
                lane: 0,
                point: LapsePoint::new(at, -0.1, CurveShape::Linear),
            })
            .unwrap();
    }
    let lane = subject.scenes[0].lane(LapseLaneKind::Time).unwrap();
    assert!(
        lane.points.windows(2).all(|w| w[0].at < w[1].at),
        "sorted: {:?}",
        lane.points.iter().map(|p| p.at).collect::<Vec<_>>()
    );

    // Dragging one onto another is the later one winning, not two points at
    // one place — which the RT evaluator's search could not tell apart.
    let count = lane.points.len();
    subject
        .apply(&LapseEdit::MovePoint {
            scene: 0,
            lane: 0,
            index: 1,
            to: (0.5, -0.9),
        })
        .unwrap();
    let lane = subject.scenes[0].lane(LapseLaneKind::Time).unwrap();
    assert_eq!(lane.points.len(), count - 1, "two became one");
}

#[test]
fn a_lane_always_has_a_point() {
    // An empty lane would have to mean something, and the two candidates —
    // silence, and the wire — are both wrong half the time.
    let mut subject = bank();
    subject
        .apply(&LapseEdit::ClearLane { scene: 0, lane: 1 })
        .map(|_| ())
        .unwrap_or_default();
    let lane = subject.scenes[0].lane(LapseLaneKind::Volume).unwrap();
    assert_eq!(lane.points.len(), 1);
    assert_eq!(lane.points[0].value, 1.0, "at the lane's own neutral");

    // And the last point cannot be removed.
    assert!(
        subject
            .apply(&LapseEdit::RemovePoint {
                scene: 0,
                lane: 1,
                index: 0,
            })
            .is_none()
    );
}

#[test]
fn the_sixty_fifth_point_is_refused() {
    let mut subject = bank();
    for step in 1..LAPSE_POINTS {
        subject
            .apply(&LapseEdit::AddPoint {
                scene: 0,
                lane: 0,
                point: LapsePoint::new(step as f64 / 100.0, -0.1, CurveShape::Linear),
            })
            .unwrap_or_else(|| panic!("point {step} was refused"));
    }
    assert_eq!(
        subject.scenes[0]
            .lane(LapseLaneKind::Time)
            .unwrap()
            .points
            .len(),
        LAPSE_POINTS
    );
    assert!(
        subject
            .apply(&LapseEdit::AddPoint {
                scene: 0,
                lane: 0,
                point: LapsePoint::new(0.999, -0.1, CurveShape::Linear),
            })
            .is_none(),
        "the cap is a rule with a message, not a silent truncation"
    );
}

#[test]
fn a_value_off_the_lane_is_clamped_to_it() {
    let mut subject = bank();
    subject
        .apply(&LapseEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: LapsePoint::new(0.5, -9.0, CurveShape::Linear),
        })
        .unwrap();
    let lane = subject.scenes[0].lane(LapseLaneKind::Time).unwrap();
    assert_eq!(lane.points[1].value, -1.0, "one lane-length is the floor");
}

#[test]
fn copying_a_scene_copies_its_curves_and_not_its_name() {
    let mut subject = bank();
    subject
        .apply(&LapseEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: LapsePoint::new(0.5, -0.5, CurveShape::Linear),
        })
        .unwrap();
    subject
        .apply(&LapseEdit::RenameScene {
            scene: 0,
            name: "Hold".to_string(),
        })
        .unwrap();
    subject
        .apply(&LapseEdit::CopyScene { from: 0, to: 4 })
        .unwrap();
    assert_eq!(
        subject.scenes[4].lane(LapseLaneKind::Time).unwrap().points,
        subject.scenes[0].lane(LapseLaneKind::Time).unwrap().points
    );
    assert_eq!(subject.scenes[4].name, "Hold", "and the name comes too");
}

#[test]
fn the_grid_is_the_bank_the_audio_thread_can_hold() {
    let mut subject = bank();
    subject
        .apply(&LapseEdit::AddPoint {
            scene: 2,
            lane: 0,
            point: LapsePoint::new(0.5, -0.25, CurveShape::Stepped),
        })
        .unwrap();
    let grid = LapseGrid::from(&subject);
    let lane = grid.lane(2, LapseLaneKind::Time);
    assert_eq!(lane.len, 2);
    assert_eq!(lane.points[1].at, 0.5);
    assert_eq!(lane.points[1].value, -0.25);
    assert_eq!(lane.points[1].curve, CurveShape::Stepped);

    // And an empty grid is a wire, so a node with no bank is not a silence.
    let empty = LapseGrid::empty();
    assert_eq!(empty.lane(0, LapseLaneKind::Volume).value_at(0.5, 1.0), 1.0);
    assert_eq!(empty.lane(0, LapseLaneKind::Time).value_at(0.5, 0.0), 0.0);
}

#[test]
fn a_lane_is_a_loop() {
    // The segment after the last point runs round to the first: a pattern
    // that stopped at its last point would jump at the bar line.
    let mut subject = bank();
    let lane = subject.scenes[0].lane_mut(LapseLaneKind::Volume).unwrap();
    lane.points = vec![
        LapsePoint::new(0.25, 1.0, CurveShape::Linear),
        LapsePoint::new(0.75, 0.0, CurveShape::Linear),
    ];
    let grid = LapseGrid::from(&subject);
    let lane = grid.lane(0, LapseLaneKind::Volume);
    // Half way round the wrapping segment, from 0.75 back to 0.25.
    let wrapped = lane.value_at(0.0, 1.0);
    assert!(
        (wrapped - 0.5).abs() < 1e-6,
        "the wrap interpolates: {wrapped}"
    );
}

#[test]
fn a_bank_from_a_file_is_filled_to_what_the_program_expects() {
    // Short, long, or with a lane missing: `fill` is what makes the rest of
    // the program able to index twelve without asking.
    let mut short = LapseBank { scenes: Vec::new() };
    short.fill();
    assert_eq!(short.scenes.len(), LAPSE_SCENES);
    assert_eq!(short.scenes[0].lanes.len(), LAPSE_LANES);

    let mut long = LapseBank::new();
    let extra = long.scenes[0].clone();
    long.scenes.extend((0..5).map(|_| extra.clone()));
    long.fill();
    assert_eq!(long.scenes.len(), LAPSE_SCENES);
}

#[test]
fn the_slug_is_fx_lapse() {
    use fontelle_types::DeviceKind;
    assert_eq!(DeviceKind::Effect(EffectKind::Lapse).slug(), "fx-lapse");
}
