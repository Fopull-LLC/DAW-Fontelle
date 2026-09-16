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
    Action, KEYBIND_SECTIONS, KEYBINDS_TITLE, KeybindEntry, KeybindRow, KeybindsHit,
    KeybindsLayout, Keymap, keybinds_hit, keybinds_layout, keybinds_scroll_max, keybinds_scrolled,
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
    let map = Keymap::default();
    for section in KEYBIND_SECTIONS {
        assert!(!section.title.trim().is_empty(), "a section with no title");
        assert!(
            !section.binds.is_empty(),
            "section {:?} lists nothing",
            section.title
        );
        for entry in section.binds {
            assert!(
                !entry.keys(&map).trim().is_empty(),
                "an entry in {:?} has no keys",
                section.title
            );
            assert!(
                !entry.does().trim().is_empty(),
                "{:?} in {:?} says nothing about what it does",
                entry.keys(&map),
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
    let map = Keymap::default();
    for section in KEYBIND_SECTIONS {
        let mut keys: Vec<String> = section.binds.iter().map(|b| b.keys(&map)).collect();
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
fn every_rebindable_action_is_on_the_page_exactly_once() {
    // The page is the only place a binding can be changed, so an action that
    // is not on it is one nobody can rebind — and one listed twice would be
    // two rows arguing about one binding.
    for action in Action::ALL {
        let listed = KEYBIND_SECTIONS
            .iter()
            .flat_map(|section| section.binds.iter())
            .filter(|entry| entry.action() == Some(action))
            .count();
        assert_eq!(listed, 1, "{action:?} is on the page {listed} times");
    }
}

#[test]
fn a_rebindable_row_shows_the_binding_the_map_has_now_not_the_default() {
    let entry = KEYBIND_SECTIONS
        .iter()
        .flat_map(|section| section.binds.iter())
        .find(|entry| entry.action() == Some(Action::Play))
        .expect("Play is on the page");
    assert_eq!(entry.keys(&Keymap::default()), "Space");
    let mut map = Keymap::default();
    map.rebind(
        Action::Play,
        fontelle_ui::canvas::Chord::parse("Ctrl+P").unwrap(),
    );
    assert_eq!(entry.keys(&map), "Ctrl+P");
    // A fixed entry — a mouse gesture, a text-field key — says the same
    // whatever the map says.
    let fixed = KEYBIND_SECTIONS
        .iter()
        .flat_map(|section| section.binds.iter())
        .find(|entry| matches!(entry, KeybindEntry::Fixed { .. }))
        .expect("there are fixed entries");
    assert_eq!(fixed.keys(&Keymap::default()), fixed.keys(&map));
    assert_eq!(fixed.action(), None);
}

#[test]
fn the_bindings_the_window_answers_are_all_on_the_page() {
    // The mouse gestures and text-field keys the window answers outside the
    // keymap, by the name the page writes them under. The keymap's own
    // actions are covered by `every_rebindable_action_is_on_the_page`.
    let map = Keymap::default();
    for wanted in ["Shift", "Alt", "Esc", "Enter", "Home / End"] {
        let found = KEYBIND_SECTIONS.iter().any(|section| {
            section.binds.iter().any(|bind| {
                let keys = bind.keys(&map);
                keys == wanted || keys.split(['/', ',']).any(|part| part.trim() == wanted)
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
    let (hx, hy) = centre(l.hint);
    assert_eq!(
        keybinds_hit(&l, hx, hy),
        KeybindsHit::Card,
        "a press on the card's own text is nothing — it stays up"
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

#[test]
fn a_press_on_a_rebindable_row_names_its_action_and_a_fixed_row_is_nothing() {
    let l = keybinds_layout(Rect::new(0.0, 0.0, 1280.0, 4000.0), &metrics(), 0.0);
    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let mut seen_action = false;
    let mut seen_fixed = false;
    for row in &l.rows {
        let KeybindRow::Bind {
            action, keys, does, ..
        } = row
        else {
            continue;
        };
        // Anywhere along the line: the chip and the words are one row.
        let (kx, ky) = centre(*keys);
        let (dx, dy) = centre(*does);
        match action {
            Some(action) => {
                assert_eq!(keybinds_hit(&l, kx, ky), KeybindsHit::Row(*action));
                assert_eq!(keybinds_hit(&l, dx, dy), KeybindsHit::Row(*action));
                seen_action = true;
            }
            None => {
                assert_eq!(keybinds_hit(&l, kx, ky), KeybindsHit::Card);
                seen_fixed = true;
            }
        }
    }
    assert!(seen_action && seen_fixed);
}

#[test]
fn the_sheet_has_a_reset_button_beside_the_close_button() {
    let l = keybinds_layout(window(), &metrics(), 0.0);
    assert!(!l.reset.is_empty(), "no reset button");
    assert!(within(l.reset, l.frame));
    assert!(!l.reset.intersects(&l.close) && !l.reset.intersects(&l.title));
    assert!(
        l.reset.right() <= l.close.x + 0.01,
        "the reset button sits left of the ×"
    );
    let (x, y) = (
        l.reset.x + l.reset.width / 2.0,
        l.reset.y + l.reset.height / 2.0,
    );
    assert_eq!(keybinds_hit(&l, x, y), KeybindsHit::Reset);
}

#[test]
fn the_rows_carry_their_actions_in_catalogue_order() {
    let l = keybinds_layout(Rect::new(0.0, 0.0, 1280.0, 4000.0), &metrics(), 0.0);
    for row in &l.rows {
        if let KeybindRow::Bind {
            section,
            index,
            action,
            ..
        } = row
        {
            assert_eq!(
                *action,
                KEYBIND_SECTIONS[*section].binds[*index].action(),
                "row {section}/{index} names the wrong action"
            );
        }
    }
}
