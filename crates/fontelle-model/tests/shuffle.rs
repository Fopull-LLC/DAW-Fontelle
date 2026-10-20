//! Shuffle — Live 12's Recombine: keep the rhythm, rearrange the pitches.

mod tool_kit;

use fontelle_model::{ShuffleHow, ShuffleSpec, ShuffleWhat, shuffled};
use tool_kit::{loud, note, scope};

fn melody() -> Vec<(fontelle_types::NoteId, fontelle_model::Note)> {
    scope(vec![
        note(0, 240, 60),
        note(240, 120, 62),
        note(480, 60, 64),
        note(720, 30, 65),
        note(960, 480, 67),
    ])
}

fn keys(spec: ShuffleSpec) -> Vec<u8> {
    let s = melody();
    shuffled(&s, spec).after(&s).iter().map(|n| n.key).collect()
}

fn of(what: ShuffleWhat, how: ShuffleHow) -> ShuffleSpec {
    ShuffleSpec { what, how, seed: 1 }
}

#[test]
fn rotate_and_mirror() {
    assert_eq!(
        keys(of(ShuffleWhat::Pitch, ShuffleHow::Rotate)),
        vec![67, 60, 62, 64, 65]
    );
    assert_eq!(
        keys(of(ShuffleWhat::Pitch, ShuffleHow::Mirror)),
        vec![67, 65, 64, 62, 60]
    );
}

#[test]
fn a_shuffle_is_the_same_pitches_in_another_order_and_the_same_for_a_seed() {
    let a = keys(of(ShuffleWhat::Pitch, ShuffleHow::Shuffle));
    assert_ne!(a, vec![60, 62, 64, 65, 67], "it moved something");
    let mut sorted = a.clone();
    sorted.sort();
    assert_eq!(sorted, vec![60, 62, 64, 65, 67]);
    assert_eq!(a, keys(of(ShuffleWhat::Pitch, ShuffleHow::Shuffle)));
    let others: Vec<Vec<u8>> = (2..6)
        .map(|seed| {
            keys(ShuffleSpec {
                seed,
                ..of(ShuffleWhat::Pitch, ShuffleHow::Shuffle)
            })
        })
        .collect();
    assert!(
        others.iter().any(|o| *o != a),
        "another seed, another order"
    );
}

#[test]
fn the_rhythm_stays_when_the_pitches_move() {
    let s = melody();
    let after = shuffled(&s, of(ShuffleWhat::Pitch, ShuffleHow::Mirror)).after(&s);
    let rhythm: Vec<(i64, i64)> = after.iter().map(|n| (n.start, n.length)).collect();
    let before: Vec<(i64, i64)> = s.iter().map(|(_, n)| (n.start, n.length)).collect();
    assert_eq!(rhythm, before);
}

#[test]
fn velocities_and_lengths_can_be_rearranged_instead() {
    let s = scope(vec![
        loud(0, 240, 60, 10),
        loud(480, 120, 62, 20),
        loud(960, 60, 64, 30),
    ]);
    let v: Vec<u8> = shuffled(&s, of(ShuffleWhat::Velocity, ShuffleHow::Mirror))
        .after(&s)
        .iter()
        .map(|n| n.velocity)
        .collect();
    assert_eq!(v, vec![30, 20, 10]);
    let l: Vec<i64> = shuffled(&s, of(ShuffleWhat::Length, ShuffleHow::Mirror))
        .after(&s)
        .iter()
        .map(|n| n.length)
        .collect();
    assert_eq!(l, vec![60, 120, 240]);
}
