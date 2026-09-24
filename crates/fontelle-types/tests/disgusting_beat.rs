//! The bank, the grid it realises into, and the edits that change it
//! (`docs/disgusting-beat-plan.md` §6, §10.3).
//!
//! The generic parameter tests cover the config the moment the kind is in
//! `EffectKind::ALL`. What is here is the rules that are this effect's own.

use fontelle_types::{
    CurveShape, DISGUSTING_BEAT_LANES, DISGUSTING_BEAT_POINTS, DISGUSTING_BEAT_SCENES,
    DisgustingBeatBank, DisgustingBeatConfig, DisgustingBeatEdit, DisgustingBeatFactoryPreset,
    DisgustingBeatGrid, DisgustingBeatLaneKind, DisgustingBeatLength, DisgustingBeatLook,
    DisgustingBeatPoint, EffectConfig, EffectKind, curve_at,
};

fn bank() -> DisgustingBeatBank {
    DisgustingBeatBank::new()
}

#[test]
fn a_fresh_bank_is_twelve_flat_scenes() {
    let bank = bank();
    assert_eq!(bank.scenes.len(), DISGUSTING_BEAT_SCENES);
    for scene in &bank.scenes {
        assert_eq!(scene.lanes.len(), DISGUSTING_BEAT_LANES);
        assert!(scene.is_flat(), "and every one of them is a wire");
    }
    // Time and volume are drawn; tone and pan wait to be asked for.
    let scene = &bank.scenes[0];
    assert!(scene.lane(DisgustingBeatLaneKind::Time).unwrap().on);
    assert!(scene.lane(DisgustingBeatLaneKind::Volume).unwrap().on);
    assert!(!scene.lane(DisgustingBeatLaneKind::Tone).unwrap().on);
    assert!(!scene.lane(DisgustingBeatLaneKind::Pan).unwrap().on);
}

#[test]
fn a_fresh_disgusting_beat_config_is_a_wire() {
    let config = DisgustingBeatConfig::new();
    assert_eq!(config.mix, 1.0, "fully wet: it replaces the signal");
    assert_eq!(config.scene, 0);
    assert_eq!(config.tone, 0.0, "the two extra lanes are off");
    assert_eq!(config.pan, 0.0);
    assert_eq!(config.output_db, 0.0);
    assert_eq!(
        EffectConfig::new(EffectKind::DisgustingBeat),
        EffectConfig::DisgustingBeat(config)
    );
}

