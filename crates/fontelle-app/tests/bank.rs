//! The soundfont bank: the folder you drop your `.sf2` files into, and the
//! index the browser searches (TDD §17.5, and the storage rules in §17.3).
//!
//! Everything here is a pure function or a directory walk, deliberately: the
//! browser panel on top of it is a list of rows and a text field, and none of
//! *this* should need a window to be sure of.

use std::path::{Path, PathBuf};

use fontelle_app::bank::{BankEntry, SoundfontBank, fuzzy_score, is_soundfont, matches};
use fontelle_app::settings::{SETTINGS_FORMAT_VERSION, Settings, SettingsError};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-bank-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch directory must be creatable");
    path
}

fn touch(dir: &Path, name: &str, bytes: usize) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent must be creatable");
    }
    std::fs::write(&path, vec![0u8; bytes]).expect("the file must be writable");
}

// ------------------------------------------------------------ what counts ---

#[test]
fn a_soundfont_is_recognised_by_extension_whatever_its_case() {
    assert!(is_soundfont(Path::new("Piano.sf2")));
    assert!(is_soundfont(Path::new("PIANO.SF2")));
    assert!(is_soundfont(Path::new("/a/b/Weird.Name.sf2")));
    // sf3 is the same format with compressed samples. The importer does not
    // read one yet, and listing files nothing can open would be a lie.
    assert!(!is_soundfont(Path::new("Piano.sf3")));
    assert!(!is_soundfont(Path::new("notes.txt")));
    assert!(!is_soundfont(Path::new("sf2")));
    assert!(!is_soundfont(Path::new("a.sf2.bak")));
}

// ------------------------------------------------------------- the search ---

#[test]
fn fuzzy_matching_is_a_case_insensitive_subsequence() {
    assert!(fuzzy_score("mar", "Marimba").is_some());
    assert!(fuzzy_score("MAR", "marimba").is_some());
    // Not a substring — the letters just have to arrive in order, which is what
    // makes "gus" find "GeneralUser GS".
    assert!(fuzzy_score("gus", "GeneralUser GS").is_some());
    assert!(fuzzy_score("xyz", "GeneralUser GS").is_none());
    // Everything matches nothing.
    assert_eq!(fuzzy_score("", "anything"), Some(0));
}

#[test]
fn a_tighter_match_scores_higher() {
    let tight = fuzzy_score("piano", "Piano").expect("matches");
    let scattered = fuzzy_score("piano", "Pizzicato And Others").expect("matches");
    assert!(
        tight > scattered,
        "a run of consecutive letters must beat letters scattered across a name \
         — otherwise the list is ordered by accident ({tight} vs {scattered})"
    );

    let at_start = fuzzy_score("bass", "Bass Guitar").expect("matches");
    let mid_word = fuzzy_score("bass", "Contrabassoon").expect("matches");
    assert!(
        at_start > mid_word,
        "a name that starts with what was typed is the one being looked for \
         ({at_start} vs {mid_word})"
    );
}

#[test]
fn searching_the_bank_orders_by_score_and_keeps_everything_when_empty() {
    let entries = vec![
        BankEntry::for_test("/f/Contrabassoon.sf2", 10),
        BankEntry::for_test("/f/Bass Guitar.sf2", 20),
        BankEntry::for_test("/f/Marimba.sf2", 30),
    ];

    let all = matches(&entries, "");
    assert_eq!(
        all,
        vec![0, 1, 2],
        "an empty query keeps the bank's own order"
    );

    let hits = matches(&entries, "bass");
    assert_eq!(
        hits,
        vec![1, 0],
        "both match; the one that starts with it comes first"
    );

    assert!(matches(&entries, "zzz").is_empty());
}

// -------------------------------------------------------------- the scan ---

