//! Formats that live outside this tree: the bridge loader.
//!
//! > *"if vst3 and vst2 are legally murky we could always try and add it in
//! > a way that keeps it completely separate to the open source stuff and
//! > never gets included with it ... that way i can locally use vsts lv2s or
//! > clap plugins"*
//!
//! `fontelle-bridge-abi` is the contract and `fontelle-testbridge` is a bridge
//! with no SDK in it. What these tests hold is that a bridge dropped into
//! Fontelle's own folder turns a format this build refuses into one it
//! hosts — through the same `HostedPlugin` and `HostedProcessor` the other
//! two formats go through, so that nothing above the host can tell.

// A bridge is a `.so` this host dlopens, and the fixture is built as one.
#![cfg(target_os = "linux")]

mod common;

use std::sync::Arc;

use fontelle_host::{Bridges, HostError, PluginHost, PluginScan, bridge_search_paths};
use fontelle_types::{PluginFormat, PluginKey};

fn bridges() -> Arc<Bridges> {
    Arc::new(Bridges::load(&[common::bridge_folder()]))
}

fn host() -> PluginHost {
    PluginHost::with_bridges(bridges())
}

fn gain_key() -> PluginKey {
    PluginKey::new(PluginFormat::Vst3, common::BRIDGED_GAIN)
}

fn sine_key() -> PluginKey {
    PluginKey::new(PluginFormat::Vst3, common::BRIDGED_SINE)
}

