//! The files a plugin loaded, travelling with the song.
//!
//! > `docs/plugin-experience-backlog.md` §9: a sampler (LSP, sfizz,
//! > drumkv1) keeps the path of the file it loaded in its saved state, so a
//! > song moved to another computer, or sent to a friend, opened with the
//! > sampler empty.
//!
//! An LV2 plugin's state says which of its values are paths (`atom:Path`),
//! so the studio can do for them what it does for its own samples: a path
//! inside the song's folder is saved relative to it, so the song moves
//! whole; *Collect* copies one from outside into `assets/plugin-files/`;
//! and one that is not there when the song opens is named. CLAP and VST 3
//! states are the plugin's own bytes and are left as they are.

mod common;

use std::path::{Path, PathBuf};

use fontelle_host::{Lv2Property, Lv2State};
use fontelle_types::{PluginFormat, PluginKey, PluginState};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

const ATOM_PATH: &str = "http://lv2plug.in/ns/ext/atom#Path";
const ATOM_INT: &str = "http://lv2plug.in/ns/ext/atom#Int";

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-plugin-files-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A sampler's state: the file it loaded, and a number beside it.
fn a_sampler_state(file: &Path) -> PluginState {
    let mut value = file.to_string_lossy().into_owned().into_bytes();
    value.push(0);
    let state = Lv2State {
        properties: vec![
            Lv2Property {
                key: "urn:sampler#file".into(),
                type_uri: ATOM_PATH.into(),
                flags: 0,
                value,
            },
            Lv2Property {
                key: "urn:sampler#voices".into(),
                type_uri: ATOM_INT.into(),
                flags: 0,
                value: 8i32.to_ne_bytes().to_vec(),
            },
        ],
    };
    let mut plugin = PluginState::new(
        PluginKey::new(PluginFormat::Lv2, "urn:a-sampler-not-installed"),
        "Sampler",
    );
    plugin.blob = Some(fontelle_types::encode_base64(&state.encode()));
    plugin
}

/// The file a state names, as it is kept.
fn file_in(state: &PluginState) -> PathBuf {
    let bytes = fontelle_types::decode_base64(state.blob.as_deref().unwrap()).unwrap();
    let decoded = Lv2State::decode(&bytes).unwrap();
    let path = decoded
        .properties
        .iter()
        .find(|p| p.type_uri == ATOM_PATH)
        .unwrap();
    PathBuf::from(
        String::from_utf8(
            path.value
                .strip_suffix(&[0])
                .unwrap_or(&path.value)
                .to_vec(),
        )
        .unwrap(),
    )
}

fn a_song_with(dir: &Path, file: &Path) -> (fontelle_model::Project, PathBuf) {
    let mut project = common::a_project_with_a_clip(2, 120.0, SR);
    let channel = project.channels.keys().next().unwrap();
    project.channels[channel].instrument = Some(fontelle_types::InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(a_sampler_state(file));
    (project, dir.join("Song.fontelle"))
}

fn the_state(project: &fontelle_model::Project) -> &PluginState {
    let channel = project.channels.keys().next().unwrap();
    project.channels[channel].plugin.as_ref().unwrap()
}

#[test]
fn a_file_inside_the_song_is_saved_relative_and_found_after_the_song_moves() {
    let dir = scratch("moves");
    let bundle = dir.join("Song.fontelle");
    std::fs::create_dir_all(bundle.join("samples")).unwrap();
    let file = bundle.join("samples").join("kick.wav");
    std::fs::write(&file, b"RIFF").unwrap();
    let (project, _) = a_song_with(&dir, &file);
    fontelle_app::save_project(&project, &bundle).unwrap();

    // On disk, relative to the song.
    let raw = fontelle_model::load_project(&bundle).unwrap();
    assert_eq!(file_in(the_state(&raw)), PathBuf::from("samples/kick.wav"));

    // And the song moved somewhere else opens with the file where it is now.
    let moved = dir.join("elsewhere").join("Song.fontelle");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&bundle, &moved).unwrap();
    let opened = fontelle_app::open_project(&moved).unwrap();
    assert_eq!(
        file_in(the_state(&opened.project)),
        moved.join("samples").join("kick.wav")
    );
    assert!(opened.missing_plugin_files.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_outside_the_song_is_kept_where_it_is_until_the_song_is_collected() {
    let dir = scratch("collect");
    let outside = dir.join("library").join("snare.wav");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, b"RIFF....snare").unwrap();
    let (project, bundle) = a_song_with(&dir, &outside);
    fontelle_app::save_project(&project, &bundle).unwrap();
    let raw = fontelle_model::load_project(&bundle).unwrap();
    assert_eq!(file_in(the_state(&raw)), outside, "referenced, not copied");

    let mut session = common::a_session_in(project, Some(bundle.clone()));
    assert!(
        session.collect_assets().expect("collects"),
        "something moved"
    );
    let collected = file_in(the_state(session.project()));
    assert!(
        collected.starts_with(bundle.join("assets").join("plugin-files")),
        "{}",
        collected.display()
    );
    assert_eq!(std::fs::read(&collected).unwrap(), b"RIFF....snare");
    session.save().expect("saves");
    let raw = fontelle_model::load_project(&bundle).unwrap();
    assert!(
        file_in(the_state(&raw)).is_relative(),
        "and saved relative, so the copy travels with the song"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_a_plugin_needs_that_is_not_there_is_named_when_the_song_opens() {
    let dir = scratch("missing");
    let gone = dir.join("nowhere").join("pad.sfz");
    let (project, bundle) = a_song_with(&dir, &gone);
    fontelle_app::save_project(&project, &bundle).unwrap();
    let opened = fontelle_app::open_project(&bundle).unwrap();
    assert_eq!(opened.missing_plugin_files, vec![gone.clone()]);

    let mut session = common::a_session_for(common::a_project_with_a_clip(2, 120.0, SR));
    session.open_project_path(&bundle).expect("opens");
    let said = session.take_message().unwrap_or_default();
    assert!(said.contains("pad.sfz"), "{said:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A CLAP or VST 3 state is the plugin's own bytes: nothing in it is
/// touched, whatever it happens to contain.
#[test]
fn another_formats_state_is_left_as_it_is() {
    let dir = scratch("clap");
    let bundle = dir.join("Song.fontelle");
    let mut project = common::a_project_with_a_clip(2, 120.0, SR);
    let channel = project.channels.keys().next().unwrap();
    let mut state = a_sampler_state(&bundle.join("x.wav"));
    state.key = PluginKey::new(PluginFormat::Clap, "com.example.sampler");
    let blob = state.blob.clone();
    project.channels[channel].plugin = Some(state);
    fontelle_app::save_project(&project, &bundle).unwrap();
    let raw = fontelle_model::load_project(&bundle).unwrap();
    assert_eq!(the_state(&raw).blob, blob);
    std::fs::remove_dir_all(&dir).ok();
}
