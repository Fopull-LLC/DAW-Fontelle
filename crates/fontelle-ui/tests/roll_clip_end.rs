//! Where the clip ends, drawn.
//!
//! A clip's `length` is the window on its content: a note past it does not
//! sound, and is cut where the clip stops (TDD §11.4, from *"clip endings
//! don't actually cut the clip short audibly right now it keeps playing"*).
//! A clip does **not** grow to swallow a note drawn beyond it — which was
//! asked for in those words — so the roll has to *show* where the end is,
//! or a note drawn out there is silent for no visible reason.
//!
//! The geometry is a pure function and this is its test; the fill itself is
//! one rectangle in `draw_piano_roll`.

use fontelle_types::{PPQN, Tick};
use fontelle_ui::canvas::{RollView, SnapDivision, roll_past_end, tick_to_x};
use fontelle_ui::layout::Rect;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
}

/// Four beats in, at a quarter of a pixel a tick, is 960 px — off the right
/// of an 800 px grid, so there is nothing to shade.
#[test]
fn an_end_beyond_the_right_edge_shades_nothing() {
    assert_eq!(roll_past_end(&view(), grid(), PPQN * 4), None);
}

/// Two beats in is 480 px: everything right of that is outside the clip.
#[test]
fn the_region_past_the_end_is_the_rest_of_the_grid() {
    let (view, grid) = (view(), grid());
    let rect = roll_past_end(&view, grid, PPQN * 2).expect("the end is on screen");
    assert!(
        (rect.x - tick_to_x(&view, grid, PPQN * 2)).abs() < 0.01,
        "{rect:?}"
    );
    assert!((rect.right() - grid.right()).abs() < 0.01, "{rect:?}");
    assert!((rect.y - grid.y).abs() < 0.01);
    assert!((rect.height - grid.height).abs() < 0.01);
}

/// Scrolled past the end, the whole grid is outside the clip — a shade over
/// all of it, not a rectangle starting off the left edge.
#[test]
fn scrolled_past_the_end_the_whole_grid_is_shaded() {
    let mut view = view();
    view.scroll_tick = PPQN * 8;
    let rect = roll_past_end(&view, grid(), PPQN * 2).expect("all of it");
    assert!((rect.x - grid().x).abs() < 0.01, "{rect:?}");
    assert!((rect.width - grid().width).abs() < 0.01, "{rect:?}");
}

/// A clip of no length shades everything; a negative one is not a shape.
#[test]
fn a_clip_of_nothing_shades_the_whole_grid() {
    let rect = roll_past_end(&view(), grid(), 0).expect("everything");
    assert!((rect.width - grid().width).abs() < 0.01);
    assert_eq!(
        roll_past_end(&view(), grid(), -PPQN as Tick).map(|r| r.width),
        Some(grid().width)
    );
}

/// An empty grid has nothing to draw on.
#[test]
fn an_empty_grid_shades_nothing() {
    assert_eq!(
        roll_past_end(&view(), Rect::new(0.0, 0.0, 0.0, 0.0), PPQN),
        None
    );
}
