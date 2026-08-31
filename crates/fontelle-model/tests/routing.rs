//! Channels, mixer tracks, and the wire between them.
//!
//! Reported from using the window:
//!
//! > *"Channels shouldn't have their own mixer track. You should be able to
//! > make as many mixer tracks as you want and then route any channel to any
//! > mixer track you want. By default it just goes straight to master."*
//!
//! That is FL Studio's model and it is the right one: a mixer track is a
//! *destination* you build deliberately — a drum bus, a reverb send — not a
//! thing that appears every time you load a soundfont. Twenty channels used to
//! mean twenty strips nobody asked for.
//!
//! The change has one consequence worth naming, and it is why this file also
//! covers mute and solo: **the channel rack's two switches used to be the
//! mixer track's.** With every channel on master by default, a rack mute would
//! now mute the master and silence the song. They are the channel's own now,
//! and they are *sequencer* mutes (TDD §10.3's reading, the same one lane mute
//! has) rather than fader mutes.

use fontelle_model::{
    AddChannel, AddMixerTrack, Command, FlagTarget, Project, RemoveChannel, RemoveMixerTrack,
    RenameMixerTrack, SetChannelRoute, SetFlag,
};
use fontelle_types::MixerTrackId;

fn project() -> Project {
    Project::new("routing")
}

/// Applies `command`, checks its inverse puts the document back, and leaves
/// the command applied.
fn round_trip(
    mut command: Box<dyn Command>,
    project: &mut Project,
    before: impl Fn(&Project) -> String,
) {
    let state = before(project);
    command.apply(project).expect("the command must apply");
    let mut inverse = command.invert();
    inverse.apply(project).expect("the inverse must apply");
    assert_eq!(
        before(project),
        state,
        "{} did not round-trip",
        command.label()
    );
    command.apply(project).expect("and it must re-apply");
}

// -------------------------------------------------------------- routing ---

#[test]
fn a_new_channel_goes_to_the_master_and_makes_no_track_of_its_own() {
    let mut project = project();
    let tracks_before = project.mixer.tracks.len();

    let mut add = AddChannel::new("Strings", None);
    add.apply(&mut project).unwrap();
    let channel = add.channel().expect("the id is known once it is applied");

    assert_eq!(
        project.mixer.tracks.len(),
        tracks_before,
        "loading a soundfont is not a request for a mixer strip"
    );
    assert_eq!(
        project.channels[channel].mixer_track, None,
        "and None is the master, which is where a new channel plays"
    );
}

#[test]
fn a_mixer_track_is_made_deliberately_and_can_be_taken_back() {
    let mut project = project();
    let mut add = AddMixerTrack::new("Drum bus");
    add.apply(&mut project).unwrap();

    let track = add.track().expect("the id is known once it is applied");
    assert_eq!(project.mixer.tracks[track].name, "Drum bus");
    assert_eq!(
        project.mixer.tracks[track].output, project.mixer.master,
        "a new track feeds the master until it is routed somewhere else"
    );

    add.invert().apply(&mut project).unwrap();
    assert!(!project.mixer.tracks.contains_key(track));
}

#[test]
fn a_channel_can_be_pointed_at_any_track_and_back_at_the_master() {
    let mut project = project();
    let mut add_track = AddMixerTrack::new("Drum bus");
    add_track.apply(&mut project).unwrap();
    let track = add_track.track().unwrap();

    let mut add_channel = AddChannel::new("Kick", None);
    add_channel.apply(&mut project).unwrap();
    let channel = add_channel.channel().unwrap();

    SetChannelRoute::new(channel, Some(track))
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.channels[channel].mixer_track, Some(track));

    SetChannelRoute::new(channel, None)
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.channels[channel].mixer_track, None);
}

#[test]
fn routing_a_channel_is_undoable() {
    let mut project = project();
    let mut add_track = AddMixerTrack::new("Bus");
    add_track.apply(&mut project).unwrap();
    let track = add_track.track().unwrap();
    let mut add_channel = AddChannel::new("Kick", None);
    add_channel.apply(&mut project).unwrap();
    let channel = add_channel.channel().unwrap();

    round_trip(
        Box::new(SetChannelRoute::new(channel, Some(track))),
        &mut project,
        move |p| format!("{:?}", p.channels[channel].mixer_track),
    );
}

#[test]
fn several_channels_can_share_one_track() {
    // TDD §13.1 allows it and this is what makes a drum bus a drum bus.
    let mut project = project();
    let mut add_track = AddMixerTrack::new("Drums");
    add_track.apply(&mut project).unwrap();
    let track = add_track.track().unwrap();

    for name in ["Kick", "Snare", "Hat"] {
        let mut add = AddChannel::new(name, None);
        add.apply(&mut project).unwrap();
        SetChannelRoute::new(add.channel().unwrap(), Some(track))
            .apply(&mut project)
            .unwrap();
    }
    assert_eq!(project.mixer.tracks.len(), 2, "a master and one drum bus");
    assert_eq!(
        project
            .channels
            .values()
            .filter(|c| c.mixer_track == Some(track))
            .count(),
        3
    );
}