#[test]
fn scanning_finds_soundfonts_in_subfolders_and_ignores_everything_else() {
    let dir = scratch("scan");
    touch(&dir, "Piano.sf2", 128);
    touch(&dir, "readme.txt", 4);
    touch(&dir, "Orchestral/Strings.sf2", 64);
    touch(&dir, "Orchestral/notes.md", 4);
    touch(&dir, "Kits/Drums/Acoustic.SF2", 32);

    let mut bank = SoundfontBank::new(vec![dir.clone()]);
    bank.rescan();

    let names: Vec<&str> = bank.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Acoustic", "Piano", "Strings"],
        "sorted by name, extension stripped, and nothing that is not a soundfont"
    );
    assert_eq!(
        bank.entries()[1].size_bytes,
        128,
        "the size is carried because a 325 MB soundfont is a decision the user \
         is entitled to make knowingly"
    );
    assert!(bank.entries()[2].path.ends_with("Orchestral/Strings.sf2"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_bank_folder_is_an_empty_bank_and_not_an_error() {
    let mut bank = SoundfontBank::new(vec![PathBuf::from("/nowhere/at/all/fontelle")]);
    bank.rescan();
    assert!(bank.entries().is_empty());
    // Reported, though: a browser that is empty because the folder is gone must
    // be able to say so rather than looking like an empty collection.
    assert_eq!(bank.unreadable().len(), 1);
}

#[test]
fn the_same_file_reachable_through_two_folders_is_listed_once() {
    let dir = scratch("dupes");
    touch(&dir, "Piano.sf2", 8);

    let mut bank = SoundfontBank::new(vec![dir.clone(), dir.clone()]);
    bank.rescan();
    assert_eq!(bank.entries().len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- settings ---

#[test]
fn settings_round_trip_through_their_file_form() {
    let settings = Settings {
        format_version: SETTINGS_FORMAT_VERSION,
        soundfont_dirs: vec![PathBuf::from("/music/sf2"), PathBuf::from("/more/sf2")],
        projects_dir: Some(PathBuf::from("/music/projects")),
        theme: None,
    };
    let back = Settings::from_json(&settings.to_json()).expect("what we wrote must read back");
    assert_eq!(back.soundfont_dirs, settings.soundfont_dirs);
    assert_eq!(back.projects_dir, settings.projects_dir);
    assert_eq!(back.format_version, SETTINGS_FORMAT_VERSION);
}

#[test]
fn settings_from_a_newer_build_are_refused_by_version_rather_than_by_field() {
    let json = format!(
        r#"{{"format_version": {}, "soundfont_dirs": [], "projects_dir": null, "theme": null}}"#,
        SETTINGS_FORMAT_VERSION + 1
    );
    match Settings::from_json(&json) {
        Err(SettingsError::FromTheFuture { found, newest }) => {
            assert_eq!(found, SETTINGS_FORMAT_VERSION + 1);
            assert_eq!(newest, SETTINGS_FORMAT_VERSION);
        }
        other => panic!("expected a version refusal, got {other:?}"),
    }
}

#[test]
fn settings_without_a_version_stamp_are_not_fontelles() {
    assert!(matches!(
        Settings::from_json(r#"{"soundfont_dirs": []}"#),
        Err(SettingsError::Format(_))
    ));
}

/// INVARIANT 10, as arithmetic on the environment rather than as a promise.
#[test]
fn every_path_fontelle_chooses_for_itself_is_under_the_xdg_dirs() {
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some("/home/someone/.config".to_string()),
        "XDG_DATA_HOME" => Some("/home/someone/.local/share".to_string()),
        "HOME" => Some("/home/someone".to_string()),
        _ => None,
    };
    assert_eq!(
        Settings::config_dir_from(&env),
        Some(PathBuf::from("/home/someone/.config/fontelle"))
    );
    assert_eq!(
        Settings::default_soundfont_dir_from(&env),
        Some(PathBuf::from(
            "/home/someone/.local/share/fontelle/soundfonts"
        ))
    );

    // With XDG unset, the spec's own fallbacks — and still never anything the
    // user did not ask for, like Documents or Music.
    let bare = |key: &str| (key == "HOME").then(|| "/home/someone".to_string());
    assert_eq!(
        Settings::config_dir_from(&bare),
        Some(PathBuf::from("/home/someone/.config/fontelle"))
    );
    assert_eq!(
        Settings::default_soundfont_dir_from(&bare),
        Some(PathBuf::from(
            "/home/someone/.local/share/fontelle/soundfonts"
        ))
    );

    // No home at all is not a crash, and not a guess.
    assert_eq!(Settings::config_dir_from(&|_: &str| None), None);
}

#[test]
fn a_settings_file_that_does_not_exist_yet_loads_as_the_defaults() {
    let dir = scratch("settings");
    let path = dir.join("settings.json");
    let (settings, error) = Settings::load_from(&path);
    assert!(error.is_none(), "a first run is not a failure");
    assert!(settings.soundfont_dirs.is_empty());

    // And what is saved comes back.
    let mut settings = settings;
    settings.soundfont_dirs.push(dir.join("sf2"));
    settings.save_to(&path).expect("must write");
    let (back, error) = Settings::load_from(&path);
    assert!(error.is_none());
    assert_eq!(back.soundfont_dirs, settings.soundfont_dirs);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_damaged_settings_file_is_reported_and_never_silently_overwritten() {
    let dir = scratch("damaged");
    let path = dir.join("settings.json");
    std::fs::write(&path, "{ this is not json").unwrap();

    let (settings, error) = Settings::load_from(&path);
    assert!(
        error.is_some(),
        "a file we could not read has to be said out loud — the alternative is \
         a user's soundfont folders quietly disappearing"
    );
    assert!(settings.soundfont_dirs.is_empty());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{ this is not json",
        "and it is still on disk, so it can be fixed by hand"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Two saves at once must not leave a half-written settings file.
///
/// Found the direct way: the integration tests run in parallel, several of them
/// wrote settings, and what came out the other end was a JSON document with an
/// extra closing brace on it — a temp file with one fixed name, written by two
/// writers, renamed twice.
#[test]
fn concurrent_saves_never_leave_a_damaged_file() {
    let dir = scratch("concurrent");
    let path = dir.join("settings.json");

    std::thread::scope(|scope| {
        for n in 0..8 {
            let path = path.clone();
            scope.spawn(move || {
                let settings = Settings {
                    format_version: SETTINGS_FORMAT_VERSION,
                    soundfont_dirs: vec![PathBuf::from(format!("/sf/{n}"))],
                    projects_dir: None,
                    theme: None,
                };
                for _ in 0..20 {
                    settings.save_to(&path).expect("must write");
                    let (_, error) = Settings::load_from(&path);
                    assert!(
                        error.is_none(),
                        "a concurrent save left a file that cannot be read back: {error:?}"
                    );
                }
            });
        }
    });

    std::fs::remove_dir_all(&dir).ok();
}
