//! Thin out — FL's Claw machine: every Nth slice emptied.

mod tool_kit;

use fontelle_model::{ThinSpec, thinned};
use fontelle_types::PPQN;
use tool_kit::{note, scope};

const S: i64 = PPQN / 4;

fn eight() -> Vec<(fontelle_types::NoteId, fontelle_model::Note)> {
    scope((0..8).map(|i| note(i * S, S, 60)).collect())
}

fn starts(spec: ThinSpec) -> Vec<(i64, i64)> {
    let s = eight();
    thinned(&s, spec)
        .after(&s)
        .iter()
        .map(|n| (n.start, n.length))
        .collect()
}

#[test]
fn every_second_slice_goes() {
    let spec = ThinSpec {
        period: S,
        every: 2,
        stretch: false,
    };
    assert_eq!(
        starts(spec),
        vec![(0, S), (2 * S, S), (4 * S, S), (6 * S, S)]
    );
}

#[test]
fn every_third_slice_goes() {
    let spec = ThinSpec {
        period: S,
        every: 3,
        stretch: false,
    };
    let st: Vec<i64> = starts(spec).into_iter().map(|(s, _)| s / S).collect();
    assert_eq!(st, vec![0, 1, 3, 4, 6, 7]);
}

#[test]
fn stretched_what_is_left_fills_the_span_it_had() {
    let spec = ThinSpec {
        period: S,
        every: 2,
        stretch: true,
    };
    assert_eq!(
        starts(spec),
        vec![(0, 2 * S), (2 * S, 2 * S), (4 * S, 2 * S), (6 * S, 2 * S)]
    );
}
