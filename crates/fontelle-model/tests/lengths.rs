//! Lengths: legato, staccato and the rest (FL's Articulate, Live's Span).

mod tool_kit;

use fontelle_model::{LengthsMode, LengthsSpec, legato_lengths_to, lengthened};
use fontelle_types::PPQN;
use tool_kit::{note, scope};

fn lengths(notes: Vec<fontelle_model::Note>, spec: LengthsSpec) -> Vec<i64> {
    let s = scope(notes);
    lengthened(&s, spec, Some(PPQN * 4))
        .after(&s)
        .iter()
        .map(|n| n.length)
        .collect()
}

fn with(mode: LengthsMode) -> LengthsSpec {
    LengthsSpec {
        mode,
        ..LengthsSpec::default()
    }
}

fn phrase() -> Vec<fontelle_model::Note> {
    vec![note(0, 100, 60), note(480, 700, 62), note(960, 240, 64)]
}

#[test]
fn legato_is_ctrl_l() {
    let spans: Vec<(i64, i64)> = phrase().iter().map(|n| (n.start, n.length)).collect();
    assert_eq!(
        lengths(phrase(), with(LengthsMode::Legato)),
        legato_lengths_to(&spans, Some(PPQN * 4))
    );
}

#[test]
fn legato_with_a_gap_stops_short_of_the_next() {
    let mut spec = with(LengthsMode::Legato);
    spec.gap = 30;
    assert_eq!(lengths(phrase(), spec), vec![450, 450, PPQN * 4 - 960 - 30]);
}

#[test]
fn staccato_shortens_by_a_share() {
    let mut spec = with(LengthsMode::Staccato);
    spec.percent = 50;
    assert_eq!(lengths(phrase(), spec), vec![50, 350, 120]);
}

#[test]
fn fixed_at_least_and_at_most() {
    let mut spec = with(LengthsMode::Fixed);
    spec.length = 240;
    assert_eq!(lengths(phrase(), spec), vec![240, 240, 240]);
    spec.mode = LengthsMode::AtLeast;
    assert_eq!(lengths(phrase(), spec), vec![240, 700, 240]);
    spec.mode = LengthsMode::AtMost;
    assert_eq!(lengths(phrase(), spec), vec![100, 240, 240]);
}

#[test]
fn gap_only_ever_shortens() {
    let mut spec = with(LengthsMode::Gap);
    spec.gap = 0;
    assert_eq!(lengths(phrase(), spec), vec![100, 480, 240]);
    spec.gap = 60;
    assert_eq!(lengths(phrase(), spec), vec![100, 420, 240]);
}

#[test]
fn no_length_is_ever_nothing() {
    let mut spec = with(LengthsMode::Staccato);
    spec.percent = 1;
    let mut gap = with(LengthsMode::Gap);
    gap.gap = 10_000;
    for spec in [spec, gap] {
        assert!(lengths(phrase(), spec).iter().all(|l| *l >= 1));
    }
}
