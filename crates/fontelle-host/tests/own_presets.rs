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
