//! What the arrangement's brush puts down, and that a stroke is one undo.
//!
//! `fontelle-ui/tests/arrange_paint.rs` is the gesture; this is what the host
//! does with the `ArrangeEdit::Paint` steps it sends. A copy is the
//! pencil's Shift+press stamp, and a blank is its double-click — but a
//! whole stroke, however many clips it laid, is **one** entry on the history
//! and one edit to anybody sharing the song: *"one undo per gesture"*
//! (INVARIANT 9's other half, §10.6).

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::Note;
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::{ArrangeEdit, RollEdit};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

const BAR: i64 = PPQN * 4;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-paint-{name}-{}", std::process::id()));
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
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

/// The host's half of a stroke: each step as the window sends it, and the
/// mouse coming up.
fn paint(
    session: &mut Session,
    source: Option<fontelle_types::ClipId>,
    lane: usize,
    bars: &[i64],
) -> Vec<fontelle_types::ClipId> {
    let mut made = Vec::new();
    for bar in bars {
        made.extend(
            session
                .arrange(ArrangeEdit::Paint {
                    source,
                    lane,
                    start: BAR * bar,
                })
                .clips,
        );
    }
    session.end_gesture();
    made
}

#[test]
fn a_stroke_lays_exact_copies_and_hands_back_every_id() {
    let dir = scratch("copies");
    let mut session = studio(&dir);
    session.edit(RollEdit::Add {
        note: a_note(0, 42),
    });
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 42),
    });
    session.end_gesture();
    let source = session.clips()[0].clone();

    let made = paint(&mut session, Some(source.id), 2, &[4, 5, 6, 7]);
    assert_eq!(made.len(), 4, "an id for every copy");
    let clips = session.clips();
    for (id, bar) in made.iter().zip(4..) {
        let copy = clips.iter().find(|c| c.id == *id).expect("the copy");
        assert_eq!((copy.lane, copy.start), (2, BAR * bar));
        assert_eq!(copy.length, source.length);
        assert_eq!(copy.notes, source.notes, "the same hats");
        assert_eq!(copy.name, source.name);
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_whole_stroke_is_one_undo() {
    let dir = scratch("undo");
    let mut session = studio(&dir);
    let source = session.clips()[0].id;
    let before = session.clips().len();
    let depth = session.undo_depth();

    paint(&mut session, Some(source), 1, &[2, 3, 4, 5, 6]);
    assert_eq!(session.clips().len(), before + 5);
    assert_eq!(session.undo_depth(), depth + 1, "one entry for the stroke");

    session.undo();
    assert_eq!(session.clips().len(), before, "one Ctrl+Z takes all five");
    session.redo();
    assert_eq!(
        session.clips().len(),
        before + 5,
        "and one Ctrl+Y puts them back"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_strokes_are_two_undos() {
    let dir = scratch("two");
    let mut session = studio(&dir);
    let source = session.clips()[0].id;
    let before = session.clips().len();
    paint(&mut session, Some(source), 1, &[2, 3]);
    paint(&mut session, Some(source), 2, &[2, 3, 4]);
    assert_eq!(session.clips().len(), before + 5);
    session.undo();
    assert_eq!(session.clips().len(), before + 2, "the second stroke only");
    session.undo();
    assert_eq!(session.clips().len(), before);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn with_nothing_in_hand_a_stroke_lays_blank_bars_of_the_selected_instrument() {
    let dir = scratch("blank");
    let mut session = studio(&dir);
    session.add_channel().expect("a second channel");
    assert_eq!(session.selected_channel(), 1);
    let second = session.channels()[1].name.clone();
    let before = session.clips().len();
    let depth = session.undo_depth();

    let made = paint(&mut session, None, 1, &[0, 1, 2]);
    assert_eq!(made.len(), 3);
    let clips = session.clips();
    for (id, bar) in made.iter().zip(0..) {
        let blank = clips.iter().find(|c| c.id == *id).expect("the blank");
        assert_eq!((blank.lane, blank.start, blank.length), (1, BAR * bar, BAR));
        assert_eq!(
            blank.name, second,
            "the rack's selection, as a drawn clip is"
        );
        assert!(blank.notes.is_empty());
    }
    assert_eq!(session.undo_depth(), depth + 1);
    session.undo();
    assert_eq!(session.clips().len(), before);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stroke_from_a_clip_that_has_gone_lays_nothing_and_says_so() {
    let dir = scratch("gone");
    let mut session = studio(&dir);
    let source = session.clips()[0].id;
    session.arrange(ArrangeEdit::Remove(vec![source]));
    let made = paint(&mut session, Some(source), 0, &[2]);
    assert!(made.is_empty());
    assert!(session.take_message().is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn with_a_prefab_picked_a_stroke_lays_places_for_it_in_one_undo() {
    // The prefab tab's draw makes a place, not a blank; so does its brush.
    let dir = scratch("prefab");
    let mut session = studio(&dir);
    session.add_prefab();
    session.select_prefab(Some(0));
    let depth = session.undo_depth();
    let mut made = Vec::new();
    for bar in 2..6 {
        made.extend(session.paint_prefab(0, 1, BAR * bar));
    }
    session.end_gesture();
    assert_eq!(made.len(), 4);
    assert_eq!(session.prefabs()[0].uses, 4, "four places");
    assert_eq!(session.undo_depth(), depth + 1, "one stroke, one undo");
    session.undo();
    assert_eq!(session.prefabs()[0].uses, 0);
    std::fs::remove_dir_all(&dir).ok();
}
