//! The keyboard shortcuts page.
//!
//! > *"a dedicated keybinds page that shows every single keybind in the
//! > entire app organized cleanly sectioned to be easily understandable and
//! > grouped. should be accessible from the start screen with a new ? icon,
//! > and should also be accessible within a project by a new small ? icon
//! > next to the tempo indicator"*
//!
//! Two halves, both pure. The **catalogue** (`KEYBIND_SECTIONS`) is the one
//! list of what the keys do, written down where a page can read it; these
//! tests hold it to a few rules that keep it honest — nothing empty, nothing
//! said twice in one section, and the bindings the window answers actually
//! in it. The **sheet** (`keybinds_layout`) is the card the page is drawn on:
//! a scrolling list of sections in as many columns as the window has room
//! for, with a close button, laid out from a rectangle and a scroll offset.

use fontelle_ui::canvas::{
    KEYBIND_SECTIONS, KEYBINDS_TITLE, KeybindRow, KeybindsHit, KeybindsLayout, keybinds_hit,
    keybinds_layout, keybinds_scroll_max, keybinds_scrolled,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn window() -> Rect {
    Rect::new(0.0, 0.0, 1280.0, 720.0)
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x - 0.01
        && inner.y >= outer.y - 0.01
        && inner.right() <= outer.right() + 0.01
        && inner.bottom() <= outer.bottom() + 0.01
}

fn every_row_rect(layout: &KeybindsLayout) -> Vec<Rect> {
    layout
        .rows
        .iter()
        .flat_map(|row| match row {
            KeybindRow::Heading { rect, .. } => vec![*rect],
            KeybindRow::Bind { keys, does, .. } => vec![*keys, *does],
        })
        .collect()
}

// ------------------------------------------------------------ the catalogue ---

#[test]
fn every_section_has_a_title_and_something_in_it_and_nothing_is_blank() {
    assert!(
        KEYBIND_SECTIONS.len() >= 6,
        "a page with {} sections is not a page of every keybind in the app",
        KEYBIND_SECTIONS.len()
    );
    for section in KEYBIND_SECTIONS {
        assert!(!section.title.trim().is_empty(), "a section with no title");
        assert!(
            !section.binds.is_empty(),
            "section {:?} lists nothing",
            section.title
        );
        for bind in section.binds {
            assert!(
                !bind.keys.trim().is_empty(),
                "a binding in {:?} has no keys",
                section.title
            );
            assert!(
                !bind.does.trim().is_empty(),
                "{:?} in {:?} says nothing about what it does",
                bind.keys,
                section.title
            );
        }
    }
}

#[test]
fn no_section_title_repeats_and_no_key_is_listed_twice_in_one_section() {
    let mut titles: Vec<&str> = KEYBIND_SECTIONS.iter().map(|s| s.title).collect();
    titles.sort_unstable();
    titles.dedup();
    assert_eq!(
        titles.len(),
        KEYBIND_SECTIONS.len(),
        "two sections share a title"
    );
    for section in KEYBIND_SECTIONS {
        let mut keys: Vec<&str> = section.binds.iter().map(|b| b.keys).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(
            keys.len(),
            before,
            "section {:?} lists a key twice",
            section.title
        );
    }
}

#[test]
fn the_bindings_the_window_answers_are_all_on_the_page() {
    // The keys `WindowApp::key` and `global_key` match on, by the name the
    // page writes them under. A binding added to the window and not here is
    // a binding nobody can find out about — which is the whole reason the
    // page exists.
    let must_mention = [
        // transport and project
        "Space",
        "Home",
        "Ctrl+M",
        "Ctrl+S",
        "Ctrl+Z",
        "Ctrl+Shift+Z",
        "Ctrl+Y",
        "Ctrl+E",
        "Ctrl+Shift+E",
        "Ctrl+L",
        "F1",
        "Esc",
        // panels
        "1",
        "2",
        "F",
        "Tab",
        "Ctrl+T",
        "T",
        "Ctrl+F",
        // tools
        "P",
        "B",
        "E",
        "D",
        "C",
        "S",
        "A",
        "L",
        "G",
        // editing
        "Ctrl+A",
        "Ctrl+C",
        "Ctrl+X",
        "Ctrl+V",
        "Ctrl+B",
        "Ctrl+D",
        "Delete",
        "Ctrl+Shift+M",
        // mixer
        "M",
        "N",
        // held while dragging
        "Shift",
        "Alt",
    ];
    for wanted in must_mention {
        let found = KEYBIND_SECTIONS.iter().any(|section| {
            section.binds.iter().any(|bind| {
                bind.keys == wanted
                    || bind
                        .keys
                        .split(['/', ','])
                        .any(|part| part.trim() == wanted)
            })
        });
        assert!(found, "the page never mentions {wanted:?}");
    }
}

// ---------------------------------------------------------------- the sheet ---

#[test]
fn the_sheet_is_a_card_inside_the_window_with_its_title_and_close_button_on_it() {
    let l = keybinds_layout(window(), &metrics(), 0.0);
    assert!(!l.frame.is_empty(), "no card");
    assert!(within(l.frame, window()), "the card runs off the window");
    assert!(within(l.title, l.frame), "the title is off the card");
    assert!(within(l.close, l.frame), "the close button is off the card");
    assert!(within(l.body, l.frame), "the list is off the card");
    assert!(
        !l.title.intersects(&l.close),
        "the close button is over the title"
    );
    assert!(
        l.body.y >= l.title.bottom() - 0.01,
        "the list starts under the title"
    );
    assert_eq!(KEYBINDS_TITLE, "Keyboard shortcuts");
}

#[test]
fn every_section_is_laid_out_once_starting_with_its_heading() {
    // A window tall enough for the whole catalogue: every heading is on the
    // sheet, once, and each section's rows follow their heading downward.
    let tall = Rect::new(0.0, 0.0, 1280.0, 4000.0);
    let l = keybinds_layout(tall, &metrics(), 0.0);
    for (index, section) in KEYBIND_SECTIONS.iter().enumerate() {
        let headings: Vec<Rect> = l
            .rows
            .iter()
            .filter_map(|row| match row {
                KeybindRow::Heading { section: s, rect } if *s == index => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(
            headings.len(),
            1,
            "section {:?} has {} headings",
            section.title,
            headings.len()
        );
        let binds: Vec<(usize, Rect)> = l
            .rows
            .iter()
            .filter_map(|row| match row {
                KeybindRow::Bind {
                    section: s,
                    index: i,
                    keys,
                    ..
                } if *s == index => Some((*i, *keys)),
                _ => None,
            })
            .collect();
        assert_eq!(
            binds.len(),
            section.binds.len(),
            "section {:?} lost rows",
            section.title
        );
        let mut last_bottom = headings[0].bottom();
        for (i, (index, keys)) in binds.iter().enumerate() {
            assert_eq!(*index, i, "rows out of order in {:?}", section.title);
            assert!(
                keys.y >= last_bottom - 0.01,
                "row {i} of {:?} is above the row before it",
                section.title
            );
            last_bottom = keys.bottom();
        }
    }
}

#[test]
fn rows_stay_inside_the_list_and_never_overlap_each_other() {
    for width in [400.0, 640.0, 900.0, 1280.0] {
        let l = keybinds_layout(Rect::new(0.0, 0.0, width, 720.0), &metrics(), 0.0);
        let rects = every_row_rect(&l);
        assert!(!rects.is_empty(), "nothing laid out at {width}");
        for (i, a) in rects.iter().enumerate() {
            assert!(
                a.intersects(&l.body),
                "a row at {a:?} is outside the list {:?} at width {width}",
                l.body
            );
            assert!(
                a.x >= l.body.x - 0.01 && a.right() <= l.body.right() + 0.01,
                "a row at {a:?} runs past the list's sides at width {width}"
            );
            for b in rects.iter().skip(i + 1) {
                assert!(
                    !a.intersects(b),
                    "rows overlap at width {width}: {a:?} {b:?}"
                );
            }
        }
    }
}

#[test]
fn a_key_chip_sits_left_of_what_it_does_on_the_same_line() {
    let l = keybinds_layout(window(), &metrics(), 0.0);
    let mut seen = 0;
    for row in &l.rows {
        if let KeybindRow::Bind { keys, does, .. } = row {
            assert!(
                keys.right() <= does.x + 0.01,
                "the description is not to the right of its keys: {keys:?} {does:?}"
            );
            assert!(
                (keys.y - does.y).abs() < 0.01,
                "keys and description are not on one line"
            );
            assert!(
                does.width > keys.width,
                "the description column is narrower than the key column"
            );
            seen += 1;
        }
    }
    assert!(seen > 0);
}

#[test]
fn a_wide_window_lays_the_sections_in_two_columns_and_a_narrow_one_in_one() {
    let columns = |width: f32| {
        let l = keybinds_layout(Rect::new(0.0, 0.0, width, 720.0), &metrics(), 0.0);
        let mut xs: Vec<i32> = l
            .rows
            .iter()
            .filter_map(|row| match row {
                KeybindRow::Heading { rect, .. } => Some(rect.x.round() as i32),
                _ => None,
            })
            .collect();
        xs.sort_unstable();
        xs.dedup();
        xs.len()
    };
    assert_eq!(columns(1280.0), 2, "a wide window has room for two columns");
    assert_eq!(columns(900.0), 1, "a narrow one does not");
}

#[test]
fn the_list_scrolls_and_the_scroll_is_clamped_to_what_is_off_the_bottom() {
    let short = Rect::new(0.0, 0.0, 700.0, 360.0);
    let at_top = keybinds_layout(short, &metrics(), 0.0);
    let max = keybinds_scroll_max(&at_top);
    assert!(
        max > 0.0,
        "the whole catalogue cannot fit a 360-pixel window, so there must be something to scroll to"
    );
    // Scrolling moves every row up by the same amount. Less than a heading
    // is tall, so the first one is still there to measure.
    let by = 20.0_f32.min(max);
    let scrolled = keybinds_layout(short, &metrics(), by);
    let first_top = |l: &KeybindsLayout| {
        l.rows
            .iter()
            .find_map(|row| match row {
                KeybindRow::Heading { section: 0, rect } => Some(rect.y),
                _ => None,
            })
            .expect("the first heading is still on the sheet after a small scroll")
    };
    assert!(
        (first_top(&at_top) - first_top(&scrolled) - by).abs() < 0.01,
        "a scroll of {by} moved the first heading by {}",
        first_top(&at_top) - first_top(&scrolled)
    );
    // The clamp, both ends.
    assert_eq!(
        keybinds_scrolled(&at_top, 0.0, 1.0),
        0.0,
        "cannot scroll above the top"
    );
    assert_eq!(
        keybinds_scrolled(&at_top, max, -1.0),
        max,
        "cannot scroll past the bottom"
    );
    let stepped = keybinds_scrolled(&at_top, 0.0, -1.0);
    assert!(
        stepped > 0.0 && stepped <= max,
        "one notch down scrolls down"
    );
    // Rows that have scrolled off the top are not laid out at all.
    let at_bottom = keybinds_layout(short, &metrics(), max);
    for rect in every_row_rect(&at_bottom) {
        assert!(
            rect.intersects(&at_bottom.body),
            "a row off the list was laid out"
        );
    }
    // A tall window has nothing to scroll.
    let tall = keybinds_layout(Rect::new(0.0, 0.0, 1280.0, 4000.0), &metrics(), 0.0);
    assert_eq!(keybinds_scroll_max(&tall), 0.0);
}

#[test]
fn a_press_on_the_close_button_closes_and_one_off_the_card_closes_too() {
    let l = keybinds_layout(window(), &metrics(), 0.0);
    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (cx, cy) = centre(l.close);
    assert_eq!(keybinds_hit(&l, cx, cy), KeybindsHit::Close);
    let (bx, by) = centre(l.body);
    assert_eq!(
        keybinds_hit(&l, bx, by),
        KeybindsHit::Card,
        "a press on the list is nothing — it stays up"
    );
    assert_eq!(
        keybinds_hit(&l, window().x + 1.0, window().y + 1.0),
        KeybindsHit::Outside,
        "a press off the card is how a sheet is dismissed"
    );
}

#[test]
fn a_window_of_no_size_lays_out_without_a_negative_anything() {
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (60.0, 30.0), (200.0, 120.0)] {
        let l = keybinds_layout(Rect::new(0.0, 0.0, w, h), &metrics(), 0.0);
        for r in [l.frame, l.title, l.close, l.body]
            .into_iter()
            .chain(every_row_rect(&l))
        {
            assert!(
                r.width >= 0.0 && r.height >= 0.0,
                "{r:?} went negative at {w}x{h}"
            );
        }
        assert!(keybinds_scroll_max(&l) >= 0.0);
    }
}
