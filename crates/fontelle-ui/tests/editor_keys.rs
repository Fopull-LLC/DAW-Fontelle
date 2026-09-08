//! Switching the editor column from the keyboard.
//!
//! Reported from using the window: *"i want to easily be able to swap between
//! piano roll and mixer by pressing 1 and 2 that would be fast and very
//! nice."*
//!
//! The number row used to pick tools — a second binding for keys that already
//! had FL's letters — and it went unused for exactly that reason. The two
//! views that are genuinely *the document* get the two keys nearest the
//! hand instead.

use fontelle_ui::layout::{EditorTab, editor_tab_for_key};

#[test]
fn one_is_the_roll_and_two_is_the_mixer() {
    assert_eq!(editor_tab_for_key("1"), Some(EditorTab::Roll));
    assert_eq!(editor_tab_for_key("2"), Some(EditorTab::Mixer));
}

#[test]
fn no_other_key_switches_the_tab() {
    for key in ["0", "3", "9", "a", "p", " ", "", "12"] {
        assert_eq!(editor_tab_for_key(key), None, "{key:?} switched the tab");
    }
}
