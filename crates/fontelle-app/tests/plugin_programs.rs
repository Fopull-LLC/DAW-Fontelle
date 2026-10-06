//! A plugin's presets compiled into it — its **programs** — in the studio's
//! preset menu.
//!
//! > *"our built in preset menu does not interface with plugin presets that
//! > are built into it. other daws manage to do this like in fl their preset
//! > menu thats attached to the plugin windows show the user presets as well
//! > as all the presets in the plugin thats made into its bank"*
//!
//! A JUCE plugin's `getNumPrograms()` reaches a VST 3 host as a program list
//! and a program-change parameter (Dexed's 32 cartridge voices). The test
//! fixture is `fontelle-testvst3`'s Programs: four programs, each setting a
//! level, and a hidden bank switch after which the four are others.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{PluginSlot, RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_testvst3::{PROGRAMS_ID, PROGRAMS_NAME};
use fontelle_types::{CompiledTimeline, PluginKey, PluginState, PresetOrigin};
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::{DocumentHost, LibraryKind, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-plugin-programs-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A folder holding nothing but the VST 3 test bundle — see
/// `plugin_hosting.rs`, whose layout this is.
fn vst3_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(if cfg!(target_os = "windows") {
        "fontelle_testvst3.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testvst3.dylib"
    } else {
        "libfontelle_testvst3.so"
    });
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testvst3`",
        built.display()
    );
    let (arch, file) = if cfg!(target_os = "windows") {
        ("Contents/x86_64-win", "fontelle-testvst3.vst3")
    } else if cfg!(target_os = "macos") {
        ("Contents/MacOS", "fontelle-testvst3")
    } else {
        ("Contents/x86_64-linux", "fontelle-testvst3.so")
    };
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = std::env::temp_dir().join(format!(
                "fontelle-app-vst3-program-tests-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(folder.join("fontelle-testvst3.vst3").join(arch)).unwrap();
            std::fs::copy(
                &built,
                folder.join("fontelle-testvst3.vst3").join(arch).join(file),
            )
            .unwrap();
            folder
        })
        .clone()
}

/// A folder holding the LV2 test bundle — `plugin_hosting.rs`'s layout.
/// LV2 is hosted on Linux alone.
#[cfg(target_os = "linux")]
fn lv2_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join("libfontelle_testlv2.so");
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testlv2`",
        built.display()
    );
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = std::env::temp_dir().join(format!(
                "fontelle-app-lv2-program-tests-{}",
                std::process::id()
            ));
            let bundle = folder.join("fontelle-testlv2.lv2");
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&bundle).unwrap();
            std::fs::copy(&built, bundle.join(fontelle_testlv2::BINARY_NAME)).unwrap();
            std::fs::write(bundle.join("manifest.ttl"), fontelle_testlv2::MANIFEST_TTL).unwrap();
            std::fs::write(bundle.join("testlv2.ttl"), fontelle_testlv2::PLUGIN_TTL).unwrap();
            folder
        })
        .clone()
}

fn plugin_folders() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    return vec![vst3_folder(), lv2_folder()];
    #[cfg(not(target_os = "linux"))]
    vec![vst3_folder()]
}

fn settings(dir: &Path) -> PathBuf {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        projects_dir: Some(dir.join("projects")),
        ..Default::default()
    };
    let path = dir.join("settings.json");
    std::fs::write(&path, settings.to_json()).unwrap();
    path
}

fn a_session_on(dir: &Path, project: fontelle_model::Project) -> Session {
    let clip = Session::first_clip(&project).unwrap_or_default();
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
    .with_settings_path(settings(dir))
    .with_plugin_folders(plugin_folders());
    session.set_projects_dir(Some(dir.join("projects")));
    session.pump();
    session.settle_plugin_presets();
    session
}

/// The programs fixture in an insert on the master track, its library
/// listed.
fn with_programs(dir: &Path) -> (Session, PresetDevice) {
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let master = project.mixer.master.expect("a project has a master track");
    project.mixer.tracks[master]
        .inserts
        .push(fontelle_model::EffectSlot::hosting(PluginState::new(
            PluginKey::new(fontelle_types::PluginFormat::Vst3, PROGRAMS_ID),
            PROGRAMS_NAME,
        )));
    let session = a_session_on(dir, project);
    let strip = (0..session.mixer_strips().len())
        .find(|&strip| {
            !session
                .preset_choices(PresetDevice::Insert { strip, slot: 0 })
                .is_empty()
        })
        .expect("the programs plugin is on a strip");
    (session, PresetDevice::Insert { strip, slot: 0 })
}

