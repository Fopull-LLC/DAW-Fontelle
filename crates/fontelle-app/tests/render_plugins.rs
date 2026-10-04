//! A render with a plugin in it.
//!
//! > *"I find I have trouble rendering midi to audio. most times it just
//! > renders with nothing"*
//!
//! The window's two renders — Export, and a row to audio — built their graph
//! with no plugin host, so a channel playing somebody else's instrument had
//! no node in it and the file was the right length and silent. `--render-wav`
//! had the same mistake and was fixed on its own (`fontelle_app::bounce`);
//! nothing rendered a plugin through the session, so nothing noticed.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::Session;
use fontelle_types::PPQN;
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{
    DocumentHost, ExportOptions, ExportRange, ExportTail, JobPoll, StudioHost,
};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-render-plugins-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

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
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testplug`",
        built.display()
    );
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = std::env::temp_dir().join("fontelle-app-render-plugin-tests");
            let _ = std::fs::create_dir_all(&folder);
            let staging = folder.join(format!("staging.{}.tmp", std::process::id()));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, folder.join("fontelle-testplug.clap"));
            }
            let _ = std::fs::remove_file(&staging);
            folder
        })
        .clone()
}

/// A saved project whose one channel plays the test sine, with a bar-long
/// note on it.
fn a_song_played_by_a_plugin(dir: &Path) -> Session {
    let mut session = common::a_session_in(
        common::a_project_with_a_clip(2, 120.0, SR),
        Some(dir.join("Song.fontelle")),
    )
    .with_plugin_folders(vec![plugin_folder()]);
    session.set_channel_plugin(0, 0);
    session.edit(RollEdit::Add {
        note: fontelle_model::Note {
            start: 0,
            length: PPQN * 4,
            key: 69,
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
    session
}

fn peak_of(path: &Path) -> f32 {
    let asset = fontelle_assets::import_audio(path).expect("the render reads back");
    asset.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn renders(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir.join("Song.fontelle").join("renders"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// The one render there is.
fn the_render(dir: &Path) -> PathBuf {
    let mut found = renders(dir);
    assert_eq!(found.len(), 1, "{found:?}");
    found.pop().unwrap()
}

#[test]
fn an_export_has_the_plugin_in_it() {
    let dir = scratch("export");
    let mut session = a_song_played_by_a_plugin(&dir);
    session.export_wav().expect("exports");
    let peak = peak_of(&the_render(&dir));
    assert!(
        peak > 0.05,
        "the plugin's channel exported as silence: {peak}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_row_rendered_to_audio_has_the_plugin_in_it() {
    let dir = scratch("row");
    let mut session = a_song_played_by_a_plugin(&dir);
    session.render_lane(0, None).expect("renders");
    let peak = peak_of(&the_render(&dir));
    assert!(peak > 0.05, "the plugin's row rendered as silence: {peak}");
    std::fs::remove_dir_all(&dir).ok();
}

/// The render the window starts: on a thread of its own, polled.
#[test]
fn a_render_in_the_background_has_the_plugin_in_it() {
    let dir = scratch("job");
    let mut session = a_song_played_by_a_plugin(&dir);
    StudioHost::export_wav_with(
        &mut session,
        ExportOptions {
            range: ExportRange::WholeSong,
            tail: ExportTail::Keep,
        },
    )
    .expect("starts");
    let started = std::time::Instant::now();
    loop {
        match Session::poll_job(&mut session) {
            JobPoll::Running(_) => {}
            JobPoll::Idle => panic!("the job vanished"),
            _ => break,
        }
        assert!(started.elapsed().as_secs() < 30, "the render never ended");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let peak = peak_of(&the_render(&dir));
    assert!(
        peak > 0.05,
        "the plugin's channel exported as silence: {peak}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// What is rendered is the patch that is **playing**, not the one the last
/// save wrote down: a render gets instances of its own, and they are set up
/// from the document.
#[test]
fn a_render_plays_what_the_plugins_window_was_last_set_to() {
    let dir = scratch("patch");
    let mut session = a_song_played_by_a_plugin(&dir);
    session.export_wav().expect("exports");
    let before = peak_of(&the_render(&dir));

    // Its own window turns its level down; the document hears nothing.
    let slot = fontelle_app::plugin_slots(session.project())[0];
    session
        .plugin_rack_mut()
        .plugin_mut(slot)
        .unwrap()
        .set_param(7, 0.05);
    let first = renders(&dir);
    session.export_wav().expect("exports again");
    let second = renders(&dir)
        .into_iter()
        .find(|path| !first.contains(path))
        .expect("a second file was written");
    let after = peak_of(&second);
    assert!(
        after < before * 0.5,
        "the render used the patch from before: {before} then {after}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// And the studio's own instance is left as it was: still open, still the
/// one the graph plays.
#[test]
fn a_render_leaves_the_studios_plugin_open() {
    let dir = scratch("left");
    let mut session = a_song_played_by_a_plugin(&dir);
    let slot = fontelle_app::plugin_slots(session.project())[0];
    session.export_wav().expect("exports");
    assert!(session.plugin_rack_mut().plugin_mut(slot).is_some());
    session.export_wav().expect("and a second time");
    std::fs::remove_dir_all(&dir).ok();
}
