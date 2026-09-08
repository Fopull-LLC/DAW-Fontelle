//! Shared rig for the app's rendering tests: the demo document with an
//! instrument on it, put through the same realisation step `--play-sf2` uses.
//!
//! Not every test binary uses every helper here, and each compiles this module
//! separately, so the unused ones would otherwise warn.
#![allow(dead_code)]

use fontelle_app::{
    RealiseOptions, Realised, SampleLibrary, demo_project, realise, set_channel_patch,
};
use fontelle_core::Patch;
use fontelle_dsp::Interpolation;
use fontelle_model::Project;
use fontelle_types::CompiledTimeline;

pub const SR: u32 = 48_000;
pub const BPM: f64 = 120.0;

/// Every document change in a test goes through a command, the same as
/// everywhere else (INVARIANT 9).
pub fn set_number(project: &mut Project, target: fontelle_model::NumberTarget, value: f64) {
    use fontelle_model::Command;
    fontelle_model::SetNumber::new(target, value)
        .apply(project)
        .expect("the target must exist");
}

/// The demo phrase with `patch` on its one channel — the document a run of
/// `--play-sf2` builds, minus the soundfont.
///
/// The patch goes through its serialised form on the way in and comes back out
/// of it in `realise`, exactly as a saved project would, so every test here is
/// also a test that the round trip did not change what the patch does.
pub fn demo_with(patch: &Patch, library: &SampleLibrary) -> Project {
    let mut project = demo_project(60, BPM, SR);
    let channel = project
        .channels
        .keys()
        .next()
        .expect("the demo project has one channel");
    set_channel_patch(&mut project, channel, patch, library)
        .expect("a patch this build built must serialise");
    project
}

pub fn realise_at(
    project: &Project,
    library: &SampleLibrary,
    quality: Interpolation,
) -> (Realised, CompiledTimeline) {
    let realised = realise(
        project,
        library,
        RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality,
        },
    )
    .expect("this project must realise");
    let timeline =
        fontelle_sequencer::compile(project, &realised.channel_nodes, &realised.param_nodes);
    (realised, timeline)
}

/// The demo document, its graph and its timeline in one call — what almost
/// every rendering test wants.
pub fn demo_rig(
    patch: &Patch,
    library: &SampleLibrary,
    quality: Interpolation,
) -> (Project, Realised, CompiledTimeline) {
    let project = demo_with(patch, library);
    let (realised, timeline) = realise_at(&project, library, quality);
    (project, realised, timeline)
}

/// A `Session` around `project`, wired the way the studio wires one: realised,
/// with its graph and param nodes, and a settings path in a scratch directory
/// so nothing here touches the real config.
pub fn a_session_for(project: Project) -> fontelle_app::Session {
    a_session_in(project, None)
}

/// [`a_session_for`], with a bundle path — what anything that writes into the
/// project folder needs (a render, an export).
pub fn a_session_in(project: Project, bundle: Option<std::path::PathBuf>) -> fontelle_app::Session {
    use fontelle_app::Session;
    use fontelle_engine::{graph_channel, timeline_channel};

    // `unwrap_or_default`, the same as `Session::adopt` does, because a
    // project no longer arrives with a clip in it (see
    // `tests/starting_project.rs`) and a studio opened on an empty arrangement
    // is an ordinary state rather than a broken one: every read of the open
    // clip goes through `project.clips.get`, which answers `None` for an id
    // that names nothing.
    let clip = Session::first_clip(&project).unwrap_or_default();
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = realise(&project, &library, options).expect("this project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    let dir = std::env::temp_dir().join(format!(
        "fontelle-session-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("creatable");
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        bundle,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

/// A blank project with **one empty clip** on its first row — the shape
/// `blank_project` had before a new project stopped arriving with content in
/// it (`tests/starting_project.rs`).
///
/// Every test that wants *a clip to work on*, rather than to make a claim
/// about what a new project contains, takes one from here. That keeps the two
/// claims separable: what a new project holds is one test's business, and what
/// a stamp or a fade or a render does to a clip is another's.
pub fn a_project_with_a_clip(bars: i64, bpm: f64, sample_rate: u32) -> Project {
    use fontelle_model::{AddClip, Arena, Clip, ClipSource, Command, NoteData};
    use fontelle_types::PPQN;

    let mut project = fontelle_app::blank_project(bars, bpm, sample_rate);
    let channel = project
        .channels
        .keys()
        .next()
        .expect("a blank project has a channel");
    let lane = project
        .lane_ids()
        .first()
        .copied()
        .expect("a blank project has rows");
    AddClip::new(Clip {
        lane,
        start: 0,
        length: PPQN * 4 * bars.max(1),
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(&mut project)
    .expect("a blank project must take a clip");
    project
}

/// [`a_project_with_a_clip`] at the rig's own rate and tempo.
pub fn a_clip_project(bars: i64) -> Project {
    a_project_with_a_clip(bars, BPM, SR)
}
