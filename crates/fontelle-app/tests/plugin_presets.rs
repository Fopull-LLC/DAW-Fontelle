//! A plugin somebody else wrote, in the preset system.
//!
//! > *"it also looks like non native plugins are not integrated with the
//! > presets system, we need to ensure our presets system works with it kind
//! > of like how flx does so that you can use the presets system for all
//! > plugins cleanly and it just works."*
//!
//! The types already had a plugin preset — `DeviceKind::Plugin`, a payload
//! that is the plugin's whole `PluginState` — and the preset bar already
//! named one. What did not work was everything around it: a save wrote the
//! state the document had at the last Ctrl+S, a load changed the knobs and
//! left the rest of the patch where it was, and the browser put an effect
//! plugin's preset on the selected channel. The test gain keeps a **trim**
//! in its state and nowhere else, which is the part of a real plugin that
//! was being lost.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{PluginSlot, RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, PluginState};
use fontelle_ui::canvas::{BrowserMode, PresetDevice};
use fontelle_ui::document::{DocumentHost, LibraryKind, StudioHost};

use common::SR;

const GAIN_INSERT: PresetDevice = PresetDevice::Insert { strip: 0, slot: 0 };

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-plugin-presets-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
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
            let folder = std::env::temp_dir().join("fontelle-app-plugin-preset-tests");
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

