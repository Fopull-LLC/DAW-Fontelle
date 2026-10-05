//! Work a crash would have lost, and the plugin that would crash it again.
//!
//! > *"i want to give people a good experience"* — and the backlog's first
//! > item (`docs/plugin-experience-backlog.md` §1): plugins run in the
//! > studio's process, and when one crashes, an hour on a song nobody had
//! > saved yet was gone, a saved song's backup sat in its `backups` folder
//! > with nothing saying so, and opening the song again opened the plugin
//! > that had just crashed it.
//!
//! Three answers. A song never saved is backed up where Fontelle keeps its
//! own files. After a run that did not end cleanly, a backup that holds
//! more than the song on disk is offered, and opens **as that song**,
//! unsaved, so Save puts it where it belongs. And the plugin the crash
//! report names is held back the next time that song opens — kept in the
//! document, not run — once.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::crashlog::{self, LastRun, Marker, Previous, Recovery};
use fontelle_app::{PluginSlot, Session};
use fontelle_types::PPQN;
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-recovery-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A song saved at `dir/Song.fontelle`, open.
fn saved_session(dir: &Path) -> (Session, PathBuf) {
    let bundle = dir.join("Song.fontelle");
    let project = common::a_project_with_a_clip(2, 120.0, SR);
    fontelle_app::save_project(&project, &bundle).expect("a blank project saves");
    (common::a_session_in(project, Some(bundle.clone())), bundle)
}

/// A session with nothing open yet — what the studio is on its start menu.
fn a_fresh_session() -> Session {
    common::a_session_in(common::a_project_with_a_clip(2, 120.0, SR), None)
}

fn a_note(session: &mut Session) {
    session.edit(RollEdit::Add {
        note: fontelle_model::Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
        },
    });
}

fn notes_on_disk(bundle: &Path) -> usize {
    fontelle_app::open_project(bundle)
        .expect("it opens")
        .project
        .clips
        .values()
        .filter_map(|c| match &c.source {
            fontelle_model::ClipSource::Notes(d) => Some(d.notes.len()),
            _ => None,
        })
        .sum()
}

/// What the run before this one left, as `crashlog::begin_run` reads it.
fn a_crash_in(path: Option<&Path>, backup: Option<&Path>) -> Previous {
    let mut marker = Marker::here(Some("Song"));
    marker.path = path.map(Path::to_path_buf);
    marker.backup = backup.map(Path::to_path_buf);
    Previous {
        verdict: LastRun::Killed { pid: marker.pid },
        marker: Some(marker),
        culprit: None,
    }
}

// ------------------------------------------------------------- the marker

/// Where the song lives and where an untitled one was backed up, on lines
/// of their own after the one an older Fontelle reads — so a path with
/// spaces is read whole, and an older build still reads its line.
#[test]
fn the_marker_says_where_the_song_and_its_backup_are() {
    let mut marker = Marker::here(Some("My Song"));
    marker.path = Some(PathBuf::from("/home/k/Music/My Song.fontelle"));
    marker.backup = Some(PathBuf::from(
        "/home/k/.local/share/fontelle/recovery/Untitled.fontelle",
    ));
    let text = marker.text();
    assert_eq!(Marker::parse(&text), Some(marker.clone()));
    assert_eq!(
        text.lines().next(),
        Some(marker.line().as_str()),
        "the first line is what it always was"
    );

    let old = Marker::here(Some("Old")).line();
    let read = Marker::parse(&old).expect("an older marker still reads");
    assert_eq!((read.path, read.backup), (None, None));
}

// ---------------------------------------------------------- untitled work

