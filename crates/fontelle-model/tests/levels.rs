//! Levels: a property shaped across the selection (FL's Scale levels,
//! Cubase's Velocity, Logic's Crescendo). It replaces Adjust.

mod tool_kit;

use fontelle_model::{LevelsMode, LevelsSpec, NoteProperty, levelled};
use tool_kit::{loud, scope};

fn velocities(v: &[u8], spec: LevelsSpec) -> Vec<u8> {
    let s = scope(
        v.iter()
            .enumerate()
            .map(|(i, v)| loud(i as i64 * 480, 240, 60, *v))
            .collect(),
    );
    levelled(&s, spec)
        .after(&s)
        .iter()
        .map(|n| n.velocity)
        .collect()
}

fn with(mode: LevelsMode) -> LevelsSpec {
    LevelsSpec {
        mode,
        ..LevelsSpec::default()
    }
}

#[test]
fn add_adds_and_stops_at_the_top() {
    assert_eq!(
        velocities(&[100, 120], with(LevelsMode::Add)),
        vec![110, 127]
    );
    let mut take = with(LevelsMode::Add);
    take.amount = -30;
    assert_eq!(
        velocities(&[100, 20], take),
        vec![70, 1],
        "never a note-off"
    );
}

#[test]
fn set_puts_everything_at_one_value() {
    let mut set = with(LevelsMode::Set);
    set.amount = 64;
    assert_eq!(velocities(&[10, 100, 127], set), vec![64, 64, 64]);
}

#[test]
fn scale_multiplies() {
    let mut half = with(LevelsMode::Scale);
    half.percent = 50;
    assert_eq!(velocities(&[100, 40], half), vec![50, 20]);
}

#[test]
fn compress_pulls_towards_the_average_and_over_a_hundred_spreads() {
    let mut squeeze = with(LevelsMode::Compress);
    squeeze.percent = 50;
    assert_eq!(velocities(&[40, 120], squeeze), vec![60, 100]);
    squeeze.percent = 150;
    assert_eq!(velocities(&[60, 100], squeeze), vec![50, 110]);
}

#[test]
fn limit_keeps_inside_its_range() {
    let mut limit = with(LevelsMode::Limit);
    limit.low = 50;
    limit.high = 90;
    assert_eq!(velocities(&[40, 120, 70], limit), vec![50, 90, 70]);
}

#[test]
fn a_ramp_runs_from_the_first_start_to_the_last() {
    let mut ramp = with(LevelsMode::Ramp);
    ramp.from = 40;
    ramp.to = 120;
    assert_eq!(velocities(&[100, 100, 100], ramp), vec![40, 80, 120]);
}

#[test]
fn it_shapes_whichever_property_it_is_set_to() {
    let s = scope(vec![loud(0, 240, 60, 100), loud(480, 240, 60, 100)]);
    let spec = LevelsSpec {
        property: NoteProperty::Pan,
        mode: LevelsMode::Set,
        amount: -64,
        ..LevelsSpec::default()
    };
    let after = levelled(&s, spec).after(&s);
    assert!(after.iter().all(|n| n.pan == -64 && n.velocity == 100));
}

#[test]
fn notes_it_does_not_change_are_not_rewritten() {
    let s = scope(vec![loud(0, 240, 60, 50), loud(480, 240, 60, 100)]);
    let mut limit = with(LevelsMode::Limit);
    limit.low = 40;
    limit.high = 90;
    let r = levelled(&s, limit);
    assert_eq!(r.rewrite.len(), 1);
    assert_eq!(r.rewrite[0].0, s[1].0);
}
