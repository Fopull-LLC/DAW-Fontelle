//! The meter: a song's time signature, and where it changes.
//!
//! Ty, 2026-10-18: *"right clicking the time signature thing up top and a
//! context menu says change time signature point ... the project time
//! signature i guess would be considered the default."* So the song's own
//! signature is the first, and each change starts on a bar line. A beat is
//! a quarter in 4, an eighth in 8: `PPQN × 4 / bottom` ticks, whole for 2,
//! 4, 8 and 16.

use fontelle_types::{Accent, Meter, MeterChange, MeterLine, PPQN, Signature};

const Q: i64 = PPQN;

fn waltz_then_seven_eight() -> Meter {
    // Two bars of 4/4, then 3/4 from bar 2, then 7/8 from bar 4.
    Meter::new(4, &[MeterChange::new(2, 3, 4), MeterChange::new(4, 7, 8)])
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
            MeterLine::Beat | MeterLine::Group => None,
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
            MeterChange::new(4, 5, 4),
            MeterChange::new(2, 3, 4),
            MeterChange::new(4, 6, 8),
        ],
    );
    assert_eq!(m.signature(3), (3, 4));
    assert_eq!(m.signature(4), (6, 8));
}

// ---------------------------------------------------------------------
// Any signature (Ty, 2026-10-19: *"signatures NEED to be flexible enough to
// let you chose whatever top or division number you want so youre not just
// locked to x/4"*).

/// A whole note is `4 × PPQN` ticks, so a beat of 1/`bottom` is
/// `4 × PPQN / bottom` — whole for most bottoms, and for the rest (a 5/7
/// bar) each bar line is rounded from the start of its stretch, so nothing
/// drifts however long the song is.
#[test]
fn any_bottom_is_a_beat_of_that_size() {
    let m = Meter::with_song(
        &Signature::new(4, 4),
        &[
            MeterChange::new(1, 7, 8),
            MeterChange::new(2, 4, 3),
            MeterChange::new(3, 5, 16),
        ],
    );
    assert_eq!(m.tick_of_bar(1), 4 * Q);
    assert_eq!(m.tick_of_bar(2), 4 * Q + 7 * Q / 2);
    // Four thirds of a whole note: 4 × 3840 / 3.
    assert_eq!(m.tick_of_bar(3), 4 * Q + 7 * Q / 2 + 5120);
    assert_eq!(m.tick_of_bar(4), 4 * Q + 7 * Q / 2 + 5120 + 5 * Q / 4);
    assert_eq!(m.signature(3), (5, 16));
    assert_eq!(m.beats_in_bar(2), 4);
}

#[test]
fn a_bottom_that_does_not_divide_never_drifts() {
    let m = Meter::with_song(&Signature::new(5, 7), &[]);
    // 5 × 3840 / 7 = 2742.857… a bar.
    assert_eq!(m.tick_of_bar(1), 2743);
    assert_eq!(m.tick_of_bar(4), 10971);
    // Seven hundred bars is exactly 1 920 000 ticks: no rounding piles up.
    assert_eq!(m.tick_of_bar(700), 1_920_000);
    for bar in [0, 1, 7, 699] {
        assert_eq!(m.bar_at(m.tick_of_bar(bar)), (bar, 0));
        assert_eq!(m.bar_at(m.tick_of_bar(bar + 1) - 1).0, bar);
    }
    // Beats inside a bar are rounded from the bar line too.
    assert_eq!(m.beat_tick(1, 0), 2743);
    assert_eq!(m.beat_tick(0, 1), 549);
    assert_eq!(m.beat_tick(0, 4), 2194);
}

#[test]
fn signatures_are_read_from_what_people_type() {
    assert_eq!(Signature::parse("7/8"), Ok(Signature::new(7, 8)));
    assert_eq!(Signature::parse(" 5 / 4 "), Ok(Signature::new(5, 4)));
    assert_eq!(
        Signature::parse("3+3+2/8"),
        Ok(Signature::new(8, 8).with_groups(vec![3, 3, 2]))
    );
    assert_eq!(Signature::parse("4/3"), Ok(Signature::new(4, 3)));
    for bad in [
        "", "x", "4", "0/4", "4/0", "4/65", "65/4", "3+0/4", "/4", "4/",
    ] {
        assert!(Signature::parse(bad).is_err(), "{bad:?} was taken");
    }
    // Said back the way it is written: the grouping only when it is not
    // the one the signature has anyway.
    assert_eq!(Signature::new(7, 8).label(), "7/8");
    assert_eq!(
        Signature::new(7, 8).with_groups(vec![2, 2, 3]).label(),
        "7/8"
    );
    assert_eq!(
        Signature::new(7, 8).with_groups(vec![3, 2, 2]).label(),
        "3+2+2/8"
    );
    assert!(
        Signature::new(8, 8)
            .with_groups(vec![3, 3])
            .check()
            .is_err(),
        "groups must add up"
    );
}

/// How the beats of a bar fall into groups: compound signatures in threes,
/// odd eighths in twos ending on a three, the rest one beat at a time — or
/// however the person said.
#[test]
fn beats_fall_into_groups() {
    assert_eq!(Signature::new(6, 8).groups(), vec![3, 3]);
    assert_eq!(Signature::new(12, 8).groups(), vec![3, 3, 3, 3]);
    assert_eq!(Signature::new(7, 8).groups(), vec![2, 2, 3]);
    assert_eq!(Signature::new(5, 8).groups(), vec![2, 3]);
    assert_eq!(Signature::new(4, 4).groups(), vec![1, 1, 1, 1]);
    assert_eq!(
        Signature::new(7, 8).with_groups(vec![3, 2, 2]).groups(),
        vec![3, 2, 2]
    );
}

