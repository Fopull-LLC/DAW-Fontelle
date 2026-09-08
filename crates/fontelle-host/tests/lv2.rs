//! The second format: LV2, through lilv.
//!
//! > *"i agree with implementing LV2 for sure"*
//!
//! §8.4's order of intent was CLAP first and LV2 second, and this is second.
//! Everything here is the CLAP suite asked again of an LV2 bundle — found,
//! opened, read, set, heard, saved — because the whole claim of
//! `fontelle-host` is that a format is an arm in a `match` and not a second
//! host: the same `HostedPlugin`, the same `HostedProcessor`, the same wire a
//! knob writes on. Where LV2 differs (a bundle is a folder, a parameter is a
//! control port, there is no state extension in this build) the test says so.

mod common;

use fontelle_host::{HostError, PluginHost, PluginScan, scan_bundle, search_paths};
use fontelle_types::{PluginFormat, PluginKey};

fn gain_key() -> PluginKey {
    PluginKey::new(PluginFormat::Lv2, common::LV2_GAIN)
}

fn sine_key() -> PluginKey {
    PluginKey::new(PluginFormat::Lv2, common::LV2_SINE)
}

fn gain(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::lv2_bundle(), &gain_key())
        .expect("the LV2 test gain opens")
}

fn sine(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::lv2_bundle(), &sine_key())
        .expect("the LV2 test sine opens")
}

