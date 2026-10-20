//! Humanize: the randomizer, now with timing and length (Ty, 2026-10-20,
//! `docs/roll-tools-plan.md` §7 Q4). Pitch stays out.

mod tool_kit;

use fontelle_model::{HumanizeSpec, humanized};
use fontelle_types::PPQN;
use tool_kit::{note, scope};

const S: i64 = PPQN / 4;

fn sixteenths() -> Vec<(fontelle_types::NoteId, fontelle_model::Note)> {
    scope(
        (0..16)
            .map(|i| note(i * S, S, 60 + (i % 5) as u8))
            .collect(),
    )
}

fn spec(timing: i64, length: i32, amount: i32) -> HumanizeSpec {
    HumanizeSpec {
        timing,
        length,
        amount,
        ..HumanizeSpec::default()
    }
}

#[test]
fn nothing_asked_is_nothing_done() {
    assert!(humanized(&sixteenths(), spec(0, 0, 0)).is_empty());
}

#[test]
fn starts_wobble_within_the_timing_and_keep_their_order() {
    let s = sixteenths();
    let r = humanized(&s, spec(30, 0, 0));
    let after = r.after(&s);
    assert!(!r.is_empty());
    for (n, (_, was)) in after.iter().zip(&s) {
        assert!((n.start - was.start).abs() <= 30, "{n:?}");
        assert_eq!(n.key, was.key, "pitch never moves");
    }
}

#[test]
fn a_wide_wobble_never_reorders_the_phrase() {
    let s = sixteenths();
    let after = humanized(&s, spec(S * 4, 0, 0)).after(&s);
    let keys: Vec<u8> = after.iter().map(|n| n.key).collect();
    let was: Vec<u8> = s.iter().map(|(_, n)| n.key).collect();
    assert_eq!(keys, was, "time order is the order it was played in");
    assert!(after.iter().all(|n| n.start >= 0));
}

#[test]
fn lengths_wobble_within_their_share() {
    let s = sixteenths();
    let after = humanized(&s, spec(0, 20, 0)).after(&s);
    assert!(after.iter().any(|n| n.length != S));
    assert!(
        after
            .iter()
            .all(|n| (n.length - S).abs() <= S / 5 && n.length >= 1)
    );
}

#[test]
fn the_same_seed_is_the_same_and_the_next_is_not() {
    let s = sixteenths();
    let a = humanized(&s, spec(30, 10, 20));
    assert_eq!(a, humanized(&s, spec(30, 10, 20)));
    let b = humanized(
        &s,
        HumanizeSpec {
            seed: 2,
            ..spec(30, 10, 20)
        },
    );
    assert_ne!(a, b);
}

#[test]
fn the_property_is_randomised_as_before() {
    let s = sixteenths();
    let after = humanized(&s, spec(0, 0, 30)).after(&s);
    assert!(after.iter().any(|n| n.velocity != 100));
    assert!(after.iter().all(|n| n.start % S == 0), "timing left alone");
}
