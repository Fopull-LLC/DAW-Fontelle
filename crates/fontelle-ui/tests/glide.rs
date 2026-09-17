//! Scrolling that glides, and scrolls by the screen rather than by the row.
//!
//! > *"scrolling feels so rigid it should be smoother animated and it should
//! > also scroll slower when really zoomed in right now its still scrolling
//! > super fast when zoomed far in"*
//!
//! Two things, both pure and both here:
//!
//! - **A wheel notch moves a fraction of what is on screen**
//!   (`wheel_travel`), so how far it goes depends on the zoom the way a
//!   person expects — a notch is "a bit further along", whatever a bit is at
//!   this zoom. Rows and keys move by fractions of a row (`lane_offset`,
//!   `key_offset`), so a tall row is not a tall jump.
//! - **The view glides to where it was sent** (`Glide`): the wheel moves a
//!   target, and the position follows it over a few frames, so a burst of
//!   notches is one motion rather than a stutter of jumps. Anything else
//!   that moves the view — a zoom, the playhead, a scroll-to-show — is
//!   adopted rather than fought.

use fontelle_types::PPQN;
use fontelle_ui::canvas::{
    Glide, RollView, TimelineView, key_to_y, lane_to_y, visible_keys, wheel_travel, y_to_key,
    y_to_lane,
};
use fontelle_ui::layout::Rect;

const GRID: Rect = Rect::new(100.0, 50.0, 800.0, 400.0);

// --------------------------------------------------------------- travel ---

#[test]
fn a_notch_travels_a_fraction_of_the_view_and_a_burst_at_most_half_of_it() {
    let one = wheel_travel(800.0, 1.0);
    assert!(
        one > 40.0 && one < 200.0,
        "one notch is {one} px of an 800 px view"
    );
    assert_eq!(
        wheel_travel(800.0, 2.0),
        one * 2.0,
        "two notches, twice as far"
    );
    assert_eq!(
        wheel_travel(800.0, -1.0),
        -one,
        "and back the same distance"
    );
    // A desktop that reports three lines a notch, or a flick of a wheel,
    // must not throw the view a whole screen at a time.
    assert_eq!(wheel_travel(800.0, 30.0), 400.0);
    assert_eq!(wheel_travel(800.0, -30.0), -400.0);
}

#[test]
fn zoomed_in_a_notch_covers_less_of_the_song_than_zoomed_out() {
    // The same pixels, fewer ticks: this is what "slower when zoomed in"
    // means in the units the view keeps.
    let px = wheel_travel(800.0, 1.0);
    let far = f64::from(px) / 0.025;
    let near = f64::from(px) / 0.4;
    assert!(near < far / 8.0, "near {near} ticks against far {far}");
}

// ---------------------------------------------------------------- glide ---

#[test]
fn a_push_moves_the_target_and_the_position_follows_over_a_few_frames() {
    let mut glide = Glide::at(0.0);
    glide.push(1000.0);
    assert_eq!(glide.position(), 0.0, "a push moves nothing on its own");
    assert!(glide.moving());
    let first = glide.step(1.0 / 60.0, 0.5).expect("it moved");
    assert!(
        first > 0.0 && first < 1000.0,
        "the first frame is part of the way: {first}"
    );
    let mut frames = 1;
    while glide.step(1.0 / 60.0, 0.5).is_some() {
        frames += 1;
        assert!(frames < 120, "still gliding after two seconds");
    }
    assert_eq!(glide.position(), 1000.0, "it settles exactly on the target");
    assert!(!glide.moving());
    assert!(frames >= 4, "{frames} frames is a jump, not a glide");
    assert!(frames <= 40, "{frames} frames is a crawl");
}

#[test]
fn a_glide_never_goes_before_the_start() {
    let mut glide = Glide::at(100.0);
    glide.push(-1000.0);
    while glide.step(1.0 / 60.0, 0.5).is_some() {}
    assert_eq!(glide.position(), 0.0);
    let mut bounded = Glide::at(100.0).with_bounds(11.0, 127.0);
    bounded.push(1000.0);
    while bounded.step(1.0 / 60.0, 0.01).is_some() {}
    assert_eq!(bounded.position(), 127.0);
    bounded.push(-1000.0);
    while bounded.step(1.0 / 60.0, 0.01).is_some() {}
    assert_eq!(bounded.position(), 11.0);
}

#[test]
fn a_view_moved_by_something_else_is_adopted_rather_than_fought() {
    // A zoom about the pointer, the playhead pulling the view along, a
    // scroll-to-show: each writes the view directly. The glide must take
    // that as the new truth, not drag the view back to where it was going.
    let mut glide = Glide::at(0.0);
    glide.push(1000.0);
    glide.step(1.0 / 60.0, 0.5);
    let written = glide.position();
    // Nothing else touched the view: adopting what the glide itself wrote
    // changes nothing and the glide keeps going.
    glide.adopt(written);
    assert!(glide.moving());
    // Something else moved it to 5000.
    glide.adopt(5000.0);
    assert_eq!(glide.position(), 5000.0);
    assert!(!glide.moving(), "the old target was dropped");
}

#[test]
fn a_push_while_gliding_extends_the_same_motion() {
    let mut glide = Glide::at(0.0);
    glide.push(100.0);
    glide.step(1.0 / 60.0, 0.5);
    glide.push(100.0);
    while glide.step(1.0 / 60.0, 0.5).is_some() {}
    assert_eq!(glide.position(), 200.0);
}

// ---------------------------------------------------- fractional rows ---

#[test]
fn a_lane_offset_scrolls_the_rows_by_a_fraction_of_a_row() {
    let view = TimelineView {
        top_lane: 2,
        lane_offset: 0.5,
        lane_height: 40.0,
        ..TimelineView::default()
    };
    // Lane 2's row starts half a row above the grid; lane 3 starts half a
    // row down.
    assert_eq!(lane_to_y(&view, GRID, 2), GRID.y - 20.0);
    assert_eq!(lane_to_y(&view, GRID, 3), GRID.y + 20.0);
    // And the pointer agrees: the top 20 px are lane 2, then lane 3.
    assert_eq!(y_to_lane(&view, GRID, GRID.y + 10.0), 2);
    assert_eq!(y_to_lane(&view, GRID, GRID.y + 30.0), 3);
}

#[test]
fn a_key_offset_scrolls_the_keys_by_a_fraction_of_a_key() {
    let view = RollView {
        top_key: 84,
        key_offset: 0.5,
        key_height: 20.0,
        ..RollView::default()
    };
    // Half a row of key 85 shows above key 84.
    assert_eq!(key_to_y(&view, GRID, 85), GRID.y - 10.0);
    assert_eq!(key_to_y(&view, GRID, 84), GRID.y + 10.0);
    assert_eq!(y_to_key(&view, GRID, GRID.y + 5.0), 85);
    assert_eq!(y_to_key(&view, GRID, GRID.y + 15.0), 84);
    // So the visible keys include it, or the top of the grid is blank.
    assert!(visible_keys(&view, GRID).contains(&85));
}

#[test]
fn the_offsets_default_to_nothing_so_every_old_view_reads_as_it_did() {
    assert_eq!(TimelineView::default().lane_offset, 0.0);
    assert_eq!(RollView::default().key_offset, 0.0);
    let _ = PPQN;
}
