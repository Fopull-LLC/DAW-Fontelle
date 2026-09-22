//! The notepad's window, before anything draws one
//! (`docs/effects-catalogue.md` §2.8).
//!
//! > *"its just a basic text editor with pages you can go left and right
//! > between and use it like a normal text editor to write down lyrics for
//! > example as you record to sing them back."*
//!
//! **A normal text editor** is the hard half. The single-line model is already
//! here (`TextEntry`, shared by every field in the program); what a page needs
//! on top of it is *lines* — text that wraps at the width of the sheet, a caret
//! that moves up and down through the wrapping, Home and End that mean the line
//! rather than the page, and a click that lands where the pointer is. All of
//! that is arithmetic over a monospace grid, so all of it is checked here
//! without a window.

use fontelle_types::{NotepadSize, NotepadTheme};
use fontelle_ui::canvas::{
    NOTEPAD_LEADING, NotepadHit, NotepadRow, NotepadView, TextEntry, notepad_columns, notepad_hit,
    notepad_index_at, notepad_index_of, notepad_layout, notepad_line_end, notepad_line_home,
    notepad_row_of, notepad_rows, notepad_scroll_to, notepad_scrollbar, notepad_step_row,
    notepad_text_px,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

/// A page 20 characters across and 6 lines down, at an advance of 10 and a
/// line of 20 — round numbers, so a failure is arithmetic and not rounding.
const ADVANCE: f32 = 10.0;

fn view(text: &str) -> NotepadView {
    NotepadView {
        track: "Vocal".to_string(),
        theme: NotepadTheme::Phosphor,
        size: NotepadSize::Medium,
        page: 0,
        pages: 1,
        text: text.to_string(),
        captions: vec![None],
    }
}

fn rows_of(text: &str, columns: usize) -> Vec<NotepadRow> {
    notepad_rows(text, columns)
}

/// The text the rows cover, one string per row.
fn drawn(text: &str, rows: &[NotepadRow]) -> Vec<String> {
    rows.iter()
        .map(|row| text[row.from..row.to].to_string())
        .collect()
}

// ------------------------------------------------------------- the layout

#[test]
fn the_body_is_a_sheet_over_a_footer() {
    let theme = Theme::dark_default();
    let body = Rect::new(0.0, 0.0, 400.0, 300.0);
    let layout = notepad_layout(body, &theme.metrics, &view(""), ADVANCE, 20.0);
    assert!(layout.sheet.width > 0.0 && layout.sheet.height > 0.0);
    assert!(
        layout.sheet.bottom() <= layout.footer.y + 0.01,
        "the sheet sits above the footer"
    );
    assert!(
        layout.footer.bottom() <= body.bottom() + 0.01,
        "and the footer inside the window"
    );
    // The writing area is inside the sheet, with room for the border it is
    // drawn in.
    assert!(layout.text.x > layout.sheet.x);
    assert!(layout.text.right() < layout.sheet.right());
    assert!(layout.lines >= 1 && layout.columns >= 1);
}

#[test]
fn a_window_too_small_to_write_in_still_lays_out() {
    // Dragged to nothing: every rectangle is empty or tiny, nothing is
    // negative, and the grid never claims a zeroth column — a `% 0` would be
    // a panic in the middle of somebody's session.
    let theme = Theme::dark_default();
    for (w, h) in [(0.0, 0.0), (10.0, 10.0), (60.0, 40.0)] {
        let layout = notepad_layout(
            Rect::new(0.0, 0.0, w, h),
            &theme.metrics,
            &view("something"),
            ADVANCE,
            20.0,
        );
        assert!(layout.columns >= 1, "{w}x{h} claimed no columns");
        assert!(layout.lines >= 1, "{w}x{h} claimed no lines");
        assert!(layout.text.width >= 0.0 && layout.text.height >= 0.0);
    }
}

#[test]
fn the_footer_reads_left_to_right_and_the_hit_test_agrees() {
    let theme = Theme::dark_default();
    let body = Rect::new(0.0, 0.0, 400.0, 300.0);
    let layout = notepad_layout(body, &theme.metrics, &view(""), ADVANCE, 20.0);
    // ‹ page 2 / 5 › then the two that change how many pages there are, and
    // the two chips that change how it looks over on the right.
    assert!(layout.previous.right() <= layout.count.x + 0.01);
    assert!(layout.count.right() <= layout.next.x + 0.01);
    assert!(layout.next.right() <= layout.add.x + 0.01);
    assert!(layout.add.right() <= layout.remove.x + 0.01);
    assert!(layout.remove.right() <= layout.size_chip.x + 0.01);
    assert!(layout.size_chip.right() <= layout.theme_chip.x + 0.01);
    assert!(layout.theme_chip.right() <= body.right() + 0.01);

    let middle = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    for (rect, expected) in [
        (layout.text, NotepadHit::Page),
        (layout.previous, NotepadHit::Previous),
        // The counter is a control too: it drops down the pages by name.
        (layout.count, NotepadHit::Pages),
        (layout.next, NotepadHit::Next),
        (layout.add, NotepadHit::AddPage),
        (layout.remove, NotepadHit::RemovePage),
        (layout.size_chip, NotepadHit::Size),
        (layout.theme_chip, NotepadHit::Theme),
    ] {
        let (x, y) = middle(rect);
        assert_eq!(notepad_hit(&layout, x, y), expected, "at {x},{y}");
    }
    assert_eq!(
        notepad_hit(&layout, -5.0, -5.0),
        NotepadHit::Nothing,
        "outside the window"
    );
}

#[test]
fn the_footer_has_room_to_say_how_to_leave() {
    // The one thing about this window that would otherwise be reported as a
    // bug: while the pad has the keyboard, Space types a space rather than
    // playing the song. It is said in the pad's **own footer** rather than on
    // the studio's status line, which anything else — a backup, a preset —
    // overwrites a second later.
    let theme = Theme::dark_default();
    let wide = notepad_layout(
        Rect::new(0.0, 0.0, 560.0, 540.0),
        &theme.metrics,
        &view(""),
        ADVANCE,
        20.0,
    );
    assert!(
        !wide.hint.is_empty(),
        "no room for the hint at the design size"
    );
    assert!(
        wide.hint.x >= wide.remove.right(),
        "it sits after the pages"
    );
    assert!(
        wide.hint.right() <= wide.size_chip.x + 0.01,
        "and before the chips"
    );
    // A window dragged narrow drops it rather than drawing it over a chip.
    let narrow = notepad_layout(
        Rect::new(0.0, 0.0, 300.0, 300.0),
        &theme.metrics,
        &view(""),
        ADVANCE,
        20.0,
    );
    assert!(
        narrow.hint.is_empty(),
        "{:?} should have been dropped",
        narrow.hint
    );
}

#[test]
fn the_size_chooser_changes_how_big_the_words_are() {
    let base = 13.0;
    let small = notepad_text_px(NotepadSize::Small, base);
    let medium = notepad_text_px(NotepadSize::Medium, base);
    let large = notepad_text_px(NotepadSize::Large, base);
    assert!(small < medium && medium < large, "{small} {medium} {large}");
    assert!(small >= 9.0, "still readable");
    // A terminal has air between its lines, and a page of lyrics is not a
    // block: the leading is a multiple of the text size, more than one and
    // less than two.
    let line = notepad_text_px(NotepadSize::Medium, base) * NOTEPAD_LEADING;
    assert!(line > medium && line < medium * 2.0, "{line} for {medium}");
}

#[test]
fn the_columns_are_what_fits_across_the_page() {
    assert_eq!(notepad_columns(200.0, ADVANCE), 20);
    assert_eq!(notepad_columns(205.0, ADVANCE), 20, "half a column is none");
    // Never zero, whatever the arithmetic is handed.
    assert_eq!(notepad_columns(0.0, ADVANCE), 1);
    assert_eq!(notepad_columns(200.0, 0.0), 1);
}

// --------------------------------------------------------------- the rows

#[test]
fn a_line_that_fits_is_one_row() {
    let text = "when the lights";
    assert_eq!(drawn(text, &rows_of(text, 20)), ["when the lights"]);
}

#[test]
fn a_new_line_starts_a_row_however_short_the_one_before() {
    let text = "one\ntwo";
    assert_eq!(drawn(text, &rows_of(text, 20)), ["one", "two"]);
}

#[test]
fn the_caret_can_sit_on_a_last_empty_line() {
    // A page ending in a newline has a row after it, or the caret after the
    // last Enter would have nowhere to be drawn.
    let text = "one\n";
    assert_eq!(drawn(text, &rows_of(text, 20)), ["one", ""]);
    let empty = "";
    assert_eq!(
        rows_of(empty, 20).len(),
        1,
        "an empty page is one empty row"
    );
}

#[test]
fn a_long_line_wraps_at_a_word() {
    // Wrapped rather than run off the edge: a lyric is written to be read, and
    // a sheet you have to scroll sideways is not one.
    let text = "when the lights go down and the room goes quiet";
    let rows = rows_of(text, 20);
    assert_eq!(
        drawn(text, &rows),
        ["when the lights go ", "down and the room ", "goes quiet"],
        "each row is at most twenty columns and breaks after a space"
    );
    for row in &rows {
        assert!(text[row.from..row.to].len() <= 20);
    }
}

#[test]
fn a_word_longer_than_the_page_is_broken_rather_than_hidden() {
    let text = "supercalifragilistic";
    let rows = rows_of(text, 8);
    assert_eq!(drawn(text, &rows), ["supercal", "ifragili", "stic"]);
}

#[test]
fn the_rows_cover_the_whole_page_and_nothing_twice() {
    let text = "verse one\nand a much longer second line that has to wrap\n\nend";
    let rows = rows_of(text, 12);
    let mut at = 0;
    for row in &rows {
        assert!(row.from >= at, "rows go forwards");
        assert!(row.to >= row.from);
        at = row.to;
    }
    // Every character is in exactly one row, bar the newlines the rows end on.
    let joined: String = rows
        .iter()
        .map(|row| &text[row.from..row.to])
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(joined.replace('\n', ""), text.replace('\n', ""));
}

// -------------------------------------------------------- caret and rows

#[test]
fn the_caret_maps_to_a_row_and_a_column_and_back() {
    let text = "one\ntwo three";
    let rows = rows_of(text, 20);
    assert_eq!(notepad_row_of(&rows, 0), 0);
    assert_eq!(notepad_row_of(&rows, 3), 0, "the end of the first line");
    assert_eq!(notepad_row_of(&rows, 4), 1, "past the newline");
    assert_eq!(notepad_row_of(&rows, text.len()), 1);
    // And back: row 1, column 3 is the "e" of "three"'s word before it.
    assert_eq!(notepad_index_of(text, &rows, 1, 3), 7);
    // A column past the end of a row lands at the row's end rather than on
    // the next row, which is what makes a long line and a short one walk
    // together.
    assert_eq!(notepad_index_of(text, &rows, 0, 40), 3);
    // A row past the end is the last row — which is where a click below the
    // words lands, and nowhere is not an answer a caret can take.
    assert_eq!(notepad_index_of(text, &rows, 9, 0), 4);
}

#[test]
fn up_and_down_keep_the_column_they_started_in() {
    let text = "the first line\nx\nthe third line";
    let rows = rows_of(text, 20);
    let mut entry = TextEntry::new(text);
    entry.place(10, false); // "the first |line"
    // Down onto the short line: the caret goes to its end...
    notepad_step_row(&mut entry, &rows, 1, 10, false);
    assert_eq!(entry.caret(), 16, "the end of 'x'");
    // ...and down again lands back at column ten, because the *goal* is kept
    // by whoever is pressing the key, not by where the caret ended up.
    notepad_step_row(&mut entry, &rows, 1, 10, false);
    assert_eq!(entry.caret(), 27);
    // Up from the top and down from the bottom do nothing rather than
    // wrapping round.
    notepad_step_row(&mut entry, &rows, 1, 10, false);
    assert_eq!(entry.caret(), 27, "already on the last row");
    entry.place(2, false);
    notepad_step_row(&mut entry, &rows, -1, 2, false);
    assert_eq!(entry.caret(), 2, "already on the first row");
}

#[test]
fn shift_and_an_arrow_selects_down_the_page() {
    let text = "one\ntwo\nthree";
    let rows = rows_of(text, 20);
    let mut entry = TextEntry::new(text);
    entry.place(0, false);
    notepad_step_row(&mut entry, &rows, 1, 0, true);
    assert_eq!(entry.selection(), Some((0, 4)));
}

#[test]
fn home_and_end_are_the_line_rather_than_the_page() {
    // The one place a page differs from a name field: End on a wrapped line
    // means the end of *that row*, which is what a person watching a caret
    // expects.
    let text = "one\ntwo three";
    let rows = rows_of(text, 20);
    let mut entry = TextEntry::new(text);
    entry.place(6, false);
    notepad_line_home(&mut entry, &rows, false);
    assert_eq!(entry.caret(), 4);
    notepad_line_end(&mut entry, &rows, false);
    assert_eq!(entry.caret(), text.len());
    entry.place(1, false);
    notepad_line_end(&mut entry, &rows, false);
    assert_eq!(
        entry.caret(),
        3,
        "the end of the first line, not of the page"
    );
}

// ------------------------------------------------------ pointer and scroll

#[test]
fn a_click_puts_the_caret_where_the_pointer_is() {
    let theme = Theme::dark_default();
    let text = "one\ntwo three";
    let layout = notepad_layout(
        Rect::new(0.0, 0.0, 400.0, 300.0),
        &theme.metrics,
        &view(text),
        ADVANCE,
        20.0,
    );
    let rows = rows_of(text, layout.columns);
    // Two and a half characters into the second row: the right-hand half of a
    // letter means after it, as every text field does.
    let x = layout.text.x + 2.5 * ADVANCE;
    let y = layout.text.y + 1.5 * layout.line_height;
    assert_eq!(notepad_index_at(&layout, text, &rows, 0, x, y), 4 + 3);
    // Left of the first column is the start of the row; past the last is its
    // end; above the sheet is the first row and below it the last.
    assert_eq!(notepad_index_at(&layout, text, &rows, 0, -50.0, y), 4);
    assert_eq!(
        notepad_index_at(&layout, text, &rows, 0, 9_000.0, y),
        text.len()
    );
    assert_eq!(notepad_index_at(&layout, text, &rows, 0, x, -50.0), 3);
    assert_eq!(
        notepad_index_at(&layout, text, &rows, 0, x, 9_000.0),
        4 + 3,
        "the last row, at the column clicked"
    );
    // And with the page scrolled, a click counts from what is on screen.
    assert_eq!(
        notepad_index_at(
            &layout,
            text,
            &rows,
            1,
            x,
            layout.text.y + 0.5 * layout.line_height
        ),
        4 + 3
    );
}

#[test]
fn a_page_that_outruns_its_window_says_so() {
    // Without it, a pad scrolled down the page looks like a pad whose first
    // lines have been lost. The bar is the only thing in this window that is
    // not a control: it says where you are and is never dragged.
    let theme = Theme::dark_default();
    let layout = notepad_layout(
        Rect::new(0.0, 0.0, 400.0, 300.0),
        &theme.metrics,
        &view(""),
        ADVANCE,
        20.0,
    );
    let lines = layout.lines;
    // Everything fits: nothing is drawn.
    assert_eq!(notepad_scrollbar(&layout, lines, 0), None);
    assert_eq!(notepad_scrollbar(&layout, lines - 1, 0), None);
    // Twice the room: the thumb is half the track, at the top.
    let top = notepad_scrollbar(&layout, lines * 2, 0).expect("a bar");
    assert!((top.height - layout.text.height / 2.0).abs() < 1.0);
    assert!((top.y - layout.text.y).abs() < 0.01);
    assert!(top.width > 0.0 && top.right() <= layout.sheet.right() + 0.01);
    // Scrolled to the bottom, it sits at the bottom and no further.
    let bottom = notepad_scrollbar(&layout, lines * 2, lines).expect("a bar");
    assert!(
        (bottom.bottom() - layout.text.bottom()).abs() < 1.0,
        "{bottom:?} against {:?}",
        layout.text
    );
    // A page far longer than its window still has a thumb somebody can see.
    let tiny = notepad_scrollbar(&layout, lines * 400, 0).expect("a bar");
    assert!(tiny.height >= 8.0, "the thumb vanished: {tiny:?}");
}

#[test]
fn the_page_scrolls_the_least_that_keeps_the_caret_in_sight() {
    // The rule the preset list already follows (`presets_scroll_to`): a caret
    // that is on screen moves nothing, and one that has gone off scrolls by
    // exactly what it takes to bring it back.
    assert_eq!(notepad_scroll_to(3, 10, 0), 0, "already in sight");
    assert_eq!(notepad_scroll_to(14, 10, 0), 5, "just onto the last line");
    assert_eq!(notepad_scroll_to(2, 10, 5), 2, "back up to the caret");
    assert_eq!(notepad_scroll_to(0, 0, 4), 0, "a window with no room");
}

// ------------------------------------------------------------- the looks

/// Relative luminance, WCAG 2.x — the same measure `tests/theme.rs` holds the
/// chrome to.
fn luminance(c: fontelle_ui::theme::Color) -> f64 {
    let f = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.0[0]) + 0.7152 * f(c.0[1]) + 0.0722 * f(c.0[2])
}

