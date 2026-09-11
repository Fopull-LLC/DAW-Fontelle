//! The browser's fourth tab: the files you import from.
//!
//! Asked for as *"a import/midi and import/fsc option which should, if I don't
//! have a folder selected yet, take me to the settings menu where I can select
//! my preferred folder to store and load midi files, and should work cleanly
//! with subdirectories"*.
//!
//! It is a **tab on the browser** rather than a list inside the roll's Tools
//! panel, because everything a browser of files needs already exists here:
//! folders you can walk into, a `..` row back out, a search across the whole
//! collection, and a virtualised list that costs a screenful of rectangles
//! whatever is in the folder. A second implementation of all of that, dropped
//! from a chip and eleven rows tall, would be worse at every one of them.
//!
//! One tab holds both kinds — MIDI files and FL scores — with a pair of
//! buttons inside it that say which. Five tabs across a 248-pixel sidebar is a
//! row of abbreviations.

use fontelle_types::FolderKind;
use fontelle_ui::canvas::{BrowserHit, BrowserMode, browser_hit, browser_layout_for};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(0.0, 0.0, 248.0, 600.0)
}

fn imports(count: usize) -> fontelle_ui::canvas::BrowserLayout {
    browser_layout_for(body(), &metrics(), BrowserMode::Import, count, 0, 0, 0)
}

