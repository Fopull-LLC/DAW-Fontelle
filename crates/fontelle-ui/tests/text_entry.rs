//! What a text field *is*, before anything draws one.
//!
//! > *"the input entering field has really bad ux and visuals right now it
//! > doesn't look like a input field it's just text on a background making it
//! > look like it's a label and not somewhere you can type you can't even see
//! > your cursor where you're typing and i can't ctrl a to select all my text
//! > and stuff like that."*
//!
//! All three complaints are the same missing thing: there was no **model** of
//! being edited. Every place you could type held a `String` and drew it with a
//! block character stuck on the end, so there was no caret to move, nothing to
//! select, and nothing for Ctrl+A to select. This is that model — a string, a
//! caret, and an anchor — and it is pure, so every key can be checked without
//! a window.
//!
//! Byte indices throughout, always on a character boundary: Rust strings are
//! bytes and a caret that could land inside a multi-byte character is a panic
//! waiting for somebody to type an accent.

use fontelle_ui::canvas::TextEntry;

fn typed(text: &str) -> TextEntry {
    let mut entry = TextEntry::new(text);
    entry.end(false);
    entry
}

#[test]
fn a_fresh_field_holds_what_it_was_given_with_the_caret_at_the_end() {
    let entry = TextEntry::new("Verse");
    assert_eq!(entry.text(), "Verse");
    // At the end: a field you open on an existing name is a field you are
    // about to add to, and starting at the front would mean pressing End
    // before every rename.
    assert_eq!(entry.caret(), "Verse".len());
    assert!(entry.selection().is_none(), "nothing is selected yet");
}

#[test]
fn typing_inserts_at_the_caret_and_moves_it_along() {
    let mut entry = typed("Vere");
    entry.left(false, false);
    entry.insert("s");
    assert_eq!(entry.text(), "Verse");
    assert_eq!(entry.caret(), 4, "the caret is after what was typed");
}

#[test]
fn backspace_and_delete_take_the_character_each_side_of_the_caret() {
    let mut entry = typed("Verse");
    entry.backspace();
    assert_eq!(entry.text(), "Vers");
    entry.left(false, false);
    entry.delete();
    assert_eq!(entry.text(), "Ver");
    // And at the ends they do nothing rather than panicking.
    let mut edge = TextEntry::new("");
    edge.backspace();
    edge.delete();
    assert_eq!(edge.text(), "");
}

/// **Ctrl+A**, which is the complaint by name.
#[test]
fn select_all_covers_the_whole_string_and_typing_replaces_it() {
    let mut entry = typed("Old name");
    entry.select_all();
    assert_eq!(entry.selection(), Some((0, "Old name".len())));
    assert_eq!(entry.selected_text(), "Old name");
    entry.insert("New");
    assert_eq!(entry.text(), "New", "typing over a selection replaces it");
    assert!(entry.selection().is_none());
    assert_eq!(entry.caret(), 3);
}

#[test]
fn a_selection_is_deleted_as_one_by_either_key() {
    for backspace in [true, false] {
        let mut entry = typed("Hello world");
        entry.home(false);
        for _ in 0..5 {
            entry.right(false, true); // shift-right five times
        }
        assert_eq!(entry.selected_text(), "Hello");
        if backspace {
            entry.backspace();
        } else {
            entry.delete();
        }
        assert_eq!(entry.text(), " world");
        assert_eq!(entry.caret(), 0);
    }
}

#[test]
fn shift_arrows_grow_a_selection_and_plain_arrows_collapse_it() {
    let mut entry = typed("abcdef");
    entry.left(false, true);
    entry.left(false, true);
    assert_eq!(entry.selected_text(), "ef");
    // A plain arrow collapses to the near edge rather than moving from the
    // caret — which is what every text field does and what stops a selection
    // being nudged into a different one.
    entry.left(false, false);
    assert!(entry.selection().is_none());
    assert_eq!(entry.caret(), 4, "collapsed to the left edge");
}

#[test]
fn home_and_end_go_to_the_ends_and_can_select_on_the_way() {
    let mut entry = typed("abcdef");
    entry.home(false);
    assert_eq!(entry.caret(), 0);
    entry.end(true);
    assert_eq!(entry.selected_text(), "abcdef");
}

/// Ctrl+arrows move by words, to the **start of a word** in both directions —
/// the convention Windows and GTK use, and the one this program's users are
/// most likely to have in their hands. (macOS stops at the *end* of the word
/// going right; picking one and being consistent matters more than which.)
#[test]
fn ctrl_arrows_move_by_words() {
    let mut entry = typed("one two three");
    entry.left(true, false);
    assert_eq!(entry.caret(), "one two ".len(), "back to the last word");
    entry.left(true, false);
    assert_eq!(entry.caret(), "one ".len());
    // Forwards to the start of the word after this one, not to the end of
    // this one — the same rule read the other way.
    entry.right(true, false);
    assert_eq!(entry.caret(), "one two ".len());
}

/// **Multi-byte characters are characters**, not bytes. A caret that stepped
/// one byte into an accent would split it, and `String::insert` would panic.
#[test]
fn the_caret_never_lands_inside_a_character() {
    let mut entry = typed("café");
    entry.left(false, false);
    assert_eq!(entry.caret(), "caf".len(), "one step is one character");
    entry.backspace();
    assert_eq!(entry.text(), "caé");
    let mut wide = typed("aΩb");
    wide.home(false);
    wide.right(false, false);
    wide.delete();
    assert_eq!(wide.text(), "ab", "one delete is one character");
}

/// Whatever is done to it, the caret stays inside the string and on a
/// boundary — the invariant everything else rests on.
#[test]
fn the_caret_is_always_somewhere_it_could_be() {
    let mut entry = typed("héllo wörld");
    for _ in 0..40 {
        entry.left(true, true);
        entry.right(false, false);
        entry.end(true);
        entry.home(false);
        entry.right(true, true);
        assert!(entry.caret() <= entry.text().len());
        assert!(
            entry.text().is_char_boundary(entry.caret()),
            "the caret landed inside a character"
        );
        if let Some((from, to)) = entry.selection() {
            assert!(from <= to && to <= entry.text().len());
            assert!(entry.text().is_char_boundary(from));
            assert!(entry.text().is_char_boundary(to));
        }
    }
}