fn peak(buffers: &[Vec<f32>]) -> f32 {
    buffers
        .iter()
        .flat_map(|b| b.iter())
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

// ------------------------------------------------------------- finding it

#[test]
fn lv2_is_a_format_this_build_hosts() {
    assert!(PluginFormat::Lv2.hosted());
}

#[test]
fn the_folders_lv2_nominates_are_searched() {
    // `~/.lv2`, `/usr/lib/lv2` and `/usr/local/lib/lv2` are the LV2
    // specification's own list for Linux, and `LV2_PATH` overrides it.
    let paths = search_paths();
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    assert!(paths.contains(&home.join(".lv2")), "{paths:?}");
    assert!(paths.contains(&"/usr/lib/lv2".into()), "{paths:?}");
}

#[test]
fn an_lv2_bundle_is_a_folder_and_reports_every_plugin_in_it() {
    let found = scan_bundle(&common::lv2_bundle()).expect("the LV2 test bundle loads");
    // Four: the gain, the sine, the gain again under a name that ships no
    // editor (`fontelle_testlv2::PLAIN_URI`), and again under one whose
    // editor listens to nothing (`fontelle_testlv2::DEAF_URI`).
    assert_eq!(found.len(), 4, "{found:#?}");
    let ids: Vec<_> = found.iter().map(|p| p.key.id.as_str()).collect();
    assert!(ids.contains(&common::LV2_GAIN), "{ids:?}");
    assert!(ids.contains(&common::LV2_SINE), "{ids:?}");
    assert!(ids.contains(&fontelle_testlv2::PLAIN_URI), "{ids:?}");
    assert!(ids.contains(&fontelle_testlv2::DEAF_URI), "{ids:?}");
    for plugin in &found {
        assert_eq!(plugin.key.format, PluginFormat::Lv2);
        assert_eq!(plugin.path, common::lv2_bundle());
    }
}

#[test]
fn an_lv2_plugin_carries_what_a_menu_needs_to_show_it() {
    // Name, vendor and version come out of the Turtle, not the binary.
    let found = scan_bundle(&common::lv2_bundle()).unwrap();
    let gain = found.iter().find(|p| p.key.id == common::LV2_GAIN).unwrap();
    assert_eq!(gain.name, "Fontelle Test Gain LV2");
    assert_eq!(gain.vendor, "Fopull LLC");
    assert_eq!(gain.version, "1.0");
}

#[test]
fn what_an_lv2_plugin_can_be_is_read_off_its_classes_and_ports() {
    // `lv2:InstrumentPlugin` says instrument; an amplifier with an audio
    // input is an effect. Read off the bundle's own data rather than the
    // specification's class tree, so it holds for a bundle loaded on its own.
    let found = scan_bundle(&common::lv2_bundle()).unwrap();
    let gain = found.iter().find(|p| p.key.id == common::LV2_GAIN).unwrap();
    let sine = found.iter().find(|p| p.key.id == common::LV2_SINE).unwrap();
    assert!(gain.is_effect(), "{gain:#?}");
    assert!(!gain.is_instrument(), "{gain:#?}");
    assert!(sine.is_instrument(), "{sine:#?}");
    assert!(!sine.is_effect(), "{sine:#?}");
}

#[test]
fn a_scan_of_a_folder_finds_lv2_bundles_beside_clap_ones() {
    // One walk, two formats: an installer that put both kinds in one folder
    // gets one list.
    let dir = std::env::temp_dir().join(format!("fontelle-lv2-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Vendor")).unwrap();
    std::fs::copy(common::bundle(), dir.join("Vendor/Test.clap")).unwrap();
    copy_dir(&common::lv2_bundle(), &dir.join("Vendor/Test.lv2"));
    // An empty `.lv2` folder — what the specification bundles under
    // `/usr/lib/lv2` look like to a scanner — is not a failure.
    std::fs::create_dir_all(dir.join("atom.lv2")).unwrap();

    let scan = PluginScan::of(std::slice::from_ref(&dir));
    // Four CLAP and four LV2 — see the bundle test above for the third and
    // fourth LV2; `fontelle_testplug::FacePlugin` for the third CLAP, which
    // is a note effect and so on neither list below; and
    // `fontelle_testplug::SINE_CLAP_ONLY` for the fourth.
    assert_eq!(scan.plugins.len(), 8, "{:#?}", scan.plugins);
    assert!(scan.failures.is_empty(), "{:#?}", scan.failures);
    assert_eq!(scan.instruments().count(), 3);
    assert_eq!(scan.effects().count(), 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_with_the_extension_and_no_manifest_is_a_refusal_rather_than_a_crash() {
    let dir = std::env::temp_dir().join(format!("fontelle-not-lv2-{}.lv2", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a plugin").unwrap();
    // An `.lv2` folder holding nothing lilv can read holds no plugins; it is
    // an empty answer, not an error, because that is exactly what the
    // specification bundles are.
    assert_eq!(scan_bundle(&dir).unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    // But a *file* called `.lv2` is not a bundle at all.
    let file = std::env::temp_dir().join(format!("fontelle-file-{}.lv2", std::process::id()));
    std::fs::write(&file, "nope").unwrap();
    assert!(scan_bundle(&file).is_err());
    let _ = std::fs::remove_file(&file);
}

// -------------------------------------------------------------- opening it

#[test]
fn a_plugin_that_is_not_in_the_bundle_is_refused() {
    let mut host = PluginHost::new();
    let key = PluginKey::new(PluginFormat::Lv2, "http://example.com/nothing");
    match host.open(&common::lv2_bundle(), &key) {
        Err(HostError::NoSuchPlugin { .. }) => {}
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("a plugin that is not there opened"),
    }
}

#[test]
fn an_lv2_plugins_parameters_are_its_control_inputs() {
    // Each control input port is a parameter whose id is the port's index —
    // stable for the life of the plugin, which is what an automation lane
    // needs. Audio and atom ports are not parameters.
    let mut host = PluginHost::new();
    let plugin = gain(&mut host);
    let params = plugin.params();
    assert_eq!(params.len(), 2, "{params:#?}");

    let gain = plugin.param(2).expect("port 2 is the gain");
    assert_eq!(gain.name, "Gain");
    assert_eq!(gain.min, 0.0);
    assert_eq!(gain.max, 2.0);
    assert_eq!(gain.default, 1.0);
    assert!(!gain.stepped);

    let invert = plugin.param(3).expect("port 3 is the switch");
    assert_eq!(invert.name, "Invert");
    assert!(invert.stepped, "a toggled port is a stepped parameter");
    assert_eq!(invert.steps(), Some(2));
    assert!(
        plugin.param(0).is_none(),
        "the audio input is not a parameter"
    );
}

#[test]
fn an_lv2_plugin_says_what_it_is_and_what_it_can_be_wired_to() {
    let mut host = PluginHost::new();
    let gain = gain(&mut host);
    assert_eq!(gain.key(), &gain_key());
    assert_eq!(gain.name(), "Fontelle Test Gain LV2");
    assert_eq!(gain.audio_inputs(), 1);
    assert_eq!(gain.audio_outputs(), 1);
    assert!(!gain.accepts_notes());
    assert!(
        gain.keeps_state(),
        "the fixture gain declares state:interface"
    );

    let sine = sine(&mut host);
    assert_eq!(sine.audio_inputs(), 0);
    assert_eq!(sine.audio_outputs(), 1);
    assert!(
        sine.accepts_notes(),
        "an atom port that supports MIDI is a note input"
    );
}

#[test]
fn one_lv2_bundle_is_loaded_once_however_many_instances_come_out_of_it() {
    let mut host = PluginHost::new();
    let _a = gain(&mut host);
    let _b = gain(&mut host);
    let _c = sine(&mut host);
    assert_eq!(host.loaded_bundles(), 1);
}

// -------------------------------------------------------------- hearing it

#[test]
fn what_an_lv2_parameter_is_set_to_is_what_the_plugin_does() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    plugin.set_param(2, 1.5);

    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 0.75).abs() < 1e-5, "{:?}", output[0][0]);
    // A mono plugin lands on both sides of a stereo bus.
    assert!((output[1][0] - 0.75).abs() < 1e-5, "{:?}", output[1][0]);
}

#[test]
fn an_lv2_parameter_moved_while_it_is_playing_is_heard_on_the_next_block() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();

    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5, "{:?}", output[0][0]);

    plugin.set_param(2, 0.25);
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 0.25).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_value_past_a_control_ports_range_is_clamped_to_it() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    assert!(plugin.set_param(2, 40.0));
    assert_eq!(plugin.values().get(2), Some(2.0));
    assert!(!plugin.set_param(9, 1.0), "a port that is not there");
}

