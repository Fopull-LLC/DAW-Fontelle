//! The meter: a song's time signature, and where it changes.
//!
//! Ty, 2026-10-18: *"right clicking the time signature thing up top and a
//! context menu says change time signature point ... the project time
//! signature i guess would be considered the default."* So the song's own
//! signature is the first, and each change starts on a bar line. A beat is
//! a quarter in 4, an eighth in 8: `PPQN × 4 / bottom` ticks, whole for 2,
//! 4, 8 and 16.

use fontelle_types::{Meter, MeterChange, MeterLine, PPQN};

const Q: i64 = PPQN;

fn waltz_then_seven_eight() -> Meter {
    // Two bars of 4/4, then 3/4 from bar 2, then 7/8 from bar 4.
    Meter::new(
        4,
        &[
            MeterChange {
                bar: 2,
                top: 3,
                bottom: 4,
            },
            MeterChange {
                bar: 4,
                top: 7,
                bottom: 8,
            },
        ],
    )
}

#[test]
fn one_signature_is_the_old_arithmetic() {
    let m = Meter::constant(4);
    assert_eq!(m.tick_of_bar(3), 3 * 4 * Q);
    assert_eq!(m.bar_at(5 * Q), (1, Q));
    assert_eq!(m.beats_in_bar(9), 4);
    assert_eq!(m.beat_ticks(0), Q);
}

#[test]
fn bars_start_where_the_changes_put_them() {
    let m = waltz_then_seven_eight();
    assert_eq!(m.tick_of_bar(2), 8 * Q);
    assert_eq!(m.tick_of_bar(3), 8 * Q + 3 * Q);
    assert_eq!(m.tick_of_bar(4), 8 * Q + 6 * Q);
    // 7/8: seven eighths a bar.
    assert_eq!(m.tick_of_bar(5), 14 * Q + 7 * (Q / 2));
    assert_eq!(m.signature(4), (7, 8));
    assert_eq!(m.beat_ticks(4), Q / 2);
    assert_eq!(m.beats_in_bar(1), 4);
    assert_eq!(m.beats_in_bar(2), 3);
}

#[test]
fn a_tick_is_found_in_its_bar() {
    let m = waltz_then_seven_eight();
    assert_eq!(m.bar_at(0), (0, 0));
    assert_eq!(m.bar_at(9 * Q), (2, Q));
    assert_eq!(m.bar_at(14 * Q + Q / 2), (4, Q / 2));
    assert_eq!(m.bar_at(-5), (0, 0), "before the song is bar 1");
    // And each bar start is its own bar.
    for bar in 0..8 {
        assert_eq!(m.bar_at(m.tick_of_bar(bar)), (bar, 0));
    }
}

/// The grid's lines over a stretch: a bar line where each bar starts, a
/// beat line at each of its beats.
#[test]
fn the_lines_over_a_stretch_follow_the_signature() {
    let m = waltz_then_seven_eight();
    let lines = m.lines(8 * Q..15 * Q);
    let bars: Vec<(i64, u32)> = lines
        .iter()
        .filter_map(|(t, l)| match l {
            MeterLine::Bar(n) => Some((*t, *n)),
            MeterLine::Beat => None,
        })
        .collect();
    assert_eq!(bars, vec![(8 * Q, 2), (11 * Q, 3), (14 * Q, 4)]);
    let beats_in_3_4 = lines
        .iter()
        .filter(|(t, l)| *l == MeterLine::Beat && (8 * Q..11 * Q).contains(t))
        .count();
    assert_eq!(
        beats_in_3_4, 2,
        "a 3/4 bar has two beat lines after its bar line"
    );
    assert!(
        lines
            .iter()
            .any(|(t, l)| *l == MeterLine::Beat && *t == 14 * Q + Q / 2),
        "7/8 beats are eighths"
    );
}

#[test]
fn the_nearest_bar_line() {
    let m = waltz_then_seven_eight();
    assert_eq!(m.nearest_bar(9 * Q), 8 * Q);
    assert_eq!(m.nearest_bar(10 * Q), 11 * Q);
}

/// Changes out of order, or two on one bar, are put right: the last one
/// said for a bar is the one it has.
#[test]
fn changes_are_kept_in_order_one_a_bar() {
    let m = Meter::new(
        4,
        &[
            MeterChange {
                bar: 4,
                top: 5,
                bottom: 4,
            },
            MeterChange {
                bar: 2,
                top: 3,
                bottom: 4,
            },
            MeterChange {
                bar: 4,
                top: 6,
                bottom: 8,
            },
        ],
    );
    assert_eq!(m.signature(3), (3, 4));
    assert_eq!(m.signature(4), (6, 8));
}
