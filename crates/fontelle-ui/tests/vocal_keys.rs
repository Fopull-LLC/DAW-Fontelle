//! The vocal roll from the keyboard: the pitch pen, Smooth, Snap pitch to
//! scale, Reset pitch and the note menu each have a key of their own, on
//! the shortcuts page and rebindable; and an input method's composition
//! (Japanese, Chinese) is shown in the lyric and phoneme fields while it is
//! being composed, underlined at the caret.

use fontelle_ui::canvas::{
    Action, Chord, Context, KEYBIND_SECTIONS, KeybindEntry, Keymap, Preedit, TextEntry, compose,
};

const VOCAL_KEYS: [Action; 5] = [
    Action::PitchPen,
    Action::SmoothPitch,
    Action::SnapPitch,
    Action::ResetPitch,
    Action::NoteMenu,
];

#[test]
fn the_vocal_rolls_commands_have_keys_on_the_shortcuts_page() {
    let map = Keymap::default();
    for action in VOCAL_KEYS {
        assert!(Action::ALL.contains(&action), "{action:?}");
        assert_eq!(action.context(), Context::Studio, "{action:?}");
        assert_ne!(map.label(action), Keymap::UNBOUND, "{action:?}");
        assert!(
            KEYBIND_SECTIONS
                .iter()
                .any(|s| s.title == "Vocal tracks"
                    && s.binds.contains(&KeybindEntry::Action(action))),
            "{action:?} is listed with the vocal keys"
        );
    }
    assert_eq!(map.label(Action::PitchPen), "Shift+P");
    assert_eq!(map.label(Action::NoteMenu), "Alt+Enter");
    assert_eq!(map.label(Action::SmoothPitch), "Alt+S");
    assert_eq!(map.label(Action::SnapPitch), "Alt+Q");
    assert_eq!(map.label(Action::ResetPitch), "Alt+R");
    assert!(Action::NoteMenu.does().contains("menu"));
    assert!(Action::PitchPen.does().to_lowercase().contains("pitch"));
    // Ids are what the settings file keeps: stable and distinct.
    let mut ids: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), Action::ALL.len());
    assert_eq!(Action::PitchPen.id(), "pitch-pen");
    assert_eq!(Action::NoteMenu.id(), "note-menu");
}

#[test]
fn the_vocal_keys_answer_in_the_studio_and_can_be_rebound() {
    let mut map = Keymap::default();
    let chord = |t: &str| Chord::parse(t).unwrap();
    assert_eq!(
        map.action(&chord("Alt+Enter"), Context::Studio),
        Some(Action::NoteMenu)
    );
    assert_eq!(
        map.action(&chord("Shift+P"), Context::Studio),
        Some(Action::PitchPen)
    );
    // Plain P is still the pencil.
    assert_eq!(
        map.action(&chord("P"), Context::Studio),
        Some(Action::DrawTool)
    );
    map.rebind(Action::NoteMenu, chord("Ctrl+Shift+N"));
    assert_eq!(
        map.action(&chord("Ctrl+Shift+N"), Context::Studio),
        Some(Action::NoteMenu)
    );
    assert_eq!(map.action(&chord("Alt+Enter"), Context::Studio), None);
}

// ------------------------------------------------------------- preedit

#[test]
fn no_composition_shows_the_field_as_it_is() {
    let mut entry = TextEntry::new("sakura");
    entry.select_all();
    let shown = compose(&entry, &Preedit::default());
    assert_eq!(shown.text, "sakura");
    assert_eq!(shown.underline, None);
    assert_eq!(shown.selection, Some((0, 6)));
    assert_eq!(shown.caret, 6);
}

#[test]
fn a_composition_is_shown_at_the_caret_underlined() {
    let mut entry = TextEntry::new("さら");
    entry.place("さ".len(), false);
    let mut pre = Preedit::default();
    // The IME is composing く, its cursor after it.
    pre.set("く", Some(("く".len(), "く".len())));
    assert!(pre.is_active());
    let shown = compose(&entry, &pre);
    assert_eq!(shown.text, "さくら");
    assert_eq!(shown.underline, Some(("さ".len(), "さく".len())));
    assert_eq!(shown.caret, "さく".len());
    assert_eq!(shown.selection, None);
    // A hidden cursor puts the caret at the end of the composition.
    pre.set("きょう", None);
    let shown = compose(&entry, &pre);
    assert_eq!(shown.caret, "さきょう".len());
    // The field itself is untouched until the composition is committed.
    assert_eq!(entry.text(), "さら");
}

#[test]
fn a_composition_replaces_the_selection_while_it_is_shown() {
    let mut entry = TextEntry::new("la");
    entry.select_all();
    let mut pre = Preedit::default();
    pre.set("ら", Some((0, 0)));
    let shown = compose(&entry, &pre);
    assert_eq!(shown.text, "ら");
    assert_eq!(shown.underline, Some((0, "ら".len())));
    assert_eq!(shown.caret, 0);
}

#[test]
fn an_empty_composition_or_a_commit_ends_it() {
    let mut pre = Preedit::default();
    pre.set("か", Some((3, 3)));
    pre.set("", None);
    assert!(!pre.is_active());
    pre.set("か", Some((3, 3)));
    pre.clear();
    assert!(!pre.is_active());
    assert_eq!(pre.text(), "");
    // A cursor past the text (a misbehaving IME) is kept inside it, on a
    // character boundary.
    pre.set("かな", Some((100, 100)));
    let entry = TextEntry::new("");
    let shown = compose(&entry, &pre);
    assert_eq!(shown.caret, "かな".len());
    pre.set("かな", Some((1, 1)));
    let shown = compose(&entry, &pre);
    assert!(shown.text.is_char_boundary(shown.caret));
}