#[test]
fn a_song_never_saved_is_backed_up_where_fontelle_keeps_its_own_files() {
    let dir = scratch("untitled");
    let backup = dir.join("recovery").join("Untitled.fontelle");
    let mut session = a_fresh_session().with_untitled_backup(backup.clone());
    a_note(&mut session);

    assert!(session.autosave(), "there was something to keep");
    assert_eq!(notes_on_disk(&backup), 1, "and it is in the backup");
    assert!(session.is_dirty(), "a backup is not a save");
    assert_eq!(
        session.bundle_path(),
        None,
        "and the song is still untitled"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ----------------------------------------------------- what is offered

#[test]
fn after_a_crash_a_backup_holding_more_than_the_song_is_offered() {
    let dir = scratch("offered");
    let (mut session, bundle) = saved_session(&dir);
    a_note(&mut session);
    assert!(session.autosave());
    let backup = bundle.join("backups").join("autosave.fontelle");

    let offer = crashlog::recovery(&a_crash_in(Some(&bundle), None)).expect("it is offered");
    assert_eq!(offer.backup, backup);
    assert_eq!(offer.home.as_deref(), Some(bundle.as_path()));
    assert_eq!(offer.name, "Song");

    // Not after a clean exit: the backup is old news then.
    let mut clean = a_crash_in(Some(&bundle), None);
    clean.verdict = LastRun::Clean;
    assert_eq!(crashlog::recovery(&clean), None);

    // Nor once the song itself has been saved since.
    std::thread::sleep(std::time::Duration::from_millis(20));
    session.save().expect("it saves");
    assert_eq!(crashlog::recovery(&a_crash_in(Some(&bundle), None)), None);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_untitled_backup_is_offered_after_a_crash() {
    let dir = scratch("untitled-offer");
    let backup = dir.join("recovery").join("Untitled.fontelle");
    let mut session = a_fresh_session().with_untitled_backup(backup.clone());
    a_note(&mut session);
    assert!(session.autosave());

    let offer = crashlog::recovery(&a_crash_in(None, Some(&backup))).expect("it is offered");
    assert_eq!((offer.backup, offer.home), (backup, None));
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- recovering

#[test]
fn a_recovered_song_opens_as_itself_unsaved_and_saves_where_it_belongs() {
    let dir = scratch("recover");
    let (mut session, bundle) = saved_session(&dir);
    a_note(&mut session);
    assert!(session.autosave());
    drop(session);
    assert_eq!(notes_on_disk(&bundle), 0, "the note was never saved");

    let offer = crashlog::recovery(&a_crash_in(Some(&bundle), None)).unwrap();
    let mut session = a_fresh_session().with_recovery(offer);
    let label = session.recovery_offer().expect("the start menu offers it");
    assert!(label.contains("Song"), "{label}");

    session.recover().expect("it recovers");
    assert_eq!(session.notes().len(), 1, "the unsaved note is back");
    assert!(session.is_dirty(), "and unsaved, until somebody says so");
    assert_eq!(session.bundle_path(), Some(bundle.as_path()));
    assert_eq!(session.recovery_offer(), None, "and offered once");

    session.save().expect("it saves");
    assert_eq!(notes_on_disk(&bundle), 1, "into the song, not its backup");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_recovered_untitled_song_opens_untitled() {
    let dir = scratch("recover-untitled");
    let backup = dir.join("recovery").join("Untitled.fontelle");
    let mut session = a_fresh_session().with_untitled_backup(backup.clone());
    a_note(&mut session);
    assert!(session.autosave());
    drop(session);

    let mut session = a_fresh_session().with_recovery(Recovery {
        backup,
        home: None,
        name: "Untitled".into(),
    });
    session.recover().expect("it recovers");
    assert_eq!(session.notes().len(), 1);
    assert!(session.is_dirty());
    assert_eq!(session.bundle_path(), None);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------- the crash guard

/// The test bundle, alone in a folder — see `tests/plugin_ui.rs`.
fn plugin_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(if cfg!(target_os = "windows") {
        "fontelle_testplug.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    });
    let folder =
        std::env::temp_dir().join(format!("fontelle-recovery-plugins-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let staging = folder.join(format!("staging.{:?}.tmp", std::thread::current().id()));
    if std::fs::copy(&built, &staging).is_ok() {
        let _ = std::fs::rename(&staging, folder.join("fontelle-testplug.clap"));
    }
    folder
}

#[test]
fn the_plugin_that_crashed_the_studio_is_held_back_once_when_its_song_opens_again() {
    let dir = scratch("held-back");
    let (session, bundle) = saved_session(&dir);
    let mut session = session.with_plugin_folders(vec![plugin_folder()]);
    session.set_channel_plugin(0, 0);
    session.save().expect("it saves");
    let channel = session.project().channels.keys().next().expect("a channel");
    let key = session.project().channels[channel]
        .plugin
        .as_ref()
        .expect("the channel plays a plugin")
        .key
        .clone();
    drop(session);

    let mut session = a_fresh_session()
        .with_plugin_folders(vec![plugin_folder()])
        .with_held_back(Some(bundle.clone()), key.clone());
    session.open_project_path(&bundle).expect("it opens");
    let slot = PluginSlot::Channel(channel);
    assert!(
        session.plugin_rack_mut().snapshot(slot).is_none(),
        "the plugin is not run"
    );
    let said = session.take_message().unwrap_or_default();
    assert!(
        said.contains("crashed"),
        "and the studio says why: {said:?}"
    );
    assert!(
        session.project().channels[channel].plugin.is_some(),
        "but it is still the song's"
    );
    session.save().expect("it saves");
    let kept = fontelle_app::open_project(&bundle).unwrap().project;
    assert!(
        kept.channels[channel]
            .plugin
            .as_ref()
            .is_some_and(|p| p.key == key),
        "and a save keeps it"
    );

    // Once: opening the song again tries the plugin again.
    session.open_project_path(&bundle).expect("it opens");
    assert!(session.plugin_rack_mut().snapshot(slot).is_some());
    std::fs::remove_dir_all(&dir).ok();
}
