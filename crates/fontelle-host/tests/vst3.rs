//! The third format hosted in the tree: VST 3, through the `vst3` crate.
//!
//! > *"a lot of people may not want to switch to the daw if they can't use
//! > their paid vsts in it ... maximum compatibility is what's most
//! > important to me."*
//!
//! `docs/vst-plan.md` §1: Steinberg released the VST 3 SDK under MIT on
//! 31 October 2025, which put it on §3.4's allowlist and took away every
//! reason the format was behind a bridge. This is the CLAP and LV2 suites
//! asked a third time — found, opened, read, set, heard, saved, shown —
//! because the whole claim of `fontelle-host` is that a format is an arm in
//! a `match` and not a second host. Where VST 3 differs (a bundle is a
//! folder with a `moduleinfo.json`, a parameter is **normalised** on the
//! wire, the wheels are parameters the plugin maps, state is two blobs) the
//! test says so.
//!
//! The fixture is `fontelle-testvst3`: a real VST 3 module written with the
//! same crate, so every test here has a twin in `hosting.rs`.

mod common;

use fontelle_host::{HostError, PluginHost, PluginScan, scan_bundle, search_paths};
use fontelle_types::{PluginFormat, PluginKey, PluginState};

fn gain_key() -> PluginKey {
    PluginKey::new(PluginFormat::Vst3, common::VST3_GAIN)
}

fn sine_key() -> PluginKey {
    PluginKey::new(PluginFormat::Vst3, common::VST3_SINE)
}

fn gain(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::vst3_bundle(), &gain_key())
        .expect("the VST 3 test gain opens")
}

fn sine(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::vst3_bundle(), &sine_key())
        .expect("the VST 3 test sine opens")
}

fn peak(buffers: &[Vec<f32>]) -> f32 {
    buffers
        .iter()
        .flat_map(|b| b.iter())
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

fn crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count()
}

// ------------------------------------------------------------- finding it

#[test]
fn vst3_is_a_format_this_build_hosts() {
    assert!(PluginFormat::Vst3.hosted());
    assert!(PluginHost::new().can_host(PluginFormat::Vst3));
}

#[test]
fn the_folders_vst3_nominates_are_searched() {
    // The VST 3 SDK's own list for Linux: `~/.vst3`, `/usr/lib/vst3`,
    // `/usr/local/lib/vst3`; `VST3_PATH` is honoured the way `CLAP_PATH` is.
    let paths = search_paths();
    #[cfg(target_os = "linux")]
    {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        assert!(paths.contains(&home.join(".vst3")), "{paths:?}");
        assert!(paths.contains(&"/usr/lib/vst3".into()), "{paths:?}");
        assert!(paths.contains(&"/usr/local/lib/vst3".into()), "{paths:?}");
    }
    #[cfg(not(target_os = "linux"))]
    assert!(!paths.is_empty());
}

#[test]
fn a_vst3_bundle_is_a_folder_and_reports_every_plugin_in_it() {
    let found = scan_bundle(&common::vst3_bundle()).expect("the bundle scans");
    let keys: Vec<&PluginKey> = found.iter().map(|p| &p.key).collect();
    assert!(keys.contains(&&gain_key()), "{keys:?}");
    assert!(keys.contains(&&sine_key()), "{keys:?}");
    assert!(
        keys.contains(&&PluginKey::new(PluginFormat::Vst3, common::VST3_COMBINED)),
        "{keys:?}"
    );
    // Controller classes are half of a plugin, not a plugin: the listing
    // holds the audio module classes alone.
    assert_eq!(found.len(), 3, "{found:#?}");
}

#[test]
fn a_vst3_plugin_carries_what_a_menu_needs_to_show_it() {
    let found = scan_bundle(&common::vst3_bundle()).unwrap();
    let gain = found.iter().find(|p| p.key == gain_key()).unwrap();
    assert_eq!(gain.name, "Fontelle Test Gain (VST3)");
    assert_eq!(gain.vendor, "Fopull LLC");
    assert_eq!(gain.version, "1.0.0");
    assert!(gain.is_effect());
    assert!(!gain.is_instrument());
    let sine = found.iter().find(|p| p.key == sine_key()).unwrap();
    assert!(sine.is_instrument(), "{:?}", sine.features);
}

