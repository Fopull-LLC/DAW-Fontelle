//! A plugin's **own** presets — the library it ships with, found and loaded
//! the way that plugin's format keeps it.
//!
//! > *"we need to ensure our presets system works with it kind of like how
//! > flx does so that you can use the presets system for all plugins
//! > cleanly and it just works."*
//!
//! Four ways a plugin keeps a library, one per format and one more:
//!
//! - **CLAP**: a preset-discovery factory beside the plugin factory, and the
//!   `preset-load` extension to load what it listed (Surge XT).
//! - **VST 3**: `.vstpreset` files under `<root>/<vendor>/<plugin>/`.
//! - **LV2**: `pset:Preset` resources in the plugin's own Turtle.
//! - **`.fxp`**: the VST 2 patch file JUCE plugins still ship as their
//!   library (OB-Xf's 488), whose chunk is the same bytes the plugin's CLAP
//!   state is. Offered only when the chunk starts the way the plugin's own
//!   state does, so an unrelated file is never forced on it.

mod common;

use std::path::{Path, PathBuf};

use fontelle_host::{OwnPresetSource, PluginHost, PresetRoots, scan_bundle};
use fontelle_types::{PluginFormat, PluginKey};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-own-presets-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn info_of(bundle: &Path, key: &PluginKey) -> fontelle_host::PluginInfo {
    scan_bundle(bundle)
        .expect("the fixture bundle scans")
        .into_iter()
        .find(|info| &info.key == key)
        .expect("the fixture is in its bundle")
}

/// The test gain's CLAP state: gain, switch, and the trim it keeps nowhere
/// else (`fontelle-testplug`'s `GainShared::trim`).
fn gain_bytes(gain: f32, trim: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&gain.to_le_bytes());
    bytes.extend_from_slice(&0.0f32.to_le_bytes());
    bytes.extend_from_slice(&trim.to_le_bytes());
    bytes
}

fn trim_of(plugin: &mut fontelle_host::HostedPlugin) -> f32 {
    let bytes = plugin.save_state().expect("the gain keeps state");
    f32::from_le_bytes(bytes[8..12].try_into().unwrap())
}

// ------------------------------------------------------------------- CLAP

fn clap_gain() -> (PluginKey, fontelle_host::PluginInfo) {
    let key = PluginKey::clap(common::GAIN);
    let info = info_of(&common::bundle(), &key);
    (key, info)
}

#[test]
fn a_clap_plugins_own_presets_are_found_through_its_discovery_factory() {
    let (_, info) = clap_gain();
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &PresetRoots::none());
    let names: Vec<(&str, &str)> = presets
        .iter()
        .map(|p| (p.name.as_str(), p.category.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![("Loud", "Soft and loud"), ("Quiet", "Soft and loud")]
    );
    assert!(matches!(presets[0].source, OwnPresetSource::Clap { .. }));
}

#[test]
fn a_clap_plugins_own_preset_is_loaded_by_the_plugin() {
    let (key, info) = clap_gain();
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &PresetRoots::none());
    let quiet = presets.iter().find(|p| p.name == "Quiet").unwrap();
    let mut plugin = host.open(&common::bundle(), &key).unwrap();
    plugin
        .load_own_preset(quiet)
        .expect("the gain loads its own preset");
    assert_eq!(trim_of(&mut plugin), 0.25);
}

/// > *"it might be worth looking into more advanced synths like vital, serum
/// > and surge as I'm still experiencing problems with those"*
///
/// Surge XT queues a preset and swaps it in on the next block. Read straight
/// after the call, its parameters are the **old** patch's — and the host
/// marked every one of them to be sent, so the next block put the old patch's
/// values back over the new one. A soak of the real Surge came back silent
/// with a hybrid patch saved. A preset read back is what the plugin said, not
/// something to tell it.
#[test]
fn a_preset_the_plugin_queues_is_not_overwritten_by_the_values_read_before_it_landed() {
    let (key, _) = clap_gain();
    let mut host = PluginHost::new();
    let mut plugin = host.open(&common::bundle(), &key).unwrap();
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let queued = fontelle_host::OwnPreset {
        name: "Queued".into(),
        category: String::new(),
        source: OwnPresetSource::Clap {
            location: None,
            load_key: Some("queued".into()),
        },
    };
    plugin
        .load_own_preset_with(&mut processor, &queued)
        .expect("the gain takes it");
    let input = vec![vec![1.0f32; 64]; 2];
    let mut output = vec![vec![0.0f32; 64]; 2];
    for _ in 0..4 {
        processor.process_effect(&input, &mut output, 64);
    }
    assert!(
        (output[0][63] - 0.5).abs() < 1e-4,
        "the preset's gain is what plays: {}",
        output[0][63]
    );
    let gain_id = plugin.params()[0].id;
    assert_eq!(
        plugin.values().get(gain_id),
        Some(0.5),
        "and what the studio's knob says, once the plugin has said so"
    );
    plugin.deactivate(processor);
}

