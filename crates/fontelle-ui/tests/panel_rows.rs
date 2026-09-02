//! What a *row* in a list is, and why the browser looked broken.
//!
//! Reported from using the window: *"the soundfonts section on the left was
//! kind of glitchy — it was just overlaying things squished together, and not
//! letting me select different instruments either."*
//!
//! It was one bug with two faces. Both lists built one row more than would fit
//! and then clipped that row's **rectangle** to the list — so the last row was
//! a few pixels tall, its caption was centred inside those few pixels, and it
//! drew on top of the row above it. The same clipped rectangle also made the
//! boundary between the two lists a place where a click could land on neither.
//!
//! The rule this file pins down: **a row is always a whole row.** A list that
//! has run out of room stops giving them out, and the last visible one is
//! clipped when it is *drawn*, not when it is measured. Hit-testing then asks
//! which list the pointer is in before it asks which row.

use fontelle_ui::canvas::{
    BrowserHit, RackHit, browser_hit, browser_layout, rack_hit, rack_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

/// A panel body deliberately sized so neither list divides evenly by the row
/// height — the case that produced the squashed row.
fn body() -> Rect {
    Rect::new(8.0, 40.0, 248.0, 431.0)
}

#[test]
fn every_browser_row_is_a_whole_row_and_none_overlaps_another() {
    let m = metrics();
    let l = browser_layout(body(), &m, 200, 200, 0, 0);

    for (list, rows) in [(l.files, &l.file_rows), (l.presets, &l.preset_rows)] {
        assert!(!rows.is_empty(), "{list:?} produced no rows at all");
        for (_, rect) in rows {
            assert_eq!(
                rect.height, m.row_height,
                "a row of {} pixels — that is the squashed one",
                rect.height
            );
            assert!(
                rect.y < list.bottom(),
                "a row starting at {} is below its list, which ends at {}",
                rect.y,
                list.bottom()
            );
            assert_eq!(rect.x, list.x);
            assert_eq!(rect.width, list.width);
        }
        // Consecutive rows sit exactly one row height apart, so none of them
        // can print over the one above it.
        for pair in rows.windows(2) {
            assert_eq!(pair[1].1.y - pair[0].1.y, m.row_height);
            assert_eq!(pair[1].0, pair[0].0 + 1, "the indices skipped one");
        }
    }
}

#[test]
fn a_file_row_never_reaches_into_the_preset_list() {
    let m = metrics();
    let l = browser_layout(body(), &m, 200, 200, 0, 0);
    let last = l.file_rows.last().expect("a file row").1;
    assert!(
        !last.intersects(&l.presets),
        "the last file row {last:?} overlaps the preset list {:?} — this is \
         exactly what the panel looked like",
        l.presets
    );
}

#[test]
fn a_click_in_the_preset_list_is_a_preset_even_at_its_very_top() {
    let m = metrics();
    let l = browser_layout(body(), &m, 200, 200, 0, 0);

    // The top pixel of the preset list, which is the row that a file row
    // hanging over the boundary used to steal.
    let hit = browser_hit(&l, l.presets.x + 10.0, l.presets.y + 0.5);
    assert_eq!(hit, BrowserHit::Preset(0), "got {hit:?}");

    // And the bottom pixel of the file list is still a file.
    let hit = browser_hit(&l, l.files.x + 10.0, l.files.bottom() - 0.5);
    assert!(
        matches!(hit, BrowserHit::File(_)),
        "the bottom of the file list is not a file: {hit:?}"
    );

    // The gap between the two lists belongs to neither, and must not report a
    // row from the wrong one.
    for y in [l.files.bottom() + 0.5, l.presets.y - 0.5] {
        let hit = browser_hit(&l, l.files.x + 10.0, y);
        assert!(
            matches!(
                hit,
                BrowserHit::Nothing | BrowserHit::File(_) | BrowserHit::Preset(_)
            ),
            "{hit:?}"
        );
    }
}

#[test]
fn scrolled_lists_still_hand_out_whole_rows() {
    let m = metrics();
    let l = browser_layout(body(), &m, 200, 200, 37, 12);
    assert_eq!(l.file_rows.first().map(|(i, _)| *i), Some(37));
    assert_eq!(l.preset_rows.first().map(|(i, _)| *i), Some(12));
    for (_, rect) in l.file_rows.iter().chain(&l.preset_rows) {
        assert_eq!(rect.height, m.row_height);
    }
}

#[test]
fn every_rack_row_is_a_whole_row_and_stays_out_of_the_add_button() {
    let m = metrics();
    let l = rack_layout(Rect::new(8.0, 40.0, 248.0, 183.0), &m, 200, 0);

    assert!(!l.rows.is_empty());
    for row in &l.rows {
        assert_eq!(row.frame.height, m.row_height);
        assert!(
            !row.frame.intersects(&l.add),
            "a channel row {:?} runs under the add button {:?}",
            row.frame,
            l.add
        );
        assert!(row.mute.intersects(&row.frame));
        assert!(row.solo.intersects(&row.frame));
    }
    for pair in l.rows.windows(2) {
        assert_eq!(pair[1].frame.y - pair[0].frame.y, m.row_height);
    }
}

#[test]
fn a_click_below_the_last_rack_row_is_not_the_last_rack_row() {
    let m = metrics();
    // Three channels in a panel with room for many: the space under them is
    // empty, and clicking it must not select channel three.
    let l = rack_layout(Rect::new(8.0, 40.0, 248.0, 300.0), &m, 3, 0);
    let below = l.rows.last().expect("a row").frame.bottom() + 20.0;
    assert!(below < l.add.y, "the test needs room under the rows");
    assert_eq!(rack_hit(&l, 20.0, below), RackHit::Nothing);
}

#[test]
fn a_list_with_no_room_for_even_one_row_hands_out_none() {
    let m = metrics();
    // A panel squeezed to nothing must produce an empty list rather than one
    // row of negative height.
    let l = browser_layout(
        Rect::new(0.0, 0.0, 248.0, m.row_height * 2.0),
        &m,
        50,
        50,
        0,
        0,
    );
    for (_, rect) in l.file_rows.iter().chain(&l.preset_rows) {
        assert_eq!(rect.height, m.row_height);
        assert!(rect.width >= 0.0);
    }
    let l = rack_layout(Rect::new(0.0, 0.0, 248.0, 4.0), &m, 50, 0);
    assert!(l.rows.is_empty());
}

// -------------------------------------------------- opening the instrument ---
//
// *"Why can't I open up the VST options?"* — because nothing on screen opened
// them. A third switch on the channel row does, and the editor column carries
// two tabs so you can get back to the roll.

#[test]
fn every_rack_row_has_a_switch_that_opens_its_instrument() {
    use fontelle_ui::canvas::RackHit;

    let m = metrics();
    let l = rack_layout(Rect::new(8.0, 40.0, 248.0, 300.0), &m, 4, 0);
    let row = l.rows.first().expect("a row");

    assert!(!row.edit.is_empty(), "there is nothing to click");
    assert!(row.frame.intersects(&row.edit));
    for other in [row.mute, row.solo, row.name] {
        assert!(
            !row.edit.intersects(&other),
            "the edit switch {:?} overlaps {other:?}",
            row.edit
        );
    }
    assert_eq!(
        rack_hit(&l, row.edit.x + row.edit.width / 2.0, row.edit.y + 2.0),
        RackHit::Edit(row.index)
    );
}

#[test]
fn the_editor_column_has_a_tab_for_the_roll_and_one_for_the_mixer() {
    // **Two, and only two.** The instrument, the effect and the automation
    // curve were tabs here until §7.5's plugin hosting made that untenable —
    // a VST or CLAP editor is handed a parent *window* and draws into it, so
    // an editor that can only be one of this column's tabs is one the plugin
    // path can never be built on. See `fontelle_ui::layout::EditorKind`.
    use fontelle_ui::layout::{EditorTab, editor_tab_at, editor_tabs};

    let m = metrics();
    let header = Rect::new(264.0, 256.0, 728.0, 26.0);
    let tabs = editor_tabs(header, &m);

    let all = [
        (EditorTab::Roll, tabs.roll),
        (EditorTab::Mixer, tabs.mixer),
    ];
    for (i, (which, tab)) in all.iter().enumerate() {
        assert!(!tab.is_empty(), "{which:?} has no room");
        assert!(header.intersects(tab), "{tab:?} is outside the header");
        assert!(tab.bottom() <= header.bottom() + 0.001);
        for (_, other) in all.iter().skip(i + 1) {
            assert!(!tab.intersects(other), "{tab:?} and {other:?} overlap");
        }
        assert_eq!(editor_tab_at(&tabs, tab.x + 2.0, tab.y + 2.0), Some(*which));
    }
    assert_eq!(
        editor_tab_at(&tabs, header.right() - 1.0, header.y + 2.0),
        None
    );
}

#[test]
fn a_header_too_narrow_for_its_tabs_still_produces_usable_geometry() {
    use fontelle_ui::layout::editor_tabs;

    let m = metrics();
    for width in [0.0, 10.0, 60.0, 200.0, 280.0] {
        let tabs = editor_tabs(Rect::new(0.0, 0.0, width, 26.0), &m);
        for tab in [tabs.roll, tabs.mixer] {
            assert!(tab.width >= 0.0 && tab.height >= 0.0, "width {width}");
            assert!(tab.right() <= width + 0.001, "width {width} gave {tab:?}");
        }
    }
}