/// A bundle that ships a `moduleinfo.json` (SDK 3.7.9 and later) is read
/// from it **without loading the library** — the scan reads what the file
/// says, trailing commas and all, which is how a folder of forty plugins is
/// scanned in the time it takes to read forty small files.
#[test]
fn a_bundle_with_a_moduleinfo_is_read_without_being_loaded() {
    let found = scan_bundle(&common::vst3_bundle_with_moduleinfo()).unwrap();
    // The listing carries a class the library does not: proof it was read
    // off the file, not off the factory.
    assert!(
        found.iter().any(|p| p.name == common::PHANTOM_NAME),
        "{found:#?}"
    );
    assert!(found.iter().any(|p| p.key == gain_key()), "{found:#?}");
}

#[test]
fn a_scan_of_a_folder_finds_vst3_bundles_beside_clap_ones() {
    let bundle = common::vst3_bundle();
    let folder = bundle.parent().unwrap().to_path_buf();
    let scan = PluginScan::of(&[folder]);
    assert!(scan.find(&gain_key()).is_some(), "{:#?}", scan.failures);
    assert!(scan.find(&sine_key()).is_some());
}

#[test]
fn a_folder_with_the_extension_and_no_library_is_a_refusal_rather_than_a_crash() {
    let dir = std::env::temp_dir().join(format!("fontelle-vst3-empty-{}", std::process::id()));
    let bundle = dir.join("nothing.vst3");
    std::fs::create_dir_all(&bundle).unwrap();
    let error = scan_bundle(&bundle).err();
    assert!(error.is_some(), "{error:?}");
    let scan = PluginScan::of(std::slice::from_ref(&dir));
    assert_eq!(scan.failures.len(), 1, "{:#?}", scan.failures);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_plugin_that_is_not_in_the_bundle_is_refused() {
    let mut host = PluginHost::new();
    let key = PluginKey::new(PluginFormat::Vst3, "00000000000000000000000000000000");
    let error = host.open(&common::vst3_bundle(), &key).err();
    assert!(
        matches!(error, Some(HostError::NoSuchPlugin { .. })),
        "{error:?}"
    );
}

// --------------------------------------------------------------- opening it

/// VST 3 parameters are **normalised on the wire** — every value a plugin
/// exchanges with its host runs 0..1 and `normalizedParamToPlain` is a
/// controller call, which is main-thread only. So a hosted VST 3 parameter's
/// range *is* 0..1 for a continuous one, and 0..steps for a stepped one; the
/// plugin's own units appear through [`display`](fontelle_host::HostedPlugin::display).
#[test]
fn a_vst3_plugins_parameters_are_its_controllers_list() {
    let mut host = PluginHost::new();
    let plugin = gain(&mut host);
    let params = plugin.params();
    let gain = params.iter().find(|p| p.id == 0).expect("{params:#?}");
    assert_eq!(gain.name, "Gain");
    assert_eq!(gain.min, 0.0);
    assert_eq!(gain.max, 1.0);
    assert!((gain.default - 0.25).abs() < 1e-9);
    assert!(!gain.stepped);

    let invert = params.iter().find(|p| p.id == 1).unwrap();
    assert_eq!(invert.name, "Invert");
    assert!(invert.stepped);
    assert_eq!(invert.steps(), Some(2));
    assert_eq!(invert.min, 0.0);
    assert_eq!(invert.max, 1.0);

    let hidden = params.iter().find(|p| p.id == 2).unwrap();
    assert!(hidden.hidden, "kIsHidden is honoured");
    let meter = params.iter().find(|p| p.id == 3).unwrap();
    assert!(meter.readonly, "kIsReadOnly is honoured");
}

#[test]
fn a_vst3_parameter_is_displayed_the_way_the_plugin_would() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    // The fixture formats its gain as a multiplier of four.
    assert_eq!(plugin.display(0, 0.5).as_deref(), Some("2.00 x"));
    assert_eq!(plugin.display(1, 1.0).as_deref(), Some("On"));
}

