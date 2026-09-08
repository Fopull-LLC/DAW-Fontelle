//! Walking the soundfont lists with the keyboard.
//!
//! Reported from using the window:
//!
//! > *"make it easy to also go through the selected instruments with arrow
//! > keys after it being clicked on to focus it ... the way to change the
//! > sound of a soundfont instrument to a different one should be to double
//! > click it or press enter while its selected."*
//!
//! Two things have to be true for that to be usable, and both are here rather
//! than in the window: a heading is **skipped** rather than landed on (a
//! search across the collection puts one over every run of hits, and a focus
//! that stops on them makes the down arrow do nothing every few presses), and
//! walking off either end **stays** rather than wrapping — a list that jumps
//! from the bottom back to the top loses your place silently.

use fontelle_ui::canvas::browser_focus_step;
use fontelle_ui::document::{LibraryEntry, LibraryKind};

fn row(name: &str, kind: LibraryKind) -> LibraryEntry {
    LibraryEntry {
        name: name.to_string(),
        detail: String::new(),
        kind,
    }
}

/// A search result: two soundfonts, each a heading over two presets.
fn grouped() -> Vec<LibraryEntry> {
    vec![
        row("Piano.sf2", LibraryKind::Group),
        row("Grand", LibraryKind::File),
        row("Upright", LibraryKind::File),
        row("Strings.sf2", LibraryKind::Group),
        row("Violin", LibraryKind::File),
        row("Cello", LibraryKind::File),
    ]
}

fn plain() -> Vec<LibraryEntry> {
    vec![
        row("One", LibraryKind::File),
        row("Two", LibraryKind::File),
        row("Three", LibraryKind::File),
    ]
}

#[test]
fn nothing_focused_yet_lands_on_the_first_row_that_can_be_chosen() {
    assert_eq!(browser_focus_step(&plain(), None, 1), Some(0));
    // Not the heading.
    assert_eq!(browser_focus_step(&grouped(), None, 1), Some(1));
}

#[test]
fn down_walks_forwards_and_up_walks_back() {
    let rows = plain();
    assert_eq!(browser_focus_step(&rows, Some(0), 1), Some(1));
    assert_eq!(browser_focus_step(&rows, Some(1), 1), Some(2));
    assert_eq!(browser_focus_step(&rows, Some(2), -1), Some(1));
}

#[test]
fn a_heading_is_stepped_over_rather_than_landed_on() {
    let rows = grouped();
    // Down from the last preset of one soundfont lands on the first of the
    // next, not on the name of it.
    assert_eq!(browser_focus_step(&rows, Some(2), 1), Some(4));
    // And back up again the same way.
    assert_eq!(browser_focus_step(&rows, Some(4), -1), Some(2));
}

#[test]
fn walking_off_either_end_stays_where_it_is() {
    let rows = plain();
    assert_eq!(
        browser_focus_step(&rows, Some(2), 1),
        Some(2),
        "off the bottom"
    );
    assert_eq!(
        browser_focus_step(&rows, Some(0), -1),
        Some(0),
        "off the top"
    );
}

#[test]
fn a_list_of_nothing_but_headings_has_nowhere_to_go() {
    let rows = vec![
        row("A.sf2", LibraryKind::Group),
        row("B.sf2", LibraryKind::Group),
    ];
    assert_eq!(browser_focus_step(&rows, None, 1), None);
    assert_eq!(browser_focus_step(&rows, Some(0), 1), None);
}

#[test]
fn an_empty_list_has_nowhere_to_go_either() {
    assert_eq!(browser_focus_step(&[], None, 1), None);
    assert_eq!(browser_focus_step(&[], Some(3), -1), None);
}

#[test]
fn a_focus_left_pointing_past_the_end_of_a_shorter_list_comes_back_inside() {
    // The list is rebuilt on every search keystroke, so a focus from the last
    // one can be anywhere. It must land somewhere real rather than nowhere.
    let rows = plain();
    let landed = browser_focus_step(&rows, Some(99), -1);
    assert!(landed.is_some_and(|i| i < rows.len()), "{landed:?}");
    let landed = browser_focus_step(&rows, Some(99), 1);
    assert!(landed.is_some_and(|i| i < rows.len()), "{landed:?}");
}

#[test]
fn a_step_of_nothing_keeps_the_row_if_it_can_be_chosen_and_moves_off_it_if_it_cannot() {
    let rows = grouped();
    assert_eq!(browser_focus_step(&rows, Some(1), 0), Some(1));
    // Focused on a heading — which a click cannot do, but a rebuilt list can —
    // it settles on the next row that can be chosen rather than staying.
    assert_eq!(browser_focus_step(&rows, Some(3), 0), Some(4));
}
