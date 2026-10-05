//! A scroll bar on the browser's lists — the Import tab's above all.
//!
//! Reported: *"add scroll bar to the import windows"*. The lists scrolled
//! with the wheel and said nothing about it: a folder of four hundred MIDI
//! files looked like a folder of fifteen, and there was nothing to drag to
//! get to the end of it. The bar is the menus' bar (`ContextMenu`'s thumb),
//! built for a list that scrolls by whole rows: the same width, the same
//! place against the right edge, a thumb as long as the share of the list
//! that shows, and a press anywhere on its strip takes hold of it.

use fontelle_ui::canvas::{
    BrowserHit, BrowserList, BrowserMode, RowScrollbar, browser_hit, browser_layout_for, scrolled,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(0.0, 0.0, 248.0, 600.0)
}

fn imports(count: usize, scroll: usize) -> fontelle_ui::canvas::BrowserLayout {
    browser_layout_for(body(), &metrics(), BrowserMode::Import, count, 0, scroll, 0)
}

/// How many rows the Import list shows whole.
fn visible() -> usize {
    imports(1000, 0).file_rows.len()
}

#[test]
fn a_list_that_fits_has_no_bar_and_its_rows_keep_the_whole_width() {
    let l = imports(3, 0);
    assert!(
        l.file_bar.is_none(),
        "a bar the length of its track says nothing"
    );
    for (_, row) in &l.file_rows {
        assert_eq!(row.width, l.files.width);
    }
}

#[test]
fn a_list_longer_than_its_room_has_a_bar_down_its_right_edge() {
    let l = imports(400, 0);
    let bar = l.file_bar.expect("four hundred files do not fit");
    assert!(
        l.files.contains(bar.track.x, bar.track.y)
            && bar.track.right() <= l.files.right()
            && bar.track.bottom() <= l.files.bottom(),
        "the bar is inside its list: {:?} in {:?}",
        bar.track,
        l.files
    );
    assert!(
        bar.track.height > l.files.height * 0.8,
        "and runs most of its height: {:?}",
        bar.track
    );
    // The rows give up the strip the bar is in, so a name never runs under
    // the thumb and a press on the bar is never a press on a row.
    for (_, row) in &l.file_rows {
        assert!(
            row.right() <= bar.strip.x,
            "row {row:?} runs under the bar's strip {:?}",
            bar.strip
        );
    }
    // The thumb is the share of the list that shows, and at the top.
    let share = visible() as f32 / 400.0;
    assert!(
        (bar.thumb.height - (bar.track.height * share).max(18.0)).abs() < 0.5,
        "thumb {:?} for {share} of the list",
        bar.thumb
    );
    assert!((bar.thumb.y - bar.track.y).abs() < 0.01);
}

#[test]
fn the_thumb_follows_the_wheel() {
    let top = imports(400, 0).file_bar.unwrap();
    let wheeled = scrolled(0, 50, 400);
    let lower = imports(400, wheeled).file_bar.unwrap();
    assert!(lower.thumb.y > top.thumb.y, "{lower:?} after the wheel");
    let end = imports(400, 400 - visible()).file_bar.unwrap();
    assert!(
        (end.thumb.bottom() - end.track.bottom()).abs() < 0.01,
        "the last page puts the thumb on the floor: {end:?}"
    );
}

#[test]
fn a_press_on_the_bar_is_the_bar_and_not_a_row() {
    let l = imports(400, 0);
    let bar = l.file_bar.unwrap();
    let (x, y) = (bar.strip.x + bar.strip.width / 2.0, bar.thumb.y + 2.0);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::Scrollbar(BrowserList::Files)
    );
    // Beside the thumb on its strip, too: four pixels is not something to
    // aim at.
    let below = bar.track.bottom() - 3.0;
    assert_eq!(
        browser_hit(&l, bar.strip.right() - 0.5, below),
        BrowserHit::Scrollbar(BrowserList::Files)
    );
}

#[test]
fn dragging_the_thumb_scrolls_the_list_by_whole_rows_and_no_further_than_its_end() {
    let l = imports(400, 0);
    let bar: RowScrollbar = l.file_bar.unwrap();
    let x = bar.strip.x + 1.0;
    let grab = bar.grab(x, bar.thumb.y + 3.0).expect("on the thumb");
    assert!((grab - 3.0).abs() < 0.01, "held where it was taken: {grab}");

    let last = 400 - visible();
    assert_eq!(bar.scroll_at(bar.track.bottom() + 500.0, grab), last);
    assert_eq!(bar.scroll_at(bar.track.y - 500.0, grab), 0);
    let middle = bar.track.y + (bar.track.height - bar.thumb.height) / 2.0 + grab;
    let half = bar.scroll_at(middle, grab);
    assert!(
        half.abs_diff(last / 2) <= 1,
        "half way down is half way through: {half} of {last}"
    );

    // Beside the thumb, the thumb comes to the pointer by its middle.
    let beside = bar.grab(x, bar.track.bottom() - 2.0).unwrap();
    assert!((beside - bar.thumb.height / 2.0).abs() < 0.01);
    assert_eq!(bar.grab(bar.strip.x - 20.0, bar.thumb.y + 3.0), None);
}

#[test]
fn the_presets_list_has_its_own_bar() {
    let l = browser_layout_for(body(), &metrics(), BrowserMode::Sounds, 2, 300, 0, 0);
    assert!(l.file_bar.is_none());
    let bar = l.preset_bar.expect("three hundred presets do not fit");
    assert!(l.presets.contains(bar.track.x, bar.track.y));
    let (x, y) = (bar.strip.x + 1.0, bar.thumb.y + 1.0);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::Scrollbar(BrowserList::Presets)
    );
}
