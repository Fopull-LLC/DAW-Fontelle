//! A block on the arrangement is the colour of the mixer track it plays
//! into — always, and nothing else first.
//!
//! > *"fix an issue of clips not always being colored their actual track
//! > color we need clip colors to always be the track color not some
//! > arbitrary color and an outline"* (Ty, 2026-10-27)
//!
//! A clip's own colour (an import's, an old song's) and a lane's colour both
//! outranked the track, and an audio block never asked for its track at
//! all, so it was the default blue — and the mixer then ringed it in the
//! track's colour. Those colours are what is left when a block plays straight
//! into the master, which has no colour worth wearing.

mod common;

use fontelle_types::PPQN;
use fontelle_ui::StudioHost;

const OWN: [u8; 4] = [0x12, 0x34, 0x56, 0xff];

/// A song whose first instrument plays into a track of its own.
fn routed(project: fontelle_model::Project) -> (fontelle_app::Session, [u8; 4]) {
    let mut session = common::a_session_for(project);
    session.add_mixer_track();
    session.set_channel_route(0, Some(0));
    let track = session.mixer_strips()[0].color;
    (session, track)
}

#[test]
fn a_clip_with_a_colour_of_its_own_still_wears_its_tracks() {
    let mut project = common::a_clip_project(4);
    for clip in project.clips.values_mut() {
        clip.color = Some(OWN);
    }
    let (session, track) = routed(project);
    assert_eq!(session.clips()[0].color, track);
}

#[test]
fn a_coloured_lane_does_not_outrank_the_track() {
    let mut project = common::a_clip_project(4);
    for lane in project.lanes.values_mut() {
        lane.color = OWN;
    }
    let (session, track) = routed(project);
    assert_eq!(session.clips()[0].color, track);
}

#[test]
fn recolouring_the_track_recolours_its_blocks() {
    let (mut session, _) = routed(common::a_clip_project(4));
    let red = [0xe0, 0x30, 0x30, 0xff];
    session.set_track_color(0, red);
    assert_eq!(session.clips()[0].color, red);
}

#[test]
fn an_audio_clip_wears_the_colour_of_the_track_it_plays_into() {
    let mut project = common::a_clip_project(4);
    let master = project.mixer.master.expect("a master");
    let mut bus = fontelle_model::MixerTrack::new("Vox");
    bus.color = [0x30, 0xc0, 0x60, 0xff];
    bus.output = Some(master);
    let bus = project.mixer.tracks.insert(bus);
    let row = project.lane_ids()[1];
    let asset = fontelle_types::AssetRef {
        id: Default::default(),
        path: "vox.wav".into(),
        content_hash: 0,
        size: 0,
        kind: fontelle_types::AssetKind::Sample,
    };
    let mut data = fontelle_types::AudioClipData::whole(asset, 48_000, 48_000);
    data.mixer_track = Some(bus);
    let take = project.clips.insert(fontelle_model::Clip {
        name: None,
        lane: row,
        start: 0,
        length: PPQN * 4,
        source: fontelle_model::ClipSource::Audio(data),
        prefab_link: None,
        color: Some(OWN),
        muted: false,
        loop_length: None,
    });
    let session = common::a_session_for(project);
    let block = session
        .clips()
        .into_iter()
        .find(|c| c.id == take)
        .expect("the take");
    assert_eq!(block.color, [0x30, 0xc0, 0x60, 0xff]);
}

#[test]
fn a_block_on_the_master_keeps_the_colour_it_was_given() {
    // Nothing better to say: the master is grey, and an import's colours
    // tell its parts apart.
    let mut project = common::a_clip_project(4);
    for clip in project.clips.values_mut() {
        clip.color = Some(OWN);
    }
    let session = common::a_session_for(project);
    assert_eq!(session.clips()[0].color, OWN);
}
