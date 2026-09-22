//! The notepad's vocabulary (`docs/effects-catalogue.md` §2.8).
//!
//! > *"make a new built in mixer track effect called Notepad. its just a basic
//! > text editor with pages you can go left and right between and use it like a
//! > normal text editor to write down lyrics for example as you record to sing
//! > them back."*
//!
//! An insert that makes no sound is still an insert: it is in the menu, it has
//! a parameter table, it ships a bank, and it is saved with the project. What
//! is different is where its content lives — see [`NotepadPages`], which is not
//! in the config and says why.

use fontelle_types::{
    EffectConfig, EffectKind, NotepadConfig, NotepadEdit, NotepadPages, NotepadPreset, NotepadSize,
    NotepadTheme, Taper,
};

// ------------------------------------------------------------- the insert

#[test]
fn the_notepad_is_an_effect_the_menu_offers() {
    assert!(
        EffectKind::ALL.contains(&EffectKind::Notepad),
        "the notepad is not in the menu"
    );
    assert_eq!(EffectKind::Notepad.label(), "Notepad");
    // It has no detector, no use for notes, and nothing of the track goes
    // under anything: it is a wire with words on it.
    assert!(!EffectKind::Notepad.takes_key());
    assert!(!EffectKind::Notepad.takes_notes());
    assert!(!EffectKind::Notepad.is_time_based());
}

#[test]
fn its_knobs_are_its_looks() {
    // The pad has nothing to say about the sound, so its parameter table is
    // how it is read: which palette, how big the words are — and the mix
    // every effect has, which on this one blends a signal with itself.
    let config = EffectConfig::new(EffectKind::Notepad);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(ids, ["theme", "size", "mix"]);
    let theme = config.specs()[0];
    assert_eq!(
        theme.taper,
        Taper::Stepped(NotepadTheme::ALL.len() as u32),
        "a step per theme"
    );
    assert_eq!(
        theme.positions.len(),
        NotepadTheme::ALL.len(),
        "every theme is named on the control"
    );
    let size = config.specs()[1];
    assert_eq!(size.taper, Taper::Stepped(NotepadSize::ALL.len() as u32));
    assert_eq!(size.positions.len(), NotepadSize::ALL.len());
}

#[test]
fn the_theme_is_read_and_written_by_its_position() {
    let mut config = NotepadConfig::new();
    for (index, theme) in NotepadTheme::ALL.into_iter().enumerate() {
        let mut effect = EffectConfig::Notepad(config);
        effect.set("theme", index as f32);
        let EffectConfig::Notepad(back) = effect else {
            unreachable!("still a notepad")
        };
        assert_eq!(back.theme, theme, "position {index}");
        assert_eq!(effect.get("theme"), Some(index as f32));
        config = back;
    }
}

// -------------------------------------------------------------- the themes

#[test]
fn every_theme_has_a_name_and_a_slug_of_its_own() {
    // The name is what the window says; the slug is what the settings file
    // remembers, and it is a word rather than a number so that reordering the
    // list cannot silently change somebody's default.
    let mut labels: Vec<&str> = NotepadTheme::ALL.iter().map(|t| t.label()).collect();
    let mut slugs: Vec<&str> = NotepadTheme::ALL.iter().map(|t| t.slug()).collect();
    assert!(NotepadTheme::ALL.len() >= 6, "a bank's worth of looks");
    let (before_labels, before_slugs) = (labels.len(), slugs.len());
    labels.sort_unstable();
    labels.dedup();
    slugs.sort_unstable();
    slugs.dedup();
    assert_eq!(labels.len(), before_labels, "two themes share a name");
    assert_eq!(slugs.len(), before_slugs, "two themes share a slug");
    for theme in NotepadTheme::ALL {
        assert!(!theme.label().is_empty());
        assert_eq!(NotepadTheme::from_slug(theme.slug()), Some(theme));
    }
    assert_eq!(NotepadTheme::from_slug("chartreuse"), None);
}

#[test]
fn the_looks_step_both_ways_and_wrap() {
    // Seven themes is too many to walk one way: the chip steps forward on a
    // click and back on a right-click, so the one you just passed is one
    // press away rather than six.
    let first = NotepadTheme::ALL[0];
    let last = NotepadTheme::ALL[NotepadTheme::ALL.len() - 1];
    assert_eq!(first.previous(), last, "back from the first is the last");
    assert_eq!(last.next(), first, "and forward from the last is the first");
    for theme in NotepadTheme::ALL {
        assert_eq!(theme.next().previous(), theme);
    }
    for size in NotepadSize::ALL {
        assert_eq!(size.next().previous(), size);
    }
}

#[test]
fn the_bank_is_one_preset_per_theme() {
    // Every built-in ships a bank worth opening (`fontelle-app/tests/
    // effect_editor.rs`), and the pad's only knobs are its looks — so its
    // bank is its looks, and there is exactly one of each.
    assert_eq!(NotepadPreset::ALL.len(), NotepadTheme::ALL.len());
    let configs: Vec<NotepadConfig> = NotepadPreset::ALL
        .iter()
        .map(|preset| NotepadConfig::from_preset(*preset))
        .collect();
    for (i, one) in configs.iter().enumerate() {
        assert!(!NotepadPreset::ALL[i].label().is_empty());
        for two in configs.iter().skip(i + 1) {
            assert_ne!(one, two, "two presets are the same pad");
        }
    }
}

