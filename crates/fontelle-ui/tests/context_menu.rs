//! The right-click menu's geometry.

use fontelle_ui::canvas::{MenuEntry, context_menu_hit, context_menu_layout, menu_matches};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn font_size() -> f32 {
    Theme::dark_default().font.size
}

fn menu(at: (f32, f32), bounds: Rect, labels: &[&str]) -> fontelle_ui::canvas::ContextMenu {
    context_menu_layout(
        at,
        bounds,
        &metrics(),
        font_size(),
        labels.iter().map(|l| MenuEntry::new(*l)).collect(),
    )
}

#[test]
fn a_menu_opens_below_and_right_of_the_pointer() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = menu((100.0, 100.0), bounds, &["Duplicate", "Delete"]);
    assert_eq!(menu.rows.len(), 2);
    assert!(menu.frame.x >= 100.0 && menu.frame.y >= 100.0);
    assert!(menu.rows[0].y < menu.rows[1].y, "in the order given");
}

/// A menu opened **beside a control** — a knob's right-click menu — never
/// covers the control: to its right when that fits, to its left when it
/// does not, its top on the control's top, and folded up rather than down
/// at the bottom of the window. The knob's menu used to open at the
/// pointer, which is on the knob, so it covered the knob's own value and
/// its neighbours (`docs/flopsynth-next.md` §1.4(6)).
#[test]
fn a_menu_beside_a_control_never_covers_it() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let entries = || {
        ["cutoff", "Create automation clip"]
            .iter()
            .map(|l| MenuEntry::new(*l))
            .collect::<Vec<_>>()
    };
    let beside = |cell: Rect| {
        fontelle_ui::canvas::context_menu_layout_beside(
            cell,
            bounds,
            &metrics(),
            font_size(),
            entries(),
        )
    };
    // Room on the right: the menu starts a little right of the cell, level
    // with its top.
    let cell = Rect::new(100.0, 100.0, 52.0, 54.0);
    let menu = beside(cell);
    assert!(!menu.frame.is_empty());
    assert!(
        menu.frame.x >= cell.right() && (menu.frame.y - cell.y).abs() < 0.01,
        "not beside the cell: {:?} vs {cell:?}",
        menu.frame
    );
    assert!(!menu.frame.intersects(&cell));

    // Against the right edge: the menu goes to the left of the cell, still
    // clear of it.
    let cell = Rect::new(740.0, 100.0, 52.0, 54.0);
    let menu = beside(cell);
    assert!(
        menu.frame.right() <= cell.x + 0.01 && menu.frame.right() <= bounds.right() + 0.01,
        "not left of the cell: {:?} vs {cell:?}",
        menu.frame
    );
    assert!(!menu.frame.intersects(&cell));

    // At the foot: folded up, inside the window, clear of the cell.
    let cell = Rect::new(100.0, 560.0, 52.0, 40.0);
    let menu = beside(cell);
    assert!(menu.frame.bottom() <= bounds.bottom() + 0.01);
    assert!(
        !menu.frame.intersects(&cell),
        "{:?} covers {cell:?}",
        menu.frame
    );
}

#[test]
fn a_menu_near_an_edge_folds_back_inside_the_window() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = menu((795.0, 595.0), bounds, &["Duplicate", "Delete", "Rename"]);
    assert!(
        menu.frame.right() <= bounds.right() + 0.01,
        "not off the right: {:?}",
        menu.frame
    );
    assert!(
        menu.frame.bottom() <= bounds.bottom() + 0.01,
        "not off the bottom: {:?}",
        menu.frame
    );
}

#[test]
fn a_menu_is_wide_enough_for_its_longest_entry() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let short = menu((0.0, 0.0), bounds, &["Cut"]);
    let long = menu(
        (0.0, 0.0),
        bounds,
        &["Create automation clip for this control"],
    );
    assert!(
        long.frame.width > short.frame.width,
        "a long caption gets a wider menu, {} against {}",
        long.frame.width,
        short.frame.width
    );
}

