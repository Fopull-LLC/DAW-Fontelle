//! The projects folder: making, listing, opening and moving house.
//!
//! Reported from using the window:
//!
//! > *"We also need project management and stuff. I want to be able to select
//! > any folder on my disk as my projects folder where new projects will be
//! > made by default and when browsing my projects in the app it will show me
//! > projects from that folder. [...] I should be able to make new projects
//! > from within the app, browse my projects within the app, set my projects
//! > folder if not already set, change it if it is."*
//!
//! `Settings::projects_dir` has existed since the settings file did and
//! nothing read it. This is what reads it.
//!
//! **INVARIANT 10 is the constraint the whole feature is shaped by:** Fontelle
//! writes nothing outside locations the user has explicitly configured. So
//! there is no default projects folder — `None` means *ask*, never a guess at
//! `~/Documents`, and every test below runs against a scratch folder rather
//! than the developer's own config.

use std::path::{Path, PathBuf};

use fontelle_app::{ProjectEntry, ProjectLibrary};

fn scratch(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("fontelle-projects-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A folder bundle that looks like a project, without building a real one.
fn a_bundle(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.fontelle"));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("project.json"), "{}").unwrap();
    path
}

#[test]
fn with_no_folder_configured_the_library_is_empty_and_says_so() {
    // Not an error, and not a guess. INVARIANT 10: `None` means ask.
    let library = ProjectLibrary::default();
    assert!(library.entries().is_empty());
    assert!(library.dir().is_none());
    assert!(
        !library.status().is_empty(),
        "the panel has to be able to say why it is empty"
    );
}

#[test]
fn scanning_a_folder_finds_the_projects_in_it() {
    let dir = scratch("scan");
    a_bundle(&dir, "Song One");
    a_bundle(&dir, "Song Two");
    // Neither of these is a project, and neither should be listed.
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::write(dir.join("readme.txt"), "hello").unwrap();

    let mut library = ProjectLibrary::default();
    library.set_dir(Some(dir.clone()));

    let names: Vec<&str> = library.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Song One", "Song Two"],
        "sorted, and only projects"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_folder_that_is_not_there_is_reported_rather_than_created() {
    // Creating it would be a write outside anywhere the user configured — the
    // folder they *named* is configured, one that does not exist yet is a
    // typo. The bank folder is the deliberate exception (see `Settings`).
    let dir = scratch("missing").join("nope");
    let mut library = ProjectLibrary::default();
    library.set_dir(Some(dir.clone()));

    assert!(library.entries().is_empty());
    assert!(!dir.exists(), "scanning must not create anything");
    assert!(
        library.status().contains(&dir.display().to_string()),
        "the status has to name the folder it could not read: {}",
        library.status()
    );
}

#[test]
fn an_entry_carries_what_the_panel_draws() {
    let dir = scratch("entry");
    let bundle = a_bundle(&dir, "Piece");

    let mut library = ProjectLibrary::default();
    library.set_dir(Some(dir.clone()));
    let entry: &ProjectEntry = &library.entries()[0];

    assert_eq!(entry.name, "Piece", "the bundle's name without its suffix");
    assert_eq!(entry.path, bundle);
    assert!(
        !entry.modified.is_empty(),
        "when it was last touched — which is how anybody finds the one they were working on"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_newest_project_can_be_found_first() {
    // The list is alphabetical by default because that is how you look
    // something up; sorted by date it is how you get back to what you were
    // doing five minutes ago. Both, and the panel chooses.
    let dir = scratch("order");
    a_bundle(&dir, "Alpha");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let newest = a_bundle(&dir, "Zulu");

    let mut library = ProjectLibrary::default();
    library.set_dir(Some(dir.clone()));
    library.set_order(fontelle_app::ProjectOrder::Recent);

    assert_eq!(library.entries()[0].path, newest);

    library.set_order(fontelle_app::ProjectOrder::Name);
    assert_eq!(library.entries()[0].name, "Alpha");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_new_project_gets_a_name_nothing_else_in_the_folder_has() {
    // Two "Untitled"s in one folder is a mess somebody has to clean up by
    // hand, and silently overwriting the first one is worse.
    let dir = scratch("naming");
    let mut library = ProjectLibrary::default();
    library.set_dir(Some(dir.clone()));

    let first = library
        .new_project_path("Untitled")
        .expect("a folder is set");
    assert_eq!(first.file_name().unwrap(), "Untitled.fontelle");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::write(first.join("project.json"), "{}").unwrap();
    library.rescan();

    let second = library
        .new_project_path("Untitled")
        .expect("a folder is set");
    assert_eq!(second.file_name().unwrap(), "Untitled 2.fontelle");
    assert_ne!(first, second);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_new_project_needs_somewhere_to_go() {
    // The one case that has to be refused rather than guessed at.
    let library = ProjectLibrary::default();
    assert!(library.new_project_path("Untitled").is_none());
}

#[test]
fn moving_house_forgets_the_old_folders_projects() {
    let one = scratch("move-one");
    let two = scratch("move-two");
    a_bundle(&one, "In One");
    a_bundle(&two, "In Two");

    let mut library = ProjectLibrary::default();
    library.set_dir(Some(one.clone()));
    assert_eq!(library.entries()[0].name, "In One");

    library.set_dir(Some(two.clone()));
    assert_eq!(library.entries().len(), 1);
    assert_eq!(library.entries()[0].name, "In Two");

    std::fs::remove_dir_all(&one).ok();
    std::fs::remove_dir_all(&two).ok();
}

#[test]
fn a_project_saved_into_the_folder_shows_up_when_the_list_is_refreshed() {
    // The round trip the window makes: save, rescan, and it is there to open.
    let dir = scratch("roundtrip");
    let mut library = ProjectLibrary::default();
    library.set_dir(Some(dir.clone()));
    assert!(library.entries().is_empty());

    let path = library.new_project_path("First").unwrap();
    let project = fontelle_app::blank_project(8, 120.0, 48_000);
    fontelle_app::save_project(&project, &path).expect("a blank project must save");

    library.rescan();
    assert_eq!(library.entries().len(), 1);
    assert_eq!(library.entries()[0].name, "First");
    // And it opens again.
    let opened = fontelle_app::open_project(&library.entries()[0].path)
        .expect("what we just wrote must open");
    assert_eq!(opened.project.channels.len(), project.channels.len());

    std::fs::remove_dir_all(&dir).ok();
}