#[test]
fn a_vst3_plugin_says_what_it_is_and_what_it_can_be_wired_to() {
    let mut host = PluginHost::new();
    let gain = gain(&mut host);
    assert_eq!(gain.audio_inputs(), 2);
    assert_eq!(gain.audio_outputs(), 2);
    assert!(!gain.accepts_notes());
    // Every VST 3 component has a state stream; whether it writes anything
    // into it is its own business, so every one keeps state.
    assert!(gain.keeps_state());

    let sine = sine(&mut host);
    assert_eq!(sine.audio_inputs(), 0);
    assert_eq!(sine.audio_outputs(), 2);
    assert!(sine.accepts_notes());
    assert_eq!(
        sine.note_dialect(),
        Some(fontelle_host::NoteDialect::Midi),
        "an event bus takes notes and, through IMidiMapping, the wheels"
    );
}

#[test]
fn one_vst3_bundle_is_loaded_once_however_many_instances_come_out_of_it() {
    let mut host = PluginHost::new();
    let _a = gain(&mut host);
    let _b = sine(&mut host);
    let _c = gain(&mut host);
    assert_eq!(host.loaded_bundles(), 1);
}

/// A plugin whose component **is** its controller — one object answering
/// both interfaces, which the specification allows — opens like any other.
#[test]
fn a_single_component_plugin_opens_like_any_other() {
    let mut host = PluginHost::new();
    let key = PluginKey::new(PluginFormat::Vst3, common::VST3_COMBINED);
    let mut plugin = host
        .open(&common::vst3_bundle(), &key)
        .expect("the combined plugin opens");
    assert_eq!(plugin.params().len(), 1);
    assert_eq!(plugin.audio_inputs(), 1);
    assert_eq!(plugin.audio_outputs(), 1);
    plugin.set_param(0, 0.5);
    let mut processor = plugin.activate(48_000.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 0.5).abs() < 1e-5, "{}", output[0][0]);
}

// --------------------------------------------------------------- hearing it

#[test]
fn what_a_vst3_parameter_is_set_to_is_what_the_plugin_does() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    // 0.75 normalised is a gain of three on the fixture.
    plugin.set_param(0, 0.75);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 1.5).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_vst3_parameter_moved_while_it_is_playing_is_heard_on_the_next_block() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 0.5).abs() < 1e-5, "{:?}", output[0][0]);
    plugin.set_param(0, 0.5);
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 1.0).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_stepped_vst3_parameter_does_what_its_positions_say() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    plugin.set_param(1, 1.0);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!(
        (output[0][0] + 0.5).abs() < 1e-5,
        "inverted: {:?}",
        output[0][0]
    );
}

#[test]
fn a_vst3_instrument_sounds_when_a_key_is_pressed_and_stops_when_it_is_let_go() {
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
fn a_vst3_instruments_notes_are_placed_where_in_the_block_they_happened() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(128, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    let before: f32 = output[0][..128].iter().fold(0.0, |a, b| a.max(b.abs()));
    let after: f32 = output[0][128..].iter().fold(0.0, |a, b| a.max(b.abs()));
    assert!(before < 1e-6, "{before}");
    assert!(after > 0.1, "{after}");
}

#[test]
fn a_reset_silences_a_vst3_instrument() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.reset();
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
}

#[test]
fn a_vst3_processor_survives_a_block_shorter_than_the_one_it_was_prepared_for() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let input = vec![vec![0.5f32; 16], vec![0.5f32; 16]];
    let mut output = vec![vec![0.0f32; 16], vec![0.0f32; 16]];
    processor.process_effect(&input, &mut output, 16);
    assert!((output[0][15] - 0.5).abs() < 1e-5);
}

#[test]
fn a_vst3_plugin_can_be_stopped_and_started_again() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let processor = plugin.activate(48_000.0, 16).unwrap();
    assert!(plugin.is_active());
    plugin.deactivate(processor);
    assert!(!plugin.is_active());
    let mut processor = plugin.activate(44_100.0, 32).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
}

