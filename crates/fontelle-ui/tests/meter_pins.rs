//! Where a signature change really is, on the arrangement's ruler.
//!
//! Ty, 2026-10-20: *"kind of hard to tell where time change markers are
//! right now due to the offset add a little marker indicator into the time
//! bar at the top to show where its actually placed like a pin in the time
//! bar."* The flag sits right of its bar line, clear of the bar's number;
//! the pin is on the line itself, with a short leader across to the flag.

use fontelle_types::{Meter, MeterChange};
use fontelle_ui::canvas::{TimelineView, meter_flags, meter_pins, timeline_tick_to_x};
use fontelle_ui::layout::Rect;

const GRID: Rect = Rect {
    x: 200.0,
    y: 60.0,
    width: 900.0,
    height: 300.0,
};
const RULER: Rect = Rect {
    x: 200.0,
    y: 40.0,
    width: 900.0,
    height: 20.0,
};

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        ..TimelineView::default()
    }
}

#[test]
fn a_pin_stands_on_the_bar_line_the_change_starts_at() {
    let meter = Meter::new(4, &[MeterChange::new(2, 3, 4)]);
    let v = view();
    let pins = meter_pins(&v, GRID, RULER, &meter);
    assert_eq!(pins.len(), 1);
    let (bar, pin) = pins[0];
    assert_eq!(bar, 2);
    let line = timeline_tick_to_x(&v, GRID, meter.tick_of_bar(2));
    let mid = pin.head.x + pin.head.width / 2.0;
    assert!((mid - line).abs() <= 1.0, "head at {mid}, line at {line}");
    assert!((pin.stem.x + pin.stem.width / 2.0 - line).abs() <= 1.0);
    // The head at the top of the ruler, the stem down it.
    assert!(pin.head.y >= RULER.y - 0.5 && pin.head.y < RULER.y + RULER.height / 2.0);
    assert!(pin.stem.bottom() >= RULER.bottom() - 1.0);
    assert!(pin.stem.y <= pin.head.bottom());
    // The leader joins it to its flag.
    let (_, flag) = meter_flags(&v, GRID, RULER, &meter)[0];
    assert!(pin.leader.x <= line + 1.0, "{:?}", pin.leader);
    assert!(
        pin.leader.right() >= flag.x - 1.0,
        "{:?} {:?}",
        pin.leader,
        flag
    );
    for part in [pin.head, pin.stem, pin.leader] {
        assert!(
            part.intersection(&RULER) == part,
            "{part:?} leaves the ruler"
        );
    }
}

/// A line scrolled off the left has no pin, though its flag may still show.
#[test]
fn a_pin_off_the_left_is_not_drawn() {
    let meter = Meter::new(4, &[MeterChange::new(2, 3, 4)]);
    let mut v = view();
    v.scroll_tick = meter.tick_of_bar(2) + 10;
    assert!(meter_pins(&v, GRID, RULER, &meter).is_empty());
}

#[test]
fn every_change_has_its_own_pin() {
    let meter = Meter::new(4, &[MeterChange::new(2, 3, 4), MeterChange::new(5, 7, 8)]);
    let pins = meter_pins(&view(), GRID, RULER, &meter);
    assert_eq!(pins.iter().map(|p| p.0).collect::<Vec<_>>(), vec![2, 5]);
}
