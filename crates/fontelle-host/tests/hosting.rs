//! Opening a plugin, reading it, setting it and hearing it.

mod common;

use fontelle_host::{HostError, PluginHost};
use fontelle_types::{PluginKey, PluginState};

fn host() -> PluginHost {
    PluginHost::new()
}

fn gain(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::bundle(), &PluginKey::clap(common::GAIN))
        .expect("the test gain opens")
}

fn sine(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::bundle(), &PluginKey::clap(common::SINE))
        .expect("the test sine opens")
}

#[test]
fn a_plugin_that_is_not_in_the_bundle_is_refused() {
    let mut host = host();
    let error = host
        .open(&common::bundle(), &PluginKey::clap("com.example.nothing"))
        .err();
    assert!(
        matches!(error, Some(HostError::NoSuchPlugin { .. })),
        "{error:?}"
    );
}

#[test]
fn a_format_this_build_cannot_host_says_so_rather_than_failing_to_find_a_file() {
    let mut host = host();
    let key = PluginKey::new(fontelle_types::PluginFormat::Vst3, "com.example.thing");
    let error = host.open(&common::bundle(), &key).err();
    assert!(
        matches!(error, Some(HostError::Unsupported(_))),
        "{error:?}"
    );
}

#[test]
fn a_plugin_lists_its_parameters_the_way_it_describes_them() {
    let mut host = host();
    let plugin = gain(&mut host);
    let params = plugin.params();
    assert_eq!(params.len(), 2, "{params:#?}");

    let gain = &params[0];
    assert_eq!(gain.id, 0);
    assert_eq!(gain.name, "Gain");
    assert_eq!(gain.min, 0.0);
    assert_eq!(gain.max, 4.0);
    assert_eq!(gain.default, 1.0);
    assert!(!gain.stepped);

    let invert = &params[1];
    assert_eq!(invert.id, 1);
    assert_eq!(invert.name, "Invert");
    // **The plugin's own string, separators and all.** CLAP's `module` is a
    // path and plugins write it with them — Surge XT's are "/Macros/" and
    // "/Global & FX/". Turning it into a heading is the panel's job
    // (`fontelle_app::instrument::module_heading`); the host reports what was
    // said.
    assert_eq!(invert.module, "/Phase/");
    assert!(invert.stepped);
    assert_eq!(invert.steps(), Some(2));
}

#[test]
fn a_parameter_converts_between_what_it_means_and_where_a_lane_puts_it() {
    let mut host = host();
    let plugin = gain(&mut host);
    let param = &plugin.params()[0];
    assert!((param.normalise(2.0) - 0.5).abs() < 1e-9);
    assert!((param.plain(0.5) - 2.0).abs() < 1e-9);
    assert_eq!(param.normalise(-10.0), 0.0);
    assert_eq!(param.plain(9.0), 4.0);
}

#[test]
fn a_plugin_says_what_it_is_and_what_it_can_be_wired_to() {
    let mut host = host();
    let gain = gain(&mut host);
    // The main ports: the bus is copied to and from these alone.
    assert_eq!(gain.audio_inputs(), 2);
    assert_eq!(gain.audio_outputs(), 2);
    assert!(!gain.accepts_notes());
    assert!(gain.keeps_state());

    let mut host = host;
    let sine = sine(&mut host);
    assert_eq!(sine.audio_inputs(), 0);
    assert_eq!(sine.audio_outputs(), 2);
    assert!(sine.accepts_notes());
    assert!(!sine.keeps_state());
}

#[test]
fn what_a_parameter_is_set_to_is_what_the_plugin_does() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    plugin.set_param(0, 3.0);

    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    let input = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let mut output = vec![vec![0.0f32; 8], vec![0.0f32; 8]];
    processor.process_effect(&input, &mut output, 8);

    assert!((output[0][0] - 1.5).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_parameter_moved_while_it_is_playing_is_heard_on_the_next_block() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();

    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5, "{:?}", output[0][0]);

    plugin.set_param(0, 0.25);
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 0.25).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_parameter_the_plugin_does_not_have_is_ignored_rather_than_an_error() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    assert!(!plugin.set_param(9_999, 1.0));
    assert!(plugin.set_param(0, 2.0));
}