fn slot(session: &Session) -> PluginSlot {
    fontelle_app::plugin_slots(session.project())[0]
}

/// `(program, bank, level)` of the running plugin.
fn live_program(session: &mut Session) -> (u8, u8, f64) {
    let slot = slot(session);
    let state = session.plugin_rack_mut().snapshot(slot).unwrap();
    program_of(&state)
}

fn program_of(state: &PluginState) -> (u8, u8, f64) {
    let bytes = fontelle_types::decode_base64(state.blob.as_ref().expect("it keeps state"))
        .expect("a blob is base64");
    let at = bytes
        .windows(4)
        .position(|w| w == fontelle_testvst3::PROGRAMS_MAGIC)
        .expect("the component's state is in the blob");
    fontelle_testvst3::read_programs_state(&bytes[at..]).unwrap()
}

fn choice(session: &Session, device: PresetDevice, name: &str) -> usize {
    session
        .preset_choices(device)
        .iter()
        .position(|choice| choice.name == name)
        .unwrap_or_else(|| panic!("no preset called {name}"))
}

#[test]
fn a_plugins_programs_are_offered_after_the_users_own_presets() {
    let dir = scratch("offered");
    let (mut session, device) = with_programs(&dir);
    session.save_preset_as(device, "Mine", "Saved");
    let listed: Vec<(String, String, PresetOrigin)> = session
        .preset_choices(device)
        .into_iter()
        .map(|c| (c.name, c.category, c.origin))
        .collect();
    let plugin = |name: &str| {
        (
            name.to_string(),
            "Factory".to_string(),
            PresetOrigin::Plugin,
        )
    };
    assert_eq!(
        listed,
        vec![
            ("Mine".to_string(), "Saved".to_string(), PresetOrigin::User),
            plugin("1 Soft"),
            plugin("2 Medium"),
            plugin("3 Loud"),
            plugin("4 Soft"),
        ]
    );
}

#[test]
fn choosing_a_program_loads_it_and_one_undo_takes_it_back() {
    let dir = scratch("load");
    let (mut session, device) = with_programs(&dir);
    assert_eq!(live_program(&mut session), (0, 0, 0.1));
    let at = choice(&session, device, "3 Loud");
    session.apply_preset(device, at);
    assert_eq!(live_program(&mut session), (2, 0, 0.7));
    let bar = session.preset_bar(device);
    assert_eq!(bar.name.as_deref(), Some("3 Loud"));
    assert_eq!(bar.origin, Some(PresetOrigin::Plugin));

    // The song holds the state the program left, not a reference to it.
    let PluginSlot::Insert { track, slot } = slot(&session) else {
        panic!("an insert");
    };
    let held = session.project().mixer.tracks[track].inserts[slot]
        .plugin
        .clone()
        .unwrap();
    assert_eq!(program_of(&held), (2, 0, 0.7));

    session.undo();
    assert_eq!(live_program(&mut session), (0, 0, 0.1));
    session.redo();
    assert_eq!(live_program(&mut session), (2, 0, 0.7));
}

#[test]
fn stepping_walks_through_the_programs() {
    let dir = scratch("step");
    let (mut session, device) = with_programs(&dir);
    session.step_preset(device, 1);
    assert_eq!(session.preset_bar(device).name.as_deref(), Some("1 Soft"));
    session.step_preset(device, 1);
    assert_eq!(session.preset_bar(device).name.as_deref(), Some("2 Medium"));
    assert_eq!(live_program(&mut session), (1, 0, 0.4));
    session.step_preset(device, -1);
    session.step_preset(device, -1);
    assert_eq!(session.preset_bar(device).name.as_deref(), Some("4 Soft"));
    assert_eq!(live_program(&mut session), (3, 0, 0.9));
}