/// CLAP's `request_callback`: a plugin asks, and the host calls its
/// `on_main_thread` soon after, on the main thread. This host recorded the
/// request and never answered it — so whatever a plugin put off until then
/// (a patch finishing loading, among JUCE's CLAP wrappers) never happened.
/// The studio services every plugin once a frame; so does this.
#[test]
fn a_plugin_that_asks_to_be_called_back_on_the_main_thread_is() {
    let (key, _) = clap_gain();
    let mut host = PluginHost::new();
    let mut plugin = host.open(&common::bundle(), &key).unwrap();
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let deferred = fontelle_host::OwnPreset {
        name: "Deferred".into(),
        category: String::new(),
        source: OwnPresetSource::Clap {
            location: None,
            load_key: Some("deferred".into()),
        },
    };
    plugin
        .load_own_preset_with(&mut processor, &deferred)
        .expect("the gain takes it");
    let input = vec![vec![1.0f32; 64]; 2];
    let mut output = vec![vec![0.0f32; 64]; 2];
    for _ in 0..4 {
        plugin.service_main_thread();
        processor.process_effect(&input, &mut output, 64);
    }
    assert!(
        (output[0][63] - 0.25).abs() < 1e-4,
        "the preset lands once the plugin is called back: {}",
        output[0][63]
    );
    plugin.deactivate(processor);
}

/// > *"sometimes they'll just revert back to the init preset"*
///
/// OB-Xf, one time in four: a preset chosen after an undo did not stay. Its
/// state goes in on its own message thread, and it saves back what it was
/// handed straight away, so the host read "it is in", stopped waiting, and
/// moved on; the undo's state landed afterwards, over the preset chosen next,
/// and the document kept the init patch. A state the plugin saves back is
/// now waited on like any other, until the plugin has held still.
#[test]
fn a_state_a_plugin_applies_later_does_not_land_over_the_next_preset() {
    let (key, _) = clap_gain();
    let mut host = PluginHost::new();
    let mut plugin = host.open(&common::bundle(), &key).unwrap();
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![1.0f32; 64]; 2];
    let mut output = vec![vec![0.0f32; 64]; 2];
    processor.process_effect(&input, &mut output, 64);
    let by_key = |key: &str| fontelle_host::OwnPreset {
        name: key.into(),
        category: String::new(),
        source: OwnPresetSource::Clap {
            location: None,
            load_key: Some(key.into()),
        },
    };
    plugin
        .load_own_preset_with(&mut processor, &by_key(fontelle_testplug::DEFERS_STATES))
        .unwrap();
    let init = plugin.save_state().expect("it keeps state");

    // A preset, then an undo back to the state from before it.
    let before = plugin.settle_mark(&mut processor);
    plugin
        .load_own_preset_with(&mut processor, &by_key("loud"))
        .unwrap();
    plugin.settle_with(&mut processor, &before);
    let before = plugin.settle_mark_for(&mut processor, &init);
    assert!(plugin.load_state(&init));
    plugin.settle_with(&mut processor, &before);

    // And the next preset.
    let before = plugin.settle_mark(&mut processor);
    plugin
        .load_own_preset_with(&mut processor, &by_key("quiet"))
        .unwrap();
    plugin.settle_with(&mut processor, &before);
    for _ in 0..4 {
        plugin.service_main_thread();
        processor.process_effect(&input, &mut output, 64);
    }
    assert!(
        (output[0][63] - 0.25).abs() < 1e-4,
        "the preset chosen last is what plays: {}",
        output[0][63]
    );
    plugin.deactivate(processor);
}