fn mid(rect: Rect) -> (f32, f32) {
    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

// ----------------------------------------------------------------- the tab ---

#[test]
fn there_is_an_import_tab_and_it_does_not_sit_on_top_of_the_others() {
    let l = imports(4);
    let tabs = [
        l.tab(BrowserMode::Sounds),
        l.tab(BrowserMode::Projects),
        l.tab(BrowserMode::Import),
        l.tab(BrowserMode::Settings),
    ];
    for tab in tabs {
        assert!(
            !tab.is_empty(),
            "a tab with no room is a tab nobody can press"
        );
        assert!(
            tab.right() <= body().right() + 0.01,
            "{tab:?} runs off the panel"
        );
    }
    for (i, a) in tabs.iter().enumerate() {
        for b in &tabs[i + 1..] {
            assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn clicking_the_import_tab_asks_for_the_import_mode() {
    let l = imports(4);
    let (x, y) = mid(l.tab(BrowserMode::Import));
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Mode(BrowserMode::Import));
}

#[test]
fn the_four_tabs_still_each_lead_to_their_own_mode() {
    // The bug this guards is the one this panel has had before: a click
    // handler that worked out the mode from its own state rather than from
    // the hit, so pressing one tab did another one's job.
    let l = imports(4);
    for (rect, mode) in [
        (l.tab(BrowserMode::Sounds), BrowserMode::Sounds),
        (l.tab(BrowserMode::Projects), BrowserMode::Projects),
        (l.tab(BrowserMode::Import), BrowserMode::Import),
        (l.tab(BrowserMode::Settings), BrowserMode::Settings),
    ] {
        let (x, y) = mid(rect);
        assert_eq!(browser_hit(&l, x, y), BrowserHit::Mode(mode));
    }
}

#[test]
fn the_layout_says_which_mode_it_was_built_for() {
    assert_eq!(imports(4).mode, BrowserMode::Import);
}

// -------------------------------------------------------------- the kinds ---

#[test]
fn the_import_tab_carries_a_button_for_each_kind_of_file() {
    // **Every** kind, from `FolderKind::ALL` rather than from a list written
    // here. Audio was added as a variant and the tab drew two buttons, because
    // the row was built for exactly two — the same class of bug as the fourth
    // browser tab that was drawn with no words on it.
    let l = imports(4);
    assert_eq!(l.kinds.len(), FolderKind::ALL.len());
    for (kind, rect) in &l.kinds {
        assert!(!rect.is_empty(), "{kind:?} has no button");
        assert!(
            rect.right() <= body().right() + 0.01,
            "{kind:?} escapes the panel"
        );
    }
    for (i, (a_kind, a)) in l.kinds.iter().enumerate() {
        for (b_kind, b) in l.kinds.iter().skip(i + 1) {
            assert!(!a.intersects(b), "{a_kind:?} is over {b_kind:?}");
        }
    }
}

#[test]
fn clicking_a_kind_button_asks_for_that_kind() {
    let l = imports(4);
    for (kind, rect) in &l.kinds {
        let (x, y) = mid(*rect);
        assert_eq!(browser_hit(&l, x, y), BrowserHit::Kind(*kind));
    }
}

#[test]
fn the_kind_buttons_are_only_in_the_tab_they_mean_anything_in() {
    // A control that does nothing in the mode you are in is worse than one
    // that is not there — the rule this panel already follows for "New
    // project" and "Export".
    for mode in [
        BrowserMode::Sounds,
        BrowserMode::Projects,
        BrowserMode::Settings,
    ] {
        let l = browser_layout_for(body(), &metrics(), mode, 4, 4, 0, 0);
        assert!(
            l.kinds.is_empty(),
            "{mode:?} draws the import tab's buttons"
        );
    }
}

#[test]
fn the_import_tab_draws_no_project_buttons() {
    let l = imports(4);
    assert!(l.new_project.is_empty());
    assert!(l.export.is_empty());
}

// ------------------------------------------------------------- the layout ---

#[test]
fn the_import_tab_gives_its_whole_list_area_to_the_files() {
    // There is nothing *inside* a MIDI file to list the way there are presets
    // inside a soundfont, so a second list would be half the panel left
    // blank.
    let l = imports(40);
    assert!(l.presets.is_empty());
    assert_eq!(l.preset_count, 0);
    assert!(!l.files.is_empty());
    assert!(!l.file_rows.is_empty());
}

#[test]
fn nothing_in_the_import_tab_is_drawn_on_top_of_anything_else() {
    // The failure this catches has happened here before: the status line was
    // drawn straight across the buttons under it.
    let l = imports(40);
    let mut parts: Vec<(&str, Rect)> = vec![
        ("search", l.search),
        ("files", l.files),
        ("status", l.status),
        ("open folder", l.open_folder),
        ("choose folder", l.choose_folder),
        ("import tab", l.tab(BrowserMode::Import)),
    ];
    parts.extend(l.kinds.iter().map(|(_, rect)| ("kind", *rect)));
    for (i, (name_a, a)) in parts.iter().enumerate() {
        if a.is_empty() {
            continue;
        }
        assert!(
            a.bottom() <= body().bottom() + 0.01,
            "{name_a} runs off the bottom"
        );
        for (name_b, b) in &parts[i + 1..] {
            if b.is_empty() {
                continue;
            }
            assert!(!a.intersects(b), "{name_a} overlaps {name_b}");
        }
    }
}

#[test]
fn the_import_tab_has_a_search_box_because_a_collection_of_midi_is_a_collection() {
    let l = imports(4);
    assert!(!l.search.is_empty());
    let (x, y) = mid(l.search);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::Search(BrowserMode::Import)
    );
}

#[test]
fn the_folder_buttons_mean_this_tabs_folder() {
    let l = imports(4);
    let (x, y) = mid(l.open_folder);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::OpenFolder(BrowserMode::Import)
    );
    let (x, y) = mid(l.choose_folder);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::ChooseFolder(BrowserMode::Import)
    );
}

#[test]
fn a_row_in_the_import_list_is_a_file_to_open() {
    let l = imports(20);
    let (index, rect) = l.file_rows[3];
    let (x, y) = mid(rect);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::File(index));
}

#[test]
fn every_new_hit_has_something_to_say_when_it_is_hovered() {
    // The rule the rest of this panel follows: a control worth pressing is
    // worth a sentence saying what it does.
    for hit in [
        BrowserHit::Mode(BrowserMode::Import),
        BrowserHit::Search(BrowserMode::Import),
        BrowserHit::OpenFolder(BrowserMode::Import),
        BrowserHit::ChooseFolder(BrowserMode::Import),
        BrowserHit::Kind(FolderKind::Midi),
        BrowserHit::Kind(FolderKind::Scores),
    ] {
        assert!(hit.tip().is_some(), "{hit:?} has no tip");
    }
}

