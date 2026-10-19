//! The wheel over a ruler zooms in time about the pointer, as FL's does
//! (Ty, 2026-10-18). Over the lanes or keys it still scrolls: navigation,
//! never an edit.

mod common;

fn a_window() -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(common::a_session_for(common::a_clip_project(8))),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
}

#[test]
fn the_wheel_over_the_arrangements_ruler_zooms_about_the_pointer() {
    let mut w = a_window();
    let (ruler, grid, before) = w.timeline_ruler_and_view();
    let x = ruler.x + ruler.width * 0.6;
    let tick = fontelle_ui::canvas::timeline_x_to_tick(&before, grid, x);
    w.wheel_studio(x, ruler.y + ruler.height / 2.0, 1.0);
    let (_, _, after) = w.timeline_ruler_and_view();
    assert!(
        after.pixels_per_tick > before.pixels_per_tick,
        "it did not zoom in"
    );
    let still = fontelle_ui::canvas::timeline_x_to_tick(&after, grid, x);
    assert!((still - tick).abs() <= 2, "{tick} moved to {still}");
    assert_eq!(after.top_lane, before.top_lane, "the lanes scrolled");
}

#[test]
fn the_wheel_over_the_rolls_ruler_zooms_about_the_pointer() {
    let mut w = a_window();
    let (ruler, grid, before) = w.roll_ruler_and_view();
    let x = ruler.x + ruler.width * 0.4;
    let tick = fontelle_ui::canvas::x_to_tick(&before, grid, x);
    w.wheel_studio(x, ruler.y + ruler.height / 2.0, 1.0);
    let (_, _, after) = w.roll_ruler_and_view();
    assert!(
        after.pixels_per_tick > before.pixels_per_tick,
        "it did not zoom in"
    );
    let still = fontelle_ui::canvas::x_to_tick(&after, grid, x);
    assert!((still - tick).abs() <= 8, "{tick} moved to {still}");
    assert_eq!(after.top_key, before.top_key, "the keys scrolled");
}

#[test]
fn over_the_lanes_the_wheel_still_scrolls() {
    let mut w = a_window();
    let (_, grid, before) = w.timeline_ruler_and_view();
    w.wheel_studio(grid.x + 100.0, grid.y + 40.0, -1.0);
    let (_, _, after) = w.timeline_ruler_and_view();
    assert_eq!(after.pixels_per_tick, before.pixels_per_tick);
}
