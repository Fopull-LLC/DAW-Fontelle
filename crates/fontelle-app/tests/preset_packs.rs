//! Packs through the session (`docs/flopsynth-next.md` §5): **Export
//! pack** writes every preset the user made for a device as one file;
//! **Import pack** reads one back into the bank, and the browser sees the
//! rows at once. The picker-driven halves live on `StudioHost`; the
//! path-taking halves are what is held here.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, InstrumentKind, PresetOrigin};
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::StudioHost;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-packs-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn a_session(dir: &Path) -> Session {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("realises");
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

const INSTRUMENT: PresetDevice = PresetDevice::Instrument;

fn mine(session: &Session) -> Vec<String> {
    session
        .preset_choices(INSTRUMENT)
        .iter()
        .filter(|p| p.origin == PresetOrigin::User)
        .map(|p| p.name.clone())
        .collect()
}

#[test]
fn a_pack_is_the_users_presets_for_the_device_and_reads_back_into_another_bank() {
    let dir = scratch("out");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session.save_preset_as(INSTRUMENT, "My Pad", "Pad");
    session.save_preset_as(INSTRUMENT, "My Keys", "Keys");
    assert_eq!(mine(&session).len(), 2);

    let pack = dir.join("mine.fontelle-pack.json");
    let said = session
        .export_pack_to(INSTRUMENT, &pack)
        .expect("the pack is written");
    assert!(said.contains('2'), "it says how many: {said}");
    assert!(pack.is_file());

    // A second studio, with a bank of its own: the rows arrive.
    let other = scratch("in");
    let mut fresh = a_session(&other);
    fresh.set_channel_kind(0, InstrumentKind::Flopsynth);
    assert!(mine(&fresh).is_empty());
    let said = fresh.import_pack_from(&pack).expect("the pack is read");
    assert!(said.contains('2'), "{said}");
    let mut names = mine(&fresh);
    names.sort();
    assert_eq!(names, ["My Keys", "My Pad"]);
    // And can be loaded like any row.
    let keys = fresh
        .preset_choices(INSTRUMENT)
        .iter()
        .position(|p| p.name == "My Keys" && p.origin == PresetOrigin::User)
        .expect("the row");
    fresh.apply_preset(INSTRUMENT, keys);
    assert_eq!(fresh.channels()[0].name, "My Keys");

    // Again: nothing written over, and it says so.
    let said = fresh.import_pack_from(&pack).expect("read again");
    assert!(
        said.contains("skipped") || said.contains("already"),
        "{said}"
    );
    assert_eq!(mine(&fresh).len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_device_with_nothing_of_yours_has_nothing_to_pack() {
    let dir = scratch("empty");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    let pack = dir.join("none.fontelle-pack.json");
    assert!(session.export_pack_to(INSTRUMENT, &pack).is_err());
    assert!(!pack.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
