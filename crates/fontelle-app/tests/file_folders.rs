//! Where Fontelle looks for the files you import (TDD §17.3, INVARIANT 10).
//!
//! Two folders, both the user's own choice and neither guessed at: one for
//! `.mid` files and one for FL Studio's `.fsc` scores — which on most machines
//! is wherever FL keeps its own, and on this one is somebody else's install
//! directory. Fontelle cannot know either, and INVARIANT 10 says it must not
//! invent them: *"Fontelle writes nothing outside locations the user has
//! explicitly configured"*, and reading somebody's whole Documents folder
//! uninvited is the same kind of liberty.
//!
//! So the rule the window follows is: **no folder, no browser — go and set
//! one.** These are the settings half of that.

use std::path::PathBuf;

use fontelle_app::settings::{
    FolderKind, SETTING_ROWS, SETTINGS_FORMAT_VERSION, Settings, SettingRow,
};

fn temp_dir(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "fontelle-folders-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("make a temp folder");
    path
}

// ------------------------------------------------------------ the fields ---

#[test]
fn a_fresh_install_has_neither_folder_set() {
    // Not `~/Documents`, not `~/Music`, not FL Studio's own directory —
    // INVARIANT 10. `None` is "ask", and asking is the whole design.
    let settings = Settings::default();
    assert_eq!(settings.midi_dir, None);
    assert_eq!(settings.score_dir, None);
}

#[test]
fn both_folders_survive_a_trip_through_the_settings_file() {
    let settings = Settings {
        midi_dir: Some(PathBuf::from("/home/someone/MIDI")),
        score_dir: Some(PathBuf::from("/home/someone/Scores")),
        ..Settings::default()
    };

    let text = settings.to_json();
    let read = Settings::from_json(&text).expect("what we wrote, we can read");
    assert_eq!(read.midi_dir, settings.midi_dir);
    assert_eq!(read.score_dir, settings.score_dir);
}

#[test]
fn a_settings_file_written_before_these_folders_existed_still_opens() {
    // The compatibility rule this file has followed since it grew a second
    // section: a file from an older build is not a broken one.
    let older = r#"{
        "format_version": 2,
        "soundfont_dirs": [],
        "projects_dir": null,
        "theme": null
    }"#;
    let settings = Settings::from_json(older).expect("an older file still reads");
    assert_eq!(settings.midi_dir, None);
    assert_eq!(settings.score_dir, None);
}

#[test]
fn the_format_version_moved_because_the_file_grew() {
    // So that an *older* build handed one of these says "upgrade Fontelle"
    // rather than "unknown field midi_dir".
    const { assert!(SETTINGS_FORMAT_VERSION >= 3) };
    let text = Settings::default().to_json();
    assert!(text.contains(&format!("\"format_version\": {SETTINGS_FORMAT_VERSION}")));
}

#[test]
fn a_file_from_a_newer_build_is_still_refused_by_version() {
    let text = format!(
        "{{\"format_version\": {}, \"soundfont_dirs\": []}}",
        SETTINGS_FORMAT_VERSION + 1
    );
    assert!(Settings::from_json(&text).is_err());
}

// -------------------------------------------------------------- the rows ---

#[test]
fn the_settings_tab_has_a_row_for_each_folder() {
    let kinds: Vec<FolderKind> = SETTING_ROWS.iter().filter_map(|row| row.folder()).collect();
    assert!(kinds.contains(&FolderKind::Midi), "no MIDI folder row");
    assert!(kinds.contains(&FolderKind::Scores), "no score folder row");
}

#[test]
fn a_folder_row_that_is_not_set_says_so_rather_than_being_blank() {
    // An empty right-hand column reads as a bug. It has to say that there is
    // nothing there, because "nothing there" is the state the whole feature
    // starts in.
    let settings = Settings::default();
    for row in SETTING_ROWS {
        let Some(_) = row.folder() else { continue };
        let value = row.value(&settings);
        assert!(!value.is_empty(), "{row:?} draws an empty value");
        assert!(
            value.to_lowercase().contains("not set") || value.contains("choose"),
            "{row:?} says {value:?}, which does not say it is unset"
        );
    }
}

#[test]
fn a_folder_row_that_is_set_shows_the_end_of_the_path() {
    // The end, not the beginning: `…/Image-Line/FL Studio/Scores` says where
    // you are and `/home/someone/Documents/Ima…` says nothing at all. Same
    // rule the bank's own status line follows.
    let settings = Settings {
        midi_dir: Some(PathBuf::from("/home/someone/Documents/Music/MIDI Files")),
        ..Settings::default()
    };
    let row = SETTING_ROWS
        .iter()
        .find(|row| row.folder() == Some(FolderKind::Midi))
        .expect("there is a MIDI folder row");

    let value = row.value(&settings);
    assert!(value.contains("MIDI Files"), "got {value:?}");
    assert!(!value.contains("/home/someone/Documents/Music"), "got {value:?}");
}

