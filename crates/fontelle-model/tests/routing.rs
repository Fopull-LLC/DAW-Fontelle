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
    AddChannel, AddMixerTrack, AddSend, Command, FlagTarget, Project, RemoveChannel,
    RemoveMixerTrack, RemoveSend, RenameMixerTrack, SetChannelRoute, SetFlag, SetSendLevel,
    SetSendPreFader, SetTrackOutput,
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

// ------------------------------------------------- a track's own output ---
//
// Reported from using the window: *"there's also no routing wiring yet to
// route tracks to other tracks (tracks should all start just wiring into
// master by default)"*.
//
// The document has carried `MixerTrack::output` since the mixer was written,
// and `Mixer::has_cycle` has carried the check §13.2 demands — but there was
// no command, so the only way to point one track at another was to write the
// field by hand, which is what the two tests above had to do.

/// Where `track` actually ends up.
///
/// **The master has two spellings** and both mean the same destination:
/// `MixerTrack::output` documents `None` as the master, and `AddMixerTrack`
/// names it outright because it is "the only destination that is always
/// there". `realise` reads them alike (`output.unwrap_or(master)`), so a test
/// that insisted on one of them would be pinning down an incidental choice
/// rather than the routing.
fn destination(project: &Project, track: MixerTrackId) -> Option<MixerTrackId> {
    project.mixer.tracks[track].output.or(project.mixer.master)
}

#[test]
fn a_track_starts_on_the_master_and_can_be_pointed_at_another_track() {
    let mut project = project();
    let master = project.mixer.master.unwrap();
    let mut add_bus = AddMixerTrack::new("Drum bus");
    add_bus.apply(&mut project).unwrap();
    let bus = add_bus.track().unwrap();
    let mut add_kick = AddMixerTrack::new("Kick");
    add_kick.apply(&mut project).unwrap();
    let kick = add_kick.track().unwrap();

    assert_eq!(
        destination(&project, kick),
        Some(master),
        "a new track goes to the master — *\"tracks should all start just \
         wiring into master by default\"*"
    );

    round_trip(
        Box::new(SetTrackOutput::new(kick, Some(bus))),
        &mut project,
        |p| format!("{:?}", p.mixer.tracks[kick].output),
    );
    assert_eq!(destination(&project, kick), Some(bus));

    // And back to the master, which is a routing decision like any other.
    SetTrackOutput::new(kick, None).apply(&mut project).unwrap();
    assert_eq!(destination(&project, kick), Some(master));
}

#[test]
fn a_track_cannot_be_routed_into_itself() {
    // The shortest possible feedback loop, and the easiest one to click by
    // accident in a menu that lists every track.
    let mut project = project();
    let mut add = AddMixerTrack::new("Bus");
    add.apply(&mut project).unwrap();
    let bus = add.track().unwrap();

    assert!(
        SetTrackOutput::new(bus, Some(bus)).apply(&mut project).is_err(),
        "a track routed into itself is a feedback loop the graph compiler \
         cannot build"
    );
    assert_eq!(
        destination(&project, bus),
        project.mixer.master,
        "a refused command must leave the document alone"
    );
}

#[test]
fn a_routing_that_would_close_a_loop_is_refused() {
    // §13.2: the routing graph must be validated acyclic on *every* mutation —
    // reject the command, never let a feedback loop reach the graph compiler.
    let mut project = project();
    let mut add_a = AddMixerTrack::new("A");
    add_a.apply(&mut project).unwrap();
    let a = add_a.track().unwrap();
    let mut add_b = AddMixerTrack::new("B");
    add_b.apply(&mut project).unwrap();
    let b = add_b.track().unwrap();

    SetTrackOutput::new(a, Some(b)).apply(&mut project).unwrap();
    assert!(
        SetTrackOutput::new(b, Some(a)).apply(&mut project).is_err(),
        "A into B into A is a loop"
    );
    assert_eq!(destination(&project, b), project.mixer.master);
    assert!(!project.mixer.has_cycle());
}

#[test]
fn a_longer_loop_is_refused_too() {
    let mut project = project();
    let mut ids = Vec::new();
    for name in ["A", "B", "C"] {
        let mut add = AddMixerTrack::new(name);
        add.apply(&mut project).unwrap();
        ids.push(add.track().unwrap());
    }
    SetTrackOutput::new(ids[0], Some(ids[1]))
        .apply(&mut project)
        .unwrap();
    SetTrackOutput::new(ids[1], Some(ids[2]))
        .apply(&mut project)
        .unwrap();

    assert!(
        SetTrackOutput::new(ids[2], Some(ids[0]))
            .apply(&mut project)
            .is_err(),
        "A into B into C into A is still a loop, three edges out"
    );
    assert!(!project.mixer.has_cycle());
}

