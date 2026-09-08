//! Bouncing one row of the arrangement to audio.
//!
//! Reported from using the window:
//!
//! > *"please also add the ability to render a track into an audio clip by
//! > right clicking and in the options there should be a new render option i
//! > can click and then it will either do my time selection (if i have one)
//! > but it will prompt first if i want to render just time selection or the
//! > whole track, or if there was no time selection just render the whole
//! > track. this should add a new lane below it called whatever the original
//! > track name is plus "(rendered)" at the end."*
//!
//! The compiler half is `fontelle-sequencer/tests/lane_scope.rs`: a bounce of
//! a track has to *be* that track, and a scope that let the rest of the song
//! through would put the whole mix in every render.

mod common;

use std::path::PathBuf;

use fontelle_app::{Session};
use fontelle_types::PPQN;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-render-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A session with a bundle — a render goes inside the project folder, so
/// there has to be one (INVARIANT 10).
fn a_saved_session(dir: &std::path::Path) -> Session {
    common::a_session_in(common::a_project_with_a_clip(4, 120.0, SR), Some(dir.join("Song")))
}

#[test]
fn rendering_a_row_puts_a_take_on_a_new_row_named_after_it() {
    let dir = scratch("names");
    let mut session = a_saved_session(&dir);
    let rows = session.lanes().len();

    session.render_lane(0, None).expect("renders");

    let lanes = session.lanes();
    assert_eq!(lanes.len(), rows + 1, "no row was made for the render");
    assert_eq!(
        lanes[1].name,
        "Lane 1 (rendered)",
        "rows are {:?}",
        lanes.iter().map(|l| &l.name).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_new_row_sits_directly_under_the_one_it_came_from() {
    // *"add a new lane below it"* — under the row rendered, not at the bottom
    // of a ten-row arrangement where you would have to go looking for it.
    let dir = scratch("below");
    let mut session = a_saved_session(&dir);
    // Row one is the one with the clip on it — an empty row has nothing to
    // render, which is its own test below.
    session.render_lane(0, None).expect("renders");
    let lanes = session.lanes();
    assert_eq!(lanes[0].name, "Lane 1");
    assert_eq!(lanes[1].name, "Lane 1 (rendered)");
    assert_eq!(
        lanes[2].name,
        "Lane 2",
        "the rest of the stack shifted oddly: {:?}",
        lanes.iter().map(|l| &l.name).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_render_is_an_audio_clip_on_that_row() {
    let dir = scratch("clip");
    let mut session = a_saved_session(&dir);
    session.render_lane(0, None).expect("renders");
    let made: Vec<_> = session
        .clips()
        .into_iter()
        .filter(|c| c.kind == fontelle_ui::document::ClipKind::Audio)
        .collect();
    assert_eq!(made.len(), 1, "expected one take, got {}", made.len());
    assert_eq!(made[0].lane, 1, "the take is not on the row that was made");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_time_selection_renders_only_that_stretch() {
    // Two bars of a four-bar row: the take is about two bars long, not four.
    let dir = scratch("span");
    let mut session = a_saved_session(&dir);
    session
        .render_lane(0, Some((0, PPQN * 8)))
        .expect("renders");
    let clip = session
        .clips()
        .into_iter()
        .find(|c| c.kind == fontelle_ui::document::ClipKind::Audio)
        .expect("a take");
    // A release tail rides on the end, so this is "about", not "exactly".
    assert!(
        clip.length < PPQN * 12,
        "two bars asked for and {} ticks came back",
        clip.length
    );
    assert!(clip.length >= PPQN * 8, "the selection was cut short");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_selection_starting_late_puts_the_take_where_the_selection_was() {
    let dir = scratch("offset");
    let mut session = a_saved_session(&dir);
    session
        .render_lane(0, Some((PPQN * 4, PPQN * 8)))
        .expect("renders");
    let clip = session
        .clips()
        .into_iter()
        .find(|c| c.kind == fontelle_ui::document::ClipKind::Audio)
        .expect("a take");
    assert_eq!(clip.start, PPQN * 4, "the take landed at the wrong bar");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_row_with_nothing_on_it_says_so_rather_than_writing_a_silent_file() {
    let dir = scratch("empty");
    let mut session = a_saved_session(&dir);
    let error = session.render_lane(3, None).expect_err("must refuse");
    assert!(error.contains("nothing"), "got {error:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rendering_a_row_that_is_not_there_says_so_rather_than_panicking() {
    let dir = scratch("missing");
    let mut session = a_saved_session(&dir);
    assert!(session.render_lane(99, None).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_unsaved_project_is_told_to_save_rather_than_writing_somewhere_it_chose() {
    // INVARIANT 10: a render goes inside the bundle, and guessing at somewhere
    // else is a write outside anywhere the user named.
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let error = session.render_lane(0, None).expect_err("must refuse");
    assert!(error.contains("save"), "got {error:?}");
}

#[test]
fn rendering_is_one_undo() {
    let dir = scratch("undo");
    let mut session = a_saved_session(&dir);
    let rows = session.lanes().len();
    session.render_lane(0, None).expect("renders");
    assert_eq!(session.lanes().len(), rows + 1);
    session.undo();
    assert!(
        session
            .clips()
            .into_iter()
            .all(|c| c.kind != fontelle_ui::document::ClipKind::Audio),
        "the take survived the undo"
    );
    std::fs::remove_dir_all(&dir).ok();
}
