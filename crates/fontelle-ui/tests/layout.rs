//! Window geometry, which is arithmetic and therefore tested here rather than
//! looked at (`docs/first-usable-plan.md` §2.5).

use fontelle_ui::layout::{Rect, window_layout};
use fontelle_ui::theme::Theme;

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

#[test]
fn the_panel_is_the_window_inset_by_the_margin() {
    let m = metrics();
    let l = window_layout(1280.0, 720.0, &m);

    assert_eq!(l.window, Rect::new(0.0, 0.0, 1280.0, 720.0));
    assert_eq!(
        l.panel.frame,
        Rect::new(
            m.panel_margin,
            m.panel_margin,
            1280.0 - 2.0 * m.panel_margin,
            720.0 - 2.0 * m.panel_margin,
        )
    );
}

#[test]
fn the_header_sits_on_top_of_the_body_and_they_tile_the_frame() {
    let m = metrics();
    let l = window_layout(1280.0, 720.0, &m);
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
    let small = window_layout(1280.0, 720.0, &m);
    let tall = window_layout(1280.0, 820.0, &m);

    assert_eq!(small.panel.header.height, tall.panel.header.height);
    assert_eq!(small.panel.body.width, tall.panel.body.width);
    assert_eq!(tall.panel.body.height, small.panel.body.height + 100.0);
}

#[test]
fn a_window_too_small_for_its_chrome_yields_empty_rects_never_negative_ones() {
    let m = metrics();
    // A user dragging a window edge past the chrome is ordinary, and a
    // negative width reaches the GPU as a panic or a garbage draw.
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (4.0, 900.0), (900.0, 4.0)] {
        let l = window_layout(w, h, &m);
        for r in [l.panel.frame, l.panel.header, l.panel.body] {
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
    let logical = window_layout(1280.0, 720.0, &m);
    assert_eq!(
        logical.panel.frame.scale(2.0).width,
        logical.panel.frame.width * 2.0
    );
    assert_eq!(
        logical.panel.frame.scale(2.0).x,
        logical.panel.frame.x * 2.0
    );
}