#[test]
fn a_toggled_port_does_what_its_positions_say() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    plugin.set_param(3, 1.0);
    let mut processor = plugin.activate(48_000.0, 16).unwrap();
    let input = vec![vec![0.5f32; 4]];
    let mut output = vec![vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!(
        (output[0][0] + 0.5).abs() < 1e-5,
        "inverted: {:?}",
        output[0][0]
    );
}

#[test]
fn an_lv2_instrument_sounds_when_a_key_is_pressed_and_stops_when_it_is_let_go() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();

    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "silent until a key is pressed");

    processor.note_on(0, 69, 1.0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "{}", peak(&output));

    processor.note_off(0, 69);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
}

#[test]
fn an_lv2_instruments_notes_are_placed_where_in_the_block_they_happened() {
    // The event's frame goes into the atom sequence and the fixture honours
    // it, so this is the one test that can see the host's placement at all.
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();

    processor.note_on(128, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    let before: f32 = output[0][..128].iter().fold(0.0, |a, b| a.max(b.abs()));
    let after: f32 = output[0][128..].iter().fold(0.0, |a, b| a.max(b.abs()));
    assert!(before < 1e-6, "{before}");
    assert!(after > 0.1, "{after}");
}

#[test]
fn a_reset_silences_an_lv2_instrument() {
    // LV2 has no reset call; the host sends "all notes off", which is what
    // the graph's own `reset` reaches for when the transport stops.
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 60, 1.0);
    let mut output = vec![vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.reset();
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
}

#[test]
fn an_lv2_instruments_level_is_a_parameter_like_any_other() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    plugin.set_param(2, 0.1);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    let quiet = peak(&output);
    assert!(quiet > 0.05 && quiet < 0.12, "{quiet}");
}

#[test]
fn an_lv2_plugin_can_be_stopped_and_started_again() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let processor = plugin.activate(48_000.0, 16).unwrap();
    assert!(plugin.is_active());
    plugin.deactivate(processor);
    assert!(!plugin.is_active());
    let mut processor = plugin.activate(44_100.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
}

#[test]
fn an_lv2_processor_survives_a_block_shorter_than_the_one_it_was_prepared_for() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 512).unwrap();
    let input = vec![vec![1.0f32; 512]];
    let mut output = vec![vec![0.0f32; 512]];
    processor.process_effect(&input, &mut output, 3);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
    assert_eq!(output[0][3], 0.0, "nothing past the block is touched");
}

// --------------------------------------------------------------- saving it

#[test]
fn an_lv2_plugin_is_saved_as_its_parameters_and_restored_from_them() {
    // The control ports, and — for a plugin that has not been activated and
    // was never handed a state — nothing else: there is no instance yet to
    // ask for a blob. `an_lv2_plugins_own_state_survives_a_snapshot_and_a_restore`
    // is the other half.
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    plugin.set_param(2, 1.75);
    plugin.set_param(3, 1.0);
    let state = plugin.snapshot();
    assert_eq!(state.key, gain_key());
    assert_eq!(state.name, "Fontelle Test Gain LV2");
    assert_eq!(state.param(2), Some(1.75));
    assert_eq!(state.param(3), Some(1.0));
    assert!(
        state.blob.is_none(),
        "no instance has run, so there is nothing to ask"
    );

    let mut again = gain(&mut host);
    assert!(again.restore(&state));
    assert_eq!(again.values().get(2), Some(1.75));
    assert_eq!(again.values().get(3), Some(1.0));
}

