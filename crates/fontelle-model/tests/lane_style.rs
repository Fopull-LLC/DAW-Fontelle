//! Lane-style routing: a project in which every lane owns a mixer track.
//!
//! `docs/ux-routing-and-learning-plan.md` §1. Two modes per project:
//! **rack-style** (FL's, and every project before this) where a channel's
//! route chip says where its sound goes, and **lane-style** (Reaper's,
//! Logic's) where each lane has a track and what is on the lane plays
//! through it. Ty's rule above all: *the two must never interfere*.
//!
//! And Ty's answer to the one hard case, one instrument drawn onto two lanes:
//! *"i definitely do NOT want to have duplicate copies of instruments"* — an
//! instrument **claims** the first lane its clip is drawn on, and plays
//! through that lane's track. It is still one running copy with one output.

use fontelle_model::{
    AddChannel, AddClip, AddMixerTrack, Clip, ClipSource, Command, Compound, Edit, Lane,
    LaneUpkeep, NoteData, Project, RoutingMode, SetChannelRoute, SetClipChannel, SetRoutingMode,
    lane_conflicts,
};
use fontelle_types::{AssetKind, AssetRef, AudioClipData, ChannelId, ClipId, LaneId, PPQN};

fn lane(project: &mut Project, name: &str, order: u32) -> LaneId {
    project.lanes.insert(Lane {
        name: name.into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order,
    })
}

fn channel(project: &mut Project, name: &str) -> ChannelId {
    let mut add = AddChannel::new(name, None);
    add.apply(project).unwrap();
    add.channel().unwrap()
}

