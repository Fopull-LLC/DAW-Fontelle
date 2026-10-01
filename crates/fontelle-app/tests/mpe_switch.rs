//! A plugin channel's **MPE switch**, from the rack's menu
//! (`docs/note-paths-plan.md` §6): offered for a plugin that hears raw MIDI
//! — LV2, or a format reached through a bridge — and not for CLAP or VST 3,
//! which carry a slide per note already. One undo, like every switch.

mod common;

use fontelle_types::{PluginFormat, PluginKey, PluginState};
use fontelle_ui::document::{DocumentHost, StudioHost};

fn a_session_playing(format: PluginFormat) -> fontelle_app::Session {
    let mut project = common::a_clip_project(1);
    let channel = project.channels.keys().next().expect("a channel");
    project.channels[channel].plugin = Some(PluginState::new(
        PluginKey::new(format, "urn:not-installed"),
        "Synth",
    ));
    common::a_session_for(project)
}

#[test]
fn an_lv2_channel_offers_the_switch_and_it_is_one_undo() {
    let mut session = a_session_playing(PluginFormat::Lv2);
    assert_eq!(session.channels()[0].mpe, Some(false), "offered, off");

    session.set_plugin_mpe(0, true);
    assert_eq!(session.channels()[0].mpe, Some(true));
    let channel = session.project().channels.keys().next().unwrap();
    assert!(
        session.project().channels[channel]
            .plugin
            .as_ref()
            .unwrap()
            .mpe
    );

    session.undo();
    assert_eq!(session.channels()[0].mpe, Some(false));
}

#[test]
fn a_clap_channel_has_no_switch_to_offer() {
    let session = a_session_playing(PluginFormat::Clap);
    assert_eq!(session.channels()[0].mpe, None);
}

#[test]
fn a_channel_with_no_plugin_has_no_switch_either() {
    let session = common::a_session_for(common::a_clip_project(1));
    assert_eq!(session.channels()[0].mpe, None);
}