fn peak(buffers: &[Vec<f32>]) -> f32 {
    buffers
        .iter()
        .flat_map(|b| b.iter())
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

// ---------------------------------------------------------- finding one

#[test]
fn the_bridges_folder_is_fontelles_own() {
    // Under the data directory, beside the soundfont bank: a bridge is
    // something the user installs for this program, not a plugin.
    let paths = bridge_search_paths();
    assert!(
        paths.iter().any(|p| p.ends_with("fontelle/bridges")),
        "{paths:?}"
    );
}

#[test]
fn a_bridge_in_a_folder_is_found_and_says_which_format_it_serves() {
    let bridges = Bridges::load(&[common::bridge_folder()]);
    assert!(bridges.failures.is_empty(), "{:#?}", bridges.failures);
    assert_eq!(bridges.formats(), vec![PluginFormat::Vst3]);
    assert_eq!(bridges.names(), vec!["Fontelle Test Bridge".to_string()]);
    assert!(bridges.serves(PluginFormat::Vst3));
    assert!(!bridges.serves(PluginFormat::Clap), "CLAP is not bridged");
}

#[test]
fn a_library_that_is_not_a_bridge_is_a_failure_and_not_a_crash() {
    // The LV2 test plugin is a perfectly good shared library with no entry
    // point of ours in it — which is what a stray `.so` in the folder is.
    let dir = std::env::temp_dir().join(format!("fontelle-not-bridges-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        common::lv2_bundle().join(fontelle_testlv2::BINARY_NAME),
        dir.join("libsomething.so"),
    )
    .unwrap();
    std::fs::write(dir.join("libtext.so"), "not a library").unwrap();
    std::fs::write(dir.join("readme.txt"), "ignored").unwrap();
    let bridges = Bridges::load(std::slice::from_ref(&dir));
    assert!(bridges.formats().is_empty());
    assert_eq!(bridges.failures.len(), 2, "{:#?}", bridges.failures);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_that_is_not_there_is_no_bridges_and_no_failure() {
    let bridges = Bridges::load(&[std::path::PathBuf::from("/nonexistent/fontelle/bridges")]);
    assert!(bridges.formats().is_empty());
    assert!(bridges.failures.is_empty());
}

#[test]
fn with_no_bridge_a_bridged_format_is_still_refused() {
    let mut host = PluginHost::new();
    assert!(!host.can_host(PluginFormat::Vst3));
    match host.open(&common::bridged_bundle(), &gain_key()) {
        Err(HostError::Unsupported(PluginFormat::Vst3)) => {}
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("opened without a bridge"),
    }
    assert!(
        PluginScan::of_with(
            &[common::bridged_bundle().parent().unwrap().to_path_buf()],
            &Bridges::none()
        )
        .plugins
        .iter()
        .all(|p| p.key.format != PluginFormat::Vst3)
    );
}

// ------------------------------------------------------------ scanning

#[test]
fn a_bridged_format_is_scanned_like_any_other() {
    let bridges = bridges();
    let host = PluginHost::with_bridges(Arc::clone(&bridges));
    assert!(host.can_host(PluginFormat::Vst3));
    let folder = common::bridged_bundle().parent().unwrap().to_path_buf();
    let scan = PluginScan::of_with(&[folder], &bridges);
    let bridged: Vec<_> = scan
        .plugins
        .iter()
        .filter(|p| p.key.format == PluginFormat::Vst3)
        .collect();
    assert_eq!(bridged.len(), 2, "{:#?}", scan.plugins);
    let gain = bridged
        .iter()
        .find(|p| p.key.id == common::BRIDGED_GAIN)
        .unwrap();
    let sine = bridged
        .iter()
        .find(|p| p.key.id == common::BRIDGED_SINE)
        .unwrap();
    assert_eq!(gain.name, "Bridged Test Gain");
    assert_eq!(gain.vendor, "Fopull LLC");
    assert_eq!(gain.version, "1.0.0");
    assert_eq!(gain.path, common::bridged_bundle());
    assert!(gain.is_effect() && !gain.is_instrument());
    assert!(sine.is_instrument() && !sine.is_effect());
}

#[test]
fn the_folders_a_bridge_nominates_join_the_search() {
    let bridges = bridges();
    let paths = fontelle_host::search_paths_with(&bridges);
    assert!(
        paths.iter().any(|p| p.ends_with("test-bridge-plugins")),
        "{paths:?}"
    );
}

// ------------------------------------------------------------- hosting

#[test]
fn a_bridged_plugin_lists_its_parameters_and_says_what_it_is() {
    let mut host = host();
    let plugin = host
        .open(&common::bridged_bundle(), &gain_key())
        .expect("opens");
    assert_eq!(plugin.name(), "Bridged Test Gain");
    assert_eq!(plugin.key(), &gain_key());
    assert_eq!(plugin.params().len(), 2);
    let gain = plugin.param(0).unwrap();
    assert_eq!((gain.min, gain.max, gain.default), (0.0, 2.0, 1.0));
    assert!(!gain.stepped);
    assert!(plugin.param(1).unwrap().stepped);
    assert_eq!(plugin.audio_inputs(), 2);
    assert_eq!(plugin.audio_outputs(), 2);
    assert!(!plugin.accepts_notes());
    assert!(plugin.keeps_state());
}

#[test]
fn a_plugin_the_bridge_does_not_have_is_refused() {
    let mut host = host();
    let key = PluginKey::new(PluginFormat::Vst3, "nothing.here");
    match host.open(&common::bridged_bundle(), &key) {
        Err(HostError::NoSuchPlugin { .. }) => {}
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("opened"),
    }
}

#[test]
fn what_a_bridged_parameter_is_set_to_is_what_the_plugin_does() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &gain_key()).unwrap();
    plugin.set_param(0, 1.5);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);
    assert!((output[0][0] - 0.75).abs() < 1e-5, "{:?}", output[0][0]);

    plugin.set_param(0, 0.25);
    processor.process_effect(&input, &mut output, 8);
    assert!(
        (output[0][0] - 0.125).abs() < 1e-5,
        "moved while playing: {:?}",
        output[0][0]
    );

    plugin.set_param(1, 1.0);
    processor.process_insert(&mut output, 8);
    assert!(output[0][0] < 0.0, "inverted: {:?}", output[0][0]);
}

