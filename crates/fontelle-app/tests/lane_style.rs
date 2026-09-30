//! Lane-style routing, as the studio does it.
//!
//! `docs/ux-routing-and-learning-plan.md` §1. The model half (the mode, the
//! lanes' tracks, the claims) is `fontelle-model/tests/lane_style.rs`; this
//! is what a person meets: switching a song over, drawing an instrument onto
//! a lane, and the one question lane-style asks — Ty, 2026-09-30: *"the first
//! time you draw an instruments clip onto a lane that instrument claims the
//! lane, and drawing it onto a second lane offers to move it or duplicate the
//! channel"*.

mod common;

use fontelle_model::RoutingMode;
use fontelle_types::PPQN;
use fontelle_ui::canvas::ArrangeEdit;
use fontelle_ui::{DocumentHost, StudioHost};

const BAR: i64 = PPQN * 4;

/// A studio on a song with one instrument and one clip of it on the first
/// row, switched to lane-style.
fn lane_studio() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.set_routing_mode(RoutingMode::Lane);
    session
}

fn lane(session: &fontelle_app::Session, row: usize) -> fontelle_types::LaneId {
    session.project().lane_ids()[row]
}

fn first_channel(session: &fontelle_app::Session) -> fontelle_types::ChannelId {
    session.project().channels.keys().next().unwrap()
}

/// Draws a clip of the selected channel on `row`, and lets go.
fn draw(session: &mut fontelle_app::Session, row: usize, start: i64) -> fontelle_types::ClipId {
    let made = session.arrange(ArrangeEdit::Add { lane: row, start });
    session.end_gesture();
    made.clips[0]
}

#[test]
fn switching_a_song_to_lane_style_gives_every_lane_a_track_in_one_undo() {
    let mut session = common::a_session_for(common::a_clip_project(4));
    let tracks = session.project().mixer.tracks.len();
    session.set_routing_mode(RoutingMode::Lane);
    let project = session.project();
    assert_eq!(project.lane_routing.mode, RoutingMode::Lane);
    for id in project.lane_ids() {
        assert!(project.lane_track(id).is_some(), "a lane with no track");
    }
    let channel = first_channel(&session);
    assert_eq!(
        session.project().claimed_lane(channel),
        Some(lane(&session, 0)),
        "the instrument claims the lane its clip is on"
    );
    assert!(session.lane_style());
    session.undo();
    assert_eq!(session.project().lane_routing.mode, RoutingMode::Rack);
    assert_eq!(session.project().mixer.tracks.len(), tracks);
    assert!(!session.lane_style());
}

#[test]
fn drawing_an_unclaimed_instrument_claims_the_lane_it_lands_on() {
    let mut session = lane_studio();
    session.add_channel().expect("a channel can be added");
    let strings = *session
        .project()
        .channels
        .keys()
        .collect::<Vec<_>>()
        .last()
        .unwrap();
    draw(&mut session, 2, 0);
    assert_eq!(
        session.project().claimed_lane(strings),
        Some(lane(&session, 2))
    );
    assert!(session.session_question().is_none(), "nothing to ask");
    assert_eq!(
        session.project().channel_route(strings),
        session.project().lane_track(lane(&session, 2))
    );
}

#[test]
fn drawing_a_claimed_instrument_on_another_lane_asks_what_to_do() {
    let mut session = lane_studio();
    session.select_channel(0);
    draw(&mut session, 1, BAR * 2);
    let question = session.session_question().expect("a question");
    assert_eq!(question.buttons.len(), 3);
    let said = question.lines.join(" ");
    assert!(
        said.contains("Lane 1") || said.contains(&session.project().lanes[lane(&session, 0)].name),
        "{said}"
    );
    // The safe answer first: take the clip back.
    assert!(
        question.buttons[0].to_lowercase().contains("back"),
        "{:?}",
        question.buttons
    );
    assert!(question.buttons[1].starts_with("Move"));
    assert!(question.buttons[2].starts_with("Duplicate"));
}

#[test]
fn taking_it_back_removes_the_stray_clip() {
    let mut session = lane_studio();
    let clips = session.project().clips.len();
    session.select_channel(0);
    let stray = draw(&mut session, 1, BAR * 2);
    session.answer_session_question(0).unwrap();
    assert!(session.project().clips.get(stray).is_none());
    assert_eq!(session.project().clips.len(), clips);
    assert!(session.session_question().is_none());
}

