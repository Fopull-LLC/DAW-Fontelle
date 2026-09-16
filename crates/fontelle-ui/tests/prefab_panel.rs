//! The left-hand panel's two tabs, and the prefab list under one of them.
//!
//! > *"the prefab tab should be where the channel rack is can be tabbed
//! > between instruments and prefabs which just shows a list of all of the ones
//! > you have and you can scroll down them all and a plus icon to make a new
//! > one and name it and stuff."* — Ty
//!
//! Geometry and hit-testing only, and both pure, for the reason §2.5 gives.
//! The rules the channel rack already follows apply here unchanged, and are
//! re-asserted rather than assumed: **a row is always a whole row**
//! (`tests/panel_rows.rs`), the add button is pinned to the bottom so it never
//! scrolls away, and the list is virtualised so two hundred prefabs build a
//! screenful of rectangles rather than two hundred.

use fontelle_ui::canvas::{
    PrefabHit, RackHit, prefab_hit, prefab_layout, rack_hit, rack_layout, tab_at,
};
use fontelle_ui::document::RackTab;
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

/// Deliberately not a whole number of rows tall — the case that produced the
/// channel rack's squashed row.
fn body() -> Rect {
    Rect::new(8.0, 40.0, 248.0, 431.0)
}

// ------------------------------------------------------------- the tabs ---

#[test]
fn the_panel_has_a_tab_for_each_list_side_by_side() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 3, 0);
    assert_eq!(l.tabs.len(), RackTab::ALL.len());
    assert_eq!(l.tabs[0].0, RackTab::Instruments, "instruments first");
    assert_eq!(l.tabs[1].0, RackTab::Prefabs);

    // Side by side, filling the width, not overlapping.
    let (first, second) = (l.tabs[0].1, l.tabs[1].1);
    assert!(first.right() <= second.x + 0.01, "the tabs overlap");
    assert!((first.y - second.y).abs() < 0.01, "and they are one strip");
    assert!(first.y >= body().y, "at the top of the panel");
}

#[test]
fn the_same_tabs_sit_in_the_same_place_whichever_list_is_showing() {
    let m = metrics();
    let rack = rack_layout(body(), &m, 3, 0);
    let prefabs = prefab_layout(body(), &m, 3, 0);
    assert_eq!(
        rack.tabs, prefabs.tabs,
        "a tab strip that moved when you pressed it would be unusable"
    );
}

#[test]
fn pressing_a_tab_says_which_one() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 3, 0);
    for (tab, rect) in &l.tabs {
        let (x, y) = (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        assert_eq!(tab_at(&l.tabs, x, y), Some(*tab));
    }
    assert_eq!(tab_at(&l.tabs, 0.0, 0.0), None, "outside the strip");
}

/// The tab strip is the rack's too, and a press on it is a rack hit.
#[test]
fn the_channel_rack_answers_a_press_on_its_tabs() {
    let m = metrics();
    let l = rack_layout(body(), &m, 3, 0);
    let (tab, rect) = l.tabs[1];
    let hit = rack_hit(&l, rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    assert_eq!(hit, RackHit::Tab(tab));
}

/// And the rows start **below** the tabs, so the strip never sits on top of
/// one.
#[test]
fn the_rows_begin_under_the_tab_strip() {
    let m = metrics();
    for (list, top) in [
        {
            let l = rack_layout(body(), &m, 3, 0);
            (l.list, l.tabs[0].1.bottom())
        },
        {
            let l = prefab_layout(body(), &m, 3, 0);
            (l.list, l.tabs[0].1.bottom())
        },
    ] {
        assert!(
            list.y >= top - 0.01,
            "the list starts at {} and the tabs end at {top}",
            list.y
        );
    }
}

// ------------------------------------------------------------ the list ---

#[test]
fn every_prefab_gets_a_row_with_a_name_and_a_count_in_it() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 3, 0);
    assert_eq!(l.rows.len(), 3);
    for (slot, row) in l.rows.iter().enumerate() {
        assert_eq!(row.index, slot);
        assert_eq!(row.frame.height, m.row_height, "a row is a whole row");
        assert!(row.name.width > 0.0, "somewhere to write the name");
        assert!(row.uses.width > 0.0, "and somewhere for the count");
        assert!(
            row.name.right() <= row.uses.x + 0.01,
            "the name runs into the count"
        );
        assert!(
            row.uses.right() <= row.frame.right() + 0.01,
            "the count runs off the row"
        );
    }
}

#[test]
fn the_rows_do_not_overlap_and_none_leaves_the_list() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 40, 0);
    assert!(!l.rows.is_empty());
    for pair in l.rows.windows(2) {
        assert!(
            pair[0].frame.bottom() <= pair[1].frame.y + 0.01,
            "two rows overlap"
        );
    }
    for row in &l.rows {
        assert!(
            row.frame.bottom() <= l.list.bottom() + 0.01,
            "a row hangs out of the list"
        );
    }
}

