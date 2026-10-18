//! The Notepad as a text editor.
//!
//! > *"add ability to select characters copy paste things and entire lines
//! > all the things youd expect for a full text editor thats fast and quick
//! > and easy to use"* (2026-10-18)
//!
//! The editing is pure — a `TextEntry` in, a `TextEntry` out — so the window
//! only routes keys and clicks here. Lines are the text's own lines, split
//! on `\n`, not the rows word wrap draws.

use fontelle_ui::canvas::{
    TextEntry, notepad_copy_text, notepad_cut, notepad_delete_line, notepad_delete_word,
    notepad_duplicate_line, notepad_indent, notepad_move_lines, notepad_outdent,
    notepad_paste_text, notepad_select_line, notepad_select_word,
};

fn at(text: &str, caret: usize) -> TextEntry {
    let mut e = TextEntry::new(text);
    e.place(caret, false);
    e
}

fn selected(text: &str, from: usize, to: usize) -> TextEntry {
    let mut e = TextEntry::new(text);
    e.place(from, false);
    e.place(to, true);
    e
}

// ------------------------------------------------------------ selecting ---

#[test]
fn a_double_click_selects_the_word_under_it() {
    let mut e = at("hello brave world", 0);
    notepad_select_word(&mut e, 8);
    assert_eq!(e.selected_text(), "brave");
    // On the space between two words: the space, as editors do.
    notepad_select_word(&mut e, 5);
    assert_eq!(e.selected_text(), " ");
    // At the very end: the last word.
    notepad_select_word(&mut e, 17);
    assert_eq!(e.selected_text(), "world");
}

#[test]
fn a_triple_click_selects_the_whole_line_and_its_newline() {
    let mut e = at("one\ntwo two\nthree", 0);
    notepad_select_line(&mut e, 6, false);
    assert_eq!(e.selected_text(), "two two\n");
    // The last line has no newline to take.
    notepad_select_line(&mut e, 14, false);
    assert_eq!(e.selected_text(), "three");
}

#[test]
fn selecting_a_line_again_takes_the_next_one_too() {
    // Ctrl+L, pressed twice.
    let mut e = at("one\ntwo\nthree", 1);
    notepad_select_line(&mut e, 1, false);
    assert_eq!(e.selected_text(), "one\n");
    let caret = e.caret();
    notepad_select_line(&mut e, caret, true);
    assert_eq!(e.selected_text(), "one\ntwo\n");
}

// ------------------------------------------------- copy, cut and paste ---

#[test]
fn copy_takes_the_selection_or_with_none_the_whole_line() {
    assert_eq!(notepad_copy_text(&selected("abc def", 0, 3)), "abc");
    assert_eq!(notepad_copy_text(&at("one\ntwo\nthree", 5)), "two\n");
    // The last line is a line too: the newline is added so a paste puts it
    // on a line of its own.
    assert_eq!(notepad_copy_text(&at("one\ntwo", 5)), "two\n");
}

#[test]
fn cut_with_nothing_selected_takes_the_line_out() {
    let mut e = at("one\ntwo\nthree", 5);
    assert_eq!(notepad_cut(&mut e), "two\n");
    assert_eq!(e.text(), "one\nthree");
    let mut e = selected("abc def", 3, 7);
    assert_eq!(notepad_cut(&mut e), " def");
    assert_eq!(e.text(), "abc");
}

#[test]
fn a_paste_keeps_its_lines() {
    // The fields' paste is one line; a pad's keeps every line of a lyric.
    assert_eq!(
        notepad_paste_text("one\r\ntwo\rthree\n"),
        "one\ntwo\nthree\n"
    );
    // A tab is four spaces on a fixed grid; other control characters go.
    assert_eq!(notepad_paste_text("a\tb\u{7}c"), "a    bc");
}

// ----------------------------------------------------------- whole lines ---

