//! Previewing an audio file on click, and dropping it where the pointer is —
//! the Import tab behaving the way soundfonts already do.
//!
//! > *"clicking on an audio file in the import tab instantly imports it into
//! > your project instead of being like soundfonts where they preview on
//! > select ... if i click one it should play that audio ... double clicking it
//! > would instantly import it ... snapping to the lane nearest to my mouse."*
//!
//! The window's click/double-click/drag routing is driven in the real studio;
//! this is the seam under it — the `StudioHost` methods those gestures call.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::ClipSource;
use fontelle_types::{CompiledTimeline, FolderKind, PPQN};
use fontelle_ui::canvas::BrowserMode;
use fontelle_ui::document::StudioHost;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-preview-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
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

/// A one-second tone written where the Import tab can find it.
fn a_take(dir: &Path, name: &str) -> PathBuf {
    let samples: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.8)
        .collect();
    let path = dir.join(name);
    std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("writable");
    path
}

fn use_audio_folder(session: &mut Session, dir: &Path) {
    session.set_import_folder(FolderKind::Audio, Some(dir.to_path_buf()));
    session.set_import_kind(FolderKind::Audio);
    session.set_browser_mode(BrowserMode::Import);
}

#[test]
fn clicking_an_import_file_previews_it_without_importing() {
    let dir = scratch("listen");
    let mut session = a_session(&dir);
    a_take(&dir, "Loop.wav");
    use_audio_folder(&mut session, &dir);

    let channels_before = session.project().channels.len();
    let clips_before = session.project().clips.len();

    // A click previews: it plays the file and hands back how long it is.
    let seconds = StudioHost::preview_import(&mut session, 0).expect("the file previews");
    assert!(
        seconds > 0.5,
        "a one-second tone previews for about a second, not {seconds}"
    );

    // **A listen is not an import.** Nothing was added to the document — the
    // whole of the report.
    assert_eq!(
        session.project().channels.len(),
        channels_before,
        "previewing must not add a channel"
    );
    assert_eq!(
        session.project().clips.len(),
        clips_before,
        "previewing must not add a clip"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropping_an_import_file_onto_a_row_places_it_there() {
    let dir = scratch("onto");
    let mut session = a_session(&dir);
    a_take(&dir, "Loop.wav");
    use_audio_folder(&mut session, &dir);

    // The row already in the blank project (its one clip's lane).
    let target_index = 0;
    let target_lane = session.project().lane_ids()[target_index];
    let lanes_before = session.project().lanes.len();

    let at = session.project().tempo_map.tick_to_sample(PPQN * 2);
    StudioHost::drop_import_at(&mut session, 0, at, Some(target_index)).expect("the drop lands");

    // No new row, and the clip is on the one the pointer was over.
    assert_eq!(
        session.project().lanes.len(),
        lanes_before,
        "a drop onto an existing row makes no new row"
    );
    let audio_clip = session
        .project()
        .clips
        .values()
        .find(|clip| matches!(clip.source, ClipSource::Audio(_)))
        .expect("an audio clip was made");
    assert_eq!(audio_clip.lane, target_lane, "on the row it was dropped on");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropping_an_import_file_past_the_rows_makes_a_new_row() {
    let dir = scratch("newrow");
    let mut session = a_session(&dir);
    a_take(&dir, "Loop.wav");
    use_audio_folder(&mut session, &dir);

    let lanes_before = session.project().lanes.len();
    // `None` is the drop into the empty space past the last row.
    StudioHost::drop_import_at(&mut session, 0, 0, None).expect("the drop lands");
    assert_eq!(
        session.project().lanes.len(),
        lanes_before + 1,
        "a drop past the rows still makes one of its own"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// > *"when i import sound file and select one seemingly creates an
/// > instrument and doesn't allow u to play anything else when you select a
/// > channel."*
///
/// A click on the Import tab aims the keyboard at the file for as long as the
/// listen lasts — and choosing a channel is the end of the listen: the keys,
/// the MIDI keyboard and the roll play that channel again.
#[test]
fn choosing_a_channel_after_a_preview_gives_the_keys_back_to_the_channel() {
    let dir = scratch("give-back");
    let mut session = a_session(&dir);
    a_take(&dir, "Loop.wav");
    use_audio_folder(&mut session, &dir);
    let target = std::sync::Arc::new(fontelle_midi::LiveTarget::new(
        fontelle_types::NodeId::default(),
    ));
    let mut session = session.with_live_target(std::sync::Arc::clone(&target));
    let channel = session.audition_target();

    StudioHost::preview_import(&mut session, 0).expect("the file previews");
    assert_ne!(session.audition_target(), channel, "the listen is heard");

    StudioHost::select_channel(&mut session, 0);
    assert_eq!(
        session.audition_target(),
        channel,
        "the keys are the channel's"
    );
    assert_eq!(target.get(), channel, "and so is the MIDI keyboard");

    let _ = std::fs::remove_dir_all(&dir);
}