#[test]
fn moving_the_instrument_takes_its_clips_to_the_new_lane_in_one_undo() {
    let mut session = lane_studio();
    let channel = first_channel(&session);
    session.select_channel(0);
    draw(&mut session, 1, BAR * 2);
    session.answer_session_question(1).unwrap();
    let to = lane(&session, 1);
    assert_eq!(session.project().claimed_lane(channel), Some(to));
    assert!(
        session.project().clips.values().all(|clip| clip.lane == to),
        "every clip of it follows it, so nothing plays through a lane it is not on"
    );
    assert!(fontelle_model::lane_conflicts(session.project()).is_empty());
    assert!(session.session_question().is_none());
    session.undo();
    assert_eq!(
        session.project().claimed_lane(channel),
        Some(lane(&session, 0))
    );
}

#[test]
fn duplicating_the_channel_gives_the_stray_clip_an_instrument_of_its_own() {
    let mut session = lane_studio();
    let channel = first_channel(&session);
    let channels = session.project().channels.len();
    session.select_channel(0);
    let stray = draw(&mut session, 1, BAR * 2);
    session.answer_session_question(2).unwrap();
    let project = session.project();
    assert_eq!(project.channels.len(), channels + 1);
    let copy = match &project.clips[stray].source {
        fontelle_model::ClipSource::Notes(data) => data.channel,
        _ => unreachable!(),
    };
    assert_ne!(copy, channel);
    assert_eq!(project.claimed_lane(copy), Some(lane(&session, 1)));
    assert_eq!(project.claimed_lane(channel), Some(lane(&session, 0)));
    assert!(fontelle_model::lane_conflicts(project).is_empty());
}

#[test]
fn rack_style_asks_nothing_and_claims_nothing() {
    // The modes never interfere: a rack-style song is exactly what it was.
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.select_channel(0);
    draw(&mut session, 1, BAR * 2);
    assert!(session.session_question().is_none());
    assert!(session.project().lane_routing.claims.is_empty());
    assert!(session.project().lane_routing.tracks.is_empty());
    assert!(!session.lane_style());
}

#[test]
fn a_new_lane_in_lane_style_comes_with_its_track() {
    let mut session = lane_studio();
    let before = session.project().lane_ids().len();
    session.add_lane();
    session.end_gesture();
    let ids = session.project().lane_ids();
    assert_eq!(ids.len(), before + 1);
    assert!(session.project().lane_track(*ids.last().unwrap()).is_some());
}

#[test]
fn a_lanes_own_track_cannot_be_deleted_from_under_it() {
    let mut session = lane_studio();
    let track = session.project().lane_track(lane(&session, 0)).unwrap();
    // Strips are listed as the mixer shows them: every track, then the master.
    let master = session.project().mixer.master;
    let strip = session
        .project()
        .mixer
        .tracks
        .keys()
        .filter(|id| Some(*id) != master)
        .position(|id| id == track)
        .unwrap();
    let tracks = session.project().mixer.tracks.len();
    session.remove_mixer_track(strip);
    assert_eq!(session.project().mixer.tracks.len(), tracks, "refused");
    let said = session.take_message().unwrap_or_default();
    assert!(
        said.to_lowercase().contains("lane"),
        "and says why: {said:?}"
    );
}

// ------------------------------------------ choosing the mode (the page)

fn row(session: &fontelle_app::Session, name: &str) -> usize {
    session
        .settings()
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("no {name:?} row"))
}

#[test]
fn the_settings_page_has_a_project_section_with_the_songs_routing() {
    use fontelle_ui::canvas::SettingControl;
    let session = common::a_session_for(common::a_clip_project(4));
    let rows = session.settings();
    let heading = rows
        .iter()
        .position(|r| r.name == "Project")
        .expect("a Project heading");
    let controls = session.setting_controls();
    assert_eq!(controls[heading], SettingControl::Heading);
    let routing = row(&session, "Routing in this song");
    assert!(routing > heading);
    assert_eq!(
        controls[routing],
        SettingControl::Choice {
            options: vec!["Rack-style".into(), "Lane-style".into()],
            chosen: 0
        }
    );
    assert_eq!(rows[routing].detail, "Rack-style");
    let new_songs = row(&session, "New songs start as");
    assert_eq!(
        controls[new_songs],
        SettingControl::Choice {
            options: vec!["Ask me".into(), "Rack-style".into(), "Lane-style".into()],
            chosen: 0
        }
    );
}

