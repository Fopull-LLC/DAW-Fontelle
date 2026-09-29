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
