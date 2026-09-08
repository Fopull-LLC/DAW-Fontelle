//! The DAW-wide preset system's types (`docs/flopsynth-plan.md` §P.2).
//!
//! A preset is a **file**: a name, a category, the device it is for, and that
//! device's own saved state. These tests hold the four claims that make the
//! system work for every device without any device knowing about it — the
//! payload round-trips, a payload that does not match its device is refused,
//! every device slug is a legal folder name, and a file from a newer build is
//! refused as such rather than parsed into nonsense.

use fontelle_types::{
    DeviceKind, EffectConfig, EffectKind, InstrumentKind, PRESET_FORMAT_VERSION, PatchData,
    PluginFormat, PluginKey, PluginState, Preset, PresetOrigin, PresetPayload, PresetRef,
};

fn a_patch() -> PatchData {
    PatchData {
        format_version: 0,
        body: serde_json::json!({ "layers": [], "note": "not a real patch body" }),
    }
}

#[test]
fn every_payload_kind_round_trips_through_a_file() {
    let cases = [
        Preset {
            format_version: PRESET_FORMAT_VERSION,
            device: DeviceKind::Instrument(InstrumentKind::Osc3),
            name: "Init Lead".to_string(),
            category: "Lead".to_string(),
            payload: PresetPayload::Patch(a_patch()),
        },
        Preset {
            format_version: PRESET_FORMAT_VERSION,
            device: DeviceKind::Effect(EffectKind::Distortion),
            name: "Fuzz".to_string(),
            category: "Drive".to_string(),
            payload: PresetPayload::Effect(EffectConfig::new(EffectKind::Distortion)),
        },
        Preset {
            format_version: PRESET_FORMAT_VERSION,
            device: DeviceKind::Plugin(PluginKey::clap("com.u-he.diva")),
            name: "Big Brass".to_string(),
            category: "Brass".to_string(),
            payload: PresetPayload::Plugin(PluginState::new(
                PluginKey::clap("com.u-he.diva"),
                "Diva",
            )),
        },
    ];
    for preset in cases {
        let text = serde_json::to_string_pretty(&preset).expect("a preset serialises");
        let back: Preset = serde_json::from_str(&text).expect("and reads back");
        assert_eq!(back, preset, "a preset file must survive the round trip");
        // Readable in a text editor (TDD §17.2): every enum by name.
        assert!(
            !text.contains("\"0\":") && !text.contains("[0,"),
            "presets are serde-by-name so a file is greppable:\n{text}"
        );
    }
}

#[test]
fn a_payload_that_does_not_match_its_device_is_refused() {
    let wrong = Preset {
        format_version: PRESET_FORMAT_VERSION,
        device: DeviceKind::Instrument(InstrumentKind::Osc3),
        name: "Confused".to_string(),
        category: "Lead".to_string(),
        payload: PresetPayload::Effect(EffectConfig::new(EffectKind::Reverb)),
    };
    assert!(
        !wrong.is_consistent(),
        "an effect payload on an instrument device is not a preset anything can load"
    );

    let effect_of_another_kind = Preset {
        format_version: PRESET_FORMAT_VERSION,
        device: DeviceKind::Effect(EffectKind::Delay),
        name: "Hall".to_string(),
        category: "Space".to_string(),
        payload: PresetPayload::Effect(EffectConfig::new(EffectKind::Reverb)),
    };
    assert!(
        !effect_of_another_kind.is_consistent(),
        "a reverb's settings are not a delay preset"
    );

    let right = Preset {
        format_version: PRESET_FORMAT_VERSION,
        device: DeviceKind::Effect(EffectKind::Reverb),
        name: "Hall".to_string(),
        category: "Space".to_string(),
        payload: PresetPayload::Effect(EffectConfig::new(EffectKind::Reverb)),
    };
    assert!(right.is_consistent());
}

