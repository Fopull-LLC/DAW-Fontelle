//! What the document has to know about a plugin it did not write (TDD §8.4).

use fontelle_types::{PluginFormat, PluginKey, PluginParamValue, PluginState};

#[test]
fn a_key_reads_back_from_its_written_form() {
    let key = PluginKey::clap("com.u-he.diva");
    assert_eq!(key.to_string(), "clap:com.u-he.diva");
    assert_eq!(PluginKey::parse("clap:com.u-he.diva"), Some(key));
}

#[test]
fn an_id_containing_a_colon_survives_the_round_trip() {
    let key = PluginKey::clap("com.example:weird:id");
    assert_eq!(PluginKey::parse(&key.to_string()), Some(key));
}

#[test]
fn a_key_in_a_format_this_build_does_not_know_is_not_a_key() {
    assert_eq!(PluginKey::parse("aax:com.example.thing"), None);
    assert_eq!(PluginKey::parse("com.example.thing"), None);
    assert_eq!(PluginKey::parse("clap:"), None);
}

#[test]
fn clap_lv2_and_vst3_are_hosted_and_vst2_is_bridged_only() {
    // CLAP, LV2 and VST 3 are hosted in the tree; VST 2 is named so a
    // project can say what it could not load, but reached only through a
    // bridge (`docs/vst-plan.md`).
    assert!(PluginFormat::Clap.hosted());
    assert!(PluginFormat::Lv2.hosted());
    assert!(PluginFormat::Vst3.hosted());
    assert!(!PluginFormat::Vst2.hosted());
    assert!(PluginFormat::ALL.contains(&PluginFormat::Vst2));
}

#[test]
fn every_format_has_a_label_and_a_file_extension() {
    for format in PluginFormat::ALL {
        assert!(!format.label().is_empty());
        assert!(!format.extension().is_empty());
        assert!(!format.extension().starts_with('.'));
    }
}

#[test]
fn a_plugin_slot_round_trips_through_json() {
    let state = PluginState {
        key: PluginKey::clap("com.fopull.fontelle.testgain"),
        name: "Fontelle Test Gain".to_string(),
        params: vec![
            PluginParamValue { id: 0, value: 3.0 },
            PluginParamValue { id: 7, value: -1.5 },
        ],
        blob: Some("QUJD".to_string()),
    };
    let json = serde_json::to_string(&state).unwrap();
    let read: PluginState = serde_json::from_str(&json).unwrap();
    assert_eq!(read, state);
}

#[test]
fn a_slot_written_before_there_were_blobs_still_reads() {
    let json = r#"{"key":"clap:com.example.thing","name":"Thing","params":[]}"#;
    let read: PluginState = serde_json::from_str(json).unwrap();
    assert_eq!(read.key, PluginKey::clap("com.example.thing"));
    assert_eq!(read.blob, None);
}

#[test]
fn a_key_is_written_as_one_string_rather_than_a_pair() {
    let state = PluginState::new(PluginKey::clap("com.example.thing"), "Thing");
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains(r#""key":"clap:com.example.thing""#), "{json}");
}

#[test]
fn a_state_remembers_what_a_parameter_was_set_to() {
    let mut state = PluginState::new(PluginKey::clap("x"), "X");
    state.set_param(7, 0.25);
    state.set_param(7, 0.5);
    assert_eq!(state.param(7), Some(0.5));
    assert_eq!(state.param(8), None);
    assert_eq!(state.params.len(), 1);
}
