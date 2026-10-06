//! What a starred thing is, as the settings file remembers it.
//!
//! > *"favorite (star) plugins, instruments, effects, etc. so that the
//! > favorites are always the most visible"*
//!
//! A favourite is a fact about the person, not the project — it goes in the
//! settings file, which is read by a person with a text editor, so the written
//! form has to be one they can read and grep.

use fontelle_types::{EffectKind, Favorite, InstrumentKind, PluginKey};

#[test]
fn every_kind_of_favourite_survives_the_round_trip() {
    let all = vec![
        Favorite::Effect(EffectKind::Reverb),
        Favorite::Instrument(InstrumentKind::DrumMachine),
        Favorite::Plugin(PluginKey::clap("org.surge-synth-team.surge-xt")),
    ];
    let json = serde_json::to_string(&all).unwrap();
    let back: Vec<Favorite> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, all);
}

#[test]
fn the_written_form_says_what_it_is_in_words() {
    // `{"plugin":"clap:..."}` and `{"effect":"Reverb"}`: a line somebody can
    // read out of `settings.json` and know what they starred.
    let plugin = Favorite::Plugin(PluginKey::clap("com.u-he.diva"));
    assert_eq!(
        serde_json::to_string(&plugin).unwrap(),
        r#"{"plugin":"clap:com.u-he.diva"}"#
    );
    let effect = Favorite::Effect(EffectKind::Eq);
    assert_eq!(
        serde_json::to_string(&effect).unwrap(),
        r#"{"effect":"Eq"}"#
    );
    let kind = Favorite::Instrument(InstrumentKind::Osc3);
    assert_eq!(
        serde_json::to_string(&kind).unwrap(),
        r#"{"instrument":"Osc3"}"#
    );
}

#[test]
fn a_favourite_naming_a_plugin_this_build_cannot_name_is_refused_rather_than_guessed() {
    let read: Result<Favorite, _> = serde_json::from_str(r#"{"plugin":"aax:com.example.thing"}"#);
    assert!(read.is_err());
}

/// A preset is starred by its category (its bank) as well as its name: two
/// "Init"s in two banks are two presets. A star written before the
/// category was part of it — no `category` — still reads, as one waiting
/// to be matched to a preset.
#[test]
fn a_preset_favourite_names_its_category_and_an_old_one_without_still_reads() {
    use fontelle_types::{DeviceKind, PresetOrigin};
    let device = DeviceKind::Plugin(PluginKey::clap("org.example.synth"));
    let new = Favorite::Preset {
        device: device.clone(),
        name: "Init".to_string(),
        origin: PresetOrigin::Plugin,
        category: Some("Bank A".to_string()),
    };
    let json = serde_json::to_string(&new).unwrap();
    assert!(json.contains("\"category\":\"Bank A\""), "{json}");
    assert_eq!(serde_json::from_str::<Favorite>(&json).unwrap(), new);

    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    old["preset"].as_object_mut().unwrap().remove("category");
    let read: Favorite = serde_json::from_value(old).unwrap();
    assert_eq!(
        read,
        Favorite::Preset {
            device,
            name: "Init".to_string(),
            origin: PresetOrigin::Plugin,
            category: None,
        }
    );
}
