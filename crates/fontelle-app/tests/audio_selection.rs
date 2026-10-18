//! The audio clip editor over a selection.
//!
//! > *"selecting multiple clips and then doing edits like changing the track,
//! > changing the volume, pan, etc. all need to apply to every selected clip
//! > in the selection not just the one you double clicked to open its audio
//! > menu"* (2026-10-18)
//!
//! The window works out each clip's new settings
//! (`fontelle_ui::canvas::copy_audio_field`); this is the host's half — that
//! they land together, as one undo, by the same rules one clip's do.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-audio-selection-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_session(dir: &Path) -> Session {
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

/// A session with three takes on the arrangement, and their ids.
fn with_takes(dir: &Path) -> (Session, Vec<fontelle_types::ClipId>) {
    let mut session = a_session(dir);
    for (i, hz) in [220.0_f32, 330.0, 440.0].into_iter().enumerate() {
        let samples: Vec<f32> = (0..24_000)
            .map(|n| (n as f32 * hz * std::f32::consts::TAU / 48_000.0).sin() * 0.8)
            .collect();
        let path = dir.join(format!("take{i}.wav"));
        std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("writable");
        session.drop_file(&path).expect("the take imports");
    }
    let ids: Vec<_> = session
        .clips()
        .into_iter()
        .filter(|c| c.kind == ClipKind::Audio)
        .map(|c| c.id)
        .collect();
    assert_eq!(ids.len(), 3);
    (session, ids)
}

fn gains(session: &Session, ids: &[fontelle_types::ClipId]) -> Vec<f32> {
    ids.iter()
        .map(|id| session.audio_clip(*id).expect("audio").gain_db)
        .collect()
}

#[test]
fn a_drag_over_a_selection_sets_every_clip_and_undoes_in_one() {
    let dir = scratch("drag");
    let (mut session, ids) = with_takes(&dir);
    let before = gains(&session, &ids);
    for gain in [-1.0, -2.0, -3.0] {
        let items = ids
            .iter()
            .map(|id| {
                let mut data = session.audio_clip(*id).unwrap();
                data.gain_db = gain;
                (*id, data)
            })
            .collect();
        session.set_audio_clips(items);
    }
    session.end_gesture();
    assert_eq!(gains(&session, &ids), vec![-3.0; 3], "every clip moved");
    session.undo();
    assert_eq!(
        gains(&session, &ids),
        before,
        "and one undo puts them all back"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn each_clip_keeps_its_own_file() {
    let dir = scratch("files");
    let (mut session, ids) = with_takes(&dir);
    let files: Vec<_> = ids
        .iter()
        .map(|id| session.audio_clip(*id).unwrap().asset.path)
        .collect();
    let items = ids
        .iter()
        .map(|id| {
            let mut data = session.audio_clip(*id).unwrap();
            data.reverse = true;
            (*id, data)
        })
        .collect();
    session.set_audio_clips(items);
    for (id, file) in ids.iter().zip(&files) {
        let data = session.audio_clip(*id).unwrap();
        assert!(data.reverse);
        assert_eq!(&data.asset.path, file);
    }
    std::fs::remove_dir_all(&dir).ok();
}