#[test]
fn a_stepped_parameter_does_what_its_positions_say() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    plugin.set_param(1, 1.0);
    let mut processor = plugin.activate(48_000.0, 32).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] + 1.0).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn an_instrument_sounds_when_a_key_is_pressed_and_stops_when_it_is_let_go() {
    let mut host = host();
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
fn an_instruments_notes_are_placed_where_in_the_block_they_happened() {
    let mut host = host();
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
fn a_plugins_own_state_is_saved_and_given_back() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    plugin.set_param(0, 2.5);
    let state = plugin.snapshot();
    assert!(state.blob.is_some(), "the gain keeps state");
    assert_eq!(state.param(0), Some(2.5));

    let mut other = gain(&mut host);
    other.restore(&state);
    let mut processor = other.activate(48_000.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 2.5).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_plugin_with_no_state_of_its_own_is_restored_from_its_parameters_alone() {
    let mut host = host();
    let mut plugin = sine(&mut host);
    plugin.set_param(7, 0.125);
    let state = plugin.snapshot();
    assert_eq!(state.blob, None, "the sine keeps no state");
    assert_eq!(state.param(7), Some(0.125));

    let mut other = sine(&mut host);
    other.restore(&state);
    let mut processor = other.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!((peak(&output) - 0.125).abs() < 0.01, "{}", peak(&output));
}

#[test]
fn a_snapshot_names_the_plugin_it_came_from() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let state = plugin.snapshot();
    assert_eq!(state.key, PluginKey::clap(common::GAIN));
    assert_eq!(state.name, "Fontelle Test Gain");
}

#[test]
fn a_state_from_a_different_plugin_is_not_applied() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let mut state = PluginState::new(PluginKey::clap(common::SINE), "Fontelle Test Sine");
    state.set_param(0, 4.0);
    assert!(!plugin.restore(&state));
}

#[test]
fn a_damaged_blob_leaves_the_plugin_at_its_defaults_rather_than_half_loaded() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let mut state = PluginState::new(PluginKey::clap(common::GAIN), "Fontelle Test Gain");
    state.blob = Some("!!!! not base64 !!!!".to_string());
    assert!(plugin.restore(&state));

    let mut processor = plugin.activate(48_000.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5, "{:?}", output[0][0]);
}

#[test]
fn a_plugin_can_be_stopped_and_started_again() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let processor = plugin.activate(48_000.0, 16).unwrap();
    plugin.deactivate(processor);
    let mut processor = plugin.activate(44_100.0, 16).unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
}

#[test]
fn one_bundle_is_loaded_once_however_many_instances_come_out_of_it() {
    let mut host = host();
    let _first = gain(&mut host);
    let _second = gain(&mut host);
    let _third = sine(&mut host);
    assert_eq!(host.loaded_bundles(), 1);
}

#[test]
fn a_processor_survives_a_block_shorter_than_the_one_it_was_prepared_for() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 512).unwrap();
    let input = vec![vec![1.0f32; 512], vec![1.0f32; 512]];
    let mut output = vec![vec![0.0f32; 512], vec![0.0f32; 512]];
    processor.process_effect(&input, &mut output, 3);
    assert!((output[0][0] - 1.0).abs() < 1e-5);
    assert_eq!(output[0][3], 0.0, "nothing past the block is touched");
}

