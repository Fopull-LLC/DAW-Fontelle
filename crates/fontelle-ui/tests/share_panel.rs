//! The Share panel and the join's question (`docs/collab-plan.md` §10.1,
//! §10.2, F46, F49).
//!
//! The panel opens from the transport bar's Share button and is the whole of
//! a session from inside the studio: the code, in the largest type on it;
//! Copy and Stop sharing; who is here, each with *view only* and *remove* on
//! the host's panel; a line saying what is happening. Alone, it offers the two
//! ways in. Like every canvas here it is a layout from rectangles and counts
//! and a hit from a point — what any of it does is the window's.

use fontelle_ui::canvas::{
    ChoicePromptLayout, ShareHit, SharePanelLayout, ShareRole, choice_prompt_hit,
    choice_prompt_layout, share_hit, share_panel_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn wide() -> Rect {
    Rect::new(0.0, 0.0, 1600.0, 900.0)
}

fn narrow() -> Rect {
    Rect::new(0.0, 0.0, 360.0, 640.0)
}

/// The Share button, at the bar's right end, where the transport lays it.
fn anchor(window: Rect) -> Rect {
    Rect::new(window.right() - 140.0, 10.0, 28.0, 28.0)
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.is_empty() || inner.intersection(&outer) == inner
}

/// Everything on the panel a person can press, with what pressing it is.
fn pressables(l: &SharePanelLayout) -> Vec<(Rect, ShareHit)> {
    let mut all = vec![
        (l.primary, ShareHit::Primary),
        (l.copy, ShareHit::Copy),
        (l.join, ShareHit::Join),
    ];
    for (i, row) in l.rows.iter().enumerate() {
        all.push((row.view_only, ShareHit::ViewOnly(i)));
        all.push((row.remove, ShareHit::Remove(i)));
    }
    all.retain(|(r, _)| !r.is_empty());
    all
}

fn every_role() -> [ShareRole; 5] {
    [
        ShareRole::Alone,
        ShareRole::Starting,
        ShareRole::Hosting,
        ShareRole::Joining,
        ShareRole::Joined,
    ]
}

#[test]
fn at_both_widths_everything_is_on_the_panel_and_the_panel_is_in_the_window() {
    for window in [wide(), narrow()] {
        for role in every_role() {
            let l = share_panel_layout(window, anchor(window), &metrics(), role, 3);
            assert!(!l.frame.is_empty(), "{role:?} at {window:?}: no panel");
            assert!(within(l.frame, window), "{role:?}: {:?} escapes", l.frame);
            for r in [l.title, l.code, l.copy, l.primary, l.join, l.status] {
                assert!(within(r, l.frame), "{role:?}: {r:?} escapes the panel");
            }
            for row in &l.rows {
                for r in [row.frame, row.swatch, row.name, row.view_only, row.remove] {
                    assert!(within(r, l.frame), "{role:?}: {r:?} escapes the panel");
                }
            }
            let pressables = pressables(&l);
            for (i, (a, _)) in pressables.iter().enumerate() {
                for (b, _) in &pressables[i + 1..] {
                    assert!(!a.intersects(b), "{role:?}: {a:?} overlaps {b:?}");
                }
            }
        }
    }
}

/// The panel hangs from the button that opened it, not somewhere else.
#[test]
fn the_panel_hangs_under_the_share_button() {
    let window = wide();
    let l = share_panel_layout(window, anchor(window), &metrics(), ShareRole::Hosting, 1);
    assert!(l.frame.y >= anchor(window).bottom());
    assert!(l.frame.x <= anchor(window).x && l.frame.right() >= anchor(window).right());
}

/// The code is read aloud across a room or typed from a screenshot: it is the
/// biggest thing on the panel, and there is room for all six characters of it.
#[test]
fn the_code_is_the_largest_text_on_the_panel() {
    for window in [wide(), narrow()] {
        let l = share_panel_layout(window, anchor(window), &metrics(), ShareRole::Hosting, 2);
        assert!(
            l.code_size >= l.text_size * 2.0,
            "{} vs {}",
            l.code_size,
            l.text_size
        );
        assert!(l.code.height >= l.code_size);
        assert!(l.code.width >= 6.0 * l.code_size * 0.7, "{:?}", l.code);
        // The status is two lines (F60); each is a line of ordinary text.
        let status_line = l.status.height / 2.0;
        for h in [l.title.height, status_line, l.primary.height, l.copy.height] {
            assert!(h < l.code.height, "{h} is as tall as the code");
        }
    }
}

#[test]
fn every_pressable_thing_answers_to_a_point_inside_it() {
    for role in every_role() {
        let l = share_panel_layout(wide(), anchor(wide()), &metrics(), role, 3);
        for (r, what) in pressables(&l) {
            let (x, y) = centre(r);
            assert_eq!(share_hit(&l, x, y), Some(what), "{role:?}");
        }
        let (x, y) = centre(l.title);
        assert_eq!(share_hit(&l, x, y), None, "the title is not a button");
        assert_eq!(share_hit(&l, -5.0, -5.0), None);
    }
}

/// Alone there is no code and nobody to list: the two ways in.
#[test]
fn alone_the_panel_offers_to_share_or_to_join() {
    let l = share_panel_layout(wide(), anchor(wide()), &metrics(), ShareRole::Alone, 0);
    assert!(l.code.is_empty() && l.copy.is_empty());
    assert!(l.rows.is_empty());
    assert!(!l.primary.is_empty(), "Share this song");
    assert!(!l.join.is_empty(), "Join a shared song");
}

/// F49: only the host's panel has the two controls on a person's row; a
/// joiner's lists who is there and nothing to press.
#[test]
fn only_the_host_can_make_somebody_view_only_or_remove_them() {
    let host = share_panel_layout(wide(), anchor(wide()), &metrics(), ShareRole::Hosting, 2);
    assert_eq!(host.rows.len(), 2);
    for row in &host.rows {
        assert!(!row.view_only.is_empty() && !row.remove.is_empty());
        assert!(!row.name.intersects(&row.view_only));
    }
    let joiner = share_panel_layout(wide(), anchor(wide()), &metrics(), ShareRole::Joined, 2);
    assert_eq!(joiner.rows.len(), 2);
    for row in &joiner.rows {
        assert!(row.view_only.is_empty() && row.remove.is_empty());
    }
    assert!(joiner.code.is_empty(), "the code is the host's to give out");
}

/// A room of more people than fit is cut to what fits, never run off the
/// panel.
#[test]
fn a_crowd_is_cut_to_what_fits() {
    let l = share_panel_layout(
        narrow(),
        anchor(narrow()),
        &metrics(),
        ShareRole::Hosting,
        200,
    );
    assert!(l.rows.len() < 200);
    assert!(!l.rows.is_empty());
    assert!(within(l.rows.last().unwrap().frame, l.frame));
}

// ------------------------------------------------------ the join's question ---

fn prompt(window: Rect, buttons: usize) -> ChoicePromptLayout {
    choice_prompt_layout(window, &metrics(), 3, buttons)
}

/// §4.3: *Update*, *Keep both*, *Cancel* — three buttons, left to right in
/// the order the question gives them, which is **the safe one first**.
#[test]
fn three_buttons_in_the_prompt_with_the_safe_one_first() {
    for window in [wide(), narrow()] {
        let l = prompt(window, 3);
        assert_eq!(l.buttons.len(), 3);
        assert!(within(l.frame, window));
        assert!(l.buttons[0].x < l.buttons[1].x && l.buttons[1].x < l.buttons[2].x);
        for (i, a) in l.buttons.iter().enumerate() {
            assert!(!a.is_empty() && within(*a, l.frame), "{a:?}");
            assert_eq!(a.y, l.buttons[0].y, "one row");
            for b in &l.buttons[i + 1..] {
                assert!(!a.intersects(b));
            }
            let (x, y) = centre(*a);
            assert_eq!(choice_prompt_hit(&l, x, y), Some(i));
        }
        assert_eq!(l.lines.len(), 3);
        for line in &l.lines {
            assert!(within(*line, l.frame));
            assert!(
                line.bottom() <= l.buttons[0].y,
                "the words are above the buttons"
            );
        }
        let (x, y) = centre(l.lines[0]);
        assert_eq!(choice_prompt_hit(&l, x, y), None);
    }
}

/// The same card asks two questions and four: the join's first time is Copy
/// or Cancel, a fetch is Fetch or Not now.
#[test]
fn the_prompt_takes_as_many_buttons_as_the_question_has() {
    for count in [2, 4] {
        let l = prompt(wide(), count);
        assert_eq!(l.buttons.len(), count);
        assert!(l.buttons.iter().all(|b| !b.is_empty()));
    }
}

/// F60, found on `:99`: "Alice removed you from the session — your copy is
/// still open, and yours now." ran off the panel. The status has two lines,
/// and a sentence is broken at its dash, which is where it turns.
#[test]
fn a_status_sentence_gets_two_lines_broken_at_its_dash() {
    use fontelle_ui::canvas::status_lines;
    let l = share_panel_layout(wide(), anchor(wide()), &metrics(), ShareRole::Alone, 0);
    assert!(l.status.height >= 2.0 * metrics().row_height);
    let (first, second) = status_lines(
        "Alice removed you from the session \u{2014} your copy is still open, and yours now.",
    );
    assert_eq!(first, "Alice removed you from the session");
    assert_eq!(second, "your copy is still open, and yours now.");
    assert_eq!(
        status_lines("Bob is here with you"),
        ("Bob is here with you", "")
    );
}
