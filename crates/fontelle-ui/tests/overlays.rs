//! The toast banner and the confirm modal — where they sit and where their
//! buttons are. Pure geometry, so the window only has to draw them and ask
//! "was this point on the Undo / on Remove".

use fontelle_ui::canvas::{
    SAVED_FLASH_SECONDS, TOAST_SECONDS, confirm_layout, job_card_layout, project_caption,
    save_prompt_layout, saved_flash, toast_layout,
};
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

// --- saving, leaving, and long jobs -------------------------------------
//
// > *"theres also no indication if your project is saved or not and also if
// > your project is unsaved you can close it without it asking if you want to
// > save first ... when i save it shows a Saved! text that appears in the top
// > center and moves upwards as it fades out ... should have a * next to the
// > project name when unsaved."*

#[test]
fn an_unsaved_project_wears_a_star_after_its_name() {
    assert_eq!(project_caption("Song", true), "Song*");
    assert_eq!(project_caption("Song", false), "Song");
}

#[test]
fn saved_starts_at_the_top_centre() {
    let f = saved_flash(window(), &metrics(), 0.0).expect("shown the moment it saves");
    assert!(
        (f.center_x - window().width / 2.0).abs() < 1.0,
        "not centred"
    );
    assert!(
        f.center_y < window().y + window().height / 4.0,
        "Saved! belongs at the top, not at {}",
        f.center_y
    );
    assert!(f.alpha > 0.9, "it starts fully visible, not at {}", f.alpha);
}

#[test]
fn saved_rises_as_it_fades_then_is_gone() {
    let m = metrics();
    let mut last = saved_flash(window(), &m, 0.0).unwrap();
    let steps = 10;
    for i in 1..steps {
        let t = SAVED_FLASH_SECONDS * i as f32 / steps as f32;
        let now = saved_flash(window(), &m, t).expect("still up inside its time");
        assert!(now.center_y < last.center_y, "it moves upwards, step {i}");
        assert!(now.alpha <= last.alpha, "it only fades, step {i}");
        assert!(now.center_y >= window().y, "it stays in the window");
        last = now;
    }
    assert!(last.alpha < 0.5, "near the end it is mostly faded");
    assert!(saved_flash(window(), &m, SAVED_FLASH_SECONDS + 0.01).is_none());
    assert!(
        (0.5..=3.0).contains(&SAVED_FLASH_SECONDS),
        "a glance, not a banner"
    );
}

#[test]
fn the_save_prompt_offers_save_dont_save_and_cancel_side_by_side() {
    let l = save_prompt_layout(window(), &metrics());
    assert_eq!(l.frame.intersection(&window()), l.frame);
    let buttons = [l.discard, l.cancel, l.save];
    for b in buttons.iter().chain([&l.question]) {
        assert!(!b.is_empty());
        assert_eq!(b.intersection(&l.frame), *b, "a part escapes the dialog");
    }
    // Left to right: Don't Save, Cancel, Save — the safe answer is the
    // weighted one on the right, where the confirm modal puts its own.
    assert!(l.discard.right() <= l.cancel.x + 0.01, "buttons overlap");
    assert!(l.cancel.right() <= l.save.x + 0.01, "buttons overlap");
    assert!(l.save.y >= l.question.bottom() - 0.01);
}

#[test]
fn a_job_card_holds_its_words_and_its_bar_inside_the_window() {
    let l = job_card_layout(window(), &metrics());
    assert!(!l.frame.is_empty());
    assert_eq!(l.frame.intersection(&window()), l.frame);
    for part in [l.label, l.bar] {
        assert!(!part.is_empty());
        assert_eq!(part.intersection(&l.frame), part, "a part escapes the card");
    }
    assert!(
        l.bar.y >= l.label.bottom() - 0.01,
        "the bar is under the words"
    );
    // Out of the toast's way: a finished export raises a toast while the
    // card may still be drawn for its last frame.
    let toast = toast_layout(window(), &metrics(), false).frame;
    assert!(
        l.frame.intersection(&toast).is_empty(),
        "the card sits on the toast"
    );
}