#[test]
fn nudging_a_folder_row_does_nothing_to_the_midi_input_settings() {
    // A folder row is a *button*: clicking it opens a picker, which is the
    // host's job because this crate's settings module may not spawn one. What
    // it must not do is quietly step the row above it.
    let mut settings = Settings::default();
    let before = settings.midi_input;
    for row in SETTING_ROWS {
        if row.folder().is_some() {
            row.nudge(&mut settings.midi_input, 1);
        }
    }
    assert_eq!(settings.midi_input, before);
}

#[test]
fn every_row_still_says_what_it_is_and_what_it_is_at() {
    // The rule the tab has always followed, restated over the longer list: a
    // row with no name cannot be identified and a row with no value cannot be
    // read.
    let settings = Settings::default();
    for row in SETTING_ROWS {
        assert!(!row.label().is_empty(), "{row:?} has no name");
        if matches!(row, SettingRow::Heading(_)) {
            continue;
        }
        assert!(
            !row.value(&settings).is_empty(),
            "{row:?} does not say what it is at"
        );
    }
}

#[test]
fn the_folders_are_under_a_heading_of_their_own() {
    // "MIDI folder" under the MIDI *input* heading would read as a property
    // of the keyboard. The heading is what makes it a place on disk.
    let mut seen_heading = None;
    let mut headings_before_folders = Vec::new();
    for row in SETTING_ROWS {
        if let SettingRow::Heading(title) = row {
            seen_heading = Some(title);
        }
        if row.folder().is_some() {
            headings_before_folders.push(seen_heading);
        }
    }
    assert_eq!(headings_before_folders.len(), 2);
    for heading in &headings_before_folders {
        let heading = heading.expect("a folder row must sit under a heading");
        assert!(
            !heading.to_lowercase().contains("input"),
            "the folders are under {heading:?}, which reads as the keyboard's"
        );
    }
    assert_eq!(
        headings_before_folders[0], headings_before_folders[1],
        "both folders belong under the same heading"
    );
}

// ------------------------------------------------------- what they hold ---

#[test]
fn a_folder_kind_knows_which_files_it_is_for() {
    assert!(FolderKind::Midi.accepts(std::path::Path::new("a/b/Song.mid")));
    assert!(FolderKind::Midi.accepts(std::path::Path::new("a/b/Song.MIDI")));
    assert!(!FolderKind::Midi.accepts(std::path::Path::new("a/b/Song.fsc")));

    assert!(FolderKind::Scores.accepts(std::path::Path::new("a/b/Chords.fsc")));
    assert!(FolderKind::Scores.accepts(std::path::Path::new("a/b/Chords.FSC")));
    assert!(!FolderKind::Scores.accepts(std::path::Path::new("a/b/Chords.mid")));

    // A file with no extension is not one of either.
    assert!(!FolderKind::Midi.accepts(std::path::Path::new("README")));
    assert!(!FolderKind::Scores.accepts(std::path::Path::new("README")));
}

#[test]
fn a_folder_kind_says_what_a_picker_should_call_itself() {
    // Not decoration: every picker in this program was once titled "Soundfont
    // folder" whatever it was asking for, and a wrong answer is much easier
    // to believe when the dialog agrees with it.
    let midi = FolderKind::Midi.picker_title();
    let scores = FolderKind::Scores.picker_title();
    assert_ne!(midi, scores);
    assert!(midi.to_lowercase().contains("midi"), "got {midi:?}");
}

#[test]
fn the_folder_a_kind_reads_is_the_one_it_writes() {
    // One accessor pair rather than a match at every call site, which is how
    // "change my score folder" came to replace somebody's soundfont bank in
    // this program once already.
    let mut settings = Settings::default();
    let dir = temp_dir("round-trip");
    for kind in [FolderKind::Midi, FolderKind::Scores] {
        assert_eq!(settings.folder(kind), None);
        settings.set_folder(kind, Some(dir.clone()));
        assert_eq!(settings.folder(kind), Some(dir.as_path()));
    }
    // And they are genuinely two fields, not one written twice.
    settings.set_folder(FolderKind::Midi, None);
    assert_eq!(settings.folder(FolderKind::Midi), None);
    assert_eq!(settings.folder(FolderKind::Scores), Some(dir.as_path()));
    std::fs::remove_dir_all(&dir).ok();
}