/// What the metronome plays: the bar's first beat strongest, the start of
/// each group next, the other beats softest — in whatever signature the
/// bar is in.
#[test]
fn the_clicks_follow_the_bar_they_are_in() {
    let m = Meter::with_song(
        &Signature::new(4, 4),
        &[MeterChange::new(1, 7, 8).with_groups(vec![2, 2, 3])],
    );
    let clicks = m.clicks(0..m.tick_of_bar(2));
    let (ticks, accents): (Vec<i64>, Vec<Accent>) = clicks.into_iter().unzip();
    assert_eq!(
        ticks,
        vec![
            0,
            Q,
            2 * Q,
            3 * Q,
            4 * Q,
            4 * Q + Q / 2,
            5 * Q,
            5 * Q + Q / 2,
            6 * Q,
            6 * Q + Q / 2,
            7 * Q
        ]
    );
    use Accent::{Bar, Beat, Group};
    assert_eq!(
        accents,
        vec![
            Bar, Beat, Beat, Beat, Bar, Beat, Group, Beat, Group, Beat, Beat
        ]
    );
    assert_eq!(m.accent(1, 2), Group);
    assert_eq!(m.accent(0, 2), Beat, "4/4 has no groups to mark");
}

#[test]
fn group_starts_are_lines_of_their_own() {
    let m = Meter::with_song(&Signature::new(6, 8), &[]);
    let lines = m.lines(0..m.tick_of_bar(1));
    assert_eq!(lines[0], (0, MeterLine::Bar(0)));
    assert!(lines.contains(&(3 * Q / 2, MeterLine::Group)));
    assert!(lines.contains(&(Q / 2, MeterLine::Beat)));
}

/// What the plugins and DisgustingBeat are told: the bar a tick is in, where
/// it starts and ends, and its signature.
#[test]
fn the_bar_around_a_tick() {
    let m = waltz_then_seven_eight();
    let here = m.here(9 * Q + 10);
    assert_eq!(here.bar, 2);
    assert_eq!((here.top, here.bottom), (3, 4));
    assert_eq!(here.bar_start, 8 * Q);
    assert_eq!(here.next_bar, 11 * Q);
}

/// A song written before signatures had groups reads as it was.
#[test]
fn an_old_change_reads_without_groups() {
    let c: MeterChange = serde_json::from_str(r#"{"bar":2,"top":3,"bottom":4}"#).unwrap();
    assert_eq!(c, MeterChange::new(2, 3, 4));
    let back = serde_json::to_string(&c).unwrap();
    assert!(!back.contains("groups"), "{back}");
}

// ---------------------------------------------------------------------
// Snapping by the meter: a grid starts again on every bar line, so a note
// snapped in a 7/8 bar after a 5/7 one lands on that bar's own eighths.

use fontelle_types::GridStep;

fn odd_song() -> Meter {
    // 5/7 (2742.857… a bar), then 7/8 from bar 1.
    Meter::with_song(&Signature::new(5, 7), &[MeterChange::new(1, 7, 8)])
}

#[test]
fn bars_snap_to_the_nearest_bar_line() {
    let m = waltz_then_seven_eight();
    assert_eq!(m.snap(9 * Q, GridStep::Bar), 8 * Q);
    assert_eq!(m.snap(10 * Q, GridStep::Bar), 11 * Q);
}

#[test]
fn beats_snap_to_the_bars_own_beats() {
    let m = odd_song();
    let bar1 = m.tick_of_bar(1);
    assert_eq!(bar1, 2743);
    // An eighth into the 7/8 bar, give or take.
    assert_eq!(m.snap(bar1 + Q / 2 + 30, GridStep::Beat), bar1 + Q / 2);
    // Inside the 5/7 bar: its beats are 548.57… apart.
    assert_eq!(m.snap(600, GridStep::Beat), 549);
}

#[test]
fn a_note_value_grid_starts_again_on_each_bar_line() {
    let m = odd_song();
    let bar1 = m.tick_of_bar(1);
    // Sixteenths counted from bar 1's line, not from tick 0.
    assert_eq!(m.snap(bar1 + 250, GridStep::Ticks(Q / 4)), bar1 + 240);
    // Near the end of a bar, the bar line itself is the nearest line.
    assert_eq!(m.snap(bar1 - 5, GridStep::Ticks(Q / 4)), bar1);
}

#[test]
fn stepping_goes_line_to_line() {
    let m = odd_song();
    let bar1 = m.tick_of_bar(1);
    assert_eq!(m.next_line(0, GridStep::Bar), bar1);
    assert_eq!(m.next_line(bar1, GridStep::Beat), bar1 + Q / 2);
    assert_eq!(m.previous_line(bar1, GridStep::Beat), m.beat_tick(0, 4));
    assert_eq!(m.previous_line(bar1 + 1, GridStep::Bar), bar1);
    assert_eq!(
        m.line_at_or_before(bar1 + 300, GridStep::Ticks(Q / 4)),
        bar1 + 240
    );
    assert_eq!(
        m.previous_line(0, GridStep::Bar),
        0,
        "nothing before the song"
    );
}
