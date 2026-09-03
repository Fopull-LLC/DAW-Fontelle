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
    let tabs = [l.sounds_tab, l.projects_tab, l.import_tab, l.settings_tab];
    for tab in tabs {
        assert!(!tab.is_empty(), "a tab with no room is a tab nobody can press");
        assert!(tab.right() <= body().right() + 0.01, "{tab:?} runs off the panel");
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
    let (x, y) = mid(l.import_tab);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Mode(BrowserMode::Import));
}

#[test]
fn the_four_tabs_still_each_lead_to_their_own_mode() {
    // The bug this guards is the one this panel has had before: a click
    // handler that worked out the mode from its own state rather than from
    // the hit, so pressing one tab did another one's job.
    let l = imports(4);
    for (rect, mode) in [
        (l.sounds_tab, BrowserMode::Sounds),
        (l.projects_tab, BrowserMode::Projects),
        (l.import_tab, BrowserMode::Import),
        (l.settings_tab, BrowserMode::Settings),
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
    let l = imports(4);
    assert!(!l.midi_kind.is_empty());
    assert!(!l.score_kind.is_empty());
    assert!(!l.midi_kind.intersects(&l.score_kind));
    assert!(l.score_kind.right() <= body().right() + 0.01);
}

#[test]
fn clicking_a_kind_button_asks_for_that_kind() {
    let l = imports(4);
    let (x, y) = mid(l.midi_kind);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Kind(FolderKind::Midi));
    let (x, y) = mid(l.score_kind);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Kind(FolderKind::Scores));
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
        assert!(l.midi_kind.is_empty(), "{mode:?} draws a MIDI button");
        assert!(l.score_kind.is_empty(), "{mode:?} draws a Scores button");
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
    let parts: [(&str, Rect); 8] = [
        ("search", l.search),
        ("files", l.files),
        ("status", l.status),
        ("midi kind", l.midi_kind),
        ("score kind", l.score_kind),
        ("open folder", l.open_folder),
        ("choose folder", l.choose_folder),
        ("import tab", l.import_tab),
    ];
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
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Search(BrowserMode::Import));
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
    for tab in [l.sounds_tab, l.projects_tab, l.import_tab, l.settings_tab] {
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
