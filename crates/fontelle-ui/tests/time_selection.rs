//! Selecting a stretch of time on a ruler (TDD §6.3).
//!
//! Reported from using the window: *"we should have time looping selections
//! like how fl studio works where you right click and drag on the time bar to
//! loop a time section you are editing either in the piano roll or
//! arrangement cleanly."*
//!
//! The gesture is the same on both rulers, so the arithmetic is one function:
//! an anchor where the button went down, the tick under the pointer now, and
//! the grid to land both on. A drag that never left its own bar is a click,
//! and a click clears the selection — which is what FL does, and what makes
//! the same button both make and unmake a loop.

use fontelle_types::{PPQN, Tick};
use fontelle_ui::canvas::{SnapDivision, time_selection};

const BAR: Tick = PPQN * 4;

#[test]
fn a_drag_across_bars_selects_them_on_the_grid() {
    // From a little inside bar 2 to a little inside bar 4: bars 2 and 3.
    let picked = time_selection(BAR + 100, BAR * 3 + 200, SnapDivision::Bar, 4);
    assert_eq!(picked, Some((BAR, BAR * 3)));
}

#[test]
fn dragging_backwards_selects_the_same_stretch() {
    assert_eq!(
        time_selection(BAR * 3 + 200, BAR + 100, SnapDivision::Bar, 4),
        Some((BAR, BAR * 3))
    );
}

#[test]
fn a_drag_that_stays_inside_one_grid_cell_is_a_click_and_selects_nothing() {
    assert_eq!(
        time_selection(BAR + 10, BAR + 300, SnapDivision::Bar, 4),
        None
    );
    assert_eq!(time_selection(BAR, BAR, SnapDivision::Bar, 4), None);
}

#[test]
fn a_finer_grid_selects_finer() {
    assert_eq!(
        time_selection(10, PPQN + 20, SnapDivision::Beat, 4),
        Some((0, PPQN))
    );
    // No grid: exactly what was dragged.
    assert_eq!(
        time_selection(10, 500, SnapDivision::None, 4),
        Some((10, 500))
    );
}

#[test]
fn the_selection_never_starts_before_the_song() {
    assert_eq!(
        time_selection(-BAR, BAR + 10, SnapDivision::Bar, 4),
        Some((0, BAR))
    );
}

#[test]
fn a_short_drag_that_rounds_onto_one_line_is_a_click() {
    // Both ends land on the same grid line, and a loop from a line to itself
    // is nothing — so it clears, the same as a click would.
    assert_eq!(
        time_selection(BAR - 10, BAR + 10, SnapDivision::Bar, 4),
        None
    );
}
