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
