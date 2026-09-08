//! Window geometry, which is arithmetic and therefore tested here rather than
//! looked at (`docs/first-usable-plan.md` §2.5).

use fontelle_ui::canvas::zoom_anchor;
use fontelle_ui::layout::DEFAULT_TIMELINE_HEIGHT;
use fontelle_ui::layout::{Rect, window_layout};
use fontelle_ui::theme::Theme;

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

#[test]
fn the_transport_bar_is_across_the_top_and_the_panels_are_under_it() {
    let m = metrics();
    let l = window_layout(1280.0, 720.0, &m, DEFAULT_TIMELINE_HEIGHT);

    assert_eq!(l.window, Rect::new(0.0, 0.0, 1280.0, 720.0));
    assert_eq!(
        l.transport,
        Rect::new(
            m.panel_margin,
            m.panel_margin,
            1280.0 - 2.0 * m.panel_margin,
            m.transport_bar_height,
        )
    );
    // The roll's panel starts to the right of the sidebar and runs to the
    // window's own margin. Where the sidebar itself goes is `panels.rs`.
    assert_eq!(l.rack.frame.x, m.panel_margin);
    assert!(l.panel.frame.x > m.panel_margin);
    assert_eq!(l.panel.frame.right(), 1280.0 - m.panel_margin);
    assert!(
        l.panel.frame.y >= l.transport.bottom(),
        "the panel starts above the transport bar"
    );
    assert_eq!(l.panel.frame.bottom(), 720.0 - m.panel_margin);
    assert!(!l.transport.intersects(&l.panel.frame));
}

#[test]
fn the_header_sits_on_top_of_the_body_and_they_tile_the_frame() {
    let m = metrics();
    let l = window_layout(1280.0, 720.0, &m, DEFAULT_TIMELINE_HEIGHT);
    let (frame, header, body) = (l.panel.frame, l.panel.header, l.panel.body);

    assert_eq!(header.y, frame.y);
    assert_eq!(header.x, frame.x);
    assert_eq!(header.width, frame.width);
    assert_eq!(header.height, m.panel_header_height);

    // The body is what is left, inset by the padding — so it is inside the
    // frame and it never overlaps the header.
    assert!(body.y >= header.bottom(), "body starts above the header");
    assert!(body.bottom() <= frame.bottom(), "body runs past the frame");
    assert!(body.x >= frame.x && body.right() <= frame.right());
    assert!(!header.intersects(&body));
}

#[test]
fn growing_the_window_grows_only_the_body() {
    let m = metrics();
    let small = window_layout(1280.0, 720.0, &m, DEFAULT_TIMELINE_HEIGHT);
    let tall = window_layout(1280.0, 820.0, &m, DEFAULT_TIMELINE_HEIGHT);

    assert_eq!(small.panel.header.height, tall.panel.header.height);
    assert_eq!(small.panel.body.width, tall.panel.body.width);
    assert_eq!(small.transport, tall.transport);
    assert_eq!(tall.panel.body.height, small.panel.body.height + 100.0);
}

#[test]
fn a_window_too_small_for_its_chrome_yields_empty_rects_never_negative_ones() {
    let m = metrics();
    // A user dragging a window edge past the chrome is ordinary, and a
    // negative width reaches the GPU as a panic or a garbage draw.
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (4.0, 900.0), (900.0, 4.0)] {
        let l = window_layout(w, h, &m, DEFAULT_TIMELINE_HEIGHT);
        for r in [l.transport, l.panel.frame, l.panel.header, l.panel.body] {
            assert!(
                r.width >= 0.0 && r.height >= 0.0,
                "{w}x{h} produced {r:?} with a negative dimension"
            );
        }
        assert!(
            l.panel.body.is_empty(),
            "{w}x{h} should have no room to draw in"
        );
    }
}

#[test]
fn an_empty_rect_is_the_identity_of_union() {
    let r = Rect::new(10.0, 20.0, 30.0, 40.0);
    assert_eq!(Rect::ZERO.union(&r), r);
    assert_eq!(r.union(&Rect::ZERO), r);
    assert_eq!(r.union(&r), r);
}

