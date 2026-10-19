//! The meter as each editor sees it (Ty, 2026-10-19: everything follows the
//! signature you are in). The arrangement counts from the top of the song;
//! the piano roll counts from its clip's start, so a clip that starts in a
//! 7/8 bar snaps to that bar's eighths in its own ticks.

use fontelle_types::{Meter, MeterChange, PPQN, Signature};
use fontelle_ui::canvas::{MeterView, SnapDivision};

const Q: i64 = PPQN;

fn four_then_seven() -> Meter {
    Meter::with_song(&Signature::new(4, 4), &[MeterChange::new(1, 7, 8)])
}

#[test]
fn the_arrangement_snaps_by_the_song() {
    let m = MeterView::song(four_then_seven());
    assert_eq!(
        m.snap(4 * Q + Q / 2 + 20, SnapDivision::Beat),
        4 * Q + Q / 2
    );
    assert_eq!(m.snap(4 * Q + 100, SnapDivision::Bar), 4 * Q);
    assert_eq!(m.snap(4 * Q + 170, SnapDivision::Step), 4 * Q + Q / 4);
    assert_eq!(m.snap(123, SnapDivision::None), 123);
    assert_eq!(m.snap(-50, SnapDivision::Bar), 0, "never before the song");
}

/// A clip that starts at bar 1 has its own tick 0 on the 7/8 bar line.
#[test]
fn a_clip_snaps_in_its_own_ticks() {
    let m = MeterView::clip(four_then_seven(), 4 * Q);
    assert_eq!(m.snap(Q / 2 + 20, SnapDivision::Beat), Q / 2);
    assert_eq!(m.snap(7 * Q / 2 - 10, SnapDivision::Bar), 7 * Q / 2);
    // Before the clip is before its first bar line, and stays local.
    assert_eq!(m.snap(-Q, SnapDivision::Beat), -Q);
}

#[test]
fn stepping_and_bars_follow_the_signature() {
    let m = MeterView::song(four_then_seven());
    assert_eq!(m.next(4 * Q, SnapDivision::Beat), 4 * Q + Q / 2);
    assert_eq!(m.previous(4 * Q, SnapDivision::Beat), 3 * Q);
    assert_eq!(m.bar_ticks_at(0), 4 * Q);
    assert_eq!(m.bar_ticks_at(4 * Q + 1), 7 * Q / 2);
    assert_eq!(m.bar_start_at(5 * Q), 4 * Q);
}

/// A copy goes as many whole bars later as the stretch it copies spans,
/// keeping its place inside the bar — whatever length those bars are.
#[test]
fn copies_land_whole_bars_later() {
    let m = MeterView::song(four_then_seven());
    // Beats 2 and 3 of bar 1: the copy is beats 2 and 3 of bar 2 (7/8).
    assert_eq!(m.copy_starts(Q, 3 * Q, 1), vec![4 * Q + Q]);
    // A bar and a half from the top: two bars on, then two more.
    assert_eq!(
        m.copy_starts(0, 6 * Q, 2),
        vec![4 * Q + 7 * Q / 2, 4 * Q + 3 * (7 * Q / 2)]
    );
    // Into a bar too short to hold its place, it runs on the same distance
    // past that bar's line: beat 4 of a 4/4 bar copied into 2/4 bars is
    // beat 2 of the bar after.
    let m = MeterView::song(Meter::with_song(
        &Signature::new(4, 4),
        &[MeterChange::new(1, 2, 4)],
    ));
    assert_eq!(m.copy_starts(3 * Q, 4 * Q, 1), vec![4 * Q + 3 * Q]);
    // In a clip's own ticks too.
    let m = MeterView::clip(four_then_seven(), 4 * Q);
    assert_eq!(m.copy_starts(0, Q, 1), vec![7 * Q / 2]);
}

/// The grid's finer lines start again on each bar line, so they meet the
/// bar lines whatever the signature.
#[test]
fn fine_lines_meet_every_bar_line() {
    let m = MeterView::song(Meter::with_song(&Signature::new(5, 7), &[]));
    let bar1 = m.bar_start_at(3000);
    assert_eq!(bar1, 2743);
    let lines = m.fine_lines(2600..3300, Q / 4);
    assert!(lines.contains(&bar1), "{lines:?}");
    assert!(lines.contains(&(bar1 + Q / 4)), "{lines:?}");
}