// ----------------------------------------------------------- every port

/// The CLAP lesson applies word for word: every bus the plugin declares is
/// activated and handed a buffer. The fixture sine has a mono *sub* output
/// beside its main pair and renders nothing when handed fewer.
#[test]
fn every_bus_a_vst3_plugin_declares_is_handed_over() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    assert_eq!(plugin.output_ports(), &[2, 1], "main pair, then the sub");
    assert_eq!(plugin.input_ports(), &[] as &[u32]);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    processor.note_on(0, 60, 1.0);
    let mut out = vec![vec![0.0f32; 64]; 2];
    processor.process_instrument(&mut out, 64);
    assert!(peak(&out) > 0.1, "{}", peak(&out));
}

/// A VST 3 sidechain is an **aux** input bus, and the fixture gain ducks by
/// whatever arrives on it.
#[test]
fn a_vst3_aux_input_is_the_sidechain() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    assert_eq!(plugin.input_ports(), &[2, 2], "main pair, then the key");
    assert!(plugin.takes_key());
    plugin.set_param(0, 0.5);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();

    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let quiet = vec![0.0f32; 8];
    processor.process_insert_keyed(&mut bus, &quiet, 8);
    assert!((bus[0][0] - 1.0).abs() < 1e-5, "no key: {}", bus[0][0]);

    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let loud = vec![1.0f32; 8];
    processor.process_insert_keyed(&mut bus, &loud, 8);
    assert!(bus[0][0].abs() < 1e-5, "ducked: {}", bus[0][0]);
}

// ------------------------------------------------------------- the wheels

/// VST 3 has no controller events: a wheel is a **parameter the plugin
/// maps** through `IMidiMapping`, resolved once at open and driven as a
/// parameter change. The fixture scales its level by the wheel.
#[test]
fn a_mod_wheel_reaches_a_vst3_instrument_through_its_midi_mapping() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.controller(0, 1, 0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "the wheel at zero: {}", peak(&output));
    processor.controller(0, 1, 127);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "and back up: {}", peak(&output));
}

#[test]
fn a_pitch_bend_reaches_a_vst3_instrument_through_its_midi_mapping() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 4096).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 4096], vec![0.0f32; 4096]];
    processor.process_instrument(&mut output, 4096);
    let unbent = crossings(&output[0]);
    processor.pitch_bend(0, 8191);
    processor.process_instrument(&mut output, 4096);
    let bent = crossings(&output[0]);
    assert!(bent > unbent + 5, "unbent {unbent}, bent {bent}");
}

#[test]
fn channel_pressure_reaches_a_vst3_instrument_through_its_midi_mapping() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.channel_pressure(0, 127);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
}

/// A slide is per-note pitch, and VST 3 carries exactly that: a note
/// expression of `kTuningTypeID` addressed to the note. An octave up is an
/// octave up, not the two semitones a channel bend would reach.
#[test]
fn a_slide_reaches_a_vst3_instrument_as_tuning_on_the_note() {
    let mut host = PluginHost::new();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 4096).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 4096], vec![0.0f32; 4096]];
    processor.process_instrument(&mut output, 4096);
    let unbent = crossings(&output[0]);
    processor.note_tuning(0, 69, 12.0);
    processor.process_instrument(&mut output, 4096);
    let octave = crossings(&output[0]);
    assert!(
        octave > unbent * 2 - 6 && octave < unbent * 2 + 6,
        "unbent {unbent}, an octave up {octave}"
    );
}

// ------------------------------------------------------------------ state

