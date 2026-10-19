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
        all.push((row.follow, ShareHit::Follow(i)));
        for (k, chip) in row.allow.iter().enumerate() {
            all.push((*chip, ShareHit::Allow(i, k)));
        }
    }
    all.push((l.say, ShareHit::Say));
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

// ------------------------------------------------------ what is missing ---

/// F91, Ty: *"if there are plugins missing, it should tell you which
/// plugins."* The panel lists what the song uses that this computer has not
/// got under a heading of its own — two lines each, what and where — above
/// the status, inside the panel, apart from everything pressable; and what
/// does not fit is counted ("and 3 more"), never run off the panel.
#[test]
fn what_is_missing_is_listed_on_the_panel_and_cut_to_what_fits() {
    use fontelle_ui::canvas::share_panel_layout_with;
    for window in [wide(), narrow()] {
        for role in [ShareRole::Joined, ShareRole::Hosting, ShareRole::Alone] {
            let l = share_panel_layout_with(window, anchor(window), &metrics(), role, 2, 2);
            assert!(!l.missing_title.is_empty(), "{role:?}: a heading");
            assert_eq!(l.missing.len(), 2, "{role:?} at {window:?}");
            for (what, detail) in &l.missing {
                for r in [*what, *detail] {
                    assert!(!r.is_empty() && within(r, l.frame), "{r:?} escapes");
                    assert!(r.y >= l.missing_title.bottom() - 0.5);
                    assert!(r.bottom() <= l.status.y + 0.5, "above the status");
                    for (p, _) in pressables(&l) {
                        assert!(!r.intersects(&p), "{r:?} under {p:?}");
                    }
                }
                assert!(detail.y >= what.bottom() - 0.5, "what, then where");
            }
            assert!(l.missing_more.is_empty(), "everything fits");
        }
        let none =
            share_panel_layout_with(window, anchor(window), &metrics(), ShareRole::Joined, 2, 0);
        assert!(none.missing_title.is_empty() && none.missing.is_empty());
    }
    let crowded = share_panel_layout_with(
        narrow(),
        anchor(narrow()),
        &metrics(),
        ShareRole::Joined,
        2,
        60,
    );
    assert!(crowded.missing.len() < 60);
    assert!(!crowded.missing_more.is_empty(), "the rest are counted");
    assert!(within(crowded.missing_more, crowded.frame));
}

// ------------------------------------------- leaving the song you share ---
//
// Ty, 2026-10-18: opening another song while in a session asks first —
// *"yes, ask so its more clear"*. The question says who it affects and the
// button says what it does.

mod leaving {
    use fontelle_ui::canvas::{ShareRole, end_session_question};

    #[test]
    fn a_host_is_asked_whether_to_end_it_for_everyone() {
        let (question, button) = end_session_question(ShareRole::Hosting).unwrap();
        assert!(question.contains("everyone"), "{question}");
        assert_eq!(button, "End session");
    }

    #[test]
    fn a_joiner_is_asked_whether_to_leave() {
        let (question, button) = end_session_question(ShareRole::Joined).unwrap();
        assert!(question.to_lowercase().contains("leave"), "{question}");
        assert_eq!(button, "Leave");
    }

    #[test]
    fn nobody_alone_is_asked_anything() {
        assert!(end_session_question(ShareRole::Alone).is_none());
    }

    #[test]
    fn a_session_still_starting_is_asked_too() {
        // A code being waited for, or a song still arriving, is a session
        // somebody may be about to walk into.
        assert!(end_session_question(ShareRole::Starting).is_some());
        assert!(end_session_question(ShareRole::Joining).is_some());
    }
}

/// A file somebody turned down ("Not now") or that failed can be fetched
/// after all: *Fetch again* beside the heading, for a studio that joined —
/// the host is where the files come from, and has nobody to ask (2026-10-18).
#[test]
fn a_joiner_missing_files_can_fetch_them_again() {
    use fontelle_ui::canvas::share_panel_layout_with;
    let window = wide();
    let l = share_panel_layout_with(window, anchor(window), &metrics(), ShareRole::Joined, 1, 2);
    assert!(
        !l.fetch_again.is_empty(),
        "offered to a joiner with files missing"
    );
    assert!(
        within(l.fetch_again, l.missing_title),
        "on the heading's row"
    );
    let (x, y) = centre(l.fetch_again);
    assert_eq!(share_hit(&l, x, y), Some(ShareHit::FetchAgain));
    let host =
        share_panel_layout_with(window, anchor(window), &metrics(), ShareRole::Hosting, 1, 2);
    assert!(host.fetch_again.is_empty(), "not to the host");
    let none = share_panel_layout_with(window, anchor(window), &metrics(), ShareRole::Joined, 1, 0);
    assert!(none.fetch_again.is_empty(), "nor with nothing missing");
}