#[test]
fn every_edit_undoes_itself() {
    // The algebra: `apply` hands back the inverse of what it just did, so the
    // command has nothing to work out and no second copy of these rules.
    let edits = [
        DisgustingBeatEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: DisgustingBeatPoint::new(0.5, -0.25, CurveShape::Linear),
        },
        DisgustingBeatEdit::SetCurve {
            scene: 0,
            lane: 1,
            index: 0,
            curve: CurveShape::SCurve,
        },
        DisgustingBeatEdit::SetTension {
            scene: 0,
            lane: 1,
            index: 0,
            tension: 0.4,
        },
        DisgustingBeatEdit::SetLength {
            scene: 0,
            lane: 0,
            length: DisgustingBeatLength::TwoBars,
        },
        DisgustingBeatEdit::SetLaneOn {
            scene: 0,
            lane: 2,
            on: true,
        },
        DisgustingBeatEdit::RenameScene {
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
        .apply(&DisgustingBeatEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: DisgustingBeatPoint::new(0.5, -0.25, CurveShape::Linear),
        })
        .unwrap();
    let before = subject.clone();

    let undo = subject
        .apply(&DisgustingBeatEdit::MovePoint {
            scene: 0,
            lane: 0,
            index: 1,
            to: (0.75, -0.5),
        })
        .expect("it moved");
    subject.apply(&undo).unwrap();
    assert_eq!(subject, before, "a move undoes itself");

    let undo = subject
        .apply(&DisgustingBeatEdit::RemovePoint {
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
            .apply(&DisgustingBeatEdit::SetLaneOn {
                scene: 0,
                lane: 0,
                on: true,
            })
            .is_none(),
        "the time lane is already on"
    );
    assert!(
        subject
            .apply(&DisgustingBeatEdit::RenameScene {
                scene: 0,
                name: String::new(),
            })
            .is_none(),
        "and it is already unnamed"
    );
    assert!(
        subject
            .apply(&DisgustingBeatEdit::SetLength {
                scene: 0,
                lane: 0,
                length: DisgustingBeatLength::Bar,
            })
            .is_none(),
        "and already a bar long"
    );
    // A scene that does not exist is refused rather than panicking.
    assert!(
        subject
            .apply(&DisgustingBeatEdit::ClearLane { scene: 99, lane: 0 })
            .is_none()
    );
}

#[test]
fn points_stay_sorted() {
    let mut subject = bank();
    for at in [0.75, 0.25, 0.5] {
        subject
            .apply(&DisgustingBeatEdit::AddPoint {
                scene: 0,
                lane: 0,
                point: DisgustingBeatPoint::new(at, -0.1, CurveShape::Linear),
            })
            .unwrap();
    }
    let lane = subject.scenes[0]
        .lane(DisgustingBeatLaneKind::Time)
        .unwrap();
    assert!(
        lane.points.windows(2).all(|w| w[0].at <= w[1].at),
        "sorted: {:?}",
        lane.points.iter().map(|p| p.at).collect::<Vec<_>>()
    );
}

#[test]
fn two_points_at_one_phase_are_a_vertical() {
    // > *"i cannot make 2 points on the same x position because it will
    // > delete one of them so i cant have it go from quite to immidiately
    // > audible."* — Ty, 2026-09-23
    //
    // A pair at one phase is the instant: the **first** is what the lane is
    // worth arriving there, the second what it is worth leaving. Which is
    // what dragging one point onto another has to mean, because that is what
    // somebody is drawing when they do it.
    let mut subject = bank();
    let lane = subject.scenes[0]
        .lane_mut(DisgustingBeatLaneKind::Volume)
        .unwrap();
    lane.points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 1.0, CurveShape::Linear),
    ];
    lane.tidy(DisgustingBeatLaneKind::Volume);
    assert_eq!(lane.points.len(), 3, "the pair survived tidying");

    let quiet = lane.value_at(0.499, DisgustingBeatLaneKind::Volume);
    let loud = lane.value_at(0.5, DisgustingBeatLaneKind::Volume);
    assert!(quiet < 0.01, "silent up to the instant: {quiet}");
    assert!(loud > 0.99, "and at full the sample after it: {loud}");

    // And it survives the crossing to the audio thread, which reads the same
    // function over a fixed array.
    let grid = DisgustingBeatGrid::from(&subject);
    let lane = grid.lane(0, DisgustingBeatLaneKind::Volume);
    assert!(lane.value_at(0.499, 1.0) < 0.01);
    assert!(lane.value_at(0.5, 1.0) > 0.99);
}

#[test]
fn a_third_point_at_one_phase_is_refused() {
    // Two is a vertical; three is a vertical with something invisible inside
    // it. The later one wins, which is what dragging a point onto a pair
    // means, and the pair stays a pair however many times that happens.
    let mut subject = bank();
    let lane = subject.scenes[0]
        .lane_mut(DisgustingBeatLaneKind::Volume)
        .unwrap();
    lane.points = vec![
        DisgustingBeatPoint::new(0.5, 0.1, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.2, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.3, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.4, CurveShape::Linear),
    ];
    lane.tidy(DisgustingBeatLaneKind::Volume);
    assert_eq!(lane.points.len(), 2);
    assert_eq!(lane.points[0].value, 0.1, "the arrival is the first one");
    assert_eq!(lane.points[1].value, 0.4, "and the last one wins the exit");
}

#[test]
fn a_pair_at_one_place_is_one_point() {
    // Two points at the same phase *and* the same value draw one dot and
    // sound like one point. Keeping both would leave a handle nobody can
    // pick up because it is underneath another one.
    let mut subject = bank();
    let lane = subject.scenes[0]
        .lane_mut(DisgustingBeatLaneKind::Volume)
        .unwrap();
    lane.points = vec![
        DisgustingBeatPoint::new(0.5, 0.25, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.25, CurveShape::Linear),
    ];
    lane.tidy(DisgustingBeatLaneKind::Volume);
    assert_eq!(lane.points.len(), 1);
}

