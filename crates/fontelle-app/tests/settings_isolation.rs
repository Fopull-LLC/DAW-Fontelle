//! A session nobody pointed at a settings file reads none and writes none.
//!
//! `Session::new` used to read `~/.config/fontelle/settings.json`, so every
//! test built on it read the real settings of the machine running it — and
//! wrote them, through any setting it changed. Found when
//! `live_take`'s `a_take_with_no_clip_open_gets_a_clip_of_its_own` failed on
//! the devbox and nowhere else: a preview build had left a favourite in the
//! real file that this build could not read, and the session's message said
//! so. The studio now asks for the user's file itself
//! (`Session::with_user_settings`, in `main`).

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::StudioHost;

#[test]
fn a_bare_session_reads_no_settings_file() {
    let project = common::a_project_with_a_clip(4, 120.0, common::SR);
    let clip = Session::first_clip(&project).expect("a clip");
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: common::SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("realises");
    let (graphs, _source) = graph_channel(realised.graph);
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls);
    assert_eq!(session.settings_path(), None);
    assert_eq!(
        session.take_message(),
        None,
        "nothing was read, so nothing could be wrong with it"
    );
}