#[test]
fn a_bridged_instrument_plays_at_the_frame_it_was_told() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &sine_key()).unwrap();
    assert!(plugin.accepts_notes());
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6);
    processor.note_on(128, 69, 1.0);
    processor.process_instrument(&mut output, 256);
    let before: f32 = output[0][..128].iter().fold(0.0, |a, b| a.max(b.abs()));
    let after: f32 = output[0][128..].iter().fold(0.0, |a, b| a.max(b.abs()));
    assert!(before < 1e-6 && after > 0.1, "{before} {after}");
    processor.reset();
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "reset did not silence it");
}

#[test]
fn a_bridged_plugins_display_comes_from_the_bridge() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &gain_key()).unwrap();
    assert_eq!(plugin.display(0, 1.5).as_deref(), Some("1.50x"));
    assert_eq!(plugin.display(1, 1.0), None, "no formatter for the switch");
}

#[test]
fn a_bridged_plugins_own_state_is_saved_and_given_back() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &gain_key()).unwrap();
    plugin.set_param(0, 1.75);
    plugin.set_param(1, 1.0);
    let state = plugin.snapshot();
    assert_eq!(state.key, gain_key());
    assert!(state.blob.is_some(), "the gain keeps state");
    assert_eq!(state.param(0), Some(1.75));

    let mut again = host.open(&common::bridged_bundle(), &gain_key()).unwrap();
    assert!(again.restore(&state));
    assert_eq!(again.values().get(0), Some(1.75));
    assert_eq!(again.values().get(1), Some(1.0));

    let mut sine = host.open(&common::bridged_bundle(), &sine_key()).unwrap();
    assert!(!sine.keeps_state());
    assert!(sine.snapshot().blob.is_none());
}

#[test]
fn a_bridged_plugin_can_be_stopped_and_started_again() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &gain_key()).unwrap();
    let processor = plugin.activate(48_000.0, 16).unwrap();
    plugin.deactivate(processor);
    let mut processor = plugin.activate(44_100.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
}

// ------------------------------ the bridge's editor entry points (2026-09-05)
//
// ABI v2: the vtable grew the four things both hosted formats turned out to
// need — `has_editor`, `open_editor` into an X11 window (reporting the size
// it wants), `close_editor`, and a per-frame `tick_editor` — plus
// `resize_editor`. The test bridge's gain has no face and its sine has one
// that draws nothing, so both answers are fixtures rather than assumptions.

/// A bridged plugin with no editor says so **through the bridge**, and
/// asking to open one is a refusal, not a window.
#[test]
fn a_bridged_plugin_with_no_editor_says_so_through_the_bridge() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &gain_key()).unwrap();
    assert!(!plugin.has_editor(), "the bridged gain has no face");
    let window = fontelle_host::PluginWindow::headless(320, 200);
    assert!(matches!(
        plugin.open_editor(&window, 1.0),
        Err(fontelle_host::GuiError::NoEditor)
    ));
    assert!(!plugin.editor_is_open());
    plugin.tick_editor();
    plugin.close_editor();
    assert!(!plugin.editor_is_open());
}

/// And one that has is opened into the window Fontelle made, says how big it
/// wants to be, is driven once a frame, and **what it writes comes back on
/// the wire** — the bridge's editor moves a knob, and the host reads it off
/// the bridge after every tick.
#[test]
fn a_bridged_editor_is_opened_into_the_window_driven_and_heard() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &sine_key()).unwrap();
    assert!(plugin.has_editor(), "the test bridge's sine has a face");
    let window = fontelle_host::PluginWindow::headless(640, 480);
    let wanted = plugin
        .open_editor(&window, 1.0)
        .expect("the bridge opens it");
    assert_eq!(
        (wanted.width, wanted.height),
        (
            fontelle_testbridge::EDITOR_WIDTH,
            fontelle_testbridge::EDITOR_HEIGHT
        ),
        "the size the bridge reported"
    );
    assert!(plugin.editor_is_open());
    assert!(
        !plugin.editor_resizable(),
        "a bridged editor is the size the bridge says it is"
    );

    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(7),
        Some(fontelle_testbridge::HELLO_FROM_THE_EDITOR),
        "written by the editor on its first tick, read back off the bridge"
    );
    plugin.close_editor();
    assert!(!plugin.editor_is_open());

    // Opened again, a fresh editor greets again — so open and close are a
    // pair on the bridge's side too.
    plugin.set_param(7, 0.5);
    plugin.open_editor(&window, 1.0).expect("and again");
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(7),
        Some(fontelle_testbridge::HELLO_FROM_THE_EDITOR)
    );
    plugin.close_editor();
}