#[test]
fn a_union_covers_both_of_its_arguments() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(90.0, 40.0, 10.0, 10.0);
    let u = a.union(&b);
    assert_eq!(u, Rect::new(0.0, 0.0, 100.0, 50.0));
    for r in [a, b] {
        assert!(u.contains(r.x, r.y) && u.contains(r.right() - 0.1, r.bottom() - 0.1));
    }
}

#[test]
fn rects_that_only_touch_do_not_intersect() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    // Sharing an edge is how tiled regions abut; if that counted as an
    // intersection, every neighbour would redirty every neighbour.
    assert!(!a.intersects(&Rect::new(10.0, 0.0, 10.0, 10.0)));
    assert!(a.intersects(&Rect::new(9.9, 0.0, 10.0, 10.0)));
    assert!(!a.intersects(&Rect::ZERO));
}

#[test]
fn contains_is_half_open_so_abutting_rects_do_not_share_a_pixel() {
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(r.contains(0.0, 0.0));
    assert!(r.contains(9.99, 9.99));
    assert!(!r.contains(10.0, 5.0));
    assert!(!r.contains(5.0, 10.0));
    assert!(!r.contains(-0.01, 5.0));
}

#[test]
fn an_inset_never_turns_a_rect_inside_out() {
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    let squeezed = r.inset(20.0);
    assert!(squeezed.width >= 0.0 && squeezed.height >= 0.0);
    assert!(squeezed.is_empty());
    assert_eq!(r.inset(2.0), Rect::new(2.0, 2.0, 6.0, 6.0));
}

#[test]
fn a_physical_size_scales_by_the_dpi_factor() {
    let m = metrics();
    // The layout is in logical pixels; the surface is in physical ones. Doing
    // this arithmetic in two places is how a HiDPI window ends up with chrome
    // at half size, so there is one function for it.
    let logical = window_layout(1280.0, 720.0, &m, DEFAULT_TIMELINE_HEIGHT);
    assert_eq!(
        logical.panel.frame.scale(2.0).width,
        logical.panel.frame.width * 2.0
    );
    assert_eq!(
        logical.panel.frame.scale(2.0).x,
        logical.panel.frame.x * 2.0
    );
}

// ------------------------------------------------------ where zoom lands ---
//
// > *"i currently cant [...] i dont like when i zoom in and out its based
// > around where my playhead is and i dont like that i want it to be based on
// > my cursor for maximum user control."*
//
// The wheel already zoomed about the pointer. The **buttons** and the `+`/`-`
// keys did not: they took the middle of the grid, which on a view scrolled to
// follow the playhead is the playhead, near enough. One rule for all three now,
// and it lives here rather than at each of the three call sites.

#[test]
fn zoom_lands_on_the_pointer_when_the_pointer_is_over_the_grid() {
    let grid = Rect::new(100.0, 50.0, 400.0, 300.0);
    for x in [100.0f32, 180.0, 300.0, 499.0] {
        assert_eq!(
            zoom_anchor(grid, (x, 120.0)),
            x,
            "a pointer at {x} did not anchor the zoom"
        );
    }
}

#[test]
fn zoom_falls_back_to_the_middle_when_the_pointer_is_elsewhere() {
    // A button pressed with the pointer on the *toolbar* is still a zoom, and
    // it has to land somewhere sensible: the middle of what you are looking at.
    let grid = Rect::new(100.0, 50.0, 400.0, 300.0);
    let middle = grid.x + grid.width / 2.0;
    for at in [
        (50.0f32, 120.0f32),
        (900.0, 120.0),
        (180.0, 10.0),
        (180.0, 900.0),
    ] {
        assert_eq!(
            zoom_anchor(grid, at),
            middle,
            "a pointer at {at:?} anchored somewhere else"
        );
    }
}

#[test]
fn an_empty_grid_still_answers_rather_than_producing_a_nan() {
    let anchor = zoom_anchor(Rect::ZERO, (10.0, 10.0));
    assert!(anchor.is_finite());
}
