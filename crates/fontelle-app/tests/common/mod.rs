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
    let timeline = fontelle_sequencer::compile(project, &realised.channel_nodes);
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
