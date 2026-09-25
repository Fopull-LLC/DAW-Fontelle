//! Working on one song together (`docs/collab-plan.md`).
//!
//! Phase 0 is here: what the session does with an edit from somebody else,
//! the project's identity as the session saves it, the edits that used to
//! skip a command, and finding a song on disk by its id. Phase 1 grows this
//! into two sessions in one process (§12.2).

mod common;

use std::path::{Path, PathBuf};

use fontelle_model::{Command, RenameLane};
use fontelle_types::ParamAddress;
use fontelle_ui::document::{DocumentHost, StudioHost};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-collab-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_session() -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(4))
}

/// F12. A joiner's copy has to ask to be saved when somebody else changes the
/// song, or it is closed believing it is on disk.
#[test]
fn a_foreign_edit_marks_the_document_dirty() {
    let mut session = a_session();
    assert!(!session.is_dirty());
    let revision = session.revision();
    let lane = session.project().lane_ids()[0];

    let mut elsewhere = session.project().clone();
    let mut theirs = RenameLane::new(lane, "Theirs");
    theirs.apply(&mut elsewhere).unwrap();
    session
        .apply_foreign(theirs.to_edit())
        .expect("their edit applies here");

    assert_eq!(session.project().lanes[lane].name, "Theirs");
    assert!(
        session.is_dirty(),
        "somebody else's edit is unsaved work too"
    );
    assert_ne!(session.revision(), revision, "and the window re-reads");
    assert_eq!(
        session.project().sync_hash(),
        elsewhere.sync_hash(),
        "the two copies are the same song"
    );
}

/// F12's other half: an edit that does not apply here changes nothing and
/// says so.
#[test]
fn a_foreign_edit_that_does_not_apply_is_refused_and_changes_nothing() {
    let mut session = a_session();
    let before = session.project().sync_hash();
    let mut elsewhere = session.project().clone();
    let mut spare = fontelle_model::AddLane::new("Spare");
    spare.apply(&mut elsewhere).unwrap();
    let mut theirs = fontelle_model::RemoveLane::new(spare.id().unwrap());
    theirs.apply(&mut elsewhere).unwrap();

    assert!(
        session.apply_foreign(theirs.to_edit()).is_err(),
        "that row was never here"
    );
    assert_eq!(session.project().sync_hash(), before);
    assert!(!session.is_dirty());
}

/// F5. Making automation made its row off the undo stack, so taking the
/// automation back left an empty row behind — and nobody sharing the song
/// ever heard of the row.
#[test]
fn making_automation_is_one_undo_and_takes_its_row_with_it() {
    let mut session = a_session();
    let rows = session.lanes().len();
    let address = ParamAddress::new("master/gain");
    session.create_automation(&address, "Master \u{2014} gain", 0);
    assert_eq!(session.lanes().len(), rows + 1, "a row of its own");

    session.undo();
    assert_eq!(
        session.lanes().len(),
        rows,
        "one undo takes the clip and the row it was made on"
    );
}

/// F7 and §15 decision 2. Saving names the project after its folder without
/// an undo entry; a Save As of a saved song is a new song that remembers its
/// parent; every save stamps the revision.
#[test]
fn saving_stamps_the_song_and_save_as_forks_it() {
    let dir = scratch("stamps");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session();
    session.set_projects_dir(Some(projects.clone()));
    let born = session.project().meta.id;

    session.save_as("First").expect("saves");
    assert_eq!(session.name(), "First");
    assert_eq!(
        session.project().meta.id,
        born,
        "the first save of a new song is that song, not a fork of it"
    );
    assert_eq!(session.project().meta.saved_revision, 1);
    assert!(!session.project().meta.saved_by.is_empty());

    session.undo();
    assert_eq!(
        session.name(),
        "First",
        "the name follows the folder, not the undo stack"
    );

    session.save().expect("saves again");
    assert_eq!(session.project().meta.saved_revision, 2);
    let on_disk = fontelle_model::peek_meta(session.bundle_path().unwrap()).unwrap();
    assert_eq!(on_disk.saved_revision, 2, "the stamp is what is on disk");
    assert_eq!(on_disk.id, born);

    session.save_as("Second").expect("saves as");
    assert_ne!(session.project().meta.id, born, "a Save As is a new song");
    assert_eq!(session.project().meta.forked_from, Some(born));
    assert_eq!(
        fontelle_model::peek_meta(&projects.join("First.fontelle"))
            .unwrap()
            .id,
        born,
        "and the song it came from is still itself"
    );

    // The autosave is not a save.
    let revision = session.project().meta.saved_revision;
    session.set_tempo(133.0);
    session.autosave();
    assert_eq!(session.project().meta.saved_revision, revision);
    std::fs::remove_dir_all(&dir).ok();
}

fn a_bundle_at(path: &Path, id: Option<fontelle_types::PersistentId>) {
    let mut project = fontelle_model::Project::new("song");
    if let Some(id) = id {
        project.meta.id = id;
    }
    fontelle_model::save_project(&project, path).unwrap();
}

/// §4.2 (F19's lookup). A join finds the song by its id, in the projects
/// folder and in its *Shared* folder, whatever the bundles are called.
#[test]
fn a_song_is_found_by_its_id_wherever_it_is_and_whatever_it_is_called() {
    let dir = scratch("find");
    let id = fontelle_types::PersistentId::new();
    a_bundle_at(&dir.join("Mine.fontelle"), Some(id));
    a_bundle_at(&dir.join("Other.fontelle"), None);
    a_bundle_at(&dir.join("Shared").join("Theirs (2).fontelle"), Some(id));
    a_bundle_at(&dir.join("Shared").join("Unrelated.fontelle"), None);
    // Not a bundle: nothing to peek.
    std::fs::create_dir_all(dir.join("Not a song")).unwrap();
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();

    let mut found = fontelle_app::find_by_id(&dir, id);
    found.sort();
    assert_eq!(
        found,
        vec![
            dir.join("Mine.fontelle"),
            dir.join("Shared").join("Theirs (2).fontelle")
        ]
    );
    assert!(fontelle_app::find_by_id(&dir, fontelle_types::PersistentId::new()).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