#[test]
fn clicking_a_row_chooses_it() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = menu((40.0, 40.0), bounds, &["Duplicate", "Delete"]);
    let row = menu.rows[1];
    assert_eq!(
        context_menu_hit(&menu, row.x + 4.0, row.y + row.height / 2.0),
        Some(1)
    );
    assert_eq!(
        context_menu_hit(&menu, row.x - 40.0, row.y),
        None,
        "a press off the menu chooses nothing"
    );
}

/// A greyed entry says why something is not available and does nothing.
#[test]
fn a_disabled_row_cannot_be_chosen() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = context_menu_layout(
        (40.0, 40.0),
        bounds,
        &metrics(),
        font_size(),
        vec![
            MenuEntry::disabled("Delete lane"),
            MenuEntry::new("Rename lane"),
        ],
    );
    let row = menu.rows[0];
    assert_eq!(context_menu_hit(&menu, row.x + 4.0, row.y + 2.0), None);
    assert!(
        menu.frame.contains(row.x + 4.0, row.y + 2.0),
        "it is still inside the menu, so the press does not dismiss it either"
    );
}

/// A panel with no room for **even one row** draws nothing rather than a
/// sliver. A menu that is merely taller than its room scrolls instead — see
/// `a_menu_too_long_for_the_window_scrolls_rather_than_vanishing`.
#[test]
fn a_menu_that_will_not_fit_is_not_drawn() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 10.0);
    let menu = menu((0.0, 0.0), bounds, &["One", "Two", "Three", "Four"]);
    assert!(menu.is_empty());
    assert_eq!(context_menu_hit(&menu, 1.0, 1.0), None);
}

// ------------------------------------------------------------- long menus ---
//
// > *"its still not seeing my plugins right now but i updated i installed
// > them and restarted"*
//
// The scan had found all 370 of them and the status line said so; clicking
// *"Plugin…"* opened **nothing at all**, because 357 effects plus a heading
// plus a rescan row is a menu 7900 pixels tall and the layout refused any
// menu taller than the window. A menu that refuses to exist is
// indistinguishable, from the outside, from a program that cannot see the
// plugins — which is exactly the report. A long menu scrolls now.

/// `n` entries, in a window a normal size.
fn long_menu(n: usize) -> (fontelle_ui::canvas::ContextMenu, Rect) {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let labels: Vec<String> = (0..n).map(|i| format!("Plugin {i}")).collect();
    let entries = labels.iter().map(MenuEntry::new).collect();
    let menu = context_menu_layout((10.0, 10.0), bounds, &metrics(), font_size(), entries);
    (menu, bounds)
}

#[test]
fn a_menu_too_long_for_the_window_scrolls_rather_than_vanishing() {
    let (menu, bounds) = long_menu(359);
    assert!(!menu.is_empty(), "the plugin menu vanished again");
    assert!(
        menu.frame.height <= bounds.height + 0.01,
        "it hangs off the window: {:?}",
        menu.frame
    );
    assert!(menu.scrolls(), "a menu that does not fit has to scroll");
    assert!(menu.max_scroll() > 0.0);
    // The first rows are there to be clicked.
    assert!(!menu.rows[0].is_empty());
    assert_eq!(
        context_menu_hit(&menu, menu.rows[0].x + 4.0, menu.rows[0].y + 4.0),
        Some(0)
    );
}

#[test]
fn what_is_scrolled_out_of_sight_is_neither_drawn_nor_clickable() {
    // An entry off the end of the frame has an **empty** rectangle, which is
    // what the renderer skips and what the hit test cannot land on. One rule,
    // two places, no third opinion about which rows are showing.
    let (menu, _) = long_menu(359);
    assert!(
        menu.rows.last().expect("a last row").is_empty(),
        "the far end is drawn before it is scrolled to"
    );
    for row in menu.rows.iter().filter(|row| !row.is_empty()) {
        assert!(
            row.y >= menu.frame.y - 0.01 && row.bottom() <= menu.frame.bottom() + 0.01,
            "a visible row is outside the frame: {row:?} in {:?}",
            menu.frame
        );
    }
}

