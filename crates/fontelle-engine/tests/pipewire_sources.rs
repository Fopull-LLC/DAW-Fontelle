//! Which microphones the input menu offers, on a machine running PipeWire.
//!
//! Reported from using the window:
//!
//! > *"for some reason its not recognizing my logitech camera mic input / i
//! > think it actually is but it is naming it something different sometimes
//! > than others where its just called logitech cam or something along those
//! > lines."*
//!
//! Both halves of that were true, and one cause explains both. The input
//! list was built from ALSA's own PCM names and filtered by asking each one
//! whether it would open — and on a PipeWire desktop the sound server holds
//! the hardware. When another program was listening to the camera through
//! PipeWire, every direct ALSA open of it failed and the camera vanished;
//! when nothing was, whichever of ALSA's several aliases for the same card
//! happened to open first gave the entry its name, so the *same* microphone
//! was "Logi Webcam C920e, USB Audio" on one day and "Logi Webcam C920e" on
//! another — and a project saved with one name could not find the other.
//!
//! So on a PipeWire machine the sources are asked of **PipeWire**, which
//! names each one once and shares it with whoever else is listening. The
//! server is read through `pw-dump`, whose output is JSON; parsing it is pure
//! and is what is tested here. Opening a source needs a sound server and is
//! checked by hand.

use fontelle_engine::{
    PipeWireSource, find_pipewire_source, parse_pipewire_default_source, parse_pipewire_sources,
    pipewire_pcm, source_menu,
};

/// A cut-down `pw-dump`: a sink, two sources, a running stream and a device.
const DUMP: &str = r#"[
  {
    "id": 46,
    "type": "PipeWire:Interface:Device",
    "info": { "props": { "device.name": "alsa_card.pci-0000_08_00.4" } }
  },
  {
    "id": 64,
    "type": "PipeWire:Interface:Node",
    "info": {
      "props": {
        "node.name": "alsa_output.pci-0000_06_00.1.hdmi-stereo",
        "node.description": "Navi HDMI Audio",
        "media.class": "Audio/Sink",
        "audio.channels": 2
      }
    }
  },
  {
    "id": 95,
    "type": "PipeWire:Interface:Node",
    "info": {
      "props": {
        "node.name": "alsa_input.usb-046d_Logi_Webcam_C920e_FE2296AF-02.analog-stereo",
        "node.description": "Logi Webcam C920e Analog Stereo",
        "node.nick": "Logi Webcam C920e",
        "api.alsa.card.name": "Logi Webcam C920e",
        "media.class": "Audio/Source",
        "audio.channels": 2
      }
    }
  },
  {
    "id": 200,
    "type": "PipeWire:Interface:Node",
    "info": {
      "props": {
        "node.name": "ALSA plug-in [resolve]",
        "media.class": "Stream/Input/Audio",
        "application.name": "ALSA plug-in [resolve]"
      }
    }
  },
  {
    "id": 0,
    "type": "PipeWire:Interface:Metadata",
    "metadata": [
      { "subject": 0, "key": "default.audio.sink", "value": { "name": "alsa_output.pci-0000_06_00.1.hdmi-stereo" } },
      { "subject": 0, "key": "default.audio.source", "value": { "name": "alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.HiFi__Mic1__source" } }
    ]
  },
  {
    "id": 59,
    "type": "PipeWire:Interface:Node",
    "info": {
      "props": {
        "node.name": "alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.HiFi__Mic1__source",
        "node.description": "Scarlett Solo (3rd Gen.) Input 1 Mic",
        "api.alsa.card.name": "Scarlett Solo USB",
        "media.class": "Audio/Source",
        "audio.channels": 1
      }
    }
  }
]"#;

fn logitech() -> PipeWireSource {
    PipeWireSource {
        node: "alsa_input.usb-046d_Logi_Webcam_C920e_FE2296AF-02.analog-stereo".to_string(),
        description: "Logi Webcam C920e Analog Stereo".to_string(),
        card: Some("Logi Webcam C920e".to_string()),
        channels: 2,
    }
}

fn scarlett() -> PipeWireSource {
    PipeWireSource {
        node: "alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.HiFi__Mic1__source".to_string(),
        description: "Scarlett Solo (3rd Gen.) Input 1 Mic".to_string(),
        card: Some("Scarlett Solo USB".to_string()),
        channels: 1,
    }
}

// --------------------------------------------------------------- parsing ---

#[test]
fn the_sources_are_the_audio_source_nodes_and_nothing_else() {
    // Not the sink, not the device, and not another program's stream — a
    // menu with "ALSA plug-in [resolve]" in it is the thirty-two-row menu
    // this replaces, again.
    let sources = parse_pipewire_sources(DUMP);
    assert_eq!(sources, vec![logitech(), scarlett()]);
}

#[test]
fn a_source_is_named_by_its_description_which_does_not_change_between_days() {
    let sources = parse_pipewire_sources(DUMP);
    assert_eq!(sources[0].description, "Logi Webcam C920e Analog Stereo");
    // And addressed by its node name, which is what opens it.
    assert!(sources[0].node.starts_with("alsa_input."));
}

#[test]
fn a_source_with_no_description_is_named_by_its_node() {
    let dump = r#"[{"id": 1, "type": "PipeWire:Interface:Node", "info": {"props": {
        "node.name": "my_virtual_mic", "media.class": "Audio/Source/Virtual"}}}]"#;
    let sources = parse_pipewire_sources(dump);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].description, "my_virtual_mic");
    assert_eq!(sources[0].card, None);
    // No channel count means stereo, which every PipeWire source can be
    // asked for.
    assert_eq!(sources[0].channels, 2);
}

