//! Typing a name into a menu.
//!
//! > *"when i make a new project i need to be prompted to name it"*

use fontelle_ui::canvas::{NAME_CARET, name_prompt_entries};

/// The prompt says what it is for and shows what has been typed, with a caret
/// — a box you type into that does not show the caret is a box nobody is sure
/// is listening.
#[test]
fn the_heading_carries_the_name_as_it_is_typed() {
    let entries = name_prompt_entries("Name the new project", "Sec");
    assert!(entries[0].label.starts_with("Name the new project"));
    assert!(entries[0].label.contains("Sec"), "{:?}", entries[0].label);
    assert!(
        entries[0].label.ends_with(NAME_CARET),
        "{:?}",
        entries[0].label
    );
    assert!(!entries[0].enabled, "the heading is not a row you press");
}

/// Nothing typed yet says what to do rather than showing an empty line.
#[test]
fn an_empty_name_says_to_type_one() {
    let entries = name_prompt_entries("Name the new project", "");
    assert!(entries[0].label.contains("type"), "{:?}", entries[0].label);
}

/// One row, and it is the one Enter presses.
#[test]
fn there_is_exactly_one_row_to_choose() {
    for typed in ["", "Song"] {
        let entries = name_prompt_entries("Name the new project", typed);
        let rows: Vec<&str> = entries
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.label.as_str())
            .collect();
        assert_eq!(rows.len(), 1, "{rows:?}");
    }
}

/// A name long enough to be a paragraph does not make a menu the width of the
/// screen: what is shown is the end of it, which is where the caret is.
#[test]
fn a_very_long_name_is_shown_from_its_end() {
    let long = "a".repeat(200);
    let entries = name_prompt_entries("Name the new project", &long);
    assert!(
        entries[0].label.chars().count() < 80,
        "{}",
        entries[0].label.len()
    );
    assert!(entries[0].label.ends_with(NAME_CARET));
}

/// The field is drawn over the heading, and the heading's words are its
/// placeholder — so a prompt that opened with something in it (your name, a
/// song's current name) said nothing about what it was asking. Sharing a
/// song asked for two things in a row with nothing to tell them apart. With
/// anything typed, the prompt still says what it is for, on a row of its own
/// that is not one you press.
#[test]
fn a_prompt_with_something_in_it_still_says_what_it_is_for() {
    let entries = name_prompt_entries("Your name, as the others will see it", "fopull");
    let said = entries
        .iter()
        .skip(1)
        .find(|entry| entry.label == "Your name, as the others will see it")
        .expect("the title below the field");
    assert!(!said.enabled);
    // Empty, the placeholder says it; a second copy would be the same words
    // twice.
    let entries = name_prompt_entries("Your name, as the others will see it", "");
    assert!(
        entries
            .iter()
            .skip(1)
            .all(|entry| entry.label != "Your name, as the others will see it")
    );
}