// --------------------------------------------------------------- the pages

#[test]
fn a_fresh_pad_is_one_empty_page() {
    // Never none: a notepad with no pages is a window with nothing to type
    // in, and "add a page before you can write" is a step nobody asked for.
    let pad = NotepadPages::new();
    assert_eq!(pad.len(), 1);
    assert_eq!(pad.showing(), 0);
    assert_eq!(pad.showing_text(), "");
}

#[test]
fn typing_is_the_page_rewritten_and_the_inverse_is_what_it_said_before() {
    let mut pad = NotepadPages::new();
    let inverse = pad
        .apply(&NotepadEdit::Write {
            page: 0,
            text: "when the lights go down".to_string(),
        })
        .expect("a page that is there takes the words");
    assert_eq!(pad.showing_text(), "when the lights go down");
    pad.apply(&inverse).expect("and back");
    assert_eq!(pad.showing_text(), "");
}

#[test]
fn a_write_turns_to_the_page_it_changed() {
    // So that an undo taken on page three puts the words back *and* shows
    // them. An edit you cannot see is an edit you undo twice.
    let mut pad = NotepadPages::new();
    pad.apply(&NotepadEdit::InsertPage {
        at: 1,
        text: "second".to_string(),
    })
    .unwrap();
    assert_eq!(pad.showing(), 1, "a page added is a page turned to");
    pad.apply(&NotepadEdit::Write {
        page: 0,
        text: "first".to_string(),
    })
    .unwrap();
    assert_eq!(pad.showing(), 0);
}

#[test]
fn a_page_past_the_end_is_refused_rather_than_invented() {
    let mut pad = NotepadPages::new();
    assert!(
        pad.apply(&NotepadEdit::Write {
            page: 4,
            text: "nowhere".to_string(),
        })
        .is_none()
    );
    assert_eq!(pad.len(), 1, "nothing was made to hold it");
    assert!(
        pad.apply(&NotepadEdit::InsertPage {
            at: 9,
            text: String::new()
        })
        .is_none()
    );
    assert!(pad.apply(&NotepadEdit::RemovePage { page: 3 }).is_none());
}

#[test]
fn the_last_page_cannot_be_taken_away() {
    let mut pad = NotepadPages::new();
    assert!(
        pad.apply(&NotepadEdit::RemovePage { page: 0 }).is_none(),
        "a pad always has a page"
    );
    assert_eq!(pad.len(), 1);
}

#[test]
fn removing_a_page_keeps_its_words_for_the_undo() {
    let mut pad = NotepadPages::new();
    pad.apply(&NotepadEdit::InsertPage {
        at: 1,
        text: "chorus".to_string(),
    })
    .unwrap();
    let inverse = pad.apply(&NotepadEdit::RemovePage { page: 1 }).unwrap();
    assert_eq!(pad.len(), 1);
    assert_eq!(pad.showing(), 0, "the page showing went, so the one before");
    pad.apply(&inverse).expect("put it back");
    assert_eq!(pad.len(), 2);
    assert_eq!(
        pad.text(1),
        "chorus",
        "an undo that lost the words is a bug"
    );
    assert_eq!(pad.showing(), 1);
}

#[test]
fn turning_past_an_end_is_refused_rather_than_clamped() {
    // Refused rather than clamped so that holding the arrow at the last page
    // does not fill the history with entries that changed nothing.
    let mut pad = NotepadPages::new();
    pad.apply(&NotepadEdit::InsertPage {
        at: 1,
        text: String::new(),
    })
    .unwrap();
    let back = pad.apply(&NotepadEdit::Show { page: 0 }).unwrap();
    assert_eq!(pad.showing(), 0);
    assert_eq!(back, NotepadEdit::Show { page: 1 });
    assert!(
        pad.apply(&NotepadEdit::Show { page: 0 }).is_none(),
        "already"
    );
    assert!(
        pad.apply(&NotepadEdit::Show { page: 7 }).is_none(),
        "no page"
    );
}

#[test]
fn the_words_survive_the_file() {
    let mut pad = NotepadPages::new();
    pad.apply(&NotepadEdit::Write {
        page: 0,
        text: "verse one\nline two".to_string(),
    })
    .unwrap();
    pad.apply(&NotepadEdit::InsertPage {
        at: 1,
        text: "chorus".to_string(),
    })
    .unwrap();
    let json = serde_json::to_string(&pad).unwrap();
    let back: NotepadPages = serde_json::from_str(&json).unwrap();
    assert_eq!(back, pad);
    assert!(json.contains("verse one"), "legible in the project file");
}

#[test]
fn a_file_that_says_nothing_sensible_still_opens() {
    // A page index past the end, and no pages at all: both are files somebody
    // hand-edited, and neither is worth refusing to open a project over.
    let pad: NotepadPages = serde_json::from_str(r#"{"pages":[],"showing":6}"#).unwrap();
    assert_eq!(pad.len(), 1, "a pad always has a page");
    assert_eq!(pad.showing(), 0);
    assert_eq!(pad.showing_text(), "");
}