#[test]
fn routing_a_track_at_one_that_does_not_exist_is_refused() {
    let mut project = project();
    let mut add = AddMixerTrack::new("Bus");
    add.apply(&mut project).unwrap();
    let bus = add.track().unwrap();
    let gone = MixerTrackId::default();

    assert!(
        SetTrackOutput::new(bus, Some(gone))
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(destination(&project, bus), project.mixer.master);
}

#[test]
fn the_master_has_nowhere_to_be_routed() {
    // It is where everything arrives. Giving it an output is either a loop or
    // a second master, and neither is a thing this document can mean.
    let mut project = project();
    let master = project.mixer.master.unwrap();
    let mut add = AddMixerTrack::new("Bus");
    add.apply(&mut project).unwrap();
    let bus = add.track().unwrap();

    assert!(
        SetTrackOutput::new(master, Some(bus))
            .apply(&mut project)
            .is_err(),
        "the master's output is the speakers"
    );
    assert_eq!(project.mixer.tracks[master].output, None);
}

// -------------------------------------------------------------- the sends ---
//
// TDD §13.2, the half that was never built: `MixerTrack::sends` has been in the
// document since the mixer was written, `Mixer::has_cycle` has counted send
// edges as signal paths from the day it was written, and nothing could make
// one — the field was only ever an empty `Vec`.
//
// A send is what a reverb bus *is*. Routing a track's output into a reverb
// sends all of it; a send takes a copy at a level and leaves the dry signal on
// its own path, which is the whole point.

#[test]
fn a_send_can_be_made_and_taken_back() {
    let mut project = project();
    let mut add_reverb = AddMixerTrack::new("Reverb");
    add_reverb.apply(&mut project).unwrap();
    let reverb = add_reverb.track().unwrap();
    let mut add_vox = AddMixerTrack::new("Vox");
    add_vox.apply(&mut project).unwrap();
    let vox = add_vox.track().unwrap();

    assert!(project.mixer.tracks[vox].sends.is_empty());
    round_trip(
        Box::new(AddSend::new(vox, reverb)),
        &mut project,
        |p| format!("{}", p.mixer.tracks[vox].sends.len()),
    );

    let sends = &project.mixer.tracks[vox].sends;
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].target, reverb);
    assert!(
        sends[0].level_db <= -60.0,
        "a new send starts at silence, not at unity: a send that arrived \
         wide open would change the mix the moment it was made"
    );
    assert!(
        !sends[0].pre_fader,
        "post-fader is what a reverb send wants: pull the fader down and the \
         reverb follows it"
    );
}

#[test]
fn the_dry_path_is_untouched_by_a_send() {
    // Which is the whole difference between a send and an output. Routing an
    // output moves the signal; a send takes a copy.
    let mut project = project();
    let mut add_reverb = AddMixerTrack::new("Reverb");
    add_reverb.apply(&mut project).unwrap();
    let reverb = add_reverb.track().unwrap();
    let mut add_vox = AddMixerTrack::new("Vox");
    add_vox.apply(&mut project).unwrap();
    let vox = add_vox.track().unwrap();

    let before = destination(&project, vox);
    AddSend::new(vox, reverb).apply(&mut project).unwrap();
    assert_eq!(destination(&project, vox), before);
}

#[test]
fn a_send_can_be_levelled_and_flipped_pre_fader() {
    let mut project = project();
    let mut add_reverb = AddMixerTrack::new("Reverb");
    add_reverb.apply(&mut project).unwrap();
    let reverb = add_reverb.track().unwrap();
    let mut add_vox = AddMixerTrack::new("Vox");
    add_vox.apply(&mut project).unwrap();
    let vox = add_vox.track().unwrap();
    AddSend::new(vox, reverb).apply(&mut project).unwrap();

    round_trip(
        Box::new(SetSendLevel::new(vox, 0, -6.0)),
        &mut project,
        |p| format!("{}", p.mixer.tracks[vox].sends[0].level_db),
    );
    assert!((project.mixer.tracks[vox].sends[0].level_db + 6.0).abs() < 1e-6);

    round_trip(
        Box::new(SetSendPreFader::new(vox, 0, true)),
        &mut project,
        |p| format!("{}", p.mixer.tracks[vox].sends[0].pre_fader),
    );
    assert!(project.mixer.tracks[vox].sends[0].pre_fader);
}

#[test]
fn a_send_level_drag_is_one_undo_entry() {
    // The same rule a fader follows: one gesture, one entry. Sixty of them for
    // one drag is an undo stack nobody can use.
    let mut project = project();
    let mut add = AddMixerTrack::new("Reverb");
    add.apply(&mut project).unwrap();
    let reverb = add.track().unwrap();
    let mut add_vox = AddMixerTrack::new("Vox");
    add_vox.apply(&mut project).unwrap();
    let vox = add_vox.track().unwrap();
    AddSend::new(vox, reverb).apply(&mut project).unwrap();

    let mut first = SetSendLevel::new(vox, 0, -20.0);
    assert!(
        first.merge_with(&SetSendLevel::new(vox, 0, -12.0)),
        "two moves of the same send's level are one gesture"
    );
    assert!(
        !first.merge_with(&SetSendLevel::new(vox, 1, -12.0)),
        "and two different sends are two"
    );
}

