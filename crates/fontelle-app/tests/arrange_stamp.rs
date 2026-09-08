//! What a stamped clip is: an exact copy, where you pressed.
//!
//! `fontelle-ui/tests/arrange_stamp.rs` is the gesture; this is what the host
//! does with what it asks for. *"a single click should instead place a exact
//! copy of whatever your last selection is."*

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{ClipSource, Note};
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::{ArrangeEdit, RollEdit};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-stamp-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn studio(dir: &std::path::Path) -> Session {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised =
        fontelle_app::realise(&project, &library, options).expect("an empty project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

fn a_note(start: i64, key: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

#[test]
fn a_stamp_is_an_exact_copy_of_the_clip_where_you_pressed() {
    let dir = scratch("copy");
    let mut session = studio(&dir);
    // Something to tell the copy by: two notes and a loop.
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 67),
    });
    let source = session.clips()[0].clone();
    session.arrange(ArrangeEdit::SetLoop {
        ids: vec![source.id],
        loop_length: Some(PPQN * 4),
    });
    let before = session.clips().len();

    let made = session.arrange(ArrangeEdit::Stamp {
        source: source.id,
        lane: 0,
        start: PPQN * 16,
    });
    assert_eq!(made.clips.len(), 1, "one clip, and its id came back");
    let clips = session.clips();
    assert_eq!(clips.len(), before + 1);
    let copy = clips
        .iter()
        .find(|c| c.id == made.clips[0])
        .expect("the copy is in the list");
    assert_eq!(copy.start, PPQN * 16);
    assert_eq!(copy.lane, 0);
    assert_eq!(copy.length, source.length);
    assert_eq!(copy.loop_length, Some(PPQN * 4));
    assert_eq!(copy.name, source.name, "it plays the same instrument");
    assert_eq!(copy.notes, source.notes, "it holds the same notes");
    // Its own notes, not the source's: editing one leaves the other alone.
    let copy_id = copy.id;
    session.open_clip(copy_id);
    let first = session.notes().keys().next().expect("the copy has notes");
    session.edit(RollEdit::Remove(vec![first]));
    let count = |session: &Session, id| match &session.project().clips[id].source {
        ClipSource::Notes(data) => data.notes.len(),
        _ => panic!("a note clip"),
    };
    assert_eq!(count(&session, copy_id), 1);
    assert_eq!(
        count(&session, source.id),
        2,
        "the source lost a note the copy lost"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stamp_opens_the_copy_in_the_roll_like_a_drawn_clip() {
    let dir = scratch("open");
    let mut session = studio(&dir);
    let source = session.clips()[0].id;
    let made = session.arrange(ArrangeEdit::Stamp {
        source,
        lane: 0,
        start: PPQN * 8,
    });
    let clips = session.clips();
    let copy = clips
        .iter()
        .find(|c| c.id == made.clips[0])
        .expect("the copy");
    assert!(copy.open, "you put it down to work in it");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stamp_onto_a_row_that_does_not_exist_lands_on_the_last_row() {
    // The same rule a drawn clip follows: a press under the last lane means
    // the last lane.
    let dir = scratch("row");
    let mut session = studio(&dir);
    let source = session.clips()[0].id;
    let rows = session.lanes().len();
    let made = session.arrange(ArrangeEdit::Stamp {
        source,
        lane: rows + 5,
        start: PPQN * 8,
    });
    let clips = session.clips();
    let copy = clips
        .iter()
        .find(|c| c.id == made.clips[0])
        .expect("the copy");
    assert_eq!(copy.lane, rows - 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stamp_of_a_clip_that_has_gone_makes_nothing_and_says_so() {
    let dir = scratch("gone");
    let mut session = studio(&dir);
    let source = session.clips()[0].id;
    session.arrange(ArrangeEdit::Remove(vec![source]));
    let made = session.arrange(ArrangeEdit::Stamp {
        source,
        lane: 0,
        start: PPQN * 8,
    });
    assert!(made.clips.is_empty());
    assert!(session.take_message().is_some(), "a refused stamp says why");
    std::fs::remove_dir_all(&dir).ok();
}
