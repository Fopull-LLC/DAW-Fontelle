//! The icon set, as geometry.
//!
//! Reported from using the window: *"I want the cursor icons to actually
//! represent the action better and I want the app to have more icons instead
//! of just text."*
//!
//! # Why these are drawn rather than loaded
//!
//! vello is a path renderer, so an icon that is a path costs nothing to draw
//! and is crisp at any scale — a `2x` display and a `1x` one get the same
//! shape rather than the same pixels stretched. It takes the theme's own ink,
//! so a light theme does not need a second set. It adds no files to ship, no
//! atlas to pack, and no licence to track. And the same shapes rasterise into
//! the **mouse cursor**, so the pencil on the toolbar and the pencil under the
//! pointer are one definition rather than two that drift.
//!
//! Everything here is a pure function of an [`Icon`], in a unit box, so the
//! whole set is checkable without a window: nothing empty, nothing off the
//! edge, nothing degenerate.

use fontelle_ui::icon::{EVERY_ICON, Icon, Shape, rasterise, shapes};

#[test]
fn every_icon_draws_something() {
    // An icon that is a blank square is worse than the word it replaced.
    for icon in EVERY_ICON {
        let shapes = shapes(icon);
        assert!(!shapes.is_empty(), "{icon:?} draws nothing");
        for shape in &shapes {
            match shape {
                Shape::Line { points, .. } => {
                    assert!(points.len() >= 2, "{icon:?} has a line of one point");
                }
                Shape::Poly(points) => {
                    assert!(points.len() >= 3, "{icon:?} has a polygon of two points");
                }
                Shape::Circle { r, .. } => {
                    assert!(*r > 0.0, "{icon:?} has a circle of no size");
                }
            }
        }
    }
}

#[test]
fn every_icon_stays_inside_its_own_box() {
    // The renderer maps the unit box onto whatever rectangle the layout gave
    // it, so a point outside is an icon that draws over its neighbour.
    for icon in EVERY_ICON {
        for shape in shapes(icon) {
            let points: Vec<(f32, f32)> = match &shape {
                Shape::Line { points, .. } | Shape::Poly(points) => points.clone(),
                Shape::Circle { at, r, .. } => vec![(at.0 - r, at.1 - r), (at.0 + r, at.1 + r)],
            };
            for (x, y) in points {
                assert!(
                    (-0.001..=1.001).contains(&x) && (-0.001..=1.001).contains(&y),
                    "{icon:?} has a point at ({x}, {y}), outside the unit box"
                );
            }
        }
    }
}

#[test]
fn every_icon_actually_uses_the_box_it_is_given() {
    // A shape huddled in one corner reads as a smudge at 16 pixels. Each icon
    // has to span most of its box in at least one axis.
    for icon in EVERY_ICON {
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
        for shape in shapes(icon) {
            let points: Vec<(f32, f32)> = match &shape {
                Shape::Line { points, .. } | Shape::Poly(points) => points.clone(),
                Shape::Circle { at, r, .. } => vec![(at.0 - r, at.1 - r), (at.0 + r, at.1 + r)],
            };
            for (x, y) in points {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        let span = (max_x - min_x).max(max_y - min_y);
        assert!(
            span > 0.5,
            "{icon:?} spans only {span} of its box — it will read as a smudge"
        );
    }
}

// --------------------------------------------------------- the cursors ---

#[test]
fn an_icon_rasterises_into_a_cursor_bitmap_with_something_in_it() {
    // The point of the rasteriser: the pencil on the toolbar and the pencil
    // under the pointer are the same shape, not two that drift apart.
    let pixels = rasterise(Icon::Pencil, 32);
    assert_eq!(pixels.len(), 32 * 32 * 4);

    let opaque = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 0)
        .count();
    assert!(opaque > 40, "the pencil covered {opaque} pixels of 1024");
    assert!(
        opaque < 32 * 32 / 2,
        "an icon covering half the cursor is a blob: {opaque}"
    );
}

#[test]
fn a_cursor_is_drawn_in_white_with_a_dark_edge_so_it_shows_on_any_background() {
    // A cursor is drawn over the user's own colours, and a single-ink one
    // disappears against half of them.
    let pixels = rasterise(Icon::Eraser, 32);
    let mut light = 0;
    let mut dark = 0;
    for p in pixels.as_chunks::<4>().0 {
        if p[3] < 128 {
            continue;
        }
        if p[0] > 200 {
            light += 1;
        } else if p[0] < 80 {
            dark += 1;
        }
    }
    assert!(light > 0, "nothing bright in it");
    assert!(
        dark > 0,
        "and nothing to separate it from a light background"
    );
}

#[test]
fn every_icon_rasterises_without_running_off_the_bitmap() {
    for icon in EVERY_ICON {
        for size in [16u32, 24, 32] {
            let pixels = rasterise(icon, size);
            assert_eq!(
                pixels.len(),
                (size * size * 4) as usize,
                "{icon:?} at {size}px"
            );
            assert!(
                pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "{icon:?} at {size}px rasterised to nothing"
            );
        }
    }
}

#[test]
fn there_is_a_help_icon_and_it_is_in_the_set_the_tests_walk() {
    // The `?` on the start menu and the transport bar. In `EVERY_ICON` so the
    // checks above — draws something, stays in its box, fills it — hold for
    // it too.
    assert!(EVERY_ICON.contains(&Icon::Help));
    assert!(
        shapes(Icon::Help).len() >= 2,
        "a question mark is a hook and a dot"
    );
}
