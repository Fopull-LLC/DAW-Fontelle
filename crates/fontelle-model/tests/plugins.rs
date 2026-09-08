//! An insert or a channel holding a plugin somebody else wrote (TDD §8.4).

use fontelle_model::{
    AddPluginInsert, Command, EffectSlot, History, MixerTrack, Project, RemoveInsert,
    SetChannelPlugin, SetPluginParam,
};
use fontelle_types::{EffectKind, MixerTrackId, PluginKey, PluginState};

fn fixture() -> (Project, MixerTrackId) {
    let mut project = Project::new("plugins");
    let track = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(track);
    (project, track)
}

fn state() -> PluginState {
    let mut state = PluginState::new(PluginKey::clap("com.example.thing"), "Thing");
    state.set_param(0, 1.0);
    state
}

#[test]
fn an_insert_can_hold_a_plugin_instead_of_a_built_in_effect() {
    let slot = EffectSlot::hosting(state());
    assert!(slot.is_plugin());
    assert_eq!(slot.kind(), None);
    assert_eq!(slot.config(), None);
    assert_eq!(slot.label(), "Thing");
}

#[test]
fn a_built_in_insert_still_says_what_it_is() {
    let slot = EffectSlot::new(EffectKind::Reverb);
    assert!(!slot.is_plugin());
    assert_eq!(slot.kind(), Some(EffectKind::Reverb));
    assert_eq!(slot.label(), "Reverb");
}

#[test]
fn a_plugin_insert_names_the_plugin_even_when_it_is_not_installed() {
    let slot = EffectSlot::hosting(PluginState::new(PluginKey::clap("com.u-he.diva"), "Diva"));
    assert_eq!(slot.label(), "Diva");
}

/// A plugin insert **can** be keyed, and a key on it is an edge.
///
/// CLAP expresses a sidechain as a second audio input port, and whether a
/// plugin has one is the host's knowledge, not the document's — so the
/// document lets the key be set and the host decides what to feed. An edge
/// that feeds a plugin with no such port still only orders the graph and
/// refuses a cycle, which costs nothing anybody can hear.
#[test]
fn a_plugin_insert_takes_a_sidechain_key_and_the_key_is_an_edge() {
    let (mut project, master) = fixture();
    let kick = project.mixer.tracks.insert(MixerTrack::new("Kick"));
    project.mixer.tracks[master]
        .inserts
        .push(EffectSlot::hosting(state()));
    fontelle_model::SetInsertKey::new(master, 0, Some(kick))
        .apply(&mut project)
        .expect("a plugin insert takes a key");
    let slot = &project.mixer.tracks[master].inserts[0];
    assert_eq!(slot.key, Some(kick));
    assert_eq!(slot.effective_key(), Some(kick), "and it is an edge");
    assert_eq!(
        project.mixer.key_listeners().get(&kick),
        Some(&vec![master]),
        "the kick is scheduled before the track whose plugin listens to it"
    );
    // The rules a built-in effect's key follows hold here too.
    assert!(
        fontelle_model::SetInsertKey::new(master, 0, Some(master))
            .apply(&mut project)
            .is_err(),
        "a track cannot key itself"
    );
    assert_eq!(project.mixer.tracks[master].inserts[0].key, Some(kick));
}

#[test]
fn adding_a_plugin_to_a_chain_is_one_undo() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(Box::new(AddPluginInsert::new(track, state())), &mut project)
        .unwrap();
    assert_eq!(project.mixer.tracks[track].inserts.len(), 1);
    assert!(project.mixer.tracks[track].inserts[0].is_plugin());

    history.undo(&mut project).unwrap().unwrap();
    assert!(project.mixer.tracks[track].inserts.is_empty());
}

#[test]
fn removing_a_plugin_insert_keeps_it_for_the_undo() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(Box::new(AddPluginInsert::new(track, state())), &mut project)
        .unwrap();
    history
        .apply(Box::new(RemoveInsert::new(track, 0)), &mut project)
        .unwrap();
    assert!(project.mixer.tracks[track].inserts.is_empty());

    history.undo(&mut project).unwrap().unwrap();
    let slot = &project.mixer.tracks[track].inserts[0];
    assert_eq!(slot.plugin.as_ref().unwrap().param(0), Some(1.0));
}

#[test]
fn a_plugin_parameter_is_written_where_the_plugin_named_it() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(Box::new(AddPluginInsert::new(track, state())), &mut project)
        .unwrap();
    history
        .apply(
            Box::new(SetPluginParam::insert(track, 0, 7, 0.75)),
            &mut project,
        )
        .unwrap();
    let slot = &project.mixer.tracks[track].inserts[0];
    assert_eq!(slot.plugin.as_ref().unwrap().param(7), Some(0.75));
    assert_eq!(
        slot.plugin.as_ref().unwrap().param(0),
        Some(1.0),
        "the others are left alone"
    );
}

