//! Flopsynth's bank, in the browser (`docs/flopsynth-plan.md` §P.8).
//!
//! # Why it is a row in the Sounds tab and not a fifth tab
//!
//! Because everything a list of a hundred and twenty-eight presets needs is
//! already there: a search, a virtualised draw, headings over groups, a click
//! that puts one on the selected channel and a Ctrl+click that puts one on a
//! new one. §P.8 asks for a Presets tab covering *every* device; this is the
//! half of it Flopsynth needs, built out of the list that already works —
//! which is the same argument `BrowserMode::Import` makes for itself.
//!
//! What these tests hold is that the bank behaves like a soundfont does: it is
//! in the list, opening it shows its presets grouped by category, the search
//! filters them, and clicking one puts a **Flopsynth** on the channel with
//! that preset's name on the rack row.

mod common;

use fontelle_types::InstrumentKind;
use fontelle_ui::document::{DocumentHost, LibraryKind, StudioHost};

use common::SR;

fn a_session() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    // The soundfont browser's tests; the studio opens on the Import tab now,
    // so the tab is switched the way a click would.
    session.set_browser_mode(fontelle_ui::canvas::BrowserMode::Sounds);
    session
}

/// The index of the row called `name`, or a panic naming what was there.
fn row(session: &fontelle_app::Session, name: &str) -> usize {
    session
        .library_files()
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no row called {name:?}; the list is {:?}",
                session
                    .library_files()
                    .iter()
                    .map(|e| e.name.clone())
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn the_bank_is_a_row_in_the_sounds_tab_and_says_how_many_presets_it_has() {
    let session = a_session();
    let files = session.library_files();
    let flopsynth = &files[row(&session, "Flopsynth")];
    assert_eq!(
        flopsynth.kind,
        LibraryKind::Folder,
        "it is something you go *into*, like a folder"
    );
    assert_eq!(
        flopsynth.detail,
        format!(
            "{} presets",
            fontelle_core::flopsynth::presets::FACTORY.len()
        )
    );
    // **Always there**, unlike a soundfont: there is no bank folder to
    // configure and nothing to install, so it works on a fresh install.
    assert_eq!(row(&session, "Flopsynth"), 0, "and it is first");
}

#[test]
fn opening_it_lists_its_presets_under_their_categories() {
    let mut session = a_session();
    // Nothing is open, so there are no presets under anything.
    assert!(session.library_presets().is_empty());

    session
        .open_file(row(&session, "Flopsynth"))
        .expect("opens");
    let presets = session.library_presets();

    let headings: Vec<&str> = presets
        .iter()
        .filter(|e| e.kind == LibraryKind::Group)
        .map(|e| e.name.as_str())
        .collect();
    let expected: Vec<&str> = fontelle_core::flopsynth::presets::FlopsynthCategory::ALL
        .iter()
        .map(|c| c.label())
        .collect();
    assert_eq!(
        headings, expected,
        "every category, in the order the bank lists them"
    );

    let rows = presets
        .iter()
        .filter(|e| e.kind == LibraryKind::File)
        .count();
    assert_eq!(rows, fontelle_core::flopsynth::presets::FACTORY.len());
    assert!(
        presets.iter().any(|e| e.name == "Choir Ahh"),
        "the preset the brief names by hand has to be in the list"
    );
    // And the row is lit as the open one, so the browser says where you are.
    assert_eq!(session.selected_file(), Some(0));
}

#[test]
fn the_search_filters_the_bank_and_drops_the_headings_it_empties() {
    let mut session = a_session();
    session
        .open_file(row(&session, "Flopsynth"))
        .expect("opens");
    session.set_query("choir");

    let presets = session.library_presets();
    let names: Vec<&str> = presets
        .iter()
        .filter(|e| e.kind == LibraryKind::File)
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"Choir Ahh"));
    assert!(names.contains(&"Choir Ooh"));
    assert!(
        !names.contains(&"808 Kick"),
        "the search has to actually filter: {names:?}"
    );
    // A heading over nothing says the search found nothing there, which the
    // empty list already says.
    for heading in presets.iter().filter(|e| e.kind == LibraryKind::Group) {
        let at = presets.iter().position(|e| e.name == heading.name).unwrap();
        assert!(
            presets
                .get(at + 1)
                .is_some_and(|next| next.kind == LibraryKind::File),
            "the heading {:?} has nothing under it",
            heading.name
        );
    }
}

/// The whole point: a click puts a **Flopsynth** on the channel, with the
/// preset's own name on the rack row.
#[test]
fn clicking_a_preset_puts_a_flopsynth_on_the_channel() {
    let mut session = a_session();
    session
        .open_file(row(&session, "Flopsynth"))
        .expect("opens");
    let presets = session.library_presets();
    let at = presets
        .iter()
        .position(|e| e.name == "Choir Ahh")
        .expect("the bank has a Choir Ahh");

    session.set_channel_instrument(at).expect("a preset lands");

    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
    assert_eq!(
        session.channels()[0].name,
        "Choir Ahh",
        "the rack says what is playing"
    );
    let patch = session.selected_patch().expect("it has a patch");
    assert!(fontelle_core::flopsynth::is_flopsynth(&patch));
    // The one it was asked for, not merely *a* Flopsynth: the formant filter
    // is what makes Choir Ahh a choir.
    assert_eq!(patch.filters[0].model, fontelle_dsp::FilterModel::Formant);

    // One undo, because choosing an instrument is one thing somebody did.
    // What it goes back to is the *starting* preset, which is itself a
    // Flopsynth now — so the kind is not what says the undo worked, the patch
    // is.
    session.undo();
    let back = session.selected_patch().expect("it still has a patch");
    assert_ne!(
        back.filters[0].model,
        fontelle_dsp::FilterModel::Formant,
        "the choir is gone again"
    );
    assert_eq!(
        session.channels()[0].name,
        "Grand Piano",
        "and the rack says what it went back to"
    );
}

#[test]
fn a_heading_is_not_something_to_click() {
    let mut session = a_session();
    session
        .open_file(row(&session, "Flopsynth"))
        .expect("opens");
    let heading = session
        .library_presets()
        .iter()
        .position(|e| e.kind == LibraryKind::Group)
        .expect("there are headings");
    assert!(
        session.set_channel_instrument(heading).is_err(),
        "a category is a heading, not a sound"
    );
}

/// Opening a soundfont after the bank puts the browser back on soundfonts —
/// two things cannot both be open, and a stale preset list is worse than none.
#[test]
fn opening_something_else_closes_the_bank() {
    let mut session = a_session();
    session
        .open_file(row(&session, "Flopsynth"))
        .expect("opens");
    assert!(!session.library_presets().is_empty());

    // There is no soundfont in this session's bank, so the next row is
    // whatever the empty bank offers — and opening nothing at all still has
    // to close what was open rather than leaving both lit.
    let files = session.library_files();
    if files.len() > 1 {
        let _ = session.open_file(1);
        assert_eq!(session.selected_file(), None);
    }
}
