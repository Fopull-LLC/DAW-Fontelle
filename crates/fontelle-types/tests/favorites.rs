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