#[test]
fn a_state_from_a_clap_plugin_is_not_applied_to_an_lv2_one() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut other = fontelle_types::PluginState::new(PluginKey::clap("com.example.other"), "x");
    other.set_param(2, 0.0);
    assert!(!plugin.restore(&other));
    assert_eq!(plugin.values().get(2), Some(1.0), "nothing was applied");
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

// ----------------------------------------- the plugin's own editor (2026-09-05)

/// An LV2 plugin that ships an `ui:X11UI` has an editor this host can show.
///
/// > *"shouldnt these also be showing the custom plugins own display in their
/// > windows"* — and for LV2 that is most of what is installed: every bundle
/// > on the reporter's machine but Calf ships an X11 UI, including the
/// > autotune and the samplers whose whole state is a file they loaded.
#[test]
fn an_lv2_plugin_that_ships_an_x11_editor_says_it_has_one() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    assert!(plugin.has_editor(), "the fixture ships an ui:X11UI");
}

/// And one that ships none says so, rather than opening an empty window.
///
/// Calf's whole suite is this case — it declares no UI of any kind — so it is
/// worth a fixture of its own rather than an assumption.
#[test]
fn an_lv2_plugin_with_no_editor_says_it_has_none() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(
            &common::lv2_bundle(),
            &PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PLAIN_URI),
        )
        .expect("the plain LV2 test plugin opens");
    assert!(!plugin.has_editor());
}

/// Opening it loads the editor's binary, hands it the window, and drives its
/// idle loop — and the editor says so in the only way it can, by writing a
/// parameter.
///
/// See `fontelle_testlv2::HELLO_FROM_THE_EDITOR`: that number appears only if
/// the host found the UI, loaded it, passed `ui:parent`, and called `idle`.
#[test]
fn an_lv2_editor_is_given_a_window_and_driven() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    assert!(plugin.editor_is_open());

    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::GAIN_PORT),
        Some(f64::from(fontelle_testlv2::HELLO_FROM_THE_EDITOR)),
        "the editor writes this on its first idle, and only when it was parented"
    );
    plugin.close_editor();
    assert!(!plugin.editor_is_open());
}

/// A knob moved in Fontelle reaches the plugin's own editor.
///
/// The fixture editor mirrors whatever it is told about the gain onto the
/// invert port, so one assertion covers the whole round trip: host → editor
/// through `port_event`, editor → host through the write function.
#[test]
fn a_parameter_moved_in_the_studio_reaches_the_editor_and_comes_back() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    // Past the greeting, so what is measured is this change and not that one.
    plugin.tick_editor();

    plugin.set_param(fontelle_testlv2::GAIN_PORT, 0.25);
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::INVERT_PORT),
        Some(0.25),
        "the editor was told, and wrote what it was told somewhere we can see"
    );
    plugin.close_editor();
}

/// The editor's own write is **not** echoed back at it.
///
/// A host that told an editor about the value the editor had just sent would
/// be a feedback loop — and with a knob under the mouse, a fight over its
/// position. The fixture would make that visible: an echo of the greeting
/// would land on the invert port.
#[test]
fn an_editors_own_write_is_not_sent_back_to_it() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    plugin.tick_editor();
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::INVERT_PORT),
        Some(0.0),
        "nothing came back, so the mirror never fired"
    );
    plugin.close_editor();
}

/// Closing and reopening works: an editor is loaded and unloaded, not leaked.
#[test]
fn an_lv2_editor_can_be_closed_and_opened_again() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    plugin.close_editor();
    plugin.open_editor(&window, 1.0).expect("and again");
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::GAIN_PORT),
        Some(f64::from(fontelle_testlv2::HELLO_FROM_THE_EDITOR)),
        "the second editor greeted us too"
    );
    plugin.close_editor();
}

/// An editor can send the plugin an **atom**, not just a control value.
///
/// This is the whole reason an LV2 editor is worth hosting. A control port is
/// a float, and a float cannot say *"load this file"* — an LSP sampler's whole
/// state is a file it was handed, and it is handed one as a `patch:Set` atom
/// written by its editor. A host that carries floats and drops atoms opens a
/// sampler's editor onto a sampler that can never be given a sample.
///
/// The fixture's sine editor writes a MIDI note-on the same way, through
/// `atom:eventTransfer` on the plugin's atom port — so what is asserted here
/// is a sound, which is the only end of that path anybody cares about.
#[test]
fn an_editors_atom_reaches_the_plugin() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let window = fontelle_host::PluginWindow::headless(320, 240);
    plugin.open_editor(&window, 1.0).expect("the editor opens");

    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(
        peak(&output) < 1e-6,
        "silent before the editor says anything"
    );

    // The editor writes its note on its first idle. It reaches the plugin at
    // the top of the block **after** the one being rendered when it arrived —
    // see `Lv2Processor::run`, which fills the next block's sequence as it
    // finishes this one.
    plugin.tick_editor();
    processor.process_instrument(&mut output, 256);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "{}", peak(&output));
    plugin.close_editor();
}