// -------------------------------------------------------------------- .fxp

/// A VST 2 patch file holding `chunk` as its opaque program chunk.
fn fxp(plugin_id: &[u8; 4], name: &str, chunk: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"CcnK");
    bytes.extend_from_slice(&((52 + chunk.len()) as u32).to_be_bytes());
    bytes.extend_from_slice(b"FPCh");
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(plugin_id);
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    let mut program = [0u8; 28];
    program[..name.len()].copy_from_slice(name.as_bytes());
    bytes.extend_from_slice(&program);
    bytes.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    bytes.extend_from_slice(chunk);
    bytes
}

/// A data root laid out the way OB-Xf's is:
/// `<root>/<plugin name>/Patches/<category>/<name>.fxp`.
fn fxp_root(name: &str, files: &[(&str, &str, Vec<u8>)]) -> PathBuf {
    let root = scratch(name);
    for (category, preset, bytes) in files {
        let folder = root
            .join("Fontelle Test Gain")
            .join("Patches")
            .join(category);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{preset}.fxp")), bytes).unwrap();
    }
    root
}

#[test]
fn a_plugins_fxp_library_is_listed_under_its_folders() {
    let (_, info) = clap_gain();
    let root = fxp_root(
        "listed",
        &[("Pads", "Half", fxp(b"FTgn", "Half", &gain_bytes(1.0, 0.5)))],
    );
    let roots = PresetRoots {
        data: vec![root],
        vst3: Vec::new(),
    };
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &roots);
    let half = presets
        .iter()
        .find(|p| p.name == "Half")
        .expect("the .fxp is listed");
    assert_eq!(half.category, "Pads");
    assert!(matches!(half.source, OwnPresetSource::Fxp(_)));
}

#[test]
fn an_fxp_chunk_is_loaded_as_the_plugins_state() {
    let (key, info) = clap_gain();
    let root = fxp_root(
        "loaded",
        &[("Pads", "Half", fxp(b"FTgn", "Half", &gain_bytes(1.0, 0.5)))],
    );
    let roots = PresetRoots {
        data: vec![root],
        vst3: Vec::new(),
    };
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &roots);
    let half = presets.iter().find(|p| p.name == "Half").unwrap();
    let mut plugin = host.open(&common::bundle(), &key).unwrap();
    plugin.load_own_preset(half).unwrap();
    assert_eq!(trim_of(&mut plugin), 0.5);
}

/// A chunk that does not start the way the plugin's own state does is some
/// other program's patch that happens to share a folder name, and is left
/// out rather than handed to the plugin to choke on.
#[test]
fn an_fxp_whose_chunk_is_not_this_plugins_kind_is_left_out() {
    let (_, info) = clap_gain();
    let root = fxp_root(
        "foreign",
        &[(
            "Pads",
            "Stranger",
            fxp(b"Xxxx", "Stranger", b"<?xml nothing like it"),
        )],
    );
    let roots = PresetRoots {
        data: vec![root],
        vst3: Vec::new(),
    };
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &roots);
    assert!(presets.iter().all(|p| p.name != "Stranger"), "{presets:?}");
}

// ------------------------------------------------------------------- VST 3

/// A `.vstpreset`: the header, the component's state, and the chunk list
/// that says where it is.
fn vstpreset(class_id: &str, component: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"VST3");
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(class_id.as_bytes());
    let list_at = 48 + component.len() as i64;
    bytes.extend_from_slice(&list_at.to_le_bytes());
    bytes.extend_from_slice(component);
    bytes.extend_from_slice(b"List");
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(b"Comp");
    bytes.extend_from_slice(&48i64.to_le_bytes());
    bytes.extend_from_slice(&(component.len() as i64).to_le_bytes());
    bytes
}

/// The VST 3 test gain's component state: its magic, the gain as an `f64`,
/// and the switch.
fn vst3_gain_state(gain: f64) -> Vec<u8> {
    let mut bytes = fontelle_testvst3::COMPONENT_MAGIC.to_vec();
    bytes.extend_from_slice(&gain.to_le_bytes());
    bytes.push(0);
    bytes
}