/// Everybody's row, host's panel or joiner's, has a Follow: the view goes
/// where that person's goes — *"one person could teach some one how to do
/// something and they can show them"* (Ty, 2026-10-18).
#[test]
fn every_row_can_be_followed() {
    for role in [ShareRole::Hosting, ShareRole::Joined] {
        let l = share_panel_layout(wide(), anchor(wide()), &metrics(), role, 2);
        assert_eq!(l.rows.len(), 2);
        for (i, row) in l.rows.iter().enumerate() {
            assert!(!row.follow.is_empty(), "{role:?} row {i}");
            assert!(within(row.follow, row.frame));
            assert!(
                row.follow.intersection(&row.name).is_empty(),
                "over the name"
            );
            assert!(row.follow.intersection(&row.view_only_label).is_empty());
            let (x, y) = centre(row.follow);
            assert_eq!(share_hit(&l, x, y), Some(ShareHit::Follow(i)));
        }
    }
}

// --- chat (phase 2.3) ---

fn said(name: &str, text: &str) -> fontelle_ui::document::SaidLine {
    fontelle_ui::document::SaidLine {
        peer: 1,
        name: name.into(),
        colour: 1,
        text: text.into(),
        mine: false,
    }
}

/// In a session the panel carries the chat — its last lines, then a way to
/// say something — between who is here and what is going on. Alone, none.
#[test]
fn the_chat_sits_between_the_people_and_the_status() {
    use fontelle_ui::canvas::share_panel_layout_full;
    for role in [ShareRole::Hosting, ShareRole::Joined] {
        let l = share_panel_layout_full(wide(), anchor(wide()), &metrics(), role, 2, 0, 3);
        assert_eq!(l.chat.len(), 3, "{role:?}");
        let last_row = l.rows.last().unwrap();
        for line in &l.chat {
            assert!(within(*line, l.frame));
            assert!(line.y >= last_row.frame.bottom() - 0.5, "over the people");
            assert!(line.bottom() <= l.status.y + 0.5, "over the status");
        }
        assert!(!l.say.is_empty());
        assert!(within(l.say, l.frame));
        assert!(l.say.intersection(&l.title).is_empty(), "over the title");
        let (x, y) = centre(l.say);
        assert_eq!(share_hit(&l, x, y), Some(ShareHit::Say));
    }
    let alone = share_panel_layout_full(
        wide(),
        anchor(wide()),
        &metrics(),
        ShareRole::Alone,
        0,
        0,
        3,
    );
    assert!(alone.chat.is_empty() && alone.say.is_empty());
}

/// The chat's rows: each line wrapped to the width, the name on its first
/// row, the newest at the bottom, no more than fit.
#[test]
fn chat_lines_wrap_and_the_newest_are_kept() {
    use fontelle_ui::canvas::{CHAT_ROWS, chat_rows};
    let lines = vec![
        said("Bob", "first"),
        said(
            "Carol",
            "a rather long line that will not fit on one row of the panel at all",
        ),
        said("Bob", "last"),
    ];
    let rows = chat_rows(&lines, 30);
    assert!(rows.len() <= CHAT_ROWS);
    assert!(
        rows.iter().all(|(_, text)| text.chars().count() <= 30),
        "{rows:?}"
    );
    assert_eq!(rows.last().unwrap().1, "Bob: last");
    assert!(
        rows.iter()
            .any(|(_, text)| text.starts_with("Carol: a rather"))
    );
    let many: Vec<_> = (0..20).map(|n| said("Bob", &format!("line {n}"))).collect();
    let rows = chat_rows(&many, 30);
    assert_eq!(rows.len(), CHAT_ROWS);
    assert_eq!(rows.last().unwrap().1, "Bob: line 19");
}

// --- permissions (phase 2.4) ---

/// On the host's panel each person has three switches under their name —
/// the mixer, sounds, plugins — and a joiner's panel has none.
#[test]
fn the_host_can_allow_each_person_the_mixer_sounds_and_plugins() {
    let l = share_panel_layout(wide(), anchor(wide()), &metrics(), ShareRole::Hosting, 2);
    for (i, row) in l.rows.iter().enumerate() {
        for (k, chip) in row.allow.iter().enumerate() {
            assert!(!chip.is_empty(), "row {i} chip {k}");
            assert!(within(*chip, row.frame));
            assert!(
                chip.y >= row.name.bottom() - 0.5,
                "beside the name, not under it"
            );
            let (x, y) = centre(*chip);
            assert_eq!(share_hit(&l, x, y), Some(ShareHit::Allow(i, k)));
        }
    }
    let joined = share_panel_layout(wide(), anchor(wide()), &metrics(), ShareRole::Joined, 2);
    assert!(
        joined
            .rows
            .iter()
            .all(|r| r.allow.iter().all(|c| c.is_empty()))
    );
}