/// And the plugin's own atoms reach the editor.
///
/// The other direction of the same wire: a plugin answers a `patch:Set` with a
/// `patch:Set` of its own saying what it actually loaded, and an editor that
/// never hears one shows an empty slot forever. The fixture's editor turns
/// whatever atom it is handed into a control value, which is something a test
/// can see.
#[test]
fn a_plugins_atom_reaches_the_editor() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let window = fontelle_host::PluginWindow::headless(320, 240);
    plugin.open_editor(&window, 1.0).expect("the editor opens");

    // The sine answers every note it is given with an atom of its own.
    plugin.tick_editor();
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    processor.process_instrument(&mut output, 256);
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::SINE_LEVEL_PORT),
        Some(f64::from(fontelle_testlv2::HEARD_AN_ATOM)),
        "the editor was handed the plugin's atom and said so"
    );
    plugin.close_editor();
}

/// An editor that needs the **running plugin** is handed it.
///
/// `instance-access` is the feature every DPF-built editor requires —
/// Cardinal, Dexed, drumsynth, and two dozen more of the bundles installed
/// on the reporter's machine — and without it they refuse to open at all:
/// *"Host does not support instance-access, cannot use UI"*. The fixture
/// editor checks that what it was handed really is the live `Gain` (by its
/// magic) before it says so.
#[test]
fn an_editor_that_needs_the_running_plugin_is_handed_it() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let _processor = plugin.activate(48_000.0, 64).unwrap();
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::GAIN_PORT),
        Some(f64::from(fontelle_testlv2::HELLO_FROM_THE_INSTANCE)),
        "the editor was given the instance, and it was the real one"
    );
    plugin.close_editor();
}

/// And a plugin that is not running has no instance to hand over, so the
/// editor is opened without one rather than with a stale pointer.
#[test]
fn an_editor_opened_before_the_plugin_runs_gets_no_instance() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::GAIN_PORT),
        Some(f64::from(fontelle_testlv2::HELLO_FROM_THE_EDITOR)),
    );
    plugin.close_editor();
}

/// Starting the plugin again makes a new instance and drops the old one —
/// and an editor holding the old one through `instance-access` would then be
/// holding freed memory. So the editor is closed first, every time.
#[test]
fn starting_the_plugin_again_closes_an_editor_that_could_be_holding_it() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let first = plugin.activate(48_000.0, 64).unwrap();
    let window = fontelle_host::PluginWindow::headless(640, 480);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    assert!(plugin.editor_is_open());
    drop(first);
    let _second = plugin.activate(48_000.0, 64).unwrap();
    assert!(
        !plugin.editor_is_open(),
        "an editor that outlived the instance it was handed"
    );
    // And a fresh one gets the fresh instance.
    plugin.open_editor(&window, 1.0).expect("opens again");
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(fontelle_testlv2::GAIN_PORT),
        Some(f64::from(fontelle_testlv2::HELLO_FROM_THE_INSTANCE)),
    );
    plugin.close_editor();
}

// ------------------------------------------ an editor that listens to nothing (2026-09-05)

/// A UI whose `port_event` is NULL is driven without being told anything.
///
/// LV2 says the member *may be NULL if the UI is not interested in any port
/// events*, and JuceOPL's is. The studio called it anyway — three times in a
/// minute, each a crash at address zero — because the descriptor was read
/// through a type that cannot be null, which let the optimiser drop the
/// check. The fixture's fourth plugin ships exactly that editor: no
/// `port_event`, no `extension_data`, nothing but instantiate and cleanup.
#[test]
fn an_editor_with_no_port_event_is_not_told_about_a_knob() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(
            &common::lv2_bundle(),
            &PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::DEAF_URI),
        )
        .expect("the deaf LV2 test plugin opens");
    assert!(
        plugin.has_editor(),
        "it ships an editor, one that does not listen"
    );
    let window = fontelle_host::PluginWindow::headless(320, 200);
    plugin.open_editor(&window, 1.0).expect("the editor opens");
    // The first tick would tell it every port; the second tells it this one.
    plugin.tick_editor();
    plugin.set_param(fontelle_testlv2::GAIN_PORT, 0.5);
    plugin.tick_editor();
    assert!(plugin.editor_is_open());
    assert_eq!(plugin.values().get(fontelle_testlv2::GAIN_PORT), Some(0.5));
    plugin.close_editor();
}