fn peak(buffers: &[Vec<f32>]) -> f32 {
    buffers
        .iter()
        .flat_map(|b| b.iter())
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

// --------------------------- what a plugin's parameters start at (2026-09-05)

/// A plugin's own answer beats what its parameter *description* claims.
///
/// > *"i added plugin instruments but cant hear them"*
///
/// Surge XT's CLAP build reports `default_value` as zero for every one of its
/// seven hundred and seventy-five parameters, while `get_value` on the freshly
/// instantiated plugin returns the real setting. A host that seeds its wire
/// from the description and then sends the lot — which is what activating does
/// — turns Global Volume down to -48 dB before the first block, and the synth
/// is silent. `fontelle-testplug`'s sine tells the same lie about its `Output`
/// parameter, so this is checked against a plugin whose answers are known.
#[test]
fn a_parameter_starts_where_the_plugin_says_it_is_rather_than_where_its_description_does() {
    let mut host = host();
    let plugin = sine(&mut host);
    let output = plugin
        .param(fontelle_testplug::OUTPUT_PARAM)
        .expect("the sine has an Output");
    assert_eq!(
        output.default, 1.0,
        "the plugin's own value, not the zero its description claims"
    );
    assert_eq!(
        plugin.values().get(fontelle_testplug::OUTPUT_PARAM),
        Some(1.0),
        "and the wire the audio thread reads agrees with it"
    );
}

/// The whole point of the one above, said in sound.
#[test]
fn an_instrument_whose_description_understates_its_defaults_still_sounds() {
    let mut host = host();
    let mut plugin = sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "{}", peak(&output));
}

/// A plugin whose `show` answers **false** keeps its editor.
///
/// Reported: *"the ones that seem to be trying to open a custom ui most
/// often are just closing their window as soon as it opens"* — SpectMorph
/// and the OneTrick drum synths among them. Both are built on clap-helpers,
/// whose default `guiShow` returns false, and both put their window up in
/// `set_parent` and never override it. A host that reads that false as
/// failure destroys the editor it has just embedded, and the window it made
/// for it goes with it. The fixture answers the way they do.
#[test]
fn a_plugin_whose_show_says_no_still_has_its_editor() {
    let mut host = host();
    let mut plugin = host
        .open(
            &common::bundle(),
            &PluginKey::clap("com.fopull.fontelle.testface"),
        )
        .expect("the test face opens");
    assert!(plugin.has_editor());
    let window = fontelle_host::PluginWindow::headless(320, 200);
    let size = plugin
        .open_editor(&window, 1.0)
        .expect("a false from show is not a refusal");
    assert_eq!(
        (size.width, size.height),
        (320, 200),
        "the size it asked for"
    );
    assert!(plugin.editor_is_open());
    plugin.close_editor();
    assert!(!plugin.editor_is_open());
}

// ------------------------------------ every port, not just the main one (2026-09-05)

/// A plugin that declares more ports than the bus is handed **every one of
/// them**.
///
/// > *"a lot of drum synth plugin keeps making the daw crash"*
///
/// The OneTrick drum synths declare individual outputs beside their main
/// pair. CLAP says the host passes as many ports as the plugin declared, and
/// nih-plug — which they are built on — takes the host at its word: handed
/// one port, it reads the second off the end of the array and clears memory
/// at whatever it finds there (a `memset` to address `0x13` in SIMIAN2 and
/// URCHIN, in the first block, before a note was played). The fixture sine
/// declares a mono *sub* port beside its main pair, and refuses to render
/// at all when it is handed fewer ports than it asked for — the same rule
/// several real plugins apply to their inputs.
#[test]
fn every_port_a_plugin_declares_is_handed_over() {
    let mut host = host();
    let mut plugin = sine(&mut host);
    assert_eq!(plugin.output_ports(), &[2, 1], "main pair, then the sub");
    assert_eq!(plugin.input_ports(), &[] as &[u32]);
    assert_eq!(
        plugin.audio_outputs(),
        2,
        "the bus still sees the main port"
    );

    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    processor.note_on(0, 60, 1.0);
    let mut out = vec![vec![0.0f32; 64]; 2];
    processor.process_instrument(&mut out, 64);
    assert!(
        peak(&out) > 0.1,
        "the fixture renders nothing when a port is missing: {}",
        peak(&out)
    );
}

// ------------------------------------------------- the sidechain (2026-09-05)

