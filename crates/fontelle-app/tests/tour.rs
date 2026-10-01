//! The tour's song, and what the tour sets up as it goes.
//!
//! `docs/ux-routing-and-learning-plan.md` §5 and answer D: the tour opens *a
//! demo project* that is *extremely simple — only as much song as it takes
//! to teach the necessary things well: a few lanes, an instrument or two,
//! one audio clip, a mixer track with an effect and a send*. And it *sets up
//! what a user needs as it goes*: the routing mode, and VST 2 support.

mod common;

use fontelle_model::{ClipSource, RoutingMode};
use fontelle_ui::StudioHost;
use fontelle_ui::canvas::GuideChoice;

fn session() -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(4))
}

fn lane_named(project: &fontelle_model::Project, name: &str) -> fontelle_types::LaneId {
    project
        .lanes
        .iter()
        .find(|(_, lane)| lane.name == name)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no {name} lane"))
}

#[test]
fn the_tour_opens_a_small_song_with_one_of_everything() {
    let mut session = session();
    session.start_tour().expect("the tour's song opens");
    let project = session.project();
    assert_eq!(project.meta.name, "Fontelle tour");
    assert!(project.channels.len() >= 3, "piano, bass and drums");
    for (lane, notes) in [
        ("Drums", true),
        ("Bass", true),
        ("Keys", true),
        ("Vocal", false),
    ] {
        let id = lane_named(project, lane);
        let clip = project
            .clips
            .values()
            .find(|clip| clip.lane == id)
            .unwrap_or_else(|| panic!("nothing on {lane}"));
        match &clip.source {
            ClipSource::Notes(data) => {
                assert!(notes, "{lane} should be audio");
                assert!(!data.notes.is_empty(), "{lane}'s clip has notes");
            }
            ClipSource::Audio(data) => {
                assert!(!notes, "{lane} should be notes");
                // The file is kept: a clip naming a file that was tidied
                // away is a silent, empty block.
                assert!(
                    data.asset.path.exists(),
                    "the pad is on disk: {:?}",
                    data.asset.path
                );
            }
            _ => panic!("{lane} holds automation"),
        }
    }
    let busy = project
        .mixer
        .tracks
        .values()
        .find(|track| !track.inserts.is_empty() && !track.sends.is_empty());
    assert!(busy.is_some(), "a mixer track with an effect and a send");
    assert!(
        session.session_question().is_none(),
        "the tour asks its own questions, in its own time"
    );
    assert!(
        !session
            .recent_projects()
            .iter()
            .any(|recent| recent.name.contains("tour")),
        "the tour's song is not one of yours"
    );
}

#[test]
fn starting_the_tour_again_gives_a_fresh_song() {
    let mut session = session();
    session.start_tour().unwrap();
    let lanes = session.project().lanes.len();
    session.add_lane();
    session.start_tour().unwrap();
    assert_eq!(session.project().lanes.len(), lanes);
}

#[test]
fn the_routing_step_sets_new_songs_and_this_one() {
    let mut session = session();
    session.start_tour().unwrap();
    let (options, chosen) = session.tour_options(GuideChoice::Routing);
    assert_eq!(options, vec!["Rack-style", "Lane-style"]);
    assert_eq!(chosen, None, "nothing chosen yet");
    session.choose_tour_option(GuideChoice::Routing, 1);
    assert!(session.lane_style());
    assert_eq!(session.tour_options(GuideChoice::Routing).1, Some(1));
    let new_songs = session
        .settings()
        .iter()
        .position(|row| row.name == "New songs start as")
        .unwrap();
    assert_eq!(session.settings()[new_songs].detail, "Lane-style");
    session.choose_tour_option(GuideChoice::Routing, 0);
    assert_eq!(session.project().lane_routing.mode, RoutingMode::Rack);
}

#[test]
fn the_vst2_step_offers_to_turn_it_on_or_leave_it() {
    let session = session();
    let (options, chosen) = session.tour_options(GuideChoice::Vst2);
    assert_eq!(options.len(), 2);
    assert!(options[1].to_lowercase().contains("on"), "{options:?}");
    let installed = fontelle_app::extensions::CATALOGUE
        .first()
        .is_some_and(fontelle_app::extensions::is_installed);
    assert_eq!(chosen, installed.then_some(1));
}

#[test]
fn the_first_launch_offer_is_made_once() {
    let mut session = session();
    assert!(!session.tour_offered());
    session.set_tour_offered();
    assert!(session.tour_offered());
}