#[test]
fn delete_line_takes_out_the_line_the_caret_is_on() {
    let mut e = at("one\ntwo\nthree", 5);
    notepad_delete_line(&mut e);
    assert_eq!(e.text(), "one\nthree");
    assert_eq!(e.caret(), 4, "on the line that moved up");
    let mut e = at("one\ntwo", 6);
    notepad_delete_line(&mut e);
    assert_eq!(e.text(), "one");
}

#[test]
fn duplicate_line_puts_a_copy_underneath_and_moves_onto_it() {
    let mut e = at("one\ntwo\nthree", 5);
    notepad_duplicate_line(&mut e);
    assert_eq!(e.text(), "one\ntwo\ntwo\nthree");
    assert_eq!(e.caret(), 9, "same column, on the copy");
    // The last line, which has no newline to copy.
    let mut e = at("one\ntwo", 7);
    notepad_duplicate_line(&mut e);
    assert_eq!(e.text(), "one\ntwo\ntwo");
}

#[test]
fn moving_a_line_swaps_it_with_its_neighbour() {
    let mut e = at("one\ntwo\nthree", 5);
    assert!(notepad_move_lines(&mut e, -1));
    assert_eq!(e.text(), "two\none\nthree");
    assert_eq!(e.caret(), 1, "the caret goes with its line");
    assert!(notepad_move_lines(&mut e, 1));
    assert!(notepad_move_lines(&mut e, 1));
    assert_eq!(e.text(), "one\nthree\ntwo");
    assert!(
        !notepad_move_lines(&mut e, 1),
        "the last line goes no further"
    );
    // A selection over two lines moves both.
    let mut e = selected("a\nb\nc\nd", 2, 5);
    assert!(notepad_move_lines(&mut e, 1));
    assert_eq!(e.text(), "a\nd\nb\nc");
    assert_eq!(e.selected_text(), "b\nc");
}

#[test]
fn tab_indents_and_shift_tab_takes_it_back() {
    let mut e = at("verse", 0);
    notepad_indent(&mut e);
    assert_eq!(e.text(), "    verse");
    notepad_outdent(&mut e);
    assert_eq!(e.text(), "verse");
    // Several lines selected: each is indented, and the selection kept.
    let mut e = selected("a\nb\nc", 0, 3);
    notepad_indent(&mut e);
    assert_eq!(e.text(), "    a\n    b\nc");
    notepad_outdent(&mut e);
    assert_eq!(e.text(), "a\nb\nc");
}

#[test]
fn ctrl_backspace_and_ctrl_delete_take_a_word() {
    let mut e = at("hello brave world", 11);
    notepad_delete_word(&mut e, false);
    assert_eq!(e.text(), "hello  world");
    // Forwards, to where Ctrl+Right lands: the word and the gap after it.
    let mut e = at("hello brave world", 6);
    notepad_delete_word(&mut e, true);
    assert_eq!(e.text(), "hello world");
    // A selection is deleted as it is.
    let mut e = selected("hello brave world", 0, 6);
    notepad_delete_word(&mut e, false);
    assert_eq!(e.text(), "brave world");
}

// ------------------------------------------------------------- clicking ---

#[test]
fn clicks_count_one_two_three_and_start_again() {
    use fontelle_ui::pointer::ClickCount;
    use std::time::{Duration, Instant};
    let mut clicks = ClickCount::default();
    let t = Instant::now();
    let ms = |n| t + Duration::from_millis(n);
    assert_eq!(clicks.press(10.0, 10.0, ms(0)), 1);
    assert_eq!(clicks.press(11.0, 10.0, ms(150)), 2, "a double-click");
    assert_eq!(clicks.press(10.0, 11.0, ms(300)), 3, "a triple");
    assert_eq!(clicks.press(10.0, 10.0, ms(450)), 1, "and round again");
    // Too slow, or too far, is a fresh first click.
    assert_eq!(clicks.press(10.0, 10.0, ms(2_000)), 1);
    assert_eq!(clicks.press(60.0, 10.0, ms(2_100)), 1);
}