#[test]
fn a_vst3_plugins_vstpresets_are_found_and_loaded() {
    let key = PluginKey::new(PluginFormat::Vst3, common::VST3_GAIN);
    let info = info_of(&common::vst3_bundle(), &key);
    let root = scratch("vst3");
    let folder = root.join(&info.vendor).join(&info.name);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("Most.vstpreset"),
        vstpreset(common::VST3_GAIN, &vst3_gain_state(0.75)),
    )
    .unwrap();
    // Another plugin's file in the same folder is not this one's.
    std::fs::write(
        folder.join("Other.vstpreset"),
        vstpreset(common::VST3_SINE, &vst3_gain_state(0.5)),
    )
    .unwrap();
    let roots = PresetRoots {
        data: Vec::new(),
        vst3: vec![root],
    };
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &roots);
    let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Most"]);

    let mut plugin = host.open(&common::vst3_bundle(), &key).unwrap();
    plugin.load_own_preset(&presets[0]).unwrap();
    // The gain parameter is id 0, and the controller heard the component.
    assert_eq!(plugin.values().get(0), Some(0.75));
}

// ---------------------------------------------------- VST 3 programs
//
// > *"our built in preset menu does not interface with plugin presets that
// > are built into it. other daws manage to do this like in fl their preset
// > menu thats attached to the plugin windows show the user presets as well
// > as all the presets in the plugin thats made into its bank"*
//
// A VST 3 plugin's presets compiled into it are **programs**: a program list
// on a unit (`IUnitInfo`), and a parameter flagged `kIsProgramChange` that
// selects one. A JUCE plugin offers its `getNumPrograms()` this way — Dexed's
// 32 cartridge voices — and has no files at all.

fn programs_plugin() -> (PluginHost, fontelle_host::HostedPlugin) {
    let key = PluginKey::new(PluginFormat::Vst3, fontelle_testvst3::PROGRAMS_ID);
    let mut host = PluginHost::new();
    let plugin = host.open(&common::vst3_bundle(), &key).unwrap();
    (host, plugin)
}

fn programs_level(bytes: &[u8]) -> (u8, u8, f64) {
    // The VST 3 blob is the host's framing of both halves; the component's
    // chunk is the fixture's own bytes, found by its magic.
    let at = bytes
        .windows(4)
        .position(|w| w == fontelle_testvst3::PROGRAMS_MAGIC)
        .expect("the component's state is in the blob");
    fontelle_testvst3::read_programs_state(&bytes[at..]).unwrap()
}

#[test]
fn a_vst3_plugins_programs_are_its_own_presets_in_their_order() {
    let (_host, mut plugin) = programs_plugin();
    let programs = plugin.programs();
    let listed: Vec<(&str, &str)> = programs
        .iter()
        .map(|p| (p.name.as_str(), p.category.as_str()))
        .collect();
    // Numbered, which keeps the plugin's order and tells apart two voices
    // of one name; filed under the program list's own name.
    assert_eq!(
        listed,
        vec![
            ("1 Soft", "Factory"),
            ("2 Medium", "Factory"),
            ("3 Loud", "Factory"),
            ("4 Soft", "Factory"),
        ]
    );
    assert!(matches!(
        programs[2].source,
        OwnPresetSource::Program { param, .. } if param == fontelle_testvst3::PROGRAM_PARAM
    ));
}

#[test]
fn the_library_of_a_vst3_plugin_holds_its_programs() {
    let key = PluginKey::new(PluginFormat::Vst3, fontelle_testvst3::PROGRAMS_ID);
    let info = info_of(&common::vst3_bundle(), &key);
    let mut host = PluginHost::new();
    let names: Vec<String> = host
        .own_presets(&info, &PresetRoots::none())
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, vec!["1 Soft", "2 Medium", "3 Loud", "4 Soft"]);
}

/// Chosen, a program goes through the program-change parameter — to the
/// controller now, and to the processor on its next block — and the
/// plugin's state is then the program's.
#[test]
fn choosing_a_program_sets_the_processor_to_it() {
    let (_host, mut plugin) = programs_plugin();
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let programs = plugin.programs();
    let before = plugin.settle_mark(&mut processor);
    plugin
        .load_own_preset_with(&mut processor, &programs[2])
        .unwrap();
    plugin.settle_with(&mut processor, &before);
    let state = plugin.save_state_with(&mut processor).unwrap();
    assert_eq!(programs_level(&state), (2, 0, 0.7));
    assert_eq!(
        plugin.values().get(fontelle_testvst3::PROGRAM_PARAM),
        Some(2.0),
        "the program parameter reads the program"
    );
    plugin.deactivate(processor);
}