#[test]
fn scrolling_brings_the_far_end_within_reach() {
    let (mut menu, _) = long_menu(359);
    let last = menu.entries.len() - 1;
    menu.scroll_by(menu.max_scroll());
    assert!(
        !menu.rows[last].is_empty(),
        "scrolling to the bottom did not reveal the last entry"
    );
    assert_eq!(
        context_menu_hit(&menu, menu.rows[last].x + 4.0, menu.rows[last].y + 4.0),
        Some(last),
        "the last entry cannot be chosen"
    );
    assert!(menu.rows[0].is_empty(), "the top is still showing");
}

#[test]
fn a_menu_cannot_be_scrolled_past_either_end() {
    // Past the bottom would show a page of nothing under the last entry, and
    // past the top a gap over the first: both look like the menu is broken.
    let (mut menu, _) = long_menu(359);
    menu.scroll_by(-500.0);
    assert_eq!(menu.scroll(), 0.0, "scrolled above the first entry");
    assert!(!menu.rows[0].is_empty());

    menu.scroll_by(menu.max_scroll() * 10.0);
    assert_eq!(menu.scroll(), menu.max_scroll(), "scrolled past the end");
    assert!(!menu.rows.last().expect("a last row").is_empty());
}

// ------------------------------------------------- dragging the thumb ---
//
// Reported from using it: *"i can scroll with my mouse now but i cant drag
// the scroll knob"*. A thumb that moves under the wheel and not under the
// pointer is a control that says it can be dragged and then refuses, which is
// worse than not drawing one.

#[test]
fn the_thumb_can_be_taken_hold_of_where_it_is_drawn() {
    let (menu, _) = long_menu(359);
    let thumb = menu.scrollbar();
    assert!(!thumb.is_empty(), "a scrolling menu draws a thumb");
    let grab = menu
        .thumb_grab(thumb.x + thumb.width / 2.0, thumb.y + 4.0)
        .expect("the middle of the thumb is on the thumb");
    assert!(
        (grab - 4.0).abs() < 0.01,
        "how far down it was taken hold of"
    );
}

#[test]
fn the_bar_answers_beside_the_thumb_as_well_as_on_it() {
    // The track is four pixels wide. Requiring the pointer to be *on* the
    // thumb to start a drag makes a control you have to aim at; a press on
    // the bar elsewhere takes hold of the middle of the thumb and drags from
    // there, which is what every other scrollbar does.
    let (menu, _) = long_menu(359);
    let track = menu.scrollbar_track();
    assert!(!track.is_empty());
    let grab = menu
        .thumb_grab(track.x + track.width / 2.0, track.bottom() - 2.0)
        .expect("the bar below the thumb still starts a drag");
    let thumb = menu.scrollbar();
    assert!(
        (grab - thumb.height / 2.0).abs() < 0.01,
        "taken hold of by its middle"
    );
}

#[test]
fn a_press_off_the_bar_is_not_a_drag() {
    let (menu, _) = long_menu(359);
    let row = menu.rows[0];
    assert_eq!(
        menu.thumb_grab(row.x + 4.0, row.y + 4.0),
        None,
        "a press on a row must choose that row, not scroll"
    );
}

#[test]
fn dragging_the_thumb_to_the_bottom_scrolls_to_the_end() {
    let (mut menu, _) = long_menu(359);
    let track = menu.scrollbar_track();
    let grab = menu
        .thumb_grab(track.x + 1.0, menu.scrollbar().y + 2.0)
        .expect("on the thumb");

    menu.drag_thumb(track.bottom() + 200.0, grab);
    assert_eq!(menu.scroll(), menu.max_scroll(), "dragged past the end");
    assert!(
        !menu.rows.last().expect("a last row").is_empty(),
        "the last entry is showing"
    );

    menu.drag_thumb(track.y - 200.0, grab);
    assert_eq!(menu.scroll(), 0.0, "dragged back above the first");
    assert!(!menu.rows[0].is_empty());
}