#[test]
fn a_lane_always_has_a_point() {
    // An empty lane would have to mean something, and the two candidates —
    // silence, and the wire — are both wrong half the time.
    let mut subject = bank();
    subject
        .apply(&DisgustingBeatEdit::ClearLane { scene: 0, lane: 1 })
        .map(|_| ())
        .unwrap_or_default();
    let lane = subject.scenes[0]
        .lane(DisgustingBeatLaneKind::Volume)
        .unwrap();
    assert_eq!(lane.points.len(), 1);
    assert_eq!(lane.points[0].value, 1.0, "at the lane's own neutral");

    // And the last point cannot be removed.
    assert!(
        subject
            .apply(&DisgustingBeatEdit::RemovePoint {
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
    for step in 1..DISGUSTING_BEAT_POINTS {
        subject
            .apply(&DisgustingBeatEdit::AddPoint {
                scene: 0,
                lane: 0,
                point: DisgustingBeatPoint::new(step as f64 / 100.0, -0.1, CurveShape::Linear),
            })
            .unwrap_or_else(|| panic!("point {step} was refused"));
    }
    assert_eq!(
        subject.scenes[0]
            .lane(DisgustingBeatLaneKind::Time)
            .unwrap()
            .points
            .len(),
        DISGUSTING_BEAT_POINTS
    );
    assert!(
        subject
            .apply(&DisgustingBeatEdit::AddPoint {
                scene: 0,
                lane: 0,
                point: DisgustingBeatPoint::new(0.999, -0.1, CurveShape::Linear),
            })
            .is_none(),
        "the cap is a rule with a message, not a silent truncation"
    );
}

#[test]
fn a_value_off_the_lane_is_clamped_to_it() {
    let mut subject = bank();
    subject
        .apply(&DisgustingBeatEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: DisgustingBeatPoint::new(0.5, -9.0, CurveShape::Linear),
        })
        .unwrap();
    let lane = subject.scenes[0]
        .lane(DisgustingBeatLaneKind::Time)
        .unwrap();
    assert_eq!(lane.points[1].value, -1.0, "one lane-length is the floor");
}

#[test]
fn copying_a_scene_copies_its_curves_and_not_its_name() {
    let mut subject = bank();
    subject
        .apply(&DisgustingBeatEdit::AddPoint {
            scene: 0,
            lane: 0,
            point: DisgustingBeatPoint::new(0.5, -0.5, CurveShape::Linear),
        })
        .unwrap();
    subject
        .apply(&DisgustingBeatEdit::RenameScene {
            scene: 0,
            name: "Hold".to_string(),
        })
        .unwrap();
    subject
        .apply(&DisgustingBeatEdit::CopyScene { from: 0, to: 4 })
        .unwrap();
    assert_eq!(
        subject.scenes[4]
            .lane(DisgustingBeatLaneKind::Time)
            .unwrap()
            .points,
        subject.scenes[0]
            .lane(DisgustingBeatLaneKind::Time)
            .unwrap()
            .points
    );
    assert_eq!(subject.scenes[4].name, "Hold", "and the name comes too");
}

#[test]
fn the_grid_is_the_bank_the_audio_thread_can_hold() {
    let mut subject = bank();
    subject
        .apply(&DisgustingBeatEdit::AddPoint {
            scene: 2,
            lane: 0,
            point: DisgustingBeatPoint::new(0.5, -0.25, CurveShape::Stepped),
        })
        .unwrap();
    let grid = DisgustingBeatGrid::from(&subject);
    let lane = grid.lane(2, DisgustingBeatLaneKind::Time);
    assert_eq!(lane.len, 2);
    assert_eq!(lane.points[1].at, 0.5);
    assert_eq!(lane.points[1].value, -0.25);
    assert_eq!(lane.points[1].curve, CurveShape::Stepped);

    // And an empty grid is a wire, so a node with no bank is not a silence.
    let empty = DisgustingBeatGrid::empty();
    assert_eq!(
        empty
            .lane(0, DisgustingBeatLaneKind::Volume)
            .value_at(0.5, 1.0),
        1.0
    );
    assert_eq!(
        empty
            .lane(0, DisgustingBeatLaneKind::Time)
            .value_at(0.5, 0.0),
        0.0
    );
}

#[test]
fn a_lane_holds_its_ends_and_seams_at_its_own_edge() {
    // **The lane ends where the last point is.** Past it the value holds
    // until the lane comes round again, and before the first point it is
    // already at that point's value — so the only place a lane jumps is its
    // own edge, which is where the eye expects one and where the crossfade
    // takes it.
    //
    // It ran back to the first point instead until 2026-09-23, and on the
    // *time* lane that was a speed-up nobody drew: a curve that has fallen
    // has to rise again somewhere to be both continuous and a loop, and the
    // rise landed in the stretch where nothing was drawn at all. Three
    // quarters of a lane-length recovered over the last quarter played the
    // memory back at four times speed, once a bar
    // (`fontelle-fx/tests/disgusting_beat.rs`,
    // `the_stretch_after_the_last_point_holds_instead_of_sprinting_back`).
    let mut subject = bank();
    let lane = subject.scenes[0]
        .lane_mut(DisgustingBeatLaneKind::Volume)
        .unwrap();
    lane.points = vec![
        DisgustingBeatPoint::new(0.25, 1.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.75, 0.0, CurveShape::Linear),
    ];
    let grid = DisgustingBeatGrid::from(&subject);
    let lane = grid.lane(0, DisgustingBeatLaneKind::Volume);
    assert_eq!(lane.value_at(0.0, 1.0), 1.0, "before the first point");
    assert_eq!(lane.value_at(0.1, 1.0), 1.0);
    assert_eq!(lane.value_at(0.9, 1.0), 0.0, "after the last one");
    assert!(
        (lane.value_at(0.5, 1.0) - 0.5).abs() < 1e-6,
        "and the drawn segment still interpolates"
    );
}

#[test]
fn a_bank_from_a_file_is_filled_to_what_the_program_expects() {
    // Short, long, or with a lane missing: `fill` is what makes the rest of
    // the program able to index twelve without asking.
    let mut short = DisgustingBeatBank { scenes: Vec::new() };
    short.fill();
    assert_eq!(short.scenes.len(), DISGUSTING_BEAT_SCENES);
    assert_eq!(short.scenes[0].lanes.len(), DISGUSTING_BEAT_LANES);

    let mut long = DisgustingBeatBank::new();
    let extra = long.scenes[0].clone();
    long.scenes.extend((0..5).map(|_| extra.clone()));
    long.fill();
    assert_eq!(long.scenes.len(), DISGUSTING_BEAT_SCENES);
}

#[test]
fn the_slug_is_fx_disgusting_beat() {
    use fontelle_types::DeviceKind;
    assert_eq!(
        DeviceKind::Effect(EffectKind::DisgustingBeat).slug(),
        "fx-disgusting-beat"
    );
}

#[test]
fn no_factory_row_reads_a_future_it_has_not_got() {
    // **A positive offset is a read of the future**, and there is none unless
    // look-ahead is on — the machine clamps it to now, so the drawing is
    // thrown away sample by sample and the preset is a wire that looks like a
    // preset. *Rushed* shipped that way: four points, ten milliseconds ahead,
    // doing nothing whatever, and no test noticed because a wire is a
    // perfectly good-sounding thing to be.
    //
    // So: anything drawn above the line has to say it needs look-ahead by
    // turning it on, and anything that does not have to stay below it.
    for row in DisgustingBeatFactoryPreset::ALL {
        let (config, bank) = row.build();
        if config.look != DisgustingBeatLook::Off {
            continue;
        }
        for (index, scene) in bank.scenes.iter().enumerate() {
            let Some(lane) = scene.lane(DisgustingBeatLaneKind::Time) else {
                continue;
            };
            if !lane.on || lane.points.is_empty() {
                continue;
            }
            for step in 0..=512 {
                let phase = step as f64 / 512.0;
                let value = curve_at(&lane.points, phase, 0.0);
                assert!(
                    value <= 1e-9,
                    "{} scene {} reads {value:+.3} lane-lengths ahead at phase \
                     {phase:.3}, with look-ahead off. The clamp will eat it and \
                     the preset will do nothing.",
                    row.name,
                    index + 1
                );
            }
        }
    }
}

#[test]
fn a_pair_at_the_lanes_own_edge_keeps_only_the_half_that_sounds() {
    // A vertical needs a *before* and an *after*, and at the edge of the lane
    // one of those is outside it. At phase 0 nothing arrives — the lane has
    // just come round, and what it comes round from is its last point — so
    // the first of a pair there is never read. At phase 1 it is the other way
    // about: nothing leaves.
    //
    // The half that is never read is dropped rather than kept, because a
    // handle that can be picked up and moved and changes nothing is the worst
    // kind of thing this window could offer. The jump at the edge is already
    // drawn: it is the seam.
    let mut subject = bank();
    let lane = subject.scenes[0]
        .lane_mut(DisgustingBeatLaneKind::Volume)
        .unwrap();
    lane.points = vec![
        DisgustingBeatPoint::new(0.0, 0.2, CurveShape::Linear),
        DisgustingBeatPoint::new(0.0, 0.8, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.5, CurveShape::Linear),
        DisgustingBeatPoint::new(1.0, 0.1, CurveShape::Linear),
        DisgustingBeatPoint::new(1.0, 0.9, CurveShape::Linear),
    ];
    lane.tidy(DisgustingBeatLaneKind::Volume);
    let at: Vec<(f64, f64)> = lane.points.iter().map(|p| (p.at, p.value)).collect();
    assert_eq!(
        at,
        vec![(0.0, 0.8), (0.5, 0.5), (1.0, 0.1)],
        "the lane leaves phase 0 and arrives at phase 1"
    );
}