/// A plugin that declares a second input port beside its main one has a
/// **sidechain**, and says so.
///
/// CLAP has no sidechain flag: a sidechain is any input port that is not
/// the main one, and a compressor's key arrives on it. The fixture gain
/// grew one — a mono port after its main pair — and a mono *aux* output to
/// go with it, so both directions are declared with more than the bus
/// needs. The bus still maps to the main pair alone.
#[test]
fn a_plugin_declares_its_sidechain_beside_its_main_input() {
    let mut host = host();
    let gain = gain(&mut host);
    assert_eq!(gain.input_ports(), &[2, 1], "main pair, then the sidechain");
    assert_eq!(gain.output_ports(), &[2, 1], "main pair, then the aux");
    assert_eq!(gain.audio_inputs(), 2, "the bus still sees the main port");
    assert_eq!(gain.audio_outputs(), 2);
    assert!(gain.takes_key(), "a second input port is a key input");
    let sine = sine(&mut host);
    assert!(!sine.takes_key(), "an instrument has no input to key");
}

/// And a signal on it changes what the plugin does.
///
/// The fixture ducks: its main output is the input times the gain, times one
/// minus the key. A key of one is silence, a key of zero is the gain as it
/// was — so the same call with two keys is the whole of the assertion.
#[test]
fn a_signal_on_the_sidechain_changes_what_the_plugin_does() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    plugin.set_param(0, 2.0);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();

    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let quiet = vec![0.0f32; 8];
    processor.process_insert_keyed(&mut bus, &quiet, 8);
    assert!((bus[0][0] - 1.0).abs() < 1e-5, "no key: {}", bus[0][0]);

    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let loud = vec![1.0f32; 8];
    processor.process_insert_keyed(&mut bus, &loud, 8);
    assert!(bus[0][0].abs() < 1e-5, "ducked by the key: {}", bus[0][0]);
    assert!(bus[1][0].abs() < 1e-5, "{}", bus[1][0]);

    // Half a key is half the gain: the key is *heard*, not detected.
    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let half = vec![0.5f32; 8];
    processor.process_insert_keyed(&mut bus, &half, 8);
    assert!((bus[0][0] - 0.5).abs() < 1e-5, "{}", bus[0][0]);
}

/// The main output still reaches the bus beside the aux output the plugin
/// also renders — which is dropped, not summed: a drum plugin's individual
/// outs summed into its main pair would be a plugin twice as loud as it is.
#[test]
fn the_main_output_reaches_the_bus_and_the_extra_output_is_dropped() {
    let mut host = host();
    let mut plugin = gain(&mut host);
    let mut processor = plugin.activate(48_000.0, 64).unwrap();
    // The fixture's aux carries the key. With the key at one, the main pair
    // is silence and the aux is one; a host that summed the two would put
    // the key on the bus.
    let mut bus = vec![vec![0.5f32; 8], vec![0.5f32; 8]];
    let loud = vec![1.0f32; 8];
    processor.process_insert_keyed(&mut bus, &loud, 8);
    assert!(
        bus[0][0].abs() < 1e-5,
        "the aux leaked onto the bus: {}",
        bus[0][0]
    );
}

// ------------------------------ pitch bend, mod wheel, aftertouch (2026-09-05)

/// How many times a signal crosses zero — a pitch, counted rather than
/// measured, which is enough to tell a bent note from an unbent one.
fn crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count()
}

fn clap_only_sine(host: &mut PluginHost) -> fontelle_host::HostedPlugin {
    host.open(&common::bundle(), &PluginKey::clap(common::SINE_CLAP_ONLY))
        .expect("the CLAP-only test sine opens")
}

/// A plugin says which **dialect** its note port speaks, which decides how a
/// controller reaches it.
///
/// The fixture sine offers both and prefers CLAP's; the host takes it at its
/// word that MIDI is *supported* and sends a wheel as the three bytes it was,
/// because a mod wheel has no honest CLAP equivalent — see
/// [`a_mod_wheel_reaches_a_plugin_that_speaks_only_clap_as_a_note_expression`]
/// for what a plugin that refuses MIDI gets instead.
#[test]
fn a_plugin_says_which_dialect_its_notes_arrive_in() {
    let mut host = host();
    assert_eq!(
        sine(&mut host).note_dialect(),
        Some(fontelle_host::NoteDialect::Midi),
        "supports MIDI, so MIDI"
    );
    assert_eq!(
        clap_only_sine(&mut host).note_dialect(),
        Some(fontelle_host::NoteDialect::Clap)
    );
    assert_eq!(gain(&mut host).note_dialect(), None, "no note port at all");
}