/// A bridged editor that was never opened is not driven: the bridge's tick
/// is only ever called between an open and a close.
#[test]
fn a_bridged_editor_is_only_ticked_while_it_is_open() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &sine_key()).unwrap();
    plugin.tick_editor();
    plugin.tick_editor();
    assert_eq!(
        plugin.values().get(7),
        Some(0.5),
        "the level is where it started"
    );
}

// ------------------------------------ a performance, over the ABI (2026-09-06)

fn crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count()
}

/// The wheels reach a **bridged** plugin too. Until ABI 3 the table carried
/// notes and nothing else of a performance, so a bridged instrument was the
/// one kind that could not be played with a wheel — and a VST3 bridge is
/// the whole point of the seam.
///
/// The fixture sine scales its level by the wheel and ducks under pressure,
/// the same way both in-tree fixtures do, so a bridge that swapped the two
/// is told apart from one that got it right.
#[test]
fn a_mod_wheel_and_aftertouch_reach_a_bridged_instrument() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &sine_key()).unwrap();
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "{}", peak(&output));

    processor.controller(0, 1, 0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "the wheel is down: {}", peak(&output));

    processor.controller(0, 1, 127);
    processor.process_instrument(&mut output, 256);
    let open = peak(&output);
    assert!(open > 0.1, "{open}");

    processor.channel_pressure(0, 127);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "pressure ducks it: {}", peak(&output));
    processor.channel_pressure(0, 0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
}

/// A bend, and a **slide** — which for a bridge is the same entry point as a
/// bend, because the ABI carries one pitch for the instrument rather than
/// one per note. Two semitones either way, the range every keyboard
/// defaults to, and a slide past it lands at the limit.
#[test]
fn a_bend_and_a_slide_reach_a_bridged_instrument() {
    let mut host = host();
    let mut plugin = host.open(&common::bridged_bundle(), &sine_key()).unwrap();
    let mut processor = plugin.activate(48_000.0, 4096).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 4096], vec![0.0f32; 4096]];
    processor.process_instrument(&mut output, 4096);
    // 440 Hz over 4096 frames is 37 cycles, so a whole tone — 12% — is
    // nine crossings, comfortably more than the rounding at either end.
    let unbent = crossings(&output[0]);
    assert!(unbent > 50, "the note is sounding: {unbent}");

    processor.pitch_bend(0, 8191);
    processor.process_instrument(&mut output, 4096);
    let bent = crossings(&output[0]);
    assert!(
        bent as f32 > unbent as f32 * 1.08,
        "unbent {unbent}, bent {bent}"
    );

    processor.note_tuning(0, 69, 2.0);
    processor.process_instrument(&mut output, 4096);
    let slid = crossings(&output[0]);
    assert!(
        slid.abs_diff(bent) <= 2,
        "a two-semitone slide is the full bend: {slid} vs {bent}"
    );

    processor.pitch_bend(0, 0);
    processor.process_instrument(&mut output, 4096);
    let centred = crossings(&output[0]);
    assert!(centred.abs_diff(unbent) <= 2, "{centred} vs {unbent}");
}

/// A bridge built against an older table is refused rather than read past
/// its end — the whole reason the version is in the table.
#[test]
fn the_abi_version_is_three_now_that_a_performance_crosses_it() {
    assert_eq!(fontelle_bridge_abi::ABI_VERSION, 3);
}