#[test]
fn routing_a_channel_at_a_track_that_does_not_exist_is_refused() {
    // The one way a route can be wrong. A channel pointing at nothing is
    // silent for a reason nobody can see in the UI.
    let mut project = project();
    let mut add = AddChannel::new("Kick", None);
    add.apply(&mut project).unwrap();
    let channel = add.channel().unwrap();

    let ghost = MixerTrackId::default();
    assert!(
        SetChannelRoute::new(channel, Some(ghost))
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(project.channels[channel].mixer_track, None);
}

// ------------------------------------------------- removing a track ---

#[test]
fn deleting_a_track_sends_everything_on_it_back_to_the_master() {
    // The alternative is a channel pointing at nothing, which is silent for a
    // reason nobody can see. Falling back to master is audible and fixable.
    let mut project = project();
    let mut add_track = AddMixerTrack::new("Bus");
    add_track.apply(&mut project).unwrap();
    let track = add_track.track().unwrap();
    let mut add_channel = AddChannel::new("Kick", None);
    add_channel.apply(&mut project).unwrap();
    let channel = add_channel.channel().unwrap();
    SetChannelRoute::new(channel, Some(track))
        .apply(&mut project)
        .unwrap();

    let mut remove = RemoveMixerTrack::new(track);
    remove.apply(&mut project).unwrap();
    assert!(!project.mixer.tracks.contains_key(track));
    assert_eq!(project.channels[channel].mixer_track, None);

    // And undo brings the track back *with* everything that was on it.
    remove.invert().apply(&mut project).unwrap();
    assert!(project.mixer.tracks.contains_key(track));
    assert_eq!(project.channels[channel].mixer_track, Some(track));
}

#[test]
fn a_track_feeding_a_deleted_one_is_sent_back_to_the_master_too() {
    let mut project = project();
    let mut add_bus = AddMixerTrack::new("Bus");
    add_bus.apply(&mut project).unwrap();
    let bus = add_bus.track().unwrap();
    let mut add_sub = AddMixerTrack::new("Sub");
    add_sub.apply(&mut project).unwrap();
    let sub = add_sub.track().unwrap();
    project.mixer.tracks[sub].output = Some(bus);

    RemoveMixerTrack::new(bus).apply(&mut project).unwrap();
    assert_eq!(
        project.mixer.tracks[sub].output, project.mixer.master,
        "a track routed into thin air is a track nobody can hear"
    );
}

#[test]
fn the_master_cannot_be_deleted() {
    // Every project has one (TDD §13.1) and nothing downstream handles its
    // absence — `realise` reports `NoMaster` and refuses to build a graph.
    let mut project = project();
    let master = project.mixer.master.unwrap();
    assert!(RemoveMixerTrack::new(master).apply(&mut project).is_err());
    assert!(project.mixer.tracks.contains_key(master));
}

#[test]
fn a_track_can_be_renamed_and_it_is_undoable() {
    let mut project = project();
    let mut add = AddMixerTrack::new("Track 1");
    add.apply(&mut project).unwrap();
    let track = add.track().unwrap();

    round_trip(
        Box::new(RenameMixerTrack::new(track, "Drums")),
        &mut project,
        move |p| p.mixer.tracks[track].name.clone(),
    );
    assert_eq!(project.mixer.tracks[track].name, "Drums");
}

// ------------------------------------------- the rack's two switches ---

#[test]
fn a_channels_mute_is_the_channels_and_not_its_tracks() {
    // The whole reason these moved. With every channel on the master by
    // default, a rack mute that reached for the track would mute the master
    // and silence the entire song.
    let mut project = project();
    let mut add = AddChannel::new("Kick", None);
    add.apply(&mut project).unwrap();
    let channel = add.channel().unwrap();
    let master = project.mixer.master.unwrap();

    SetFlag::new(FlagTarget::ChannelMuted(channel), true)
        .apply(&mut project)
        .unwrap();
    assert!(project.channels[channel].muted);
    assert!(
        !project.mixer.tracks[master].mute,
        "and the master is untouched"
    );
}

#[test]
fn muting_and_soloing_a_channel_round_trip() {
    let mut project = project();
    let mut add = AddChannel::new("Kick", None);
    add.apply(&mut project).unwrap();
    let channel = add.channel().unwrap();

    round_trip(
        Box::new(SetFlag::new(FlagTarget::ChannelMuted(channel), true)),
        &mut project,
        move |p| p.channels[channel].muted.to_string(),
    );
    round_trip(
        Box::new(SetFlag::new(FlagTarget::ChannelSoloed(channel), true)),
        &mut project,
        move |p| p.channels[channel].soloed.to_string(),
    );
}

#[test]
fn removing_a_channel_leaves_the_track_it_was_playing_through() {
    // It is a destination somebody built, and other channels may be on it.
    let mut project = project();
    let mut add_track = AddMixerTrack::new("Bus");
    add_track.apply(&mut project).unwrap();
    let track = add_track.track().unwrap();
    let mut add_channel = AddChannel::new("Kick", None);
    add_channel.apply(&mut project).unwrap();
    let channel = add_channel.channel().unwrap();
    SetChannelRoute::new(channel, Some(track))
        .apply(&mut project)
        .unwrap();

    RemoveChannel::new(channel).apply(&mut project).unwrap();
    assert!(
        project.mixer.tracks.contains_key(track),
        "deleting a channel must not delete a bus other things may be on"
    );
}