#[test]
fn the_thumb_follows_the_pointer_rather_than_jumping_under_it() {
    // The property that makes a drag feel like a drag: wherever on the thumb
    // it was taken hold of, that point stays under the pointer.
    let (mut menu, _) = long_menu(359);
    let track = menu.scrollbar_track();
    let thumb = menu.scrollbar();
    let grab = menu
        .thumb_grab(track.x + 1.0, thumb.y + thumb.height - 2.0)
        .expect("near the thumb's bottom");

    let to = track.y + track.height / 3.0;
    menu.drag_thumb(to, grab);
    let moved = menu.scrollbar();
    assert!(
        (moved.y + grab - to).abs() < 1.0,
        "the point taken hold of is no longer under the pointer: {moved:?}"
    );
}

#[test]
fn a_menu_that_fits_has_no_bar_to_drag() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut menu = menu((100.0, 100.0), bounds, &["Duplicate", "Delete", "Rename"]);
    assert!(menu.scrollbar_track().is_empty());
    assert_eq!(
        menu.thumb_grab(menu.frame.right() - 2.0, menu.frame.y + 4.0),
        None
    );
    let before = menu.rows.clone();
    menu.drag_thumb(500.0, 0.0);
    assert_eq!(menu.rows, before, "a short menu moved under a drag");
}

#[test]
fn a_menu_that_fits_does_not_scroll_at_all() {
    // The other several hundred menus in this window are six lines long and
    // must be exactly what they were.
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut menu = menu((100.0, 100.0), bounds, &["Duplicate", "Delete", "Rename"]);
    assert!(!menu.scrolls());
    assert_eq!(menu.max_scroll(), 0.0);
    let before = menu.rows.clone();
    menu.scroll_by(200.0);
    assert_eq!(menu.rows, before, "a short menu moved under the wheel");
}

// ---------------------------------------------------------- finding one ---
//
// Three hundred and fifty-seven effects is sixteen screens of scrolling, so
// the picker filters as you type. Substring rather than fuzzy: with this many
// plugins a match nobody can explain is worse than one that misses.

#[test]
fn a_filter_matches_anywhere_in_the_name_and_ignores_case() {
    assert!(menu_matches("Calf Reverb", "reverb"));
    assert!(menu_matches("Calf Reverb", "CALF"));
    assert!(menu_matches("Calf Reverb", "lf rev"));
    assert!(!menu_matches("Calf Reverb", "delay"));
}

#[test]
fn an_empty_filter_matches_everything() {
    // What an unfiltered menu is: nothing typed yet, every plugin listed.
    assert!(menu_matches("Surge XT", ""));
    assert!(menu_matches("", ""));
}

// ------------------------------------------------------------ columns ---
//
// The preset drop-down is a hundred and twenty-eight presets under fourteen
// headings: a single column three thousand pixels tall, of which the window
// showed the first thirty and hinted at nothing. A list that will not fit in
// one column and *will* fit in several is laid out in several — the way
// every big menu in every DAW is — and only a list too long even for that
// falls back to scrolling.

#[test]
fn a_menu_taller_than_the_window_breaks_into_columns_when_they_fit() {
    let bounds = Rect::new(0.0, 0.0, 1180.0, 700.0);
    let labels: Vec<String> = (0..142).map(|i| format!("Preset {i}")).collect();
    let entries = labels.iter().map(MenuEntry::new).collect();
    let menu = context_menu_layout((300.0, 20.0), bounds, &metrics(), font_size(), entries);
    assert!(
        menu.columns() > 1,
        "142 rows in a 700-pixel window is more than one column"
    );
    assert!(
        !menu.scrolls(),
        "a menu that fits in columns does not scroll"
    );
    assert!(
        menu.frame.right() <= bounds.right() + 0.01
            && menu.frame.bottom() <= bounds.bottom() + 0.01,
        "the columns hang off the window: {:?}",
        menu.frame
    );
    for (index, row) in menu.rows.iter().enumerate() {
        assert!(!row.is_empty(), "row {index} is not showing");
        assert!(
            row.right() <= menu.frame.right() + 0.01 && row.bottom() <= menu.frame.bottom() + 0.01,
            "row {index} is outside the frame: {row:?}"
        );
        assert_eq!(
            context_menu_hit(&menu, row.x + 4.0, row.y + 4.0),
            Some(index)
        );
    }
    // The second column starts back at the top, to the right of the first.
    let per_column = menu
        .rows
        .iter()
        .take_while(|row| (row.x - menu.rows[0].x).abs() < 0.01)
        .count();
    assert!(per_column > 1 && per_column < 142);
    let next = menu.rows[per_column];
    assert!(next.x > menu.rows[0].x && (next.y - menu.rows[0].y).abs() < 0.01);
}

