//! A plugin that plays what is not a number, said and dealt with.
//!
//! > *"most times it just renders with nothing"*
//!
//! v0.23.1 stopped one plugin's NaN at its own node, so the rest of the mix
//! plays on (`fontelle_engine::PluginNode`). That left its channel silent
//! with nothing saying why — and padthv1, once it has played NaN, plays
//! nothing else. So the studio says which plugin it was and opens it again
//! from the song's copy of its state, once; a plugin that does it again at
//! once is left silenced, and that is said too
//! (`docs/plugin-experience-backlog.md` §2).

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use fontelle_app::{PluginSlot, Session};
use fontelle_ui::document::StudioHost;

use common::SR;

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
        std::env::temp_dir().join(format!("fontelle-silenced-plugins-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let staging = folder.join(format!("staging.{:?}.tmp", std::thread::current().id()));
    if std::fs::copy(&built, &staging).is_ok() {
        let _ = std::fs::rename(&staging, folder.join("fontelle-testplug.clap"));
    }
    folder
}

fn a_song_with_a_plugin() -> (Session, PluginSlot) {
    let mut session = common::a_session_for(common::a_project_with_a_clip(2, 120.0, SR))
        .with_plugin_folders(vec![plugin_folder()]);
    session.set_channel_plugin(0, 0);
    let channel = session.project().channels.keys().next().expect("a channel");
    (session, PluginSlot::Channel(channel))
}

#[test]
fn a_plugin_silenced_for_playing_nonsense_is_named_and_opened_again() {
    let (mut session, slot) = a_song_with_a_plugin();
    let _ = session.take_message();
    let before = session.plugin_rack_mut().bay(slot).expect("it is open");

    before.mark_silenced();
    session.tick_plugin_editors();

    let said = session.take_message().unwrap_or_default();
    assert!(
        said.contains("Fontelle Test Sine") && said.contains("invalid audio"),
        "the studio says which plugin: {said:?}"
    );
    let after = session.plugin_rack_mut().bay(slot).expect("open again");
    assert!(!Arc::ptr_eq(&before, &after), "a fresh instance");
}

#[test]
fn a_plugin_that_plays_nonsense_again_at_once_is_left_silenced() {
    let (mut session, slot) = a_song_with_a_plugin();
    session.plugin_rack_mut().bay(slot).unwrap().mark_silenced();
    session.tick_plugin_editors();
    let _ = session.take_message();

    let reopened = session.plugin_rack_mut().bay(slot).unwrap();
    reopened.mark_silenced();
    session.tick_plugin_editors();

    let said = session.take_message().unwrap_or_default();
    assert!(said.contains("keeps playing"), "{said:?}");
    let now = session.plugin_rack_mut().bay(slot).unwrap();
    assert!(Arc::ptr_eq(&reopened, &now), "not opened a third time");
}