// ------------------------------------------- state that is not a port (2026-09-05)

/// A plugin that implements `state:interface` keeps state of its own.
///
/// Read off the Turtle (`lv2:extensionData state:interface`), which is how
/// every LV2 host answers it before instantiating anything. The plain plugin
/// is the same code under a URI that declares no such thing.
#[test]
fn an_lv2_plugin_with_a_state_interface_says_it_keeps_state() {
    let mut host = PluginHost::new();
    assert!(
        gain(&mut host).keeps_state(),
        "the gain declares state:interface"
    );
    assert!(!sine(&mut host).keeps_state(), "the sine does not");
    let plain = host
        .open(
            &common::lv2_bundle(),
            &PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PLAIN_URI),
        )
        .unwrap();
    assert!(!plain.keeps_state(), "nor does the plain gain");
}

/// What the fixture gain stores: how many blocks it has run, under a key no
/// control port exposes.
fn runs_in(state: &fontelle_types::PluginState) -> i32 {
    let blob = state.blob.as_deref().expect("the gain keeps a blob");
    let bytes = fontelle_types::decode_base64(blob).expect("base64");
    let decoded = fontelle_host::Lv2State::decode(&bytes).expect("Fontelle's own LV2 state form");
    let runs = decoded
        .properties
        .iter()
        .find(|property| property.key == fontelle_testlv2::STATE_RUNS_KEY)
        .expect("the run counter is stored");
    assert_eq!(runs.type_uri, "http://lv2plug.in/ns/ext/atom#Int");
    i32::from_ne_bytes(runs.value[..4].try_into().unwrap())
}

fn run_blocks(processor: &mut fontelle_host::HostedProcessor, blocks: usize) {
    let input = vec![vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8]];
    for _ in 0..blocks {
        processor.process_effect(&input, &mut output, 8);
    }
}

/// The whole of item 1: **set it, snapshot, open a second instance, restore,
/// read it back.**
///
/// The instance is inside the processor — LV2 has one object, not two — and
/// `save` may not run while `run` does, so a snapshot of a running plugin
/// is taken *with* its processor, on the main thread, which is what the
/// bay's recall is for. Here the test simply holds it.
#[test]
fn an_lv2_plugins_own_state_survives_a_snapshot_and_a_restore() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    run_blocks(&mut processor, 5);
    let state = plugin.snapshot_with(&mut processor);
    assert_eq!(state.key, gain_key());
    assert_eq!(runs_in(&state), 5, "five blocks ran before the snapshot");

    // A second instance, restored before it ever runs — which is when a
    // project reopens — carries the count into its first block.
    let mut again = gain(&mut host);
    assert!(again.restore(&state));
    let mut second = again.activate(48_000.0, 64).unwrap();
    run_blocks(&mut second, 1);
    assert_eq!(runs_in(&again.snapshot_with(&mut second)), 6);
    plugin.deactivate(processor);
    again.deactivate(second);
}

/// A path in the plugin's state goes through `state:mapPath` both ways.
///
/// A sampler saves *a reference to a file*, and the specification's answer
/// to "where will that file be when the project opens" is `mapPath`: the
/// host turns an absolute path into an abstract one to store and back again
/// on restore. This host maps **identity** — the abstract path *is* the
/// absolute one, exactly as an audio clip referenced in place is named by
/// where it is (§17.4's headless default: reference, never copy). The
/// fixture stores its own bundle folder as an `atom:Path` and refuses a
/// restore whose path does not come back as the folder it lives in, so the
/// second `restore` below is the round trip asserting itself.
#[test]
fn a_path_in_an_lv2_plugins_state_is_stored_whole_and_mapped_back_on_restore() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let state = plugin.snapshot_with(&mut processor);
    let bytes = fontelle_types::decode_base64(state.blob.as_deref().unwrap()).unwrap();
    let decoded = fontelle_host::Lv2State::decode(&bytes).unwrap();
    let home = decoded
        .properties
        .iter()
        .find(|property| property.key == fontelle_testlv2::STATE_HOME_KEY)
        .expect("the bundle path is stored");
    assert_eq!(home.type_uri, "http://lv2plug.in/ns/ext/atom#Path");
    let stored = std::str::from_utf8(home.value.strip_suffix(&[0]).unwrap_or(&home.value)).unwrap();
    assert_eq!(
        std::path::Path::new(stored.trim_end_matches('/')),
        common::lv2_bundle().as_path(),
        "identity: the abstract path is the absolute one"
    );

    let mut again = gain(&mut host);
    assert!(
        again.restore(&state),
        "the fixture refuses a path that did not map back"
    );
    let second = again.activate(48_000.0, 64).unwrap();
    plugin.deactivate(processor);
    again.deactivate(second);
}