/// The mod wheel changes what a plugin does.
///
/// The fixture scales its level by the wheel — full until told otherwise —
/// so a wheel at zero is silence on the next block.
#[test]
fn a_mod_wheel_reaches_a_plugin_that_speaks_midi() {
    let mut host = host();
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

/// A pitch bend bends: a full bend up is two semitones on the fixture, which
/// is a tenth more zero crossings over the same block.
#[test]
fn a_pitch_bend_reaches_a_plugin_that_speaks_midi() {
    let mut host = host();
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

/// And aftertouch. The fixture ducks by pressure — full pressure is silence
/// — which is the opposite of what the wheel does, so the two cannot be
/// confused for one another by a host that sent the wrong bytes.
#[test]
fn channel_pressure_reaches_a_plugin_that_speaks_midi() {
    let mut host = host();
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

/// A plugin whose note port speaks **only CLAP** is given the controller as
/// the nearest **note expression**, on every note: the wheel as vibrato,
/// aftertouch as pressure, the bend as tuning in semitones. Nothing is
/// invented onto a parameter.
#[test]
fn a_mod_wheel_reaches_a_plugin_that_speaks_only_clap_as_a_note_expression() {
    let mut host = host();
    let mut plugin = clap_only_sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);

    processor.controller(0, 1, 0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "vibrato at zero: {}", peak(&output));
}

#[test]
fn a_pitch_bend_reaches_a_plugin_that_speaks_only_clap_as_tuning() {
    let mut host = host();
    let mut plugin = clap_only_sine(&mut host);
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
fn channel_pressure_reaches_a_plugin_that_speaks_only_clap_as_pressure() {
    let mut host = host();
    let mut plugin = clap_only_sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1);
    processor.channel_pressure(0, 127);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) < 1e-6, "{}", peak(&output));
}

/// A controller with no expression to map to is dropped rather than guessed
/// at: the CLAP-only sine ducks on **CC 1** through vibrato, and CC 2 has no
/// meaning it could be given.
#[test]
fn a_controller_with_no_note_expression_is_dropped_for_a_clap_only_plugin() {
    let mut host = host();
    let mut plugin = clap_only_sine(&mut host);
    let mut processor = plugin.activate(48_000.0, 256).unwrap();
    processor.note_on(0, 69, 1.0);
    let mut output = vec![vec![0.0f32; 256], vec![0.0f32; 256]];
    processor.process_instrument(&mut output, 256);
    processor.controller(0, 2, 0);
    processor.process_instrument(&mut output, 256);
    assert!(peak(&output) > 0.1, "{}", peak(&output));
}

// ------------------------------------------------- slides (2026-09-06)

/// A slide note bends a voice that is already sounding to a new key. For a
/// CLAP plugin that is a **tuning expression on the note** — per key, in
/// semitones, unbounded — whatever dialect its port speaks, because notes
/// go to a CLAP plugin as CLAP's own events in every case and the
/// expression rides the note. Twelve semitones up doubles the frequency,
/// which is well past what a channel bend could carry.
#[test]
fn a_slide_reaches_a_clap_plugin_as_tuning_on_the_note() {
    let mut host = host();
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

// ------------------------------------------------ latency (2026-09-06)

/// A plugin says how far it puts its output behind its input, and the host
/// asks (TDD §5.5). CLAP's answer is the `latency` extension; a plugin
/// without it, and every LV2 and bridged plugin in this build, reports
/// nothing — which is what a host that cannot ask must assume.
#[test]
fn a_plugin_reports_the_latency_it_declares() {
    let mut host = host();
    // The fixture gain declares the extension and answers with a fixed
    // number of samples, so a host that never asked is told apart from one
    // that did.
    let plugin = gain(&mut host);
    assert_eq!(
        plugin.latency_samples(),
        fontelle_testplug::GAIN_LATENCY_SAMPLES
    );

    // The sine declares no latency extension at all.
    let sine = sine(&mut host);
    assert_eq!(sine.latency_samples(), 0);
}