#[test]
fn a_menu_too_long_even_for_columns_still_scrolls() {
    // Three hundred and fifty-nine plugins in an 800-pixel window is fourteen
    // columns, and fourteen columns is wider than the window. That one keeps
    // the behaviour the earlier tests hold: one column, scrolled.
    let (menu, _) = long_menu(359);
    assert_eq!(menu.columns(), 1);
    assert!(menu.scrolls());
}

#[test]
fn a_menu_that_scrolls_shows_a_scrollbar_and_one_that_fits_shows_none() {
    // The wheel worked on a long menu already; nothing said so. A thumb at
    // the right-hand edge, the length of the visible share, is what says so.
    let (mut menu, _) = long_menu(359);
    let thumb = menu.scrollbar();
    assert!(!thumb.is_empty(), "a scrolling menu has a thumb");
    assert!(
        thumb.right() <= menu.frame.right() + 0.01 && thumb.x >= menu.frame.x,
        "the thumb is inside the frame at its right edge: {thumb:?} in {:?}",
        menu.frame
    );
    assert!(
        thumb.height < menu.frame.height,
        "the thumb is shorter than the track"
    );
    let top = thumb.y;
    menu.scroll_by(menu.max_scroll());
    let moved = menu.scrollbar();
    assert!(moved.y > top, "the thumb moves down as the list scrolls");
    assert!(moved.bottom() <= menu.frame.bottom() + 0.01);

    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let short = menu_fits(bounds);
    assert!(
        short.scrollbar().is_empty(),
        "a menu that fits has no thumb"
    );
}

fn menu_fits(bounds: Rect) -> fontelle_ui::canvas::ContextMenu {
    menu((100.0, 100.0), bounds, &["Duplicate", "Delete", "Rename"])
}

// ------------------------------------------------- the input menu's rows ---
//
// A strip's input menu is built from the inputs the machine has **at the
// moment it opens**, and the row that was clicked has to mean the input that
// was written on it. The list used to be asked for again when the row was
// chosen — a microphone plugged in or unplugged between the two, or the open
// input being put back at the top of one list and not the other, made the
// second row mean a different device than the one you read.

use fontelle_ui::canvas::{input_menu_choice, input_menu_entries};

fn inputs() -> Vec<String> {
    ["Webcam", "Scarlett Solo", "Line In"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn the_input_menu_lists_no_input_first_and_then_the_machines_inputs() {
    let entries = input_menu_entries(None, &inputs());
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["No input", "Webcam", "Scarlett Solo", "Line In"]);
    assert!(
        !entries[0].enabled,
        "with nothing chosen, \"No input\" is where you already are"
    );
    assert!(entries[1..].iter().all(|e| e.enabled));
}

#[test]
fn the_input_already_chosen_is_greyed_and_no_input_is_offered() {
    let entries = input_menu_entries(Some("Scarlett Solo"), &inputs());
    assert!(entries[0].enabled, "\"No input\" is a way out");
    assert!(!entries[2].enabled, "the one you have is not a choice");
    assert!(entries[1].enabled && entries[3].enabled);
}

#[test]
fn a_machine_with_nothing_to_record_from_says_so() {
    let entries = input_menu_entries(None, &[]);
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["No input", "nothing to record from"]);
    assert!(!entries[1].enabled);
}

#[test]
fn a_chosen_row_means_the_input_that_was_written_on_it() {
    let shown = inputs();
    assert_eq!(input_menu_choice(&shown, 0), Some(None), "row 0 clears it");
    assert_eq!(
        input_menu_choice(&shown, 2),
        Some(Some("Scarlett Solo".to_string()))
    );
    assert_eq!(
        input_menu_choice(&shown, 4),
        None,
        "a row past the list is not a choice"
    );
    assert_eq!(
        input_menu_choice(&[], 1),
        None,
        "the \"nothing to record from\" row chooses nothing"
    );
}