/// A snapshot needs the processor only while the plugin is **running**.
///
/// Before activation there is no instance to ask; after deactivation there
/// is none again. In between, the instance is out in the graph, and the
/// rack has to recall it — this is the question the rack asks first.
#[test]
fn an_lv2_plugin_says_when_its_snapshot_needs_the_processor() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    assert!(!plugin.state_needs_processor(), "nothing running yet");
    let processor = plugin.activate(48_000.0, 64).unwrap();
    assert!(
        plugin.state_needs_processor(),
        "the instance is out with the processor"
    );
    plugin.deactivate(processor);
    assert!(!plugin.state_needs_processor());

    let mut sine = sine(&mut host);
    let processor = sine.activate(48_000.0, 64).unwrap();
    assert!(
        !sine.state_needs_processor(),
        "a plugin with no state has nothing to fetch"
    );
    sine.deactivate(processor);
}

/// What was restored into a plugin that has not run yet is what a snapshot
/// gives back — so a project opened and saved again on a machine where the
/// audio never started still carries the sampler's file.
#[test]
fn a_restored_state_is_kept_until_the_plugin_runs_and_given_back_meanwhile() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    run_blocks(&mut processor, 3);
    let state = plugin.snapshot_with(&mut processor);
    plugin.deactivate(processor);
    // Deactivating reads the state off the instance before it goes, so a
    // plugin that has stopped still answers with the last thing it was.
    assert_eq!(runs_in(&plugin.snapshot()), 3, "kept across deactivation");

    let mut again = gain(&mut host);
    assert!(again.restore(&state));
    let kept = again.snapshot();
    assert_eq!(kept.blob, state.blob, "given back untouched, blob and all");
    assert_eq!(runs_in(&kept), 3);
}

/// And a plugin with no state interface still has no blob, processor or not.
#[test]
fn an_lv2_plugin_without_the_interface_has_no_blob_even_while_running() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let state = plugin.snapshot_with(&mut processor);
    assert!(state.blob.is_none());
    plugin.deactivate(processor);
}

// ------------------------------------------- a key with nowhere to go (2026-09-05)

/// An LV2 insert given a key it has no port for ignores it and processes
/// the bus as it always did.
///
/// The plain plugin declares no `lv2:isSideChain` port, so a key on it is an
/// edge that orders the graph and feeds nothing — and the bus still goes
/// through.
#[test]
fn an_lv2_insert_given_a_key_it_has_no_port_for_still_processes_the_bus() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(
            &common::lv2_bundle(),
            &PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PLAIN_URI),
        )
        .unwrap();
    assert!(!plugin.takes_key());
    plugin.set_param(2, 0.5);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let mut bus = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    let key = vec![1.0f32; 8];
    processor.process_insert_keyed(&mut bus, &key, 8);
    assert!((bus[0][0] - 0.5).abs() < 1e-5, "{}", bus[0][0]);
    plugin.deactivate(processor);
}

// ------------------------------ pitch bend, mod wheel, aftertouch (2026-09-05)

fn crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count()
}

/// The mod wheel reaches an LV2 instrument as the three MIDI bytes it was,
/// in the same atom sequence its notes arrive in. The fixture scales its
/// level by the wheel.
#[test]
fn a_mod_wheel_reaches_an_lv2_instrument() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.controller(0, 1, 0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
    processor.controller(0, 1, 127);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "{}", peak(&output));
}

#[test]
fn a_pitch_bend_reaches_an_lv2_instrument() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 4096).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 4096]];
    processor.process_instrument(&mut output, 4096);
    let unbent = crossings(&output[0]);
    processor.pitch_bend(0, 8191);
    processor.process_instrument(&mut output, 4096);
    let bent = crossings(&output[0]);
    assert!(bent > unbent + 5, "unbent {unbent}, bent {bent}");
}

#[test]
fn channel_pressure_reaches_an_lv2_instrument() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.channel_pressure(0, 127);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
}

/// A controller on a plugin with no note input goes nowhere, quietly.
#[test]
fn a_controller_on_an_lv2_effect_is_ignored() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 16).unwrap();
    processor.controller(0, 1, 0);
    processor.pitch_bend(0, 100);
    let input = vec![vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
}