/// Virtualised, like the rack: a screenful of rectangles, not two hundred.
#[test]
fn a_long_list_builds_only_what_fits() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 200, 0);
    assert_eq!(l.total, 200);
    assert!(l.rows.len() < 40, "built {} rows", l.rows.len());
    assert_eq!(l.rows.len(), l.capacity.min(200));

    let scrolled = prefab_layout(body(), &m, 200, 10);
    assert_eq!(scrolled.rows[0].index, 10, "scrolling moves the window");
    assert_eq!(scrolled.scroll, 10);
}

/// The plus icon is pinned to the bottom, so it is reachable with two hundred
/// prefabs and is the only thing worth pressing with none.
#[test]
fn the_add_button_is_pinned_to_the_bottom_whatever_the_list_is_doing() {
    let m = metrics();
    let empty = prefab_layout(body(), &m, 0, 0);
    let full = prefab_layout(body(), &m, 200, 0);
    assert_eq!(empty.add, full.add);
    assert!(empty.add.bottom() <= body().bottom() + 0.01);
    assert!(
        empty.add.y >= full.list.bottom() - 0.01,
        "the button sits under the list"
    );
    assert!(!empty.rows.is_empty() || empty.total == 0);
}

// ------------------------------------------------------ hit-testing it ---

#[test]
fn a_press_on_a_row_names_that_prefab() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 5, 0);
    let row = l.rows[2];
    assert_eq!(
        prefab_hit(&l, row.name.x + 4.0, row.frame.y + row.frame.height * 0.5),
        PrefabHit::Row(2)
    );
}

#[test]
fn a_press_on_the_plus_makes_one_and_a_press_on_nothing_is_nothing() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 5, 0);
    assert_eq!(
        prefab_hit(&l, l.add.x + l.add.width * 0.5, l.add.y + 2.0),
        PrefabHit::Add
    );
    assert_eq!(prefab_hit(&l, 0.0, 0.0), PrefabHit::Nothing);
}

/// A tab press inside the prefab list is a tab press, not a row press — the
/// strip is inside the panel and would otherwise be swallowed.
#[test]
fn a_press_on_the_tab_strip_is_not_a_press_on_a_row() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 5, 0);
    let (tab, rect) = l.tabs[0];
    assert_eq!(
        prefab_hit(&l, rect.x + rect.width * 0.5, rect.y + rect.height * 0.5),
        PrefabHit::Tab(tab)
    );
}

/// An empty list still has its tabs and its plus, because a panel you cannot
/// get out of or add to is a dead end.
#[test]
fn an_empty_list_is_still_a_panel_you_can_use() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 0, 0);
    assert!(l.rows.is_empty());
    assert_eq!(l.total, 0);
    assert_eq!(
        prefab_hit(&l, l.add.x + 2.0, l.add.y + 2.0),
        PrefabHit::Add,
        "the plus still works"
    );
    let (tab, rect) = l.tabs[0];
    assert_eq!(
        prefab_hit(&l, rect.x + 2.0, rect.y + 2.0),
        PrefabHit::Tab(tab)
    );
}

/// A panel with no room at all produces no rows rather than negative ones.
#[test]
fn a_panel_with_no_room_produces_nothing_rather_than_a_sliver() {
    let m = metrics();
    let l = prefab_layout(Rect::new(0.0, 0.0, 100.0, 0.0), &m, 5, 0);
    assert!(l.rows.is_empty());
    assert!(l.list.height >= 0.0);
    assert_eq!(prefab_hit(&l, 1.0, 1.0), PrefabHit::Nothing);
}

/// Scrolled past the end, the list still shows something — the same clamp the
/// rack makes.
#[test]
fn scrolling_past_the_end_still_shows_the_last_one() {
    let m = metrics();
    let l = prefab_layout(body(), &m, 3, 99);
    assert!(!l.rows.is_empty(), "a list scrolled off its end went blank");
    assert_eq!(l.rows.last().unwrap().index, 2);
}

// ---------------------------------------------------------- the F key ---

#[test]
fn f_flips_the_rack_between_its_two_tabs() {
    // *"make it so the f key toggles the tab between instrument and prefab."*
    // Two tabs, so the other one is the answer either way.
    assert_eq!(RackTab::Instruments.other(), RackTab::Prefabs);
    assert_eq!(RackTab::Prefabs.other(), RackTab::Instruments);
    for tab in RackTab::ALL {
        assert_eq!(tab.other().other(), tab, "two presses come back");
    }
}