#[test]
fn choosing_lane_style_on_the_page_switches_the_song_and_says_so() {
    let mut session = common::a_session_for(common::a_clip_project(4));
    let routing = row(&session, "Routing in this song");
    session.choose_setting(routing, 1);
    assert!(session.lane_style());
    let (said, _) = session
        .take_settings_toast()
        .expect("it says what happened");
    assert!(said.to_lowercase().contains("lane"), "{said}");
    assert_eq!(session.settings()[routing].detail, "Lane-style");
    // And back, from the same row.
    session.choose_setting(routing, 0);
    assert!(!session.lane_style());
}

// ------------------------------------------- the first new song asks

fn projects_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-lanes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_first_new_song_asks_which_way_and_remembers_the_answer() {
    let dir = projects_dir("ask");
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.set_projects_dir(Some(dir.clone()));
    session.new_project_named("First").unwrap();
    let question = session
        .session_question()
        .expect("asked on the first new song");
    assert_eq!(question.buttons, vec!["Rack-style", "Lane-style"]);
    session.answer_session_question(1).unwrap();
    assert!(session.lane_style(), "this song is lane-style");
    assert!(session.session_question().is_none());
    session.undo();
    assert!(
        session.lane_style(),
        "the choice is how the song began, not an undo step"
    );
    // Remembered: the next new song is lane-style without asking.
    session.new_project_named("Second").unwrap();
    assert!(session.session_question().is_none());
    assert!(session.lane_style());
    let new_songs = row(&session, "New songs start as");
    assert_eq!(session.settings()[new_songs].detail, "Lane-style");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn opening_a_song_never_asks_and_never_changes_its_mode() {
    let dir = projects_dir("open");
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.set_projects_dir(Some(dir.clone()));
    session.new_project_named("Kept").unwrap();
    session.answer_session_question(0).unwrap();
    assert!(!session.lane_style());
    let new_songs = row(&session, "New songs start as");
    session.choose_setting(new_songs, 2);
    session.open_project(0).unwrap();
    assert!(!session.lane_style(), "an existing song keeps its own mode");
    assert!(session.session_question().is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ----------------------------------------------- audio clips, lane-style

#[test]
fn an_audio_clips_route_reads_as_its_lanes_track_and_is_not_changed_from_the_editor() {
    let mut project = common::a_clip_project(4);
    let row = project.lane_ids()[1];
    let asset = fontelle_types::AssetRef {
        id: Default::default(),
        path: "vox.wav".into(),
        content_hash: 0,
        size: 0,
        kind: fontelle_types::AssetKind::Sample,
    };
    let take = project.clips.insert(fontelle_model::Clip {
        lane: row,
        start: 0,
        length: BAR,
        source: fontelle_model::ClipSource::Audio(fontelle_types::AudioClipData::whole(
            asset, 48_000, 48_000,
        )),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    let mut session = common::a_session_for(project);
    session.set_routing_mode(RoutingMode::Lane);
    let lane_track = session.project().lane_track(row);
    let mut data = session.audio_clip(take).expect("an audio clip");
    assert_eq!(
        data.mixer_track, lane_track,
        "the editor reads the lane's track"
    );
    data.mixer_track = None;
    data.gain_db = -6.0;
    session.set_audio_clip(take, data);
    assert_eq!(
        session.project().clip_route(take),
        lane_track,
        "still its lane's"
    );
    assert_eq!(
        session.audio_clip(take).unwrap().gain_db,
        -6.0,
        "and everything else took"
    );
    let said = session.take_message().unwrap_or_default();
    assert!(
        said.to_lowercase().contains("lane"),
        "and it says why: {said:?}"
    );
}

// ------------------------------------------------------ lanes and tracks

#[test]
fn a_lane_style_lane_wears_its_tracks_colour_and_a_rack_style_one_does_not() {
    let mut session = common::a_session_for(common::a_clip_project(4));
    assert!(session.lanes().iter().all(|l| l.track_color.is_none()));
    session.set_routing_mode(RoutingMode::Lane);
    let first = lane(&session, 0);
    let track = session.project().lane_track(first).unwrap();
    assert_eq!(
        session.lanes()[0].track_color,
        Some(session.project().mixer.tracks[track].color)
    );
}

#[test]
fn renaming_a_lane_renames_its_track_in_one_undo() {
    let mut session = lane_studio();
    let first = lane(&session, 0);
    let track = session.project().lane_track(first).unwrap();
    let old = session.project().mixer.tracks[track].name.clone();
    session.rename_lane(0, "Drums");
    session.end_gesture();
    assert_eq!(session.project().mixer.tracks[track].name, "Drums");
    session.undo();
    assert_eq!(session.project().mixer.tracks[track].name, old);
    assert_eq!(session.project().lanes[first].name, old);
}