#[test]
fn a_send_can_be_removed_and_it_comes_back_with_its_settings() {
    let mut project = project();
    let mut add = AddMixerTrack::new("Reverb");
    add.apply(&mut project).unwrap();
    let reverb = add.track().unwrap();
    let mut add_vox = AddMixerTrack::new("Vox");
    add_vox.apply(&mut project).unwrap();
    let vox = add_vox.track().unwrap();
    AddSend::new(vox, reverb).apply(&mut project).unwrap();
    SetSendLevel::new(vox, 0, -9.0).apply(&mut project).unwrap();

    let mut remove = RemoveSend::new(vox, 0);
    remove.apply(&mut project).unwrap();
    assert!(project.mixer.tracks[vox].sends.is_empty());

    remove.invert().apply(&mut project).unwrap();
    let sends = &project.mixer.tracks[vox].sends;
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].target, reverb);
    assert!(
        (sends[0].level_db + 9.0).abs() < 1e-6,
        "a send deleted by accident comes back where it was set"
    );
}

#[test]
fn a_send_into_itself_is_refused() {
    let mut project = project();
    let mut add = AddMixerTrack::new("Bus");
    add.apply(&mut project).unwrap();
    let bus = add.track().unwrap();

    assert!(AddSend::new(bus, bus).apply(&mut project).is_err());
    assert!(project.mixer.tracks[bus].sends.is_empty());
}

#[test]
fn a_send_that_would_close_a_loop_is_refused() {
    // §13.2 is explicit that both edge kinds count, *"and a cycle through one
    // is the same feedback loop as a cycle through an output — it is just
    // harder to see in the UI"*. A into B by output, B into A by send, is a
    // loop.
    let mut project = project();
    let mut add_a = AddMixerTrack::new("A");
    add_a.apply(&mut project).unwrap();
    let a = add_a.track().unwrap();
    let mut add_b = AddMixerTrack::new("B");
    add_b.apply(&mut project).unwrap();
    let b = add_b.track().unwrap();

    SetTrackOutput::new(a, Some(b)).apply(&mut project).unwrap();
    assert!(AddSend::new(b, a).apply(&mut project).is_err());
    assert!(project.mixer.tracks[b].sends.is_empty());
    assert!(!project.mixer.has_cycle());
}

#[test]
fn an_output_that_would_close_a_loop_through_a_send_is_refused_too() {
    // The same check from the other side, so neither command is the only one
    // that knows about the other's edges.
    let mut project = project();
    let mut add_a = AddMixerTrack::new("A");
    add_a.apply(&mut project).unwrap();
    let a = add_a.track().unwrap();
    let mut add_b = AddMixerTrack::new("B");
    add_b.apply(&mut project).unwrap();
    let b = add_b.track().unwrap();

    AddSend::new(a, b).apply(&mut project).unwrap();
    assert!(SetTrackOutput::new(b, Some(a)).apply(&mut project).is_err());
    assert!(!project.mixer.has_cycle());
}

#[test]
fn sending_to_a_track_that_does_not_exist_is_refused() {
    let mut project = project();
    let mut add = AddMixerTrack::new("Vox");
    add.apply(&mut project).unwrap();
    let vox = add.track().unwrap();
    assert!(
        AddSend::new(vox, MixerTrackId::default())
            .apply(&mut project)
            .is_err()
    );
}

#[test]
fn one_track_can_feed_two_buses() {
    // A reverb and a delay off the same vocal is the ordinary case, not the
    // edge case.
    let mut project = project();
    let mut ids = Vec::new();
    for name in ["Reverb", "Delay", "Vox"] {
        let mut add = AddMixerTrack::new(name);
        add.apply(&mut project).unwrap();
        ids.push(add.track().unwrap());
    }
    let (reverb, delay, vox) = (ids[0], ids[1], ids[2]);
    AddSend::new(vox, reverb).apply(&mut project).unwrap();
    AddSend::new(vox, delay).apply(&mut project).unwrap();

    let targets: Vec<MixerTrackId> = project.mixer.tracks[vox]
        .sends
        .iter()
        .map(|s| s.target)
        .collect();
    assert_eq!(targets, vec![reverb, delay]);
    assert!(!project.mixer.has_cycle());
}

#[test]
fn deleting_a_track_takes_the_sends_that_fed_it_with_it() {
    // A send at a track that is not there is either silent or a panic, and
    // both are worse than the send simply going with the bus it fed.
    let mut project = project();
    let mut add_reverb = AddMixerTrack::new("Reverb");
    add_reverb.apply(&mut project).unwrap();
    let reverb = add_reverb.track().unwrap();
    let mut add_vox = AddMixerTrack::new("Vox");
    add_vox.apply(&mut project).unwrap();
    let vox = add_vox.track().unwrap();
    AddSend::new(vox, reverb).apply(&mut project).unwrap();

    let mut remove = RemoveMixerTrack::new(reverb);
    remove.apply(&mut project).unwrap();
    assert!(
        project.mixer.tracks[vox].sends.is_empty(),
        "a send was left pointing at a deleted track"
    );

    // And undo brings the bus back with what fed it.
    remove.invert().apply(&mut project).unwrap();
    assert_eq!(
        project.mixer.tracks[vox].sends.len(),
        1,
        "undoing the deletion has to restore the send too"
    );
    assert_eq!(project.mixer.tracks[vox].sends[0].target, reverb);
}
