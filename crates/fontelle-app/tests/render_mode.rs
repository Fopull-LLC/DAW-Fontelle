//! A render tells a CLAP plugin it is one.
//!
//! LV2 plugins are told through their free-wheeling port (v0.23.1). CLAP
//! has the `render` extension for the same thing: a plugin set to offline
//! may use its slower, better algorithms — higher oversampling, a longer
//! reverb tail computed whole — that it cannot afford in real time. A
//! render that never said so got the real-time sound
//! (`docs/plugin-experience-backlog.md` §11). Its own test binary, because
//! the fixture is told where to log through the environment.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::Session;
use fontelle_types::PPQN;
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

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
    let folder = std::env::temp_dir().join(format!("fontelle-render-mode-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    std::fs::copy(&built, folder.join("fontelle-testplug.clap")).expect("the test plugin copies");
    folder
}

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

#[test]
fn an_export_tells_a_clap_plugin_it_is_offline_and_then_that_it_is_not() {
    let dir =
        std::env::temp_dir().join(format!("fontelle-render-mode-song-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("render.log");
    // SAFETY: the only test in this binary; read only by the fixture.
    unsafe { std::env::set_var(fontelle_testplug::RENDER_LOG_ENV, &log) };

    let mut session = a_song_played_by_a_plugin(&dir);
    session.export_wav().expect("exports");

    let said = std::fs::read_to_string(&log).unwrap_or_default();
    let said: Vec<&str> = said.lines().collect();
    assert_eq!(said, ["offline", "realtime"], "what the plugin was told");
    std::fs::remove_dir_all(&dir).ok();
}