#[test]
fn a_drag_of_a_plugins_knob_is_one_undo() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(Box::new(AddPluginInsert::new(track, state())), &mut project)
        .unwrap();
    for value in [0.1, 0.2, 0.3] {
        history
            .apply(
                Box::new(SetPluginParam::insert(track, 0, 0, value)),
                &mut project,
            )
            .unwrap();
    }
    let slot = &project.mixer.tracks[track].inserts[0];
    assert_eq!(slot.plugin.as_ref().unwrap().param(0), Some(0.3));

    history.undo(&mut project).unwrap().unwrap();
    let slot = &project.mixer.tracks[track].inserts[0];
    assert_eq!(
        slot.plugin.as_ref().unwrap().param(0),
        Some(1.0),
        "back to before the drag, not to its middle"
    );
}

#[test]
fn two_different_knobs_are_two_things_a_person_did() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(Box::new(AddPluginInsert::new(track, state())), &mut project)
        .unwrap();
    history
        .apply(
            Box::new(SetPluginParam::insert(track, 0, 0, 0.5)),
            &mut project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetPluginParam::insert(track, 0, 1, 0.5)),
            &mut project,
        )
        .unwrap();

    history.undo(&mut project).unwrap().unwrap();
    let slot = &project.mixer.tracks[track].inserts[0];
    assert_eq!(slot.plugin.as_ref().unwrap().param(0), Some(0.5));
    assert_eq!(slot.plugin.as_ref().unwrap().param(1), None);
}

#[test]
fn a_parameter_of_a_slot_that_is_not_a_plugin_is_refused() {
    let (mut project, track) = fixture();
    project.mixer.tracks[track]
        .inserts
        .push(EffectSlot::new(EffectKind::Reverb));
    let mut command = SetPluginParam::insert(track, 0, 0, 0.5);
    assert!(command.apply(&mut project).is_err());
}

#[test]
fn a_channel_can_play_a_plugin() {
    let mut project = Project::new("plugins");
    let channel = project.channels.insert(fontelle_model::Channel {
        preset: None,
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        instrument: None,
        pan: 0.0,
        gain_db: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
    });
    let mut history = History::new();
    history
        .apply(
            Box::new(SetChannelPlugin::new(channel, Some(state()))),
            &mut project,
        )
        .unwrap();
    assert_eq!(
        project.channels[channel].instrument,
        Some(fontelle_types::InstrumentKind::Plugin)
    );
    assert_eq!(
        project.channels[channel].plugin.as_ref().unwrap().name,
        "Thing"
    );

    history.undo(&mut project).unwrap().unwrap();
    assert!(project.channels[channel].plugin.is_none());
    assert_eq!(project.channels[channel].instrument, None);
}

#[test]
fn a_channels_plugin_parameter_is_written_and_undone() {
    let mut project = Project::new("plugins");
    let channel = project.channels.insert(fontelle_model::Channel {
        preset: None,
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: Some(state()),
        instrument: Some(fontelle_types::InstrumentKind::Plugin),
        pan: 0.0,
        gain_db: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
    });
    let mut history = History::new();
    history
        .apply(
            Box::new(SetPluginParam::channel(channel, 0, 0.25)),
            &mut project,
        )
        .unwrap();
    assert_eq!(
        project.channels[channel].plugin.as_ref().unwrap().param(0),
        Some(0.25)
    );

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(
        project.channels[channel].plugin.as_ref().unwrap().param(0),
        Some(1.0)
    );
}

#[test]
fn a_project_with_plugins_in_it_reads_back() {
    let (mut project, track) = fixture();
    project.mixer.tracks[track]
        .inserts
        .push(EffectSlot::hosting(state()));
    let json = serde_json::to_string(&project).unwrap();
    let read: Project = serde_json::from_str(&json).unwrap();
    let slot = &read.mixer.tracks[track].inserts[0];
    assert_eq!(slot.plugin.as_ref().unwrap().name, "Thing");
}

#[test]
fn a_project_written_before_plugins_existed_still_opens() {
    let (mut project, track) = fixture();
    project.mixer.tracks[track]
        .inserts
        .push(EffectSlot::new(EffectKind::Delay));
    let json = serde_json::to_string(&project).unwrap();
    assert!(
        !json.contains("\"plugin\""),
        "nothing is written for an empty one"
    );
    let read: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(
        read.mixer.tracks[track].inserts[0].kind(),
        Some(EffectKind::Delay)
    );
}
