//! Naming a project — the one thing that stood between an open window and a
//! file on disk (TDD §17.1, §17.3).
//!
//! Reported from using the window:
//!
//! > *"when i make a new project i need to be prompted to name it and also
//! > when im not in a project yet, i currently cant save that blank no project
//! > into a new project ... if i try to save and theirs no project directory
//! > it can just make a new one ... giving you the option to name it and
//! > stuff."*
//!
//! **INVARIANT 10 is what shapes this.** A name is not a path: the project
//! goes into the projects folder the user configured, and with no folder
//! configured there is nowhere to put it and the answer is a sentence rather
//! than a guess at `~/Documents`.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{DocumentHost, StudioHost};

const SR: u32 = 48_000;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-naming-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn a_session_without_a_bundle(dir: &Path) -> Session {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised =
        fontelle_app::realise(&project, &library, options).expect("an empty project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

// -------------------------------------------------------- saving what is open

/// A studio started with no arguments has a document and no file. Saving it
/// under a name makes the file, in the projects folder, and the studio is in
/// that project from then on.
#[test]
fn an_unsaved_studio_can_be_saved_into_a_new_project_by_name() {
    let dir = scratch("save-as");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));
    assert!(!session.has_file(), "the studio started unsaved");

    session.save_as("First Song").expect("it should save");

    let bundle = session.bundle_path().expect("saving made the file");
    assert_eq!(bundle, projects.join("First Song.fontelle"));
    assert!(bundle.join("project.json").is_file());
    assert_eq!(session.name(), "First Song");
    assert!(!session.is_dirty());
    assert!(session.has_file());
    std::fs::remove_dir_all(&dir).ok();
}

/// And once it has one, saving again writes the same file rather than asking
/// for a name a second time.
#[test]
fn a_saved_studio_saves_where_it_already_lives() {
    let dir = scratch("save-again");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));
    session.save_as("Only Once").unwrap();
    let bundle = session.bundle_path().unwrap().to_path_buf();

    session.save().expect("the second save needs no name");
    assert_eq!(session.bundle_path(), Some(bundle.as_path()));
    assert_eq!(std::fs::read_dir(&projects).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// A name already taken is not overwritten. Two `Untitled`s in one folder is a
/// mess somebody cleans up by hand; silently writing over the first is worse.
#[test]
fn a_name_already_in_the_folder_is_made_unique_rather_than_overwritten() {
    let dir = scratch("unique");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut first = a_session_without_a_bundle(&dir);
    first.set_projects_dir(Some(projects.clone()));
    first.save_as("Song").unwrap();

    let mut second = a_session_without_a_bundle(&dir);
    second.set_projects_dir(Some(projects.clone()));
    second.save_as("Song").unwrap();

    let bundle = second.bundle_path().expect("saved");
    assert_ne!(bundle, projects.join("Song.fontelle"));
    assert!(
        projects
            .join("Song.fontelle")
            .join("project.json")
            .is_file()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// With no projects folder there is nowhere the user has named, and the answer
/// is a sentence that says which folder to choose — not a guess (INVARIANT 10).
#[test]
fn saving_by_name_with_no_projects_folder_says_where_to_choose_one() {
    let dir = scratch("nowhere");
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(None);
    let said = session
        .save_as("Anything")
        .expect_err("there is nowhere to put it");
    assert!(said.contains("projects"), "{said}");
    assert!(!session.has_file());
    std::fs::remove_dir_all(&dir).ok();
}

/// An empty name is not a name. It becomes the same "Untitled" the rest of the
/// program uses rather than a file called `.fontelle`.
#[test]
fn a_blank_name_becomes_untitled_rather_than_a_nameless_folder() {
    let dir = scratch("blank");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));
    session.save_as("   ").expect("it still saves");
    let bundle = session.bundle_path().expect("saved");
    assert_eq!(bundle.file_stem().unwrap(), "Untitled");
    std::fs::remove_dir_all(&dir).ok();
}

/// A name with a separator in it is a name, not a path. INVARIANT 10 again:
/// "../../etc/passwd" typed into a name box must not reach outside the folder
/// the user chose.
#[test]
fn a_name_that_looks_like_a_path_stays_inside_the_projects_folder() {
    let dir = scratch("path");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));
    session.save_as("../outside/Song").expect("it still saves");
    let bundle = session.bundle_path().expect("saved");
    assert_eq!(bundle.parent(), Some(projects.as_path()), "{bundle:?}");
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------ making a new project

/// *New* takes the name it was given rather than always making "Untitled".
#[test]
fn a_new_project_is_made_under_the_name_it_was_given() {
    let dir = scratch("new-named");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));

    session
        .new_project_named("Second Song")
        .expect("it should make one");

    let bundle = session.bundle_path().expect("a new project has a file");
    assert_eq!(bundle, projects.join("Second Song.fontelle"));
    assert_eq!(session.name(), "Second Song");
    assert!(!session.is_dirty());
    std::fs::remove_dir_all(&dir).ok();
}

/// The old, nameless call still works and still means "Untitled" — nothing
/// that used it has to change.
#[test]
fn a_new_project_with_no_name_is_still_untitled() {
    let dir = scratch("new-untitled");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));
    session.new_project().expect("it should make one");
    let bundle = session.bundle_path().expect("a new project has a file");
    assert_eq!(bundle.file_stem().unwrap(), "Untitled");
    std::fs::remove_dir_all(&dir).ok();
}