/// State is **two blobs**: the component's and the controller's, both
/// `IBStream`s, kept opaque the way an LV2 blob is.
#[test]
fn a_vst3_plugins_own_state_is_saved_and_given_back() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    plugin.set_param(0, 0.625);
    let state = plugin.snapshot();
    assert!(state.blob.is_some());
    assert_eq!(state.param(0), Some(0.625));

    let mut other = gain(&mut host);
    assert!(other.restore(&state));
    assert_eq!(
        other.values().get(0),
        Some(0.625),
        "the wire follows the blob"
    );
    let mut processor = other.activate(48_000.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 2.5).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_vst3_controllers_own_state_rides_beside_the_components() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let bytes = plugin.save_state().expect("a blob");
    // The fixture controller writes a marker of its own; the round trip
    // carries it back to a fresh controller, which says so through a
    // read-only parameter.
    assert!(
        bytes
            .windows(4)
            .any(|w| w == fontelle_testvst3::CONTROLLER_MAGIC),
        "{bytes:?}"
    );
    let mut other = gain(&mut host);
    assert!(other.load_state(&bytes));
    assert_eq!(
        other
            .values()
            .get(fontelle_testvst3::SEEN_CONTROLLER_STATE_PARAM),
        Some(1.0)
    );
}

#[test]
fn a_state_from_a_clap_plugin_is_not_applied_to_a_vst3_one() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut state = PluginState::new(PluginKey::clap(common::GAIN), "Fontelle Test Gain");
    state.set_param(0, 4.0);
    assert!(!plugin.restore(&state));
}

#[test]
fn a_damaged_vst3_blob_leaves_the_plugin_at_its_defaults() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let mut state = PluginState::new(gain_key(), "Fontelle Test Gain (VST3)");
    state.blob = Some("!!!! not base64 !!!!".to_string());
    assert!(plugin.restore(&state));
    let mut processor = plugin.activate(48_000.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5, "{:?}", output[0][0]);
}

// ---------------------------------------------------------------- latency

#[test]
fn a_vst3_plugin_reports_the_latency_it_declares() {
    let mut host = PluginHost::new();
    let plugin = gain(&mut host);
    assert_eq!(
        plugin.latency_samples(),
        fontelle_testvst3::GAIN_LATENCY_SAMPLES
    );
    let sine = sine(&mut host);
    assert_eq!(sine.latency_samples(), 0);
}

// ----------------------------------------------------------------- editor

#[test]
fn a_vst3_plugin_with_an_x11_view_says_it_has_an_editor() {
    let mut host = PluginHost::new();
    let mut gain = gain(&mut host);
    assert!(gain.has_editor());
    let mut sine = sine(&mut host);
    assert!(!sine.has_editor(), "the sine has no view at all");
}

#[test]
fn a_vst3_editor_is_given_a_window_and_driven() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(100, 100);
    let size = plugin.open_editor(&window, 1.0).expect("the view attaches");
    assert_eq!(
        (size.width, size.height),
        (
            fontelle_testvst3::VIEW_WIDTH,
            fontelle_testvst3::VIEW_HEIGHT
        )
    );
    assert!(plugin.editor_is_open());
    // The view registers a timer with the host's run loop on attach and
    // counts the times it fires; the third fire performs an edit on the
    // gain through the component handler, which is how a knob in a plugin's
    // own editor reaches the document.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while plugin.values().get(0) != Some(0.5) && std::time::Instant::now() < deadline {
        plugin.tick_editor();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        plugin.values().get(0),
        Some(0.5),
        "the editor's edit arrived"
    );
    plugin.close_editor();
    assert!(!plugin.editor_is_open());
}

#[test]
fn a_vst3_editor_that_asks_to_be_resized_is_heard() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(100, 100);
    plugin.open_editor(&window, 1.0).unwrap();
    // The fixture asks its frame for a new size on its first timer fire.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut asked = None;
    while asked.is_none() && std::time::Instant::now() < deadline {
        plugin.tick_editor();
        asked = plugin.take_editor_requests().resize;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        asked,
        Some(fontelle_host::GuiSize {
            width: fontelle_testvst3::VIEW_WIDTH * 2,
            height: fontelle_testvst3::VIEW_HEIGHT * 2
        })
    );
    assert!(plugin.editor_resizable());
    plugin.resize_editor(fontelle_host::GuiSize {
        width: 640,
        height: 400,
    });
    plugin.close_editor();
}