/// A JUCE plugin changes program only when told one other than its own
/// (`setCurrentProgram` is skipped for the current one). Chosen again after
/// a knob was turned — or after an undo put back a patch the plugin still
/// files under that program — the program has to come back whole.
#[test]
fn choosing_the_program_it_is_on_again_puts_the_program_back() {
    let (_host, mut plugin) = programs_plugin();
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let programs = plugin.programs();
    let load = |plugin: &mut fontelle_host::HostedPlugin,
                processor: &mut fontelle_host::HostedProcessor,
                at: usize| {
        let before = plugin.settle_mark(processor);
        plugin
            .load_own_preset_with(processor, &programs[at])
            .unwrap();
        plugin.settle_with(processor, &before);
    };
    load(&mut plugin, &mut processor, 2);
    // A knob turned since.
    plugin.set_param(fontelle_testvst3::PROGRAM_LEVEL_PARAM, 0.3);
    let mut bus = vec![vec![0.0f32; 256]; 2];
    processor.process_insert(&mut bus, 256);
    let state = plugin.save_state_with(&mut processor).unwrap();
    assert_eq!(programs_level(&state), (2, 0, 0.3));
    load(&mut plugin, &mut processor, 2);
    let state = plugin.save_state_with(&mut processor).unwrap();
    assert_eq!(programs_level(&state), (2, 0, 0.7), "the program, whole");
    plugin.deactivate(processor);
}

/// Not running, the controller is told and the component hears it when it
/// starts.
#[test]
fn a_program_chosen_before_the_plugin_runs_is_the_one_it_starts_on() {
    let (_host, mut plugin) = programs_plugin();
    let programs = plugin.programs();
    plugin.load_own_preset(&programs[1]).unwrap();
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let mut bus = vec![vec![0.0f32; 256]; 2];
    processor.process_insert(&mut bus, 256);
    let state = plugin.save_state_with(&mut processor).unwrap();
    assert_eq!(programs_level(&state), (1, 0, 0.4));
    plugin.deactivate(processor);
}

/// Another cartridge is other names: a plugin that says its program list
/// changed is asked again.
#[test]
fn a_program_list_the_plugin_says_has_changed_is_read_again() {
    let (_host, mut plugin) = programs_plugin();
    assert!(!plugin.take_programs_changed(), "nothing said yet");
    plugin.set_param(fontelle_testvst3::PROGRAM_BANK_PARAM, 1.0);
    assert!(plugin.take_programs_changed(), "the plugin said so");
    assert!(!plugin.take_programs_changed(), "taken once");
    let names: Vec<String> = plugin.programs().into_iter().map(|p| p.name).collect();
    assert_eq!(names, vec!["1 Pad", "2 Lead", "3 Bass", "4 Keys"]);
}

/// A plugin with no program list has no programs, whatever its format.
#[test]
fn a_plugin_without_a_program_list_has_no_programs() {
    let key = PluginKey::new(PluginFormat::Vst3, common::VST3_GAIN);
    let mut host = PluginHost::new();
    let mut plugin = host.open(&common::vst3_bundle(), &key).unwrap();
    assert!(plugin.programs().is_empty());
    let (_, info) = clap_gain();
    let mut clap = host.open(&info.path, &info.key).unwrap();
    assert!(clap.programs().is_empty());
}

// --------------------------------------------------------------------- LV2

#[cfg(target_os = "linux")]
#[test]
fn an_lv2_plugins_presets_are_read_off_its_turtle() {
    let key = PluginKey::new(PluginFormat::Lv2, common::LV2_GAIN);
    let info = info_of(&common::lv2_bundle(), &key);
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &PresetRoots::none());
    let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Half"]);
    assert!(matches!(presets[0].source, OwnPresetSource::Lv2 { .. }));
}

