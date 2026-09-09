//! Mixer track presets, end to end: saving a track's chain and putting it on
//! another track.
//!
//! The `fontelle-model` half is `tests/track_presets.rs` one crate down; this
//! is the half the window talks to — the bank, the choices a menu lists, and
//! what the two host methods do.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddInsert, Command, MixerTrack, Project};
use fontelle_types::{CompiledTimeline, EffectKind, MixerTrackId};
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-track-presets-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A session whose user preset bank is a folder of this test's own.
fn a_session(dir: &Path, project: Project) -> Session {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let clip = Session::first_clip(&project).unwrap_or_default();
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("it must realise");
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

/// Where a track sits in the strip order the window draws.
fn strip_of(session: &fontelle_app::Session, track: MixerTrackId) -> usize {
    let name = session.project().mixer.tracks[track].name.clone();
    session
        .mixer_strips()
        .iter()
        .position(|strip| strip.name == name)
        .expect("the track is on the mixer")
}

/// Two tracks: one with a chain on it, one bare.
fn two_tracks() -> (Project, MixerTrackId, MixerTrackId) {
    let mut project = common::a_clip_project(4);
    let master = project.mixer.master.expect("a master");
    let dressed = project.mixer.tracks.insert(MixerTrack::new("Lead Vocal"));
    project.mixer.tracks[dressed].output = Some(master);
    project.mixer.tracks[dressed].gain_db = -4.5;
    project.mixer.tracks[dressed].pan = 0.2;
    for kind in [EffectKind::Gate, EffectKind::Compressor, EffectKind::Reverb] {
        AddInsert::new(dressed, kind)
            .apply(&mut project)
            .expect("an insert");
    }
    let bare = project.mixer.tracks.insert(MixerTrack::new("Backing"));
    project.mixer.tracks[bare].output = Some(master);
    (project, dressed, bare)
}

#[test]
fn a_track_saves_its_chain_and_another_track_loads_it() {
    let (project, dressed, bare) = two_tracks();
    let mut session = a_session(&scratch("saves-and-loads"), project);
    let (from, onto) = (strip_of(&session, dressed), strip_of(&session, bare));

    session.save_preset_as(PresetDevice::Track { strip: from }, "Lead Chain", "Vocals");
    let choices = session.preset_choices(PresetDevice::Track { strip: onto });
    let at = choices
        .iter()
        .position(|choice| choice.name == "Lead Chain")
        .expect("the chain is in the bank");

    session.apply_preset(PresetDevice::Track { strip: onto }, at);
    let after = &session.project().mixer.tracks[bare];
    assert_eq!(after.inserts.len(), 3, "the whole chain came across");
    assert_eq!(after.inserts[0].config.kind(), EffectKind::Gate);
    assert_eq!(after.inserts[2].config.kind(), EffectKind::Reverb);
    assert!((after.gain_db + 4.5).abs() < 1e-4, "and the level with it");
    assert!((after.pan - 0.2).abs() < 1e-4);
    // The one thing it must not have taken (`TrackChain`'s own doc).
    assert_eq!(
        after.name, "Backing",
        "a preset names the sound, not the part"
    );
}

/// The bank is per **device kind**, and a track's kind is `Track`: a chain
/// must never turn up in a reverb's list, or a reverb preset in a track's.
#[test]
fn track_presets_and_effect_presets_are_different_banks() {
    let (project, dressed, _) = two_tracks();
    let mut session = a_session(&scratch("separate-banks"), project);
    let from = strip_of(&session, dressed);
    session.save_preset_as(PresetDevice::Track { strip: from }, "Lead Chain", "Vocals");

    let inserts = session.preset_choices(PresetDevice::Insert {
        strip: from,
        slot: 2,
    });
    assert!(
        !inserts.iter().any(|choice| choice.name == "Lead Chain"),
        "a whole chain turned up in the reverb's preset list"
    );
}

/// Loading a chain is **one** thing a person did.
#[test]
fn loading_a_chain_is_one_undo_entry() {
    let (project, dressed, bare) = two_tracks();
    let mut session = a_session(&scratch("one-undo"), project);
    let (from, onto) = (strip_of(&session, dressed), strip_of(&session, bare));
    session.save_preset_as(PresetDevice::Track { strip: from }, "Lead Chain", "Vocals");
    let choices = session.preset_choices(PresetDevice::Track { strip: onto });
    let at = choices
        .iter()
        .position(|choice| choice.name == "Lead Chain")
        .expect("the chain is in the bank");

    let before = session.undo_depth();
    session.apply_preset(PresetDevice::Track { strip: onto }, at);
    assert_eq!(session.undo_depth(), before + 1);
    session.undo();
    assert!(
        session.project().mixer.tracks[bare].inserts.is_empty(),
        "one undo puts the track back"
    );
}

/// The factory bank ships vocal chains, and every one of them loads.
#[test]
fn the_factory_vocal_chains_are_there_and_every_one_loads() {
    let (project, _, bare) = two_tracks();
    let mut session = a_session(&scratch("factory"), project);
    let onto = strip_of(&session, bare);
    let choices = session.preset_choices(PresetDevice::Track { strip: onto });
    let factory: Vec<_> = choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| choice.origin == fontelle_types::PresetOrigin::Factory)
        .map(|(at, choice)| (at, choice.name.clone()))
        .collect();
    assert!(
        factory.len() >= 12,
        "the factory track bank has {} chains in it",
        factory.len()
    );
    for (at, name) in factory {
        session.apply_preset(PresetDevice::Track { strip: onto }, at);
        assert!(
            !session.project().mixer.tracks[bare].inserts.is_empty(),
            "{name} loaded an empty chain"
        );
    }
}