#[test]
fn a_vst3_editor_can_be_closed_and_opened_again() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    let window = fontelle_host::PluginWindow::headless(100, 100);
    plugin.open_editor(&window, 1.0).unwrap();
    plugin.close_editor();
    plugin.open_editor(&window, 1.0).unwrap();
    assert!(plugin.editor_is_open());
    plugin.close_editor();
}

/// A plugin asking to be restarted — its latency changed, its parameters
/// were reloaded — is *recorded*, the way a CLAP plugin's request is, and
/// acted on between frames.
#[test]
fn a_vst3_plugin_that_asks_for_a_restart_is_heard_between_frames() {
    let mut host = PluginHost::new();
    let mut plugin = gain(&mut host);
    assert!(!plugin.wants_restart());
    // The fixture asks for one when its hidden parameter is set to one.
    plugin.set_param(2, 1.0);
    assert!(plugin.wants_restart());
    assert!(!plugin.wants_restart(), "taken once");
}

// ------------------------------------------------------ the real thing

/// Every VST 3 plugin installed on this machine opens, activates and runs a
/// block; every instrument among them sounds when played. Ignored because
/// it depends on what is installed: on the machine this was written on,
/// Surge XT, Dexed, the Dragonfly reverbs and the LSP suite (2026-09-14).
///
/// `cargo test -p fontelle-host --test vst3 -- --ignored --nocapture`
#[test]
#[ignore]
fn every_vst3_plugin_on_this_machine_opens_and_runs() {
    let scan = PluginScan::of(&search_paths());
    for failure in &scan.failures {
        eprintln!("could not scan {}: {}", failure.path.display(), failure.why);
    }
    // `FONTELLE_VST3_ONLY=Surge` narrows it to names containing that.
    let only = std::env::var("FONTELLE_VST3_ONLY").ok();
    let vst3: Vec<_> = scan
        .plugins
        .iter()
        .filter(|p| p.key.format == PluginFormat::Vst3)
        .filter(|p| only.as_ref().is_none_or(|only| p.name.contains(only)))
        .collect();
    assert!(!vst3.is_empty(), "no VST 3 plugins installed");
    let mut host = PluginHost::new();
    let mut opened = 0;
    let mut sounded = 0;
    for info in vst3 {
        let mut plugin = match host.open(&info.path, &info.key) {
            Ok(plugin) => plugin,
            Err(why) => panic!("{} would not open: {why}", info.name),
        };
        let mut processor = plugin
            .activate(48_000.0, 256)
            .unwrap_or_else(|why| panic!("{} would not activate: {why}", info.name));
        let mut output = vec![vec![0.0f32; 256]; plugin.audio_outputs().max(1) as usize];
        if info.is_instrument() {
            processor.note_on(0, 60, 0.8);
            let mut peak = 0.0f32;
            for _ in 0..40 {
                processor.process_instrument(&mut output, 256);
                peak = peak.max(
                    output
                        .iter()
                        .flat_map(|c| c.iter())
                        .fold(0.0f32, |a, b| a.max(b.abs())),
                );
            }
            eprintln!(
                "{:<40} {} params, latency {}, peak {peak:.3}, editor {}",
                info.name,
                plugin.params().len(),
                plugin.latency_samples(),
                plugin.has_editor()
            );
            if peak > 0.001 {
                sounded += 1;
            }
        } else {
            let input = vec![vec![0.25f32; 256]; plugin.audio_inputs().max(1) as usize];
            for _ in 0..4 {
                processor.process_effect(&input, &mut output, 256);
            }
            eprintln!(
                "{:<40} {} params, latency {}, editor {}",
                info.name,
                plugin.params().len(),
                plugin.latency_samples(),
                plugin.has_editor()
            );
        }
        let state = plugin.snapshot();
        assert!(state.blob.is_some(), "{} keeps no state", info.name);
        plugin.deactivate(processor);
        opened += 1;
    }
    eprintln!("{opened} opened, {sounded} instruments sounded");
    assert!(sounded > 0);
}
