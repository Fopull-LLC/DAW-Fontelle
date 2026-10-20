//! Speed: double, half, any factor, anchored at the first start.

mod tool_kit;

use fontelle_model::stretched;
use fontelle_types::PPQN;
use tool_kit::{note, scope, shape};

#[test]
fn twice_as_long_from_the_first_start() {
    let s = scope(vec![note(PPQN, 240, 60), note(PPQN + 240, 240, 62)]);
    assert_eq!(
        shape(&s, &stretched(&s, 2.0)),
        vec![(PPQN, 480, 60), (PPQN + 480, 480, 62)]
    );
}

#[test]
fn twice_as_fast() {
    let s = scope(vec![note(0, 240, 60), note(480, 240, 62)]);
    assert_eq!(
        shape(&s, &stretched(&s, 0.5)),
        vec![(0, 120, 60), (240, 120, 62)]
    );
}

#[test]
fn a_tiny_note_never_vanishes_and_a_path_keeps_pace() {
    let mut sliding = note(0, 3, 60);
    sliding.path = vec![fontelle_model::PathPoint { at: 2, offset: 2 }];
    let mut long = note(0, 480, 62);
    long.path = vec![fontelle_model::PathPoint { at: 240, offset: 2 }];
    let s = scope(vec![sliding, long]);
    let after = stretched(&s, 0.25).after(&s);
    assert!(after.iter().all(|n| n.length >= 1));
    let long = after.iter().find(|n| n.key == 62).unwrap();
    assert_eq!(long.path[0].at, 60);
}

#[test]
fn a_factor_of_one_is_nothing() {
    let s = scope(vec![note(0, 240, 60)]);
    assert!(stretched(&s, 1.0).is_empty());
}
