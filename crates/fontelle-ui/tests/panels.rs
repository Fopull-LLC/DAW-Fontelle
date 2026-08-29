//! The docked panel set (item 9 of `docs/first-usable-plan.md`, TDD §16.1):
//! a channel rack and a soundfont browser down the left, the piano roll beside
//! them.
//!
//! All geometry and all hit-testing, which is all of it that can be wrong
//! without anyone noticing — §2.5 of the plan again. Nothing here opens a
//! window.

use fontelle_ui::canvas::{
    BrowserHit, RackHit, browser_hit, browser_layout, rack_hit, rack_layout,
};
use fontelle_ui::layout::{Rect, window_layout};
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

// ------------------------------------------------------------ the window ---

#[test]
fn the_sidebar_is_down_the_left_and_the_roll_takes_what_is_left() {
    let m = metrics();
    let l = window_layout(1280.0, 720.0, &m);

    assert_eq!(
        l.rack.frame.x, m.panel_margin,
        "the rack starts at the margin"
    );
    assert_eq!(l.rack.frame.width, m.sidebar_width);
    assert_eq!(l.browser.frame.x, l.rack.frame.x);
    assert_eq!(l.browser.frame.width, l.rack.frame.width);
    assert!(
        l.browser.frame.y >= l.rack.frame.bottom(),
        "the browser sits under the rack"
    );
    assert!(!l.rack.frame.intersects(&l.browser.frame));

    // The roll is to their right, and does not overlap either.
    assert!(l.panel.frame.x >= l.rack.frame.right());
    assert_eq!(l.panel.frame.right(), 1280.0 - m.panel_margin);
    for panel in [l.rack.frame, l.browser.frame] {
        assert!(!l.panel.frame.intersects(&panel));
    }
    // And nothing reaches into the transport bar.
    for panel in [l.rack.frame, l.browser.frame, l.panel.frame] {
        assert!(!l.transport.intersects(&panel));
        assert!(panel.y >= l.transport.bottom());
    }
}

#[test]
fn every_sidebar_panel_reaches_the_bottom_of_the_window_between_them() {
    let m = metrics();
    let l = window_layout(1280.0, 720.0, &m);
    assert_eq!(l.browser.frame.bottom(), 720.0 - m.panel_margin);
    assert_eq!(l.panel.frame.bottom(), 720.0 - m.panel_margin);
}

#[test]
fn a_narrow_window_gives_the_sidebar_up_before_it_squeezes_the_roll_to_nothing() {
    let m = metrics();
    // A window narrower than the sidebar plus a usable roll: the roll is what
    // the window is *for*, so the sidebar is what yields.
    let l = window_layout(320.0, 720.0, &m);
    assert!(
        l.rack.frame.width < m.sidebar_width,
        "the sidebar must give way rather than push the roll off the window"
    );
    assert!(l.panel.frame.width > 0.0);
    assert!(l.panel.frame.right() <= 320.0 - m.panel_margin + 0.001);
}

#[test]
fn a_window_too_small_for_any_of_it_yields_empty_rects_and_never_negative_ones() {
    let m = metrics();
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (4.0, 900.0), (900.0, 4.0)] {
        let l = window_layout(w, h, &m);
        for r in [
            l.transport,
            l.panel.frame,
            l.panel.header,
            l.panel.body,
            l.rack.frame,
            l.rack.body,
            l.browser.frame,
            l.browser.body,
        ] {
            assert!(
                r.width >= 0.0 && r.height >= 0.0,
                "{w}x{h} produced {r:?} with a negative dimension"
            );
        }
    }
}

// -------------------------------------------------------- the channel rack ---

#[test]
fn the_rack_gives_every_channel_a_row_and_keeps_the_add_button_reachable() {
    let m = metrics();
    let body = Rect::new(10.0, 20.0, 240.0, 300.0);
    let l = rack_layout(body, &m, 3, 0);

    assert_eq!(l.rows.len(), 3);
    for (index, row) in l.rows.iter().enumerate() {
        assert_eq!(row.index, index);
        assert_eq!(row.frame.height, m.row_height);
        assert_eq!(row.frame.x, body.x);
        assert!(body.contains(row.frame.x, row.frame.y));
        // The mute and solo buttons are inside the row and do not overlap the
        // name, or each other.
        assert!(!row.mute.intersects(&row.solo));
        assert!(!row.name.intersects(&row.mute));
        assert!(row.mute.right() <= row.frame.right() + 0.001);
    }
    assert!(
        l.rows[1].frame.y > l.rows[0].frame.y,
        "rows run down the panel"
    );
    // The one control that must never scroll away: with no instrument in the
    // project, "add one" is the only thing on the panel worth clicking.
    assert!(!l.add.is_empty());
    assert!(body.contains(l.add.x, l.add.y));
}

#[test]
fn a_rack_taller_than_its_panel_shows_a_screenful_and_no_more() {
    let m = metrics();
    // Virtualisation, the same rule the roll follows (§16.4): a project with
    // two hundred channels must cost what a screenful costs.
    let body = Rect::new(0.0, 0.0, 240.0, 5.5 * m.row_height);
    let l = rack_layout(body, &m, 200, 0);
    assert!(
        l.rows.len() <= 6,
        "built {} rows for a panel that can show about five",
        l.rows.len()
    );
    assert!(
        l.rows
            .iter()
            .all(|row| row.frame.bottom() <= body.bottom() + m.row_height)
    );

    // Scrolled down, the rows are the ones scrolled to.
    let scrolled = rack_layout(body, &m, 200, 10);
    assert_eq!(scrolled.rows[0].index, 10);
}