/// LV2 restores a state only while the instance is not running, so a
/// running one is loaded with its processor in hand — the same recall a
/// snapshot makes.
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_preset_is_loaded_into_the_running_instance() {
    let key = PluginKey::new(PluginFormat::Lv2, common::LV2_GAIN);
    let info = info_of(&common::lv2_bundle(), &key);
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &PresetRoots::none());
    let mut plugin = host.open(&common::lv2_bundle(), &key).unwrap();
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    assert!(plugin.own_preset_needs_processor());
    plugin
        .load_own_preset_with(&mut processor, &presets[0])
        .unwrap();
    // The gain port is index 2.
    assert_eq!(plugin.values().get(2), Some(0.5));
    let input = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 0.5).abs() < 1e-5, "{}", output[0][0]);
}

/// An LV2 plugin with presets compiled into it and none in its Turtle —
/// Dexed's LV2, and DISTRHO's ports — offers them through the KXStudio
/// programs extension, asked of the running instance.
#[cfg(target_os = "linux")]
fn lv2_programs_plugin() -> (
    PluginHost,
    fontelle_host::HostedPlugin,
    fontelle_host::HostedProcessor,
) {
    let key = PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PROGRAMS_URI);
    let mut host = PluginHost::new();
    let mut plugin = host.open(&common::lv2_bundle(), &key).unwrap();
    let processor = plugin.activate(48_000.0, 64).unwrap();
    (host, plugin, processor)
}

#[cfg(target_os = "linux")]
#[test]
fn an_lv2_plugins_programs_are_listed_off_the_running_instance() {
    let (_host, mut plugin, mut processor) = lv2_programs_plugin();
    let listed: Vec<(String, String)> = plugin
        .programs_with(&mut processor)
        .into_iter()
        .map(|p| (p.name, p.category))
        .collect();
    assert_eq!(
        listed,
        vec![
            ("1 Quiet".to_string(), "Programs".to_string()),
            ("2 Loud".to_string(), "Programs".to_string()),
        ]
    );
    // And the library a host lists for it holds them, its Turtle having
    // no preset of its own.
    let key = PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PROGRAMS_URI);
    let info = info_of(&common::lv2_bundle(), &key);
    let names: Vec<String> = PluginHost::new()
        .own_presets(&info, &PresetRoots::none())
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, vec!["1 Quiet", "2 Loud"]);
}

/// Selected with the processor in hand, the program writes its control
/// ports, and the host reads them back onto the wire — otherwise the next
/// block wrote the old values over the program.
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_program_is_selected_in_the_running_instance() {
    let (_host, mut plugin, mut processor) = lv2_programs_plugin();
    let programs = plugin.programs_with(&mut processor);
    assert!(plugin.own_preset_needs_processor());
    plugin
        .load_own_preset_with(&mut processor, &programs[1])
        .unwrap();
    assert_eq!(plugin.values().get(2), Some(1.5));
    let input = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 1.5).abs() < 1e-5, "{}", output[0][0]);
}

/// Only a plugin whose Turtle declares the programs extension is asked for
/// it: blop's 16 Step Sequencer, asked, crashed the process. The deaf gain
/// answers for the extension without declaring it.
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_plugin_is_not_asked_for_programs_it_does_not_declare() {
    let key = PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::DEAF_URI);
    let mut host = PluginHost::new();
    let mut plugin = host.open(&common::lv2_bundle(), &key).unwrap();
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    assert!(plugin.programs_with(&mut processor).is_empty());
    plugin.deactivate(processor);
}

/// The gain ships a `pset:Preset`: what its Turtle lists is its library, and
/// nothing else is asked for (a DPF plugin offers the same set both ways).
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_plugin_with_presets_in_its_turtle_lists_no_programs_beside_them() {
    let key = PluginKey::new(PluginFormat::Lv2, common::LV2_GAIN);
    let info = info_of(&common::lv2_bundle(), &key);
    let names: Vec<String> = PluginHost::new()
        .own_presets(&info, &PresetRoots::none())
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, vec!["Half"]);
}