fn contrast(a: fontelle_ui::theme::Color, b: fontelle_ui::theme::Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn every_theme_is_something_you_can_read_lyrics_off() {
    // A look is not a licence: these are read while somebody sings, from
    // further away than the rest of the chrome is read from, so the words on
    // the page are held to WCAG's 4.5:1 like every other piece of body text
    // in this program.
    let palette = Theme::dark_default().palette;
    for theme in NotepadTheme::ALL {
        let ink = fontelle_ui::theme::notepad_ink(theme, &palette);
        let words = contrast(ink.ink, ink.paper);
        assert!(
            words >= 4.5,
            "{}: the words are {words:.2}:1 on the page",
            theme.label()
        );
        // The dim ink — the page's rule, the footer's captions — is quieter
        // on purpose, and still has to be visible.
        let faint = contrast(ink.faint, ink.paper);
        assert!(
            faint >= 2.5,
            "{}: the faint ink is {faint:.2}:1",
            theme.label()
        );
        // The caret has to be findable on the page it blinks on.
        assert!(
            contrast(ink.caret, ink.paper) >= 3.0,
            "{}: the caret hides",
            theme.label()
        );
        assert_ne!(
            ink.edge,
            ink.paper,
            "{}: the sheet has no edge",
            theme.label()
        );
        for colour in [
            ink.ground, ink.paper, ink.ink, ink.faint, ink.caret, ink.edge,
        ] {
            assert_eq!(colour.0[3], 0xff, "{}: a see-through ink", theme.label());
        }
    }
}

#[test]
fn the_seven_are_tellable_apart() {
    let palette = Theme::dark_default().palette;
    let inks: Vec<_> = NotepadTheme::ALL
        .iter()
        .map(|theme| fontelle_ui::theme::notepad_ink(*theme, &palette))
        .collect();
    for (i, one) in inks.iter().enumerate() {
        for (j, two) in inks.iter().enumerate().skip(i + 1) {
            assert!(
                one.paper != two.paper || one.ink != two.ink,
                "{} and {} are the same look",
                NotepadTheme::ALL[i].label(),
                NotepadTheme::ALL[j].label()
            );
        }
    }
    // And one of them is light, because a room with the lights on is a real
    // place to write lyrics in.
    assert!(
        inks.iter().any(|ink| luminance(ink.paper) > 0.5),
        "every theme is a dark one"
    );
}
