//! The arrangement's grid and ruler follow the song's meter: a bar line
//! where each bar starts, whatever its signature (Ty, 2026-10-18).

use fontelle_types::{Meter, MeterChange, PPQN};
use fontelle_ui::canvas::{TimelineLine, TimelineView, timeline_bar_numbers, timeline_grid_lines};
use fontelle_ui::layout::Rect;

const Q: i64 = PPQN;

fn waltz_from_bar_three() -> Meter {
    Meter::new(
        4,
        &[MeterChange {
            bar: 2,
            top: 3,
            bottom: 4,
        }],
    )
}

fn view() -> (TimelineView, Rect) {
    (
        TimelineView {
            scroll_tick: 0,
            pixels_per_tick: 0.05,
            ..TimelineView::default()
        },
        Rect::new(0.0, 0.0, 1200.0, 300.0),
    )
}

#[test]
fn bar_lines_fall_where_the_bars_start() {
    let (v, grid) = view();
    let lines = timeline_grid_lines(&v, grid, &waltz_from_bar_three());
    let bars: Vec<i64> = lines
        .iter()
        .filter(|(_, l)| matches!(l, TimelineLine::Bar | TimelineLine::Phrase))
        .map(|(t, _)| *t)
        .collect();
    for t in [0, 4 * Q, 8 * Q, 11 * Q, 14 * Q, 17 * Q] {
        assert!(bars.contains(&t), "no bar line at {t}: {bars:?}");
    }
    assert!(!bars.contains(&(12 * Q)), "a 4/4 bar line inside the 3/4");
    // Beats at this zoom (48 px a beat): two inside a 3/4 bar.
    let beats = lines
        .iter()
        .filter(|(t, l)| *l == TimelineLine::Beat && (8 * Q..11 * Q).contains(t))
        .count();
    assert_eq!(beats, 2);
}

#[test]
fn the_ruler_numbers_the_bars_from_one() {
    let (v, grid) = view();
    let numbers = timeline_bar_numbers(&v, grid, &waltz_from_bar_three());
    assert!(numbers.contains(&(8 * Q, 3)));
    assert!(numbers.contains(&(11 * Q, 4)));
}