#[test]
fn clicking_the_rack_says_what_was_clicked() {
    let m = metrics();
    let body = Rect::new(10.0, 20.0, 240.0, 300.0);
    let l = rack_layout(body, &m, 3, 0);

    let row = l.rows[1];
    let mid = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);

    assert_eq!(
        rack_hit(&l, mid(row.name).0, mid(row.name).1),
        RackHit::Row(1)
    );
    assert_eq!(
        rack_hit(&l, mid(row.mute).0, mid(row.mute).1),
        RackHit::Mute(1)
    );
    assert_eq!(
        rack_hit(&l, mid(row.solo).0, mid(row.solo).1),
        RackHit::Solo(1)
    );
    assert_eq!(rack_hit(&l, mid(l.add).0, mid(l.add).1), RackHit::Add);
    assert_eq!(rack_hit(&l, -50.0, -50.0), RackHit::Nothing);
}

// ------------------------------------------------------------ the browser ---

#[test]
fn the_browser_is_a_search_box_over_a_file_list_over_a_preset_list() {
    let m = metrics();
    let body = Rect::new(10.0, 20.0, 240.0, 400.0);
    let l = browser_layout(body, &m, 20, 8, 0, 0);

    assert_eq!(
        l.search.y, body.y,
        "the search box is the first thing in it"
    );
    assert!(l.files.y >= l.search.bottom());
    assert!(l.presets.y >= l.files.bottom());
    assert!(!l.search.intersects(&l.files));
    assert!(!l.files.intersects(&l.presets));
    assert!(l.presets.bottom() <= l.status.y + 0.001);

    assert!(!l.file_rows.is_empty());
    assert!(!l.preset_rows.is_empty());
    for (_, rect) in &l.file_rows {
        assert!(rect.bottom() <= l.files.bottom() + 0.001);
    }
    for (_, rect) in &l.preset_rows {
        assert!(rect.bottom() <= l.presets.bottom() + 0.001);
    }
}

#[test]
fn the_browser_lists_only_what_fits_however_big_the_collection_is() {
    let m = metrics();
    let body = Rect::new(0.0, 0.0, 240.0, 400.0);
    let small = browser_layout(body, &m, 4, 2, 0, 0);
    let huge = browser_layout(body, &m, 100_000, 100_000, 0, 0);

    assert_eq!(small.file_rows.len(), 4, "a short list is shown whole");
    assert!(
        huge.file_rows.len() < 40,
        "a collection of a hundred thousand soundfonts must cost a screenful, \
         not a hundred thousand rectangles"
    );
    assert_eq!(
        huge.file_rows[0].0, 0,
        "and the first row is the first entry until it is scrolled"
    );
    let scrolled = browser_layout(body, &m, 100_000, 10, 500, 0);
    assert_eq!(scrolled.file_rows[0].0, 500);
}

#[test]
fn clicking_the_browser_says_which_file_or_preset_was_clicked() {
    let m = metrics();
    let body = Rect::new(10.0, 20.0, 240.0, 400.0);
    let l = browser_layout(body, &m, 20, 8, 0, 0);
    let mid = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);

    let (x, y) = mid(l.search);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Search);

    let (index, rect) = l.file_rows[2];
    let (x, y) = mid(rect);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::File(index));

    let (index, rect) = l.preset_rows[1];
    let (x, y) = mid(rect);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Preset(index));

    assert_eq!(browser_hit(&l, -100.0, -100.0), BrowserHit::Nothing);
}

/// The bank is a folder, and the two things a person needs to do with a folder
/// are look in it and change which one it is. Neither of them should require
/// knowing that `--soundfonts` exists.
#[test]
fn the_browser_has_a_way_to_open_the_bank_folder_and_a_way_to_change_it() {
    let m = metrics();
    let body = Rect::new(10.0, 20.0, 240.0, 400.0);
    let l = browser_layout(body, &m, 20, 8, 0, 0);
    let mid = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);

    // Pinned to the bottom of the panel, under a line saying where the bank is,
    // and never scrolled away by a long list.
    assert!(!l.open_folder.is_empty());
    assert!(!l.choose_folder.is_empty());
    assert_eq!(
        l.open_folder.bottom(),
        body.bottom(),
        "the buttons sit on the bottom edge of the panel"
    );
    assert_eq!(l.choose_folder.bottom(), l.open_folder.bottom());
    assert!(!l.open_folder.intersects(&l.choose_folder));
    assert!(l.choose_folder.right() <= body.right() + 0.001);

    // The status line is above them, and the lists are above that.
    assert!(l.status.bottom() <= l.open_folder.y + 0.001);
    assert!(!l.status.intersects(&l.open_folder));
    assert!(!l.presets.intersects(&l.status));

    let (x, y) = mid(l.open_folder);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::OpenFolder);
    let (x, y) = mid(l.choose_folder);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::ChooseFolder);
}

#[test]
fn a_browser_with_no_room_still_produces_usable_geometry() {
    let m = metrics();
    for height in [0.0, 4.0, 20.0, m.row_height] {
        let l = browser_layout(Rect::new(0.0, 0.0, 240.0, height), &m, 10, 10, 0, 0);
        for r in [
            l.search,
            l.files,
            l.presets,
            l.status,
            l.open_folder,
            l.choose_folder,
        ] {
            assert!(r.width >= 0.0 && r.height >= 0.0);
        }
        for (_, rect) in l.file_rows.iter().chain(&l.preset_rows) {
            assert!(rect.height >= 0.0 && rect.width >= 0.0);
        }
    }
}
