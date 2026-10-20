//! Choosing notes by what they are — FL's Select menu.

mod tool_kit;

use fontelle_model::{SelectHow, selected_where};
use fontelle_types::PPQN;
use tool_kit::{note, scope};

const S: i64 = PPQN / 4;

fn part() -> Vec<(fontelle_types::NoteId, fontelle_model::Note)> {
    scope(vec![
        note(0, S, 60),
        note(S, S * 2, 62),
        note(PPQN, PPQN, 60),
        note(PPQN + S, S, 60),
        note(PPQN * 2, S / 2, 64),
    ])
}

fn pick(how: SelectHow, chosen: &[usize]) -> Vec<usize> {
    let s = part();
    let chosen: Vec<_> = chosen.iter().map(|i| s[*i].0).collect();
    let mut got: Vec<usize> = selected_where(&s, &chosen, how, S, 1)
        .into_iter()
        .map(|id| s.iter().position(|(i, _)| *i == id).unwrap())
        .collect();
    got.sort();
    got
}

#[test]
fn same_key_as_what_is_chosen() {
    assert_eq!(pick(SelectHow::SameKey, &[0]), vec![0, 2, 3]);
    assert!(pick(SelectHow::SameKey, &[]).is_empty());
}

#[test]
fn overlapping_is_the_hatched_ones() {
    assert_eq!(pick(SelectHow::Overlapping, &[]), vec![2, 3]);
}

#[test]
fn off_beat_and_invert() {
    assert_eq!(pick(SelectHow::OffBeat, &[]), vec![1, 3]);
    assert_eq!(pick(SelectHow::Invert, &[0, 1]), vec![2, 3, 4]);
}

#[test]
fn short_and_long_against_the_grid() {
    assert_eq!(pick(SelectHow::Short, &[]), vec![4]);
    assert_eq!(pick(SelectHow::Long, &[]), vec![1, 2]);
}

#[test]
fn random_is_some_but_not_all_and_repeatable() {
    let a = pick(SelectHow::Random, &[]);
    assert!(!a.is_empty() && a.len() < 5, "{a:?}");
    assert_eq!(a, pick(SelectHow::Random, &[]));
}