/// A session with a preset bank of its own, the test bundle in reach, and
/// the test gain in the first strip's first insert.
fn a_session(dir: &Path) -> Session {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
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
    let mut session = Session::new(
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
    .with_plugin_folders(vec![plugin_folder()]);
    session.add_plugin_insert(0, 0);
    session
}

fn gain_slot(session: &Session) -> PluginSlot {
    fontelle_app::plugin_slots(session.project())[0]
}

fn gain_bytes(trim: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.extend_from_slice(&0.0f32.to_le_bytes());
    bytes.extend_from_slice(&trim.to_le_bytes());
    bytes
}

fn trim_of(state: &PluginState) -> f32 {
    let bytes = fontelle_types::decode_base64(state.blob.as_ref().expect("the gain keeps state"))
        .expect("a blob is base64");
    f32::from_le_bytes(bytes[8..12].try_into().unwrap())
}

/// What the running plugin is set to now.
fn live_trim(session: &mut Session) -> f32 {
    let slot = gain_slot(session);
    trim_of(&session.plugin_rack_mut().snapshot(slot).unwrap())
}

/// Turned in the plugin's own editor, which the document hears nothing of.
fn turn_in_its_editor(session: &mut Session, trim: f32) {
    let slot = gain_slot(session);
    session
        .plugin_rack_mut()
        .plugin_mut(slot)
        .unwrap()
        .load_state(&gain_bytes(trim));
}

fn choice(session: &Session, name: &str) -> usize {
    session
        .preset_choices(GAIN_INSERT)
        .iter()
        .position(|choice| choice.name == name)
        .unwrap_or_else(|| panic!("no preset called {name}"))
}

#[test]
fn saving_a_plugin_preset_saves_what_the_plugin_is_set_to_now() {
    let dir = scratch("save");
    let mut session = a_session(&dir);
    turn_in_its_editor(&mut session, 0.5);
    session.save_preset_as(GAIN_INSERT, "Half", "Mine");

    let saved = dir.join("presets");
    let file = walk(&saved)
        .into_iter()
        .find(|path| path.file_name().is_some_and(|name| name == "Half.json"))
        .unwrap_or_else(|| panic!("no Half.json under {}", saved.display()));
    let preset: fontelle_types::Preset =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let fontelle_types::PresetPayload::Plugin(state) = &preset.payload else {
        panic!(
            "a plugin preset holds the plugin's state: {:?}",
            preset.payload
        );
    };
    assert_eq!(
        trim_of(state),
        0.5,
        "the save wrote the document's old copy"
    );
}

#[test]
fn choosing_a_plugin_preset_loads_the_whole_plugin_not_just_its_knobs() {
    let dir = scratch("load");
    let mut session = a_session(&dir);
    turn_in_its_editor(&mut session, 0.25);
    session.save_preset_as(GAIN_INSERT, "Quarter", "Mine");
    turn_in_its_editor(&mut session, 1.0);
    session.save_preset_as(GAIN_INSERT, "Whole", "Mine");

    let at = choice(&session, "Quarter");
    session.apply_preset(GAIN_INSERT, at);
    assert_eq!(live_trim(&mut session), 0.25);
    assert_eq!(
        session.preset_bar(GAIN_INSERT).name.as_deref(),
        Some("Quarter")
    );

    // And one Ctrl+Z puts back the one before, in the plugin too.
    session.undo();
    assert_eq!(live_trim(&mut session), 1.0);
}

/// The browser's device list names a plugin the way the plugin does, not by
/// its id: `com.fopull.fontelle.testgain` means nothing to anybody.
#[test]
fn the_browser_names_a_plugins_presets_after_the_plugin() {
    let dir = scratch("name");
    let mut session = a_session(&dir);
    session.save_preset_as(GAIN_INSERT, "Unity", "Mine");
    session.set_browser_mode(BrowserMode::Presets);
    let names: Vec<String> = session
        .library_files()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(
        names.iter().any(|name| name == "Fontelle Test Gain"),
        "{names:?}"
    );
}

/// An effect plugin's preset clicked in the browser goes to the insert whose
/// window is open — the rule a built-in effect's preset already follows — and
/// never onto the selected channel, which it would have turned into a gain.
#[test]
fn a_plugin_effect_preset_chosen_in_the_browser_goes_to_the_insert_it_belongs_to() {
    let dir = scratch("browser");
    let mut session = a_session(&dir);
    turn_in_its_editor(&mut session, 0.25);
    session.save_preset_as(GAIN_INSERT, "Quarter", "Mine");
    turn_in_its_editor(&mut session, 1.0);
    let before = session.channel_kind(0);

    session.set_browser_mode(BrowserMode::Presets);
    session.note_open_insert(Some((0, 0)));
    let device = session
        .library_files()
        .iter()
        .position(|entry| entry.name == "Fontelle Test Gain")
        .expect("the gain's presets are listed");
    session.open_file(device).unwrap();
    let row = session
        .library_presets()
        .iter()
        .position(|row| row.kind != LibraryKind::Group && row.name == "Quarter")
        .expect("the preset is listed");
    session.set_channel_instrument(row).unwrap();

    assert_eq!(live_trim(&mut session), 0.25);
    assert_eq!(
        session.channel_kind(0),
        before,
        "the channel was left alone"
    );
}

/// With no window of that plugin open there is nowhere for it to go, and
/// saying so is better than guessing.
#[test]
fn a_plugin_effect_preset_with_nowhere_to_go_says_so() {
    let dir = scratch("nowhere");
    let mut session = a_session(&dir);
    session.save_preset_as(GAIN_INSERT, "Unity", "Mine");
    let before = session.channel_kind(0);
    session.set_browser_mode(BrowserMode::Presets);
    let device = session
        .library_files()
        .iter()
        .position(|entry| entry.name == "Fontelle Test Gain")
        .expect("the gain's presets are listed");
    session.open_file(device).unwrap();
    let row = session
        .library_presets()
        .iter()
        .position(|row| row.kind != LibraryKind::Group)
        .unwrap();
    assert!(session.set_channel_instrument(row).is_err());
    assert_eq!(session.channel_kind(0), before);
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

// ------------------------------------------- the plugin's own library

/// The test gain lists two presets of its own through CLAP's discovery
/// factory, Loud and Quiet — see `fontelle-testplug`'s `OWN_PRESETS`. The
/// rack lists a plugin's library off the main thread; a test waits for it.
fn with_its_library(dir: &Path) -> Session {
    let mut session = a_session(dir);
    session.settle_plugin_presets();
    session
}

#[test]
fn a_plugins_own_presets_are_offered_beside_fontelles() {
    let dir = scratch("offered");
    let mut session = with_its_library(&dir);
    session.save_preset_as(GAIN_INSERT, "Mine", "Mine");
    let choices = session.preset_choices(GAIN_INSERT);
    let named: Vec<(&str, fontelle_types::PresetOrigin)> = choices
        .iter()
        .map(|choice| (choice.name.as_str(), choice.origin))
        .collect();
    assert!(
        named.contains(&("Loud", fontelle_types::PresetOrigin::Plugin)),
        "{named:?}"
    );
    assert!(named.contains(&("Quiet", fontelle_types::PresetOrigin::Plugin)));
    assert!(named.contains(&("Mine", fontelle_types::PresetOrigin::User)));
}

#[test]
fn choosing_one_of_a_plugins_own_presets_loads_it_into_the_plugin() {
    let dir = scratch("own-load");
    let mut session = with_its_library(&dir);
    let at = choice(&session, "Quiet");
    session.apply_preset(GAIN_INSERT, at);
    assert_eq!(live_trim(&mut session), 0.25);
    let bar = session.preset_bar(GAIN_INSERT);
    assert_eq!(bar.name.as_deref(), Some("Quiet"));
    assert_eq!(bar.origin, Some(fontelle_types::PresetOrigin::Plugin));
    assert!(!bar.dirty, "a preset just loaded is clean");
    assert!(!bar.can_save, "the plugin's own library is read-only");

    // The document holds what the plugin now is: a save and a reopen give
    // Quiet back without the plugin's library.
    let PluginSlot::Insert { track, slot } = gain_slot(&session) else {
        panic!("the gain is an insert");
    };
    let state = session.project().mixer.tracks[track].inserts[slot]
        .plugin
        .clone()
        .expect("the insert holds the gain");
    assert_eq!(trim_of(&state), 0.25);

    session.undo();
    assert_eq!(live_trim(&mut session), 1.0);
}

#[test]
fn stepping_walks_through_the_plugins_own_presets_too() {
    let dir = scratch("own-step");
    let mut session = with_its_library(&dir);
    session.step_preset(GAIN_INSERT, 1);
    let first = session.preset_bar(GAIN_INSERT).name;
    session.step_preset(GAIN_INSERT, 1);
    let second = session.preset_bar(GAIN_INSERT).name;
    let mut both = vec![first.unwrap(), second.unwrap()];
    both.sort();
    assert_eq!(both, vec!["Loud".to_string(), "Quiet".to_string()]);
}

/// In the browser a plugin with a library of its own is listed with it,
/// before anybody has saved a preset of it in Fontelle.
#[test]
fn the_browser_lists_a_plugins_own_library() {
    let dir = scratch("own-browser");
    let mut session = with_its_library(&dir);
    session.set_browser_mode(BrowserMode::Presets);
    let device = session
        .library_files()
        .iter()
        .position(|entry| entry.name == "Fontelle Test Gain")
        .expect("the gain's library is listed");
    session.open_file(device).unwrap();
    let names: Vec<String> = session
        .library_presets()
        .into_iter()
        .filter(|row| row.kind != LibraryKind::Group)
        .map(|row| row.name)
        .collect();
    assert_eq!(names, vec!["Loud".to_string(), "Quiet".to_string()]);
    session.note_open_insert(Some((0, 0)));
    let quiet = session
        .library_presets()
        .iter()
        .position(|row| row.name == "Quiet")
        .unwrap();
    session.set_channel_instrument(quiet).unwrap();
    assert_eq!(live_trim(&mut session), 0.25);
}

// ------------------------------------ the strip in the plugin's own window

/// The test face — a note effect with an editor of its own, which no
/// browser row offers, so it is in the song from the start — in an insert
/// on the master track, its editor open on no screen.
fn with_face_open(dir: &Path) -> (Session, PresetDevice) {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let master = project.mixer.master.expect("a project has a master track");
    project.mixer.tracks[master]
        .inserts
        .push(fontelle_model::EffectSlot::hosting(PluginState::new(
            fontelle_types::PluginKey::clap("com.fopull.fontelle.testface"),
            "Face",
        )));
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
        fontelle_app::realise(&project, &library, options).expect("the project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    let mut session = Session::new(
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
    .with_plugin_folders(vec![plugin_folder()])
    .with_headless_plugin_editors();
    session.pump();
    let strip = (0..session.mixer_strips().len())
        .find(|&strip| session.open_plugin_editor_for_insert(strip, 0))
        .expect("the face has an editor of its own");
    (session, PresetDevice::Insert { strip, slot: 0 })
}

/// > *"kind of like how flx does"*
///
/// A plugin's own editor carries the preset bar of the device it is — this
/// insert's, not the selected channel's — across the top of its window.
#[test]
fn a_plugins_own_editor_carries_the_preset_bar_of_its_device() {
    let dir = scratch("header");
    let (mut session, face) = with_face_open(&dir);
    session.save_preset_as(face, "Faced", "Mine");
    let headers = session.plugin_headers();
    assert_eq!(headers.len(), 1, "{headers:?}");
    assert_eq!(headers[0].device, face);
    assert_eq!(headers[0].bar, session.preset_bar(face));
    assert_eq!(headers[0].bar.name.as_deref(), Some("Faced"));
    assert_eq!(
        headers[0].height,
        fontelle_ui::canvas::PLUGIN_HEADER_HEIGHT.round() as u32
    );
    assert!(headers[0].width > 0);
}

#[test]
fn a_press_on_the_strip_comes_back_naming_its_device() {
    let dir = scratch("header-press");
    let (mut session, face) = with_face_open(&dir);
    let slot = fontelle_app::plugin_slots(session.project())[0];
    session.plugin_rack_mut().press_header(slot, 120, 9);
    session.tick_plugin_editors();
    assert_eq!(
        session.take_plugin_header_presses(),
        vec![(face, 120.0, 9.0)]
    );
    assert!(
        session.take_plugin_header_presses().is_empty(),
        "taken once"
    );
}

/// A closed editor has no strip.
#[test]
fn a_closed_editor_has_no_strip() {
    let dir = scratch("header-closed");
    let (mut session, _) = with_face_open(&dir);
    let slot = fontelle_app::plugin_slots(session.project())[0];
    session.plugin_rack_mut().close_editor(slot);
    assert!(session.plugin_headers().is_empty());
}

/// A channel named by its place, which is what a plugin's window is for:
/// its strip is that channel's whichever channel is selected.
#[test]
fn a_channel_named_by_its_place_is_that_channel_whichever_is_selected() {
    let dir = scratch("header-channel");
    let mut session = a_session(&dir);
    session.set_channel_plugin(0, 0);
    session.select_channel(0);
    session.save_preset_as(PresetDevice::Instrument, "First", "Mine");
    session.add_channel().unwrap();
    let last = session.project().channels.len() - 1;
    session.select_channel(last);
    let first = PresetDevice::Channel { index: 0 };
    assert_eq!(session.preset_bar(first).name.as_deref(), Some("First"));
    assert_ne!(
        session.preset_bar(PresetDevice::Instrument).name.as_deref(),
        Some("First")
    );
    // And a load through it lands there.
    session.save_preset_as(first, "Second", "Mine");
    assert_eq!(session.preset_bar(first).name.as_deref(), Some("Second"));
}

/// Pressing the name on the strip opens that device's presets in the
/// studio's browser — the list, and a click there going to this device.
#[test]
fn the_browser_can_be_opened_on_a_devices_presets() {
    let dir = scratch("header-browser");
    let mut session = with_its_library(&dir);
    session.open_presets_for(GAIN_INSERT);
    let names: Vec<String> = session
        .library_presets()
        .into_iter()
        .filter(|row| row.kind != LibraryKind::Group)
        .map(|row| row.name)
        .collect();
    assert_eq!(names, vec!["Loud".to_string(), "Quiet".to_string()]);
    let quiet = session
        .library_presets()
        .iter()
        .position(|row| row.name == "Quiet")
        .unwrap();
    session.set_channel_instrument(quiet).unwrap();
    assert_eq!(live_trim(&mut session), 0.25);
}

// ------------------------------------------- what is playing, when it is copied
//
// > *"sometimes they'll just revert back to the init preset when working on
// > a saved project"*
//
// The document's copy of a plugin is as old as the last save. Three things
// used it as though it were the plugin: choosing the plugin a channel
// already has, duplicating a strip, and moving an insert.

#[test]
fn choosing_the_plugin_a_channel_already_has_leaves_its_patch() {
    let dir = scratch("again");
    let mut session = a_session(&dir);
    session.set_channel_plugin(0, 0);
    let slot = PluginSlot::Channel(session.project().channels.keys().next().unwrap());
    // Its own window turns its level; the document hears nothing.
    session
        .plugin_rack_mut()
        .plugin_mut(slot)
        .unwrap()
        .set_param(7, 0.25);
    let depth = session.undo_depth();

    // The same plugin, chosen again from the menu.
    session.set_channel_plugin(0, 0);
    assert_eq!(
        session.plugin_rack_mut().snapshot(slot).unwrap().param(7),
        Some(0.25),
        "choosing it again put it back to where a new one starts"
    );
    assert_eq!(session.undo_depth(), depth, "and it is not an edit");
}

#[test]
fn a_duplicated_strip_copies_what_its_plugin_is_playing() {
    let dir = scratch("copy-strip");
    let mut session = a_session(&dir);
    // A track beside the master (which cannot be duplicated), first in the
    // row, with a gain of its own.
    session.add_mixer_track();
    let track = session.project().mixer.ordered_tracks()[0];
    let at = session.project().mixer.tracks[track].inserts.len();
    session.add_plugin_insert(0, 0);
    let slot = PluginSlot::Insert { track, slot: at };
    session
        .plugin_rack_mut()
        .plugin_mut(slot)
        .expect("the gain went on the first strip")
        .load_state(&gain_bytes(0.5));

    let before = session.project().mixer.ordered_tracks();
    session.duplicate_mixer_track(0);
    let copy = session
        .project()
        .mixer
        .ordered_tracks()
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("a copy was made");
    let copied = session
        .plugin_rack_mut()
        .snapshot(PluginSlot::Insert {
            track: copy,
            slot: at,
        })
        .expect("the copy's plugin is open");
    assert_eq!(trim_of(&copied), 0.5, "the copy is of the last save");
}

#[test]
fn moving_an_insert_carries_what_its_plugin_is_playing() {
    let dir = scratch("move");
    let mut session = a_session(&dir);
    // Two gains on the master: the first turned in its own window.
    session.add_plugin_insert(0, 0);
    let slots = fontelle_app::plugin_slots(session.project());
    let PluginSlot::Insert { track, .. } = slots[0] else {
        panic!("the gain is an insert");
    };
    session
        .plugin_rack_mut()
        .plugin_mut(PluginSlot::Insert { track, slot: 0 })
        .unwrap()
        .load_state(&gain_bytes(0.5));

    session.move_insert(0, 0, 1);
    let moved = session
        .plugin_rack_mut()
        .snapshot(PluginSlot::Insert { track, slot: 1 })
        .unwrap();
    assert_eq!(trim_of(&moved), 0.5, "what was moved is the last save");
    let other = session
        .plugin_rack_mut()
        .snapshot(PluginSlot::Insert { track, slot: 0 })
        .unwrap();
    assert_eq!(trim_of(&other), 1.0);
}
