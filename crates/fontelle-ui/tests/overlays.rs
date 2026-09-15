//! The toast banner and the confirm modal — where they sit and where their
//! buttons are. Pure geometry, so the window only has to draw them and ask
//! "was this point on the Undo / on Remove".

use fontelle_ui::canvas::{TOAST_SECONDS, confirm_layout, toast_layout};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn window() -> Rect {
    Rect::new(0.0, 0.0, 1000.0, 620.0)
}

#[test]
fn a_toast_sits_inside_the_window_near_the_bottom() {
    let l = toast_layout(window(), &metrics(), true);
    assert!(!l.frame.is_empty());
    assert_eq!(
        l.frame.intersection(&window()),
        l.frame,
        "the toast escapes"
    );
    assert!(
        l.frame.y > window().y + window().height / 2.0,
        "a toast belongs near the bottom, not the middle"
    );
}

#[test]
fn a_toast_has_an_undo_only_when_it_can_be_undone() {
    let with = toast_layout(window(), &metrics(), true);
    let undo = with.undo.expect("an undoable toast has an Undo button");
    assert_eq!(
        undo.intersection(&with.frame),
        undo,
        "Undo stays in the toast"
    );
    assert!(
        undo.right() <= with.frame.right() + 0.01 && undo.x > with.frame.x,
        "Undo is on the right"
    );
    let without = toast_layout(window(), &metrics(), false);
    assert!(without.undo.is_none(), "a plain note offers no Undo");
}

#[test]
fn the_toast_lives_a_few_seconds() {
    // Long enough to read and reach for Undo, short enough to get out of the way.
    assert!((3.0..=12.0).contains(&TOAST_SECONDS));
}

#[test]
fn a_confirm_is_centred_with_two_separate_buttons() {
    let l = confirm_layout(window(), &metrics());
    assert!(!l.frame.is_empty());
    assert_eq!(l.frame.intersection(&window()), l.frame);
    // Roughly centred.
    let cx = l.frame.x + l.frame.width / 2.0;
    assert!(
        (cx - window().width / 2.0).abs() < 1.0,
        "not centred across"
    );
    // Cancel on the left, Remove on the right, and they do not overlap.
    assert!(l.cancel.right() <= l.confirm.x + 0.01, "buttons overlap");
    assert!(!l.cancel.is_empty() && !l.confirm.is_empty());
    for b in [l.cancel, l.confirm, l.question] {
        assert_eq!(b.intersection(&l.frame), b, "a part escapes the dialog");
    }
    // The buttons are along the bottom, under the question.
    assert!(l.cancel.y >= l.question.bottom() - 0.01);
}
