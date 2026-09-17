//! Where a sound that arrives with no position of its own gets its row.
//!
//! > *"i dont like how when recording something, importing something,
//! > dragging an audio file in, etc anything it always goes on a new lane at
//! > the very bottom its very annoying ... if i wasnt dragging however and
//! > imported some other way it should go on a new lane added in between the
//! > lane in the middlemost of your arrangement screen that way its cleanly
//! > visible for you."*
//!
//! A recording, a double-click in the Import tab, a `.mid` — none of them
//! names a row, and *at the foot* is under everything in any project with a
//! screenful of rows. [`arrival_row`] is the index the window hands the host
//! for those: the middle of the rows that are actually on screen, so the new
//! row is in front of you when it appears. The model's half is
//! `AddAudioClip::at_row` (`fontelle-model/tests/audio_clips.rs`).

use fontelle_ui::canvas::{SnapDivision, TimelineView, arrival_row};
use fontelle_ui::layout::Rect;

fn view(top_lane: usize) -> TimelineView {
    TimelineView {
        scroll_tick: 0,
        top_lane,
        pixels_per_tick: 0.025,
        lane_height: 30.0,
        snap: SnapDivision::Bar,
    }
}

/// A grid ten rows tall.
fn grid() -> Rect {
    Rect::new(0.0, 0.0, 800.0, 300.0)
}

#[test]
fn an_empty_arrangement_starts_at_the_top() {
    assert_eq!(arrival_row(&view(0), grid(), 0), 0);
}

#[test]
fn a_stack_that_fits_on_screen_arrives_in_its_middle() {
    // Four rows on a screen of ten: rows 1 and 2 above it, 3 and 4 below.
    assert_eq!(arrival_row(&view(0), grid(), 4), 2);
    // An odd stack rounds towards the foot: the new row goes under the middle
    // one, which is where "in between the middlemost" puts it.
    assert_eq!(arrival_row(&view(0), grid(), 3), 2);
    assert_eq!(arrival_row(&view(0), grid(), 1), 1);
}

#[test]
fn a_stack_taller_than_the_screen_arrives_in_the_middle_of_what_is_showing() {
    // Forty rows, scrolled so rows 10..20 are on screen: the middle of those.
    assert_eq!(arrival_row(&view(10), grid(), 40), 15);
    // Scrolled to the very end, with only the last four rows showing.
    assert_eq!(arrival_row(&view(36), grid(), 40), 38);
}

#[test]
fn it_never_points_past_the_stack() {
    // Scrolled past the end of a short stack (a lane was just removed): the
    // foot, never an index the model would have to clamp.
    assert_eq!(arrival_row(&view(8), grid(), 5), 5);
}

#[test]
fn a_grid_with_no_height_still_answers() {
    let flat = Rect::new(0.0, 0.0, 800.0, 0.0);
    assert_eq!(arrival_row(&view(0), flat, 6), 3);
    let mut v = view(0);
    v.lane_height = 0.0;
    assert_eq!(arrival_row(&v, grid(), 6), 3);
}