// ------------------------------------------------ the sidechain (2026-09-06)

/// LV2 declares a sidechain with a port property, `lv2:isSideChain`, on an
/// audio input. The host reads it off the Turtle and presents the plugin the
/// way it presents a CLAP plugin with a second input port: the main input is
/// what the bus feeds, the key is its own port, and `takes_key` says so.
#[test]
fn an_lv2_plugin_declares_its_sidechain_with_the_port_property() {
    let mut host = PluginHost::new();
    let gain = gain(&mut host);
    assert!(
        gain.takes_key(),
        "the fixture gain's port 4 is lv2:isSideChain"
    );
    // The main port and the key, each mono; the bus feeds the main one only.
    assert_eq!(gain.input_ports(), &[1, 1]);
    assert_eq!(
        gain.audio_inputs(),
        1,
        "the main port's channels, not every input"
    );

    let plain = host
        .open(
            &common::lv2_bundle(),
            &PluginKey::new(PluginFormat::Lv2, fontelle_testlv2::PLAIN_URI),
        )
        .unwrap();
    assert!(!plain.takes_key());
    assert_eq!(plain.input_ports(), &[1]);
}

/// What arrives on the key changes what the plugin does — and only what
/// arrives on the key. The fixture ducks by the key sample for sample, so a
/// full-scale key silences the bus, a silent key leaves it alone, and a
/// block run **without** a key after one run with a key is not still
/// ducking: the key port is written every block, silence when there is
/// nothing to put there.
#[test]
fn a_signal_on_an_lv2_sidechain_changes_what_the_plugin_does() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    plugin.set_param(2, 1.0);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();

    let mut bus = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    let key = vec![1.0f32; 8];
    processor.process_insert_keyed(&mut bus, &key, 8);
    assert!(
        peak(&bus) < 1e-6,
        "a full-scale key ducks the bus to nothing: {}",
        peak(&bus)
    );

    let mut bus = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    let quiet = vec![0.0f32; 8];
    processor.process_insert_keyed(&mut bus, &quiet, 8);
    assert!(
        (bus[0][0] - 1.0).abs() < 1e-5,
        "a silent key lets it all through: {}",
        bus[0][0]
    );

    // Keyed once, then run as a plain insert: the key does not linger, and
    // the bus is **not** put on the key port (it would duck itself).
    let mut bus = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    processor.process_insert_keyed(&mut bus, &key, 8);
    let mut bus = vec![vec![1.0f32; 8], vec![1.0f32; 8]];
    processor.process_insert(&mut bus, 8);
    assert!((bus[0][0] - 1.0).abs() < 1e-5, "{}", bus[0][0]);
    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    processor.process_effect(&[vec![0.5f32; 8]], &mut bus, 8);
    assert!((bus[0][0] - 0.5).abs() < 1e-5, "{}", bus[0][0]);
    plugin.deactivate(processor);
}

// ------------------------------------------------- slides (2026-09-06)

/// MIDI has no per-note pitch, so a slide reaches an LV2 instrument as a
/// **channel pitch bend** — and a bend reaches two semitones either way,
/// the range every keyboard defaults to. A slide farther than that lands at
/// the limit rather than being invented as something else; the honest
/// carrier for more would be MPE, which this build does not speak.
#[test]
fn a_slide_reaches_an_lv2_instrument_as_a_bend_within_two_semitones() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 4096).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 4096]];
    processor.process_instrument(&mut output, 4096);
    let unbent = crossings(&output[0]);

    processor.pitch_bend(0, 8191);
    processor.process_instrument(&mut output, 4096);
    let full_bend = crossings(&output[0]);
    assert!(full_bend > unbent + 5, "unbent {unbent}, bent {full_bend}");

    processor.note_tuning(0, 69, 2.0);
    processor.process_instrument(&mut output, 4096);
    let two = crossings(&output[0]);
    assert!(
        two.abs_diff(full_bend) <= 1,
        "two semitones is the full bend: {two} vs {full_bend}"
    );

    processor.note_tuning(0, 69, 12.0);
    processor.process_instrument(&mut output, 4096);
    let twelve = crossings(&output[0]);
    assert!(
        twelve.abs_diff(full_bend) <= 1,
        "an octave is clamped to the range: {twelve} vs {full_bend}"
    );

    processor.note_tuning(0, 69, 0.0);
    processor.process_instrument(&mut output, 4096);
    let back = crossings(&output[0]);
    assert!(
        back.abs_diff(unbent) <= 1,
        "back to unbent: {back} vs {unbent}"
    );
}