/// Linux packages name a plugin's data folder in lower case with dashes:
/// Surge XT's factory patches are in `/usr/share/surge-xt`, not
/// `/usr/share/Surge XT`. Looked for by the plugin's name exactly, Surge's
/// VST 3 offered no presets at all on the strip — its CLAP lists them itself.
#[test]
fn a_library_folder_named_the_linux_way_is_found() {
    let (_, info) = clap_gain();
    for spelling in [
        "fontelle-test-gain",
        "fontelle_test_gain",
        "FontelleTestGain",
    ] {
        let root = scratch(&format!("spelt-{spelling}"));
        let folder = root.join(spelling).join("patches_factory").join("Pads");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("Half.fxp"),
            fxp(b"FTgn", "Half", &gain_bytes(1.0, 0.5)),
        )
        .unwrap();
        let roots = PresetRoots {
            data: vec![root],
            vst3: Vec::new(),
        };
        let mut host = PluginHost::new();
        let presets = host.own_presets(&info, &roots);
        assert!(
            presets.iter().any(|p| p.name == "Half"),
            "{spelling}: {presets:?}"
        );
    }
}

/// But not a folder that merely starts the same: "Fontelle Test Gainer" is
/// somebody else's.
#[test]
fn a_folder_for_another_plugin_is_not_taken_for_this_ones() {
    let (_, info) = clap_gain();
    let root = scratch("another");
    let folder = root.join("fontelle-test-gainer").join("Pads");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("Half.fxp"),
        fxp(b"FTgn", "Half", &gain_bytes(1.0, 0.5)),
    )
    .unwrap();
    let roots = PresetRoots {
        data: vec![root],
        vst3: Vec::new(),
    };
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &roots);
    assert!(!presets.iter().any(|p| p.name == "Half"), "{presets:?}");
}

// ----------------------------------------------------- library files
//
// > *"i tried a bunch of different instruments including obxf, amsynth, and
// > cardinal synth. all of these have built in presets but our daws preset
// > system did not detect them and let you swap between them with our preset
// > bar."*

fn roots_at(root: &Path) -> PresetRoots {
    PresetRoots {
        data: vec![root.to_path_buf()],
        vst3: Vec::new(),
    }
}

/// The presets listed from library files, as `(name, category)`.
fn files_of(presets: &[fontelle_host::OwnPreset]) -> Vec<(&str, &str)> {
    presets
        .iter()
        .filter(|p| matches!(p.source, OwnPresetSource::StateFile(_)))
        .map(|p| (p.name.as_str(), p.category.as_str()))
        .collect()
}

/// Vital's `.vital` files are its state: listed for a plugin whose state is
/// JSON opening with the same two keys, and nothing else in the folder is.
#[test]
fn a_json_states_library_files_are_listed_when_they_begin_as_its_state_does() {
    let (_, info) = clap_gain();
    let root = scratch("json-library");
    let folder = root
        .join("fontelle-test-gain")
        .join("Presets")
        .join("Leads");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("Wob.vital"),
        br#"{"author":"Mr. Bill","settings":{}}"#,
    )
    .unwrap();
    std::fs::write(folder.join("notes.txt"), b"hello").unwrap();
    std::fs::write(folder.join("Foreign.json"), br#"{"name":"x"}"#).unwrap();
    // Vital's wavetables are JSON by the same author, and not presets.
    std::fs::write(
        folder.join("Didg.vitaltable"),
        br#"{"author":"Matt Tytel","full_normalize":true}"#,
    )
    .unwrap();
    let own = br#"{"author":"","settings":{"volume":0.5}}"#;
    let presets = fontelle_host::list_own_presets(&info, &roots_at(&root), Some(own), &[]);
    // Beside the two its discovery factory lists.
    let listed: Vec<(&str, &str)> = files_of(&presets);
    assert_eq!(listed, vec![("Wob", "Leads")]);
}

/// Cardinal Synth's patches are in `/usr/share/cardinal/patches`, a folder
/// named for the family rather than for "Cardinal Synth" — and listed only
/// because its state carries a patch.
#[test]
fn a_rack_patch_library_is_listed_for_a_plugin_whose_state_carries_one() {
    let (_, info) = clap_gain();
    let root = scratch("rack-library");
    let folder = root.join("fontelle").join("patches").join("examples");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Pluck.vcv"), br#"{"version":"2.0"}"#).unwrap();
    std::fs::write(folder.join("Broken.vcv"), b"MThd").unwrap();
    let own = b"__dpf_state_begin__\0patch\0KLUv\0__dpf_state_end__\0";
    let presets = fontelle_host::list_own_presets(&info, &roots_at(&root), Some(own), &[]);
    assert_eq!(files_of(&presets), vec![("Pluck", "examples")]);

    // A plugin whose state carries no patch is not offered them.
    let presets = fontelle_host::list_own_presets(&info, &roots_at(&root), Some(b"plain"), &[]);
    assert!(files_of(&presets).is_empty(), "{presets:?}");
}