fn notes_clip(project: &mut Project, lane: LaneId, channel: ChannelId, start: i64) -> ClipId {
    let mut add = AddClip::new(Clip {
        name: None,
        lane,
        start,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Default::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    add.apply(project).unwrap();
    add.id().unwrap()
}

fn audio_clip(project: &mut Project, lane: LaneId) -> ClipId {
    let asset = AssetRef {
        id: Default::default(),
        path: "vox.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    };
    let mut add = AddClip::new(Clip {
        name: None,
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Audio(AudioClipData::whole(asset, 48_000, 48_000)),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    add.apply(project).unwrap();
    add.id().unwrap()
}

/// Runs the upkeep a local edit is followed by in lane-style, if any is due.
fn upkeep(project: &mut Project) {
    if let Some(mut command) = LaneUpkeep::due(project) {
        command.apply(project).unwrap();
    }
}

/// A plain project: two lanes, a drum bus, and a piano routed to the bus with
/// a clip on the first lane, and an audio clip on the second routed to it too.
struct Song {
    project: Project,
    keys: LaneId,
    vox: LaneId,
    bus: fontelle_types::MixerTrackId,
    piano: ChannelId,
    take: ClipId,
}

fn song() -> Song {
    let mut project = Project::new("modes");
    let keys = lane(&mut project, "Keys", 0);
    let vox = lane(&mut project, "Vox", 1);
    let mut bus = AddMixerTrack::new("Bus");
    bus.apply(&mut project).unwrap();
    let bus = bus.track().unwrap();
    let piano = channel(&mut project, "Piano");
    SetChannelRoute::new(piano, Some(bus))
        .apply(&mut project)
        .unwrap();
    notes_clip(&mut project, keys, piano, 0);
    let take = audio_clip(&mut project, vox);
    if let ClipSource::Audio(data) = &mut project.clips[take].source {
        data.mixer_track = Some(bus);
    }
    Song {
        project,
        keys,
        vox,
        bus,
        piano,
        take,
    }
}

fn to_lane_style(project: &mut Project) {
    SetRoutingMode::new(RoutingMode::Lane)
        .apply(project)
        .unwrap();
    upkeep(project);
}

// ----------------------------------------------------------------- the mode

#[test]
fn a_project_is_rack_style_unless_it_says_otherwise() {
    let project = Project::new("new");
    assert_eq!(project.lane_routing.mode, RoutingMode::Rack);
    // And one written before modes existed reads as rack-style.
    let mut json = serde_json::to_value(&project).unwrap();
    json.as_object_mut().unwrap().remove("lane_routing");
    let old: Project = serde_json::from_value(json).unwrap();
    assert_eq!(old.lane_routing.mode, RoutingMode::Rack);
}

#[test]
fn rack_style_routes_by_the_channel_and_by_the_clip_as_it_always_has() {
    let s = song();
    assert_eq!(s.project.channel_route(s.piano), Some(s.bus));
    assert_eq!(s.project.clip_route(s.take), Some(s.bus));
    assert_eq!(
        LaneUpkeep::due(&s.project).map(|_| ()),
        None,
        "nothing to do"
    );
}

// ------------------------------------------------------------ lane-style

#[test]
fn in_lane_style_every_lane_gets_a_track_named_after_it() {
    let mut s = song();
    let tracks = s.project.mixer.tracks.len();
    to_lane_style(&mut s.project);
    assert_eq!(s.project.mixer.tracks.len(), tracks + 2);
    for id in [s.keys, s.vox] {
        let track = s.project.lane_track(id).expect("a track of its own");
        assert_eq!(s.project.mixer.tracks[track].name, s.project.lanes[id].name);
    }
    assert_ne!(s.project.lane_track(s.keys), s.project.lane_track(s.vox));
}

#[test]
fn in_lane_style_an_instrument_plays_through_the_track_of_the_lane_it_claimed() {
    let mut s = song();
    to_lane_style(&mut s.project);
    assert_eq!(s.project.claimed_lane(s.piano), Some(s.keys));
    assert_eq!(
        s.project.channel_route(s.piano),
        s.project.lane_track(s.keys)
    );
    // And audio plays through its own lane's track.
    assert_eq!(s.project.clip_route(s.take), s.project.lane_track(s.vox));
}

#[test]
fn switching_mode_resets_the_routes_and_keeps_the_tracks() {
    // Ty's answer B: *keeps the mixer tracks and resets the routes* to the
    // new mode's default. Nothing is converted, so nothing is half-converted.
    let mut s = song();
    to_lane_style(&mut s.project);
    let tracks = s.project.mixer.tracks.len();
    SetRoutingMode::new(RoutingMode::Rack)
        .apply(&mut s.project)
        .unwrap();
    assert_eq!(s.project.mixer.tracks.len(), tracks, "no track is lost");
    assert!(s.project.mixer.tracks.get(s.bus).is_some());
    assert_eq!(s.project.channel_route(s.piano), None, "back to the master");
    assert_eq!(s.project.clip_route(s.take), None);
    assert_eq!(
        LaneUpkeep::due(&s.project).map(|_| ()),
        None,
        "rack-style has no upkeep"
    );
}

#[test]
fn undoing_a_switch_puts_every_route_back() {
    let mut s = song();
    let mut switch = SetRoutingMode::new(RoutingMode::Lane);
    switch.apply(&mut s.project).unwrap();
    switch.invert().apply(&mut s.project).unwrap();
    assert_eq!(s.project.lane_routing.mode, RoutingMode::Rack);
    assert_eq!(s.project.channel_route(s.piano), Some(s.bus));
    assert_eq!(s.project.clip_route(s.take), Some(s.bus));
}

#[test]
fn the_upkeep_undoes_cleanly_and_redoes_with_the_same_tracks() {
    let mut s = song();
    SetRoutingMode::new(RoutingMode::Lane)
        .apply(&mut s.project)
        .unwrap();
    let tracks = s.project.mixer.tracks.len();
    let mut upkeep = LaneUpkeep::due(&s.project).expect("lanes to give tracks");
    upkeep.apply(&mut s.project).unwrap();
    let made = s.project.lane_track(s.keys);
    upkeep.invert().apply(&mut s.project).unwrap();
    assert_eq!(s.project.mixer.tracks.len(), tracks);
    assert_eq!(s.project.lane_track(s.keys), None);
    assert_eq!(s.project.claimed_lane(s.piano), None);
    upkeep.apply(&mut s.project).unwrap();
    assert_eq!(s.project.lane_track(s.keys), made, "the same id on a redo");
}

#[test]
fn a_new_lane_in_lane_style_gets_its_track_from_the_upkeep() {
    let mut s = song();
    to_lane_style(&mut s.project);
    let late = lane(&mut s.project, "Late", 2);
    assert!(LaneUpkeep::due(&s.project).is_some());
    upkeep(&mut s.project);
    assert!(s.project.lane_track(late).is_some());
    assert!(LaneUpkeep::due(&s.project).is_none(), "and then it is done");
}

#[test]
fn a_rack_route_set_in_lane_style_changes_nothing_that_plays() {
    // The modes never interfere: a route chip is not shown in lane-style,
    // and a route that arrives anyway (an old peer, a script) is not obeyed.
    let mut s = song();
    to_lane_style(&mut s.project);
    let before = s.project.channel_route(s.piano);
    SetChannelRoute::new(s.piano, Some(s.bus))
        .apply(&mut s.project)
        .unwrap();
    assert_eq!(s.project.channel_route(s.piano), before);
}

// ----------------------------------------------------------- the claims

#[test]
fn an_unclaimed_instrument_claims_the_lane_it_is_first_drawn_on() {
    let mut s = song();
    to_lane_style(&mut s.project);
    let strings = channel(&mut s.project, "Strings");
    assert_eq!(s.project.claimed_lane(strings), None);
    notes_clip(&mut s.project, s.vox, strings, 0);
    upkeep(&mut s.project);
    assert_eq!(s.project.claimed_lane(strings), Some(s.vox));
}

#[test]
fn drawing_a_claimed_instrument_on_another_lane_is_a_conflict_not_a_new_claim() {
    let mut s = song();
    to_lane_style(&mut s.project);
    assert!(lane_conflicts(&s.project).is_empty());
    let stray = notes_clip(&mut s.project, s.vox, s.piano, PPQN * 8);
    upkeep(&mut s.project);
    assert_eq!(s.project.claimed_lane(s.piano), Some(s.keys), "claim kept");
    let conflicts = lane_conflicts(&s.project);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].channel, s.piano);
    assert_eq!(conflicts[0].claimed, s.keys);
    assert_eq!(conflicts[0].lane, s.vox);
    assert_eq!(conflicts[0].clip, stray);
}

#[test]
fn a_claim_whose_lane_has_none_of_its_clips_left_moves_to_where_they_are() {
    // Undoing the only clip on the claimed lane must not leave the
    // instrument tied to an empty row, asking a question about it later.
    let mut s = song();
    to_lane_style(&mut s.project);
    let only = s
        .project
        .clips
        .iter()
        .find(|(_, c)| c.lane == s.keys)
        .map(|(id, _)| id)
        .unwrap();
    s.project.clips.remove(only);
    notes_clip(&mut s.project, s.vox, s.piano, 0);
    assert!(lane_conflicts(&s.project).is_empty());
    upkeep(&mut s.project);
    assert_eq!(s.project.claimed_lane(s.piano), Some(s.vox));
}

#[test]
fn a_duplicated_channel_takes_the_clip_and_claims_its_lane() {
    // The "duplicate the channel" answer: a copy of the instrument, the stray
    // clip pointed at it, and the copy claiming the stray clip's lane.
    let mut s = song();
    to_lane_style(&mut s.project);
    let stray = notes_clip(&mut s.project, s.vox, s.piano, PPQN * 8);
    let mut copy = fontelle_model::DuplicateChannel::new(s.piano);
    copy.apply(&mut s.project).unwrap();
    let copy = copy.channel().unwrap();
    let mut point = SetClipChannel::new(stray, copy);
    point.apply(&mut s.project).unwrap();
    upkeep(&mut s.project);
    assert!(lane_conflicts(&s.project).is_empty());
    assert_eq!(s.project.claimed_lane(copy), Some(s.vox));
    assert_eq!(s.project.claimed_lane(s.piano), Some(s.keys));
    point.invert().apply(&mut s.project).unwrap();
    match &s.project.clips[stray].source {
        ClipSource::Notes(data) => assert_eq!(data.channel, s.piano),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------- the wire

#[test]
fn the_new_edits_cross_the_wire() {
    let mut s = song();
    let mut switch = SetRoutingMode::new(RoutingMode::Lane);
    switch.apply(&mut s.project).unwrap();
    let mut upkeep = LaneUpkeep::due(&s.project).unwrap();
    upkeep.apply(&mut s.project).unwrap();
    let both = Compound::new(
        "Lane-style",
        vec![Box::new(switch) as Box<dyn Command>, Box::new(upkeep)],
    );
    let json = serde_json::to_string(&both.to_edit()).unwrap();
    let back: Edit = serde_json::from_str(&json).unwrap();
    // A fresh copy of the song, rebuilt the same way, takes the edit and
    // ends up where this one is.
    let mut other = song().project;
    other
        .lanes
        .iter()
        .zip(s.project.lanes.iter())
        .for_each(|(a, b)| assert_eq!(a.0, b.0));
    back.into_command().apply(&mut other).unwrap();
    assert_eq!(other.lane_routing, s.project.lane_routing);
    for tag in ["SetRoutingMode", "LaneUpkeep", "SetClipChannel"] {
        assert!(Edit::TAGS.contains(&tag), "{tag} is not a wire edit");
    }
}

// ------------------------------------------------ folding into an entry

#[test]
fn an_amendment_joins_the_entry_it_follows_so_one_undo_takes_back_both() {
    // The upkeep follows the edit that made it due, and a Ctrl+Z that left a
    // lane's new track behind would be a second, invisible undo step.
    let mut s = song();
    let mut history = fontelle_model::History::new();
    let tracks = s.project.mixer.tracks.len();
    history
        .apply(
            Box::new(SetRoutingMode::new(RoutingMode::Lane)),
            &mut s.project,
        )
        .unwrap();
    let upkeep = LaneUpkeep::due(&s.project).unwrap();
    history.amend(Box::new(upkeep), &mut s.project).unwrap();
    assert_eq!(history.depth(), 1, "one entry");
    assert!(s.project.lane_track(s.keys).is_some());
    history.undo(&mut s.project).unwrap().unwrap();
    assert_eq!(s.project.lane_routing.mode, RoutingMode::Rack);
    assert_eq!(s.project.mixer.tracks.len(), tracks);
    history.redo(&mut s.project).unwrap().unwrap();
    assert!(s.project.lane_track(s.keys).is_some());
}

#[test]
fn an_amendment_after_the_entry_was_let_go_is_an_entry_of_its_own() {
    let mut s = song();
    let mut history = fontelle_model::History::new();
    history
        .apply(
            Box::new(SetRoutingMode::new(RoutingMode::Lane)),
            &mut s.project,
        )
        .unwrap();
    history.break_gesture();
    let upkeep = LaneUpkeep::due(&s.project).unwrap();
    history.amend(Box::new(upkeep), &mut s.project).unwrap();
    assert_eq!(history.depth(), 2);
}