#[test]
fn a_dump_that_is_not_json_is_no_sources_rather_than_a_crash() {
    assert!(parse_pipewire_sources("").is_empty());
    assert!(parse_pipewire_sources("not json").is_empty());
    assert!(parse_pipewire_sources("{}").is_empty());
    assert!(parse_pipewire_sources("[1, 2, 3]").is_empty());
}

#[test]
fn the_servers_default_source_is_read_off_its_metadata() {
    // What "no input chosen" records from: PipeWire's own default, not the
    // first row of the list.
    assert_eq!(
        parse_pipewire_default_source(DUMP).as_deref(),
        Some("alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.HiFi__Mic1__source")
    );
    assert_eq!(parse_pipewire_default_source("[]"), None);
    assert_eq!(parse_pipewire_default_source("nope"), None);
}

// ------------------------------------------------------------- the menu ---

#[test]
fn the_menu_lists_every_source_by_description_in_the_servers_order() {
    let menu = source_menu(&[logitech(), scarlett()]);
    assert_eq!(
        menu,
        vec![
            "Logi Webcam C920e Analog Stereo".to_string(),
            "Scarlett Solo (3rd Gen.) Input 1 Mic".to_string()
        ]
    );
}

#[test]
fn two_sources_that_describe_themselves_alike_are_told_apart() {
    // Two identical USB microphones. One name for both would be a menu
    // where choosing the second one chooses the first.
    let twin = PipeWireSource {
        node: "alsa_input.usb-Generic_Mic-01.mono".to_string(),
        ..logitech()
    };
    let menu = source_menu(&[logitech(), twin]);
    assert_eq!(menu.len(), 2);
    assert_ne!(menu[0], menu[1]);
    assert!(menu[1].starts_with("Logi Webcam C920e Analog Stereo"));
}

// ----------------------------------------------------- finding one again ---

#[test]
fn a_saved_name_finds_its_source() {
    let sources = [logitech(), scarlett()];
    let found = find_pipewire_source(&sources, "Scarlett Solo (3rd Gen.) Input 1 Mic");
    assert_eq!(found, Some(&sources[1]));
    assert_eq!(find_pipewire_source(&sources, "Blue Yeti"), None);
}

#[test]
fn a_name_saved_by_the_old_alsa_list_still_finds_the_same_microphone() {
    // Projects saved before this named the ALSA alias that happened to open:
    // "Logi Webcam C920e, USB Audio" or plain "Logi Webcam C920e". The card
    // name in front of the comma is what both have in common with the
    // source's description.
    let sources = [logitech(), scarlett()];
    assert_eq!(
        find_pipewire_source(&sources, "Logi Webcam C920e, USB Audio"),
        Some(&sources[0])
    );
    assert_eq!(
        find_pipewire_source(&sources, "Logi Webcam C920e"),
        Some(&sources[0])
    );
    assert_eq!(
        find_pipewire_source(&sources, "Scarlett Solo USB, USB Audio"),
        Some(&sources[1])
    );
}

#[test]
fn a_menu_entry_finds_the_source_it_was_made_for_even_when_disambiguated() {
    let twin = PipeWireSource {
        node: "alsa_input.usb-Generic_Mic-01.mono".to_string(),
        ..logitech()
    };
    let sources = [logitech(), twin];
    let menu = source_menu(&sources);
    assert_eq!(find_pipewire_source(&sources, &menu[0]), Some(&sources[0]));
    assert_eq!(find_pipewire_source(&sources, &menu[1]), Some(&sources[1]));
}

// ------------------------------------------------------------ opening it ---

#[test]
fn a_source_is_opened_through_pipewires_own_alsa_plugin_by_node_name() {
    // `pipewire:NODE=<name>` is the PCM the PipeWire ALSA plugin exposes for
    // one node — which is what makes the camera recordable while another
    // program holds it, and what makes the name the same every day.
    assert_eq!(
        pipewire_pcm(&logitech().node),
        "pipewire:NODE=alsa_input.usb-046d_Logi_Webcam_C920e_FE2296AF-02.analog-stereo"
    );
}

// ------------------------------------------------------- a real machine ---

/// Needs a running PipeWire with at least one source. Run by hand:
/// `cargo test -p fontelle-engine --test pipewire_sources -- --ignored`.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "opens a real microphone through PipeWire"]
fn a_real_source_delivers_frames_through_pipewire() {
    use fontelle_engine::{InputMonitor, PipeWireInput, input_capture_channel, pipewire_sources};
    let sources = pipewire_sources();
    let Some(source) = sources.first() else {
        eprintln!("no PipeWire sources here; nothing to check");
        return;
    };
    let (writer, mut reader) = input_capture_channel(48_000 * 2);
    let monitor = std::sync::Arc::new(InputMonitor::new(48_000 * 2));
    let (input, rate, channels) = PipeWireInput::open(
        &source.node,
        source.channels,
        48_000,
        writer,
        Some(monitor.clone()),
    )
    .expect("the source would not open");
    assert!(rate > 0);
    assert!(channels >= 1);
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(input);
    let mut samples = Vec::new();
    reader.drain_into(&mut samples);
    let frames = samples.len() / usize::from(channels);
    eprintln!(
        "{}: {frames} frames at {rate} Hz, {channels} ch; monitor saw {} frames at block {}",
        source.description,
        monitor.available_frames(),
        monitor.device_block()
    );
    // 300 ms less thread start-up: comfortably more than a tenth of a second.
    assert!(frames > rate as usize / 10, "only {frames} frames arrived");
    assert!(monitor.device_block() > 0, "the monitor never saw a block");
}