/// amsynth's LV2 has no preset its host can list: its banks are text files
/// of parameters by name, and the name is its port's symbol.
#[cfg(target_os = "linux")]
fn bank_root(name: &str) -> PathBuf {
    let root = scratch(name);
    let folder = root.join("Fontelle Test Plain LV2").join("banks");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("Mine.amSynth.bank"),
        "amSynth\n<preset> <name> Half\n<parameter> gain 0.5\n<preset> <name> Stranger\n<parameter> gain 0.5\n<parameter> wobble 1\n<preset> <name> Flipped\n<parameter> invert 1\n<preset> <name> Half\n<parameter> gain 0.75\n",
    )
    .unwrap();
    std::fs::write(folder.join("readme"), "not a bank\n").unwrap();
    root
}

#[cfg(target_os = "linux")]
#[test]
fn an_lv2_plugins_bank_is_mapped_onto_its_ports_by_symbol() {
    let key = PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PLAIN_URI);
    let info = info_of(&common::lv2_bundle(), &key);
    let mut host = PluginHost::new();
    let ports = host.lv2_control_ports(&info);
    let symbols: Vec<(&str, u32, f32)> = ports
        .iter()
        .map(|p| (p.symbol.as_str(), p.index, p.default))
        .collect();
    assert_eq!(symbols, vec![("gain", 2, 1.0), ("invert", 3, 0.0)]);

    let root = bank_root("bank-listed");
    let presets = host.own_presets(&info, &roots_at(&root));
    let listed: Vec<(&str, &str)> = presets
        .iter()
        .map(|p| (p.name.as_str(), p.category.as_str()))
        .collect();
    // A preset naming a parameter the plugin has no port for is some other
    // plugin's, and is left out.
    // Two of one name in a bank (amsynth's have dozens) are both offered.
    assert_eq!(
        listed,
        vec![("Flipped", "Mine"), ("Half", "Mine"), ("Half (2)", "Mine")]
    );
    let OwnPresetSource::Lv2 { ports, state, .. } = &presets[1].source else {
        panic!("a bank preset is a set of port values: {:?}", presets[1]);
    };
    // Every port, the ones it does not name at their defaults — what amsynth
    // does with a preset older than a parameter.
    assert_eq!(ports, &vec![(2, 0.5), (3, 0.0)]);
    assert!(state.is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn a_bank_preset_is_loaded_into_the_running_instance() {
    let key = PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PLAIN_URI);
    let info = info_of(&common::lv2_bundle(), &key);
    let mut host = PluginHost::new();
    let presets = host.own_presets(&info, &roots_at(&bank_root("bank-loaded")));
    let half = presets.iter().find(|p| p.name == "Half").unwrap();
    let mut plugin = host.open(&common::lv2_bundle(), &key).unwrap();
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    plugin.load_own_preset_with(&mut processor, half).unwrap();
    assert_eq!(plugin.values().get(2), Some(0.5));
}

/// Vital keeps each pack as `<pack>/Presets/…`: the pack is the category,
/// and the folder every pack has is not.
#[test]
fn a_packs_presets_folder_is_not_part_of_its_category() {
    let (_, info) = clap_gain();
    let root = scratch("packs");
    let library = root.join("fontelle-test-gain");
    for folder in [
        library.join("Mr Bill").join("Presets"),
        library
            .join("Billain")
            .join("Presets")
            .join("Factory Presets"),
    ] {
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Wob.vital"), br#"{"author":"x","settings":{}}"#).unwrap();
    }
    let own = br#"{"author":"","settings":{}}"#;
    let presets = fontelle_host::list_own_presets(&info, &roots_at(&root), Some(own), &[]);
    assert_eq!(
        files_of(&presets),
        vec![("Wob", "Billain / Factory Presets"), ("Wob", "Mr Bill")]
    );
}