#[test]
fn a_program_survives_a_save_and_a_reopen() {
    let dir = scratch("reopen");
    let (mut session, device) = with_programs(&dir);
    let at = choice(&session, device, "2 Medium");
    session.apply_preset(device, at);
    session.save_as("Programs").expect("saves");
    let bundle = session.bundle_path().unwrap().to_path_buf();
    drop(session);

    let mut again = a_session_on(&dir, common::a_project_with_a_clip(8, 120.0, SR));
    again.open_project_path(&bundle).expect("opens");
    again.pump();
    assert_eq!(live_program(&mut again), (1, 0, 0.4));
    assert_eq!(again.preset_bar(device).name.as_deref(), Some("2 Medium"));
}

/// Another cartridge loaded in the plugin is other programs, and the menu
/// follows. Loaded in the plugin's own window, which the document hears
/// nothing of: the plugin takes a state with the other bank in it, and its
/// controller says its program list changed.
#[test]
fn the_menu_follows_a_program_list_the_plugin_changed() {
    let dir = scratch("refresh");
    let (mut session, device) = with_programs(&dir);
    let slot = slot(&session);
    let state = session.plugin_rack_mut().snapshot(slot).unwrap();
    let mut bytes = fontelle_types::decode_base64(state.blob.as_ref().unwrap()).unwrap();
    let at = bytes
        .windows(4)
        .position(|w| w == fontelle_testvst3::PROGRAMS_MAGIC)
        .unwrap();
    bytes[at + 5] = 1;
    assert!(
        session
            .plugin_rack_mut()
            .plugin_mut(slot)
            .unwrap()
            .load_state(&bytes)
    );
    session.tick_plugin_editors();
    let names: Vec<String> = session
        .preset_choices(device)
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, vec!["1 Pad", "2 Lead", "3 Bass", "4 Keys"]);
    let at = choice(&session, device, "3 Bass");
    session.apply_preset(device, at);
    assert_eq!(live_program(&mut session), (2, 1, 0.6));
}

/// The plugin window's strip opens the browser on the device's presets:
/// the programs are there, and the one playing is marked.
#[test]
fn the_browser_lists_the_programs_and_marks_the_one_playing() {
    let dir = scratch("browser");
    let (mut session, device) = with_programs(&dir);
    let at = choice(&session, device, "3 Loud");
    session.apply_preset(device, at);
    session.open_presets_for(device);
    let rows: Vec<(String, String)> = session
        .library_presets()
        .into_iter()
        .filter(|row| row.kind != LibraryKind::Group)
        .map(|row| (row.name, row.detail))
        .collect();
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, vec!["1 Soft", "2 Medium", "3 Loud", "4 Soft"]);
    let marked: Vec<&str> = rows
        .iter()
        .filter(|(_, detail)| detail.contains(fontelle_app::CURRENT_PRESET_MARK))
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(marked, vec!["3 Loud"]);
}

/// An LV2 plugin whose presets are compiled into it and offered through the
/// KXStudio programs extension — Dexed's LV2 — has them in the menu, read
/// off the instance the studio is running, and one chosen is played.
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_plugins_programs_are_offered_and_load() {
    let dir = scratch("lv2");
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let master = project.mixer.master.expect("a project has a master track");
    project.mixer.tracks[master]
        .inserts
        .push(fontelle_model::EffectSlot::hosting(PluginState::new(
            PluginKey::new(
                fontelle_types::PluginFormat::Lv2,
                fontelle_testlv2::PROGRAMS_URI,
            ),
            "Fontelle Test Programs LV2",
        )));
    let mut session = a_session_on(&dir, project);
    let strip = (0..session.mixer_strips().len())
        .find(|&strip| {
            !session
                .preset_choices(PresetDevice::Insert { strip, slot: 0 })
                .is_empty()
        })
        .expect("the programs are offered on a strip");
    let device = PresetDevice::Insert { strip, slot: 0 };
    let names: Vec<String> = session
        .preset_choices(device)
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, vec!["1 Quiet", "2 Loud"]);
    let at = choice(&session, device, "2 Loud");
    session.apply_preset(device, at);
    let slot = slot(&session);
    let state = session.plugin_rack_mut().snapshot(slot).unwrap();
    // The gain port is index 2.
    assert_eq!(state.param(2), Some(1.5));
    assert_eq!(session.preset_bar(device).name.as_deref(), Some("2 Loud"));
    session.undo();
    let state = session.plugin_rack_mut().snapshot(slot).unwrap();
    assert_eq!(state.param(2), Some(1.0));
}