#[test]
fn a_narrow_panel_still_gives_every_tab_somewhere_to_be() {
    // Four tabs across a sidebar somebody has dragged narrow. They may be
    // tight; none of them may be nothing, and none may leave the panel.
    let narrow = Rect::new(0.0, 0.0, 120.0, 400.0);
    let l = browser_layout_for(narrow, &metrics(), BrowserMode::Import, 4, 0, 0, 0);
    for tab in [
        l.tab(BrowserMode::Sounds),
        l.tab(BrowserMode::Projects),
        l.tab(BrowserMode::Import),
        l.tab(BrowserMode::Settings),
    ] {
        assert!(!tab.is_empty());
        assert!(tab.x >= narrow.x - 0.01 && tab.right() <= narrow.right() + 0.01);
    }
}

#[test]
fn each_kind_knows_what_it_is_called_on_its_button() {
    assert_ne!(FolderKind::Midi.tab_label(), FolderKind::Scores.tab_label());
    for kind in FolderKind::ALL {
        assert!(!kind.tab_label().is_empty());
    }
}

// ------------------------------------------------- carrying a row out of it ---
//
// > *"i currently cannot drag audio files from the import audio tab. i want to
// > be able to click and drag them into the sampler or into the channel rack
// > to make it have a sampler with that clip sampled."*
//
// The reason it could not be done: the press asked **the soundfont list**
// whether the row under the pointer was a folder, while the rows on screen
// came from the import list. Two lists of different lengths, so the answer was
// whatever the *other* panel happened to have at that index — usually "a
// folder", which armed no drag at all, and sometimes nothing at all, which
// armed a drag on the `..` row. `browser_row_carries` is the one place that
// decision now lives, and it is handed the list the rows were drawn from.

use fontelle_ui::canvas::browser_row_carries;
use fontelle_ui::document::{LibraryEntry, LibraryKind};

fn entry(name: &str, kind: LibraryKind) -> LibraryEntry {
    LibraryEntry {
        name: name.to_string(),
        detail: String::new(),
        kind,
    }
}

/// What the Import tab looks like inside a folder: the row back out, two
/// folders, then the files.
fn import_rows() -> Vec<LibraryEntry> {
    vec![
        entry("..", LibraryKind::Up),
        entry("Breaks", LibraryKind::Folder),
        entry("Kicks", LibraryKind::Folder),
        entry("CoolBreak.wav", LibraryKind::File),
        entry("Doll_Break_120_PL.wav", LibraryKind::File),
    ]
}

/// Asked of the Import tab showing **sounds**, which is the only list a row
/// is ever carried out of — the kind is checked by its own test in
/// `carry.rs`, beside the rest of what a carried row means.
fn carries(rows: &[LibraryEntry], index: usize) -> bool {
    browser_row_carries(rows, BrowserMode::Import, FolderKind::Audio, index)
}

#[test]
fn a_file_row_in_the_import_tab_can_be_carried_out_of_the_panel() {
    let rows = import_rows();
    assert!(carries(&rows, 3));
    assert!(carries(&rows, 4));
}

#[test]
fn a_folder_is_a_place_rather_than_a_sound_and_carries_nothing() {
    let rows = import_rows();
    assert!(!carries(&rows, 0), "the .. row");
    assert!(!carries(&rows, 1));
    assert!(!carries(&rows, 2));
}

#[test]
fn a_row_that_is_not_in_the_list_carries_nothing() {
    // The exact shape of the bug: asked about row 8 of a five-row list, the
    // old test was `!matches!(list.get(8), Some(Folder | Up))` — which is true
    // for `None`, so a row nobody clicked armed a drag on nothing.
    let rows = import_rows();
    assert!(!carries(&rows, 8));
    assert!(!carries(&[], 0));
}

#[test]
fn only_the_import_tab_hands_out_sounds_to_carry() {
    // A soundfont row is carried by a different gesture with a different
    // meaning (a preset onto a channel), and a project or a setting is not a
    // sound at all.
    let rows = import_rows();
    for mode in [
        BrowserMode::Sounds,
        BrowserMode::Projects,
        BrowserMode::Settings,
    ] {
        assert!(
            !browser_row_carries(&rows, mode, FolderKind::Audio, 3),
            "{mode:?} let a file row be dragged out"
        );
    }
}