/// INVARIANT 7, one level up: a slug is a **folder name on disk**, so renaming
/// one moves everybody's presets. The literal strings are here so that a rename
/// is a conscious break rather than a silent one.
#[test]
fn the_table_of_device_slugs_is_frozen() {
    let expected = [
        (
            DeviceKind::Instrument(InstrumentKind::SoundFont),
            "soundfont",
        ),
        (DeviceKind::Instrument(InstrumentKind::Osc3), "3osc"),
        (DeviceKind::Instrument(InstrumentKind::Sampler), "sampler"),
        (
            DeviceKind::Instrument(InstrumentKind::DrumMachine),
            "drum-machine",
        ),
        (
            DeviceKind::Instrument(InstrumentKind::Flopsynth),
            "flopsynth",
        ),
        (DeviceKind::Instrument(InstrumentKind::Plugin), "plugin"),
        (DeviceKind::Effect(EffectKind::Utility), "fx-utility"),
        (DeviceKind::Effect(EffectKind::Eq), "fx-eq"),
        (DeviceKind::Effect(EffectKind::Filter), "fx-filter"),
        (DeviceKind::Effect(EffectKind::Compressor), "fx-compressor"),
        (DeviceKind::Effect(EffectKind::Gate), "fx-gate"),
        (DeviceKind::Effect(EffectKind::Distortion), "fx-distortion"),
        (DeviceKind::Effect(EffectKind::Bitcrush), "fx-bitcrush"),
        (DeviceKind::Effect(EffectKind::Soften), "fx-soften"),
        (DeviceKind::Effect(EffectKind::Chorus), "fx-chorus"),
        (DeviceKind::Effect(EffectKind::Delay), "fx-delay"),
        (DeviceKind::Effect(EffectKind::Reverb), "fx-reverb"),
        (
            DeviceKind::Plugin(PluginKey::new(PluginFormat::Clap, "com.u-he.diva")),
            "plugin-clap-com.u-he.diva",
        ),
    ];
    for (device, slug) in expected {
        assert_eq!(device.slug(), slug, "the slug for {device:?} is on disk");
    }
}

#[test]
fn every_slug_is_a_legal_folder_name() {
    let mut devices: Vec<DeviceKind> = Vec::new();
    devices.extend(
        InstrumentKind::ALL
            .iter()
            .copied()
            .map(DeviceKind::Instrument),
    );
    devices.extend(EffectKind::ALL.iter().copied().map(DeviceKind::Effect));
    // A plugin id is a reverse-domain name and may carry anything the vendor
    // liked; the slug has to survive being a folder anyway.
    devices.push(DeviceKind::Plugin(PluginKey::new(
        PluginFormat::Lv2,
        "http://example.org/plugins/weird one",
    )));
    for device in devices {
        let slug = device.slug();
        assert!(!slug.is_empty(), "{device:?} has no slug");
        assert!(
            !slug.contains(['/', '\\', ':']),
            "{device:?} slugs to {slug:?}, which is not one folder"
        );
        assert!(
            slug != "." && slug != "..",
            "{device:?} slugs to a folder that means somewhere else"
        );
        assert!(
            slug.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')),
            "{device:?} slugs to {slug:?}, which is not portable across filesystems"
        );
    }
}

#[test]
fn a_file_from_a_newer_build_is_refused_as_such() {
    let from_the_future = serde_json::json!({
        "format_version": PRESET_FORMAT_VERSION + 1,
        "device": { "instrument": "Osc3" },
        "name": "Tomorrow",
        "category": "Lead",
        "payload": { "patch": { "format_version": 0, "body": {} } },
    });
    let preset: Preset =
        serde_json::from_value(from_the_future).expect("the envelope still parses");
    assert!(
        preset.is_from_the_future(),
        "a preset a newer build wrote must be reported as such, not as damage"
    );
    let ours = Preset {
        format_version: PRESET_FORMAT_VERSION,
        device: DeviceKind::Instrument(InstrumentKind::Osc3),
        name: "Today".to_string(),
        category: "Lead".to_string(),
        payload: PresetPayload::Patch(a_patch()),
    };
    assert!(!ours.is_from_the_future());
}

#[test]
fn a_preset_ref_names_a_file_and_says_where_it_came_from() {
    let reference = PresetRef {
        name: "Choir Ahh".to_string(),
        category: "Choir & Vocal".to_string(),
        origin: PresetOrigin::Factory,
    };
    let text = serde_json::to_string(&reference).expect("serialises");
    let back: PresetRef = serde_json::from_str(&text).expect("reads back");
    assert_eq!(back, reference);
    assert!(
        text.contains("factory"),
        "the origin is written by name, so a settings file is readable: {text}"
    );
}

/// The device a payload is for, without the device kind beside it — what the
/// bank needs to file a preset it has just read.
#[test]
fn a_payload_says_which_shape_it_is() {
    assert!(matches!(
        PresetPayload::Patch(a_patch()),
        PresetPayload::Patch(_)
    ));
    let effect = PresetPayload::Effect(EffectConfig::new(EffectKind::Chorus));
    match &effect {
        PresetPayload::Effect(config) => assert_eq!(config.kind(), EffectKind::Chorus),
        other => panic!("expected an effect payload, got {other:?}"),
    }
}
