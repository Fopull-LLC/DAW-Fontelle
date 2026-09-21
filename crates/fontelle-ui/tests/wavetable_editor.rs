//! The wavetable editor's geometry (`docs/flopsynth-next.md` §4.3), pure:
//! where a pointer over the draw picture lands in the cycle, which bar it
//! is over, and what level it asks for.

use fontelle_ui::canvas::{bar_at, draw_point};
use fontelle_ui::layout::Rect;

#[test]
fn a_pointer_over_the_draw_picture_is_a_phase_and_a_level() {
    let picture = Rect::new(100.0, 50.0, 200.0, 100.0);
    // The left edge is phase 0, the right 1; the top is 1, the bottom −1.
    let (x, y) = draw_point(picture, 100.0, 50.0);
    assert!((x - 0.0).abs() < 1e-6 && (y - 1.0).abs() < 1e-6);
    let (x, y) = draw_point(picture, 300.0, 150.0);
    assert!((x - 1.0).abs() < 1e-6 && (y + 1.0).abs() < 1e-6);
    let (x, y) = draw_point(picture, 200.0, 100.0);
    assert!((x - 0.5).abs() < 1e-6 && y.abs() < 1e-6);
    // Outside is clamped to the edge: a drag that runs off the picture
    // draws to the edge rather than past it.
    let (x, y) = draw_point(picture, 50.0, 500.0);
    assert!((x - 0.0).abs() < 1e-6 && (y + 1.0).abs() < 1e-6);
}

#[test]
fn a_pointer_over_the_bars_names_the_bar_and_its_height() {
    let picture = Rect::new(0.0, 0.0, 640.0, 100.0);
    // Sixty-four bars across: ten pixels each.
    let (index, level) = bar_at(picture, 64, 5.0, 0.0).unwrap();
    assert_eq!((index, level), (0, 1.0));
    let (index, level) = bar_at(picture, 64, 635.0, 100.0).unwrap();
    assert_eq!(index, 63);
    assert!(level.abs() < 1e-6);
    let (index, level) = bar_at(picture, 64, 325.0, 50.0).unwrap();
    assert_eq!(index, 32);
    assert!((level - 0.5).abs() < 1e-6);
    // Off the picture is nothing.
    assert!(bar_at(picture, 64, -1.0, 50.0).is_none());
    assert!(bar_at(picture, 64, 5.0, 101.0).is_none());
}
