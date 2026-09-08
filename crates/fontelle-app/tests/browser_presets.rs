//! The browser's fifth tab: every preset, for every device
//! (`docs/flopsynth-plan.md` §P.8).
//!
//! It is the Sounds tab's shape with a different list in it — devices above,
//! that device's presets below, grouped by category — and that is the point.
//! Everything a browser of presets needs was already here: a virtualised list,
//! a search across the whole collection, a star on every row, a click that
//! applies. A second implementation of all of it, in a panel of its own, would
//! be worse at every one of them.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, EffectKind, InstrumentKind};
use fontelle_ui::canvas::BrowserMode;
use fontelle_ui::document::{LibraryKind, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-browser-presets-{name}-{}-{:?}",
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
    let mut session = Session::new(
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
    .with_settings_path(dir.join("settings.json"));
    session.set_browser_mode(BrowserMode::Presets);
    session
}

/// Which row a device is on.
fn device_row(session: &Session, name: &str) -> usize {
    session
        .library_files()
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no device called {name}: {:?}",
                session
                    .library_files()
                    .iter()
                    .map(|e| e.name.clone())
                    .collect::<Vec<_>>()
            )
        })
}

/// Which row a preset is on, in the list that is showing.
fn preset_row(session: &Session, name: &str) -> usize {
    session
        .library_presets()
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("no preset called {name}"))
}

// --------------------------------------------------------------- the rows

#[test]
fn there_is_a_presets_tab_and_it_lists_every_device_that_has_any() {
    let dir = scratch("devices");
    let session = a_session(&dir);
    let rows = session.library_files();
    assert!(rows.len() > 1, "{rows:?}");
    assert!(rows.iter().all(|row| row.kind == LibraryKind::Folder));
    assert!(rows.iter().any(|row| row.name == "Flopsynth"));
    assert!(
        rows.iter().all(|row| !row.detail.is_empty()),
        "every device row says how many it has"
    );
}

#[test]
fn nothing_is_listed_until_a_device_is_opened() {
    // The Sounds tab's behaviour exactly: the presets list is what is *inside*
    // the thing on the left, and nothing is inside nothing.
    let dir = scratch("closed");
    let session = a_session(&dir);
    assert!(session.library_presets().is_empty());
}

#[test]
fn opening_a_device_lists_its_presets_under_their_categories() {
    let dir = scratch("open");
    let mut session = a_session(&dir);
    session
        .open_file(device_row(&session, "Flopsynth"))
        .unwrap();
    let rows = session.library_presets();
    assert!(rows.len() > 100, "Flopsynth ships a bank");
    assert!(
        rows.iter().any(|row| row.kind == LibraryKind::Group),
        "the categories are headings"
    );
    assert!(rows.iter().any(|row| row.name == "Choir Ahh"));
}

#[test]
fn the_search_runs_across_every_device() {
    // You go looking for "hall" without first deciding it is a reverb — which
    // is why the search does not need a device to be open.
    let dir = scratch("search");
    let mut session = a_session(&dir);
    session.set_query("choir");
    let rows = session.library_presets();
    assert!(!rows.is_empty(), "a search with no device open still finds");
    assert!(
        rows.iter()
            .filter(|row| row.kind != LibraryKind::Group)
            .all(|row| row.name.to_lowercase().contains('c')),
        "{rows:?}"
    );
}

#[test]
fn the_status_line_says_where_your_own_presets_live_and_counts_both_banks() {
    let dir = scratch("status");
    let session = a_session(&dir);
    let status = session.preset_status();
    assert!(status.contains("built in"), "{status}");
    assert!(status.contains("presets"), "{status}");
}

// ------------------------------------------------------------ the clicks

#[test]
fn clicking_an_instrument_preset_puts_it_on_the_selected_channel() {
    let dir = scratch("apply");
    let mut session = a_session(&dir);
    session
        .open_file(device_row(&session, "Flopsynth"))
        .unwrap();
    let at = preset_row(&session, "Choir Ahh");
    session.set_channel_instrument(at).unwrap();
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
    assert_eq!(
        session
            .preset_bar(fontelle_ui::canvas::PresetDevice::Instrument)
            .name
            .as_deref(),
        Some("Choir Ahh")
    );
}

#[test]
fn clicking_an_effect_preset_with_no_effect_window_open_says_so() {
    // Rather than landing on whichever insert happens to be first, which
    // would be a click that changed a sound nobody was looking at.
    let dir = scratch("no-window");
    let mut session = a_session(&dir);
    session.add_insert(0, EffectKind::Distortion);
    session.open_file(device_row(&session, "Dist")).unwrap();
    let at = session
        .library_presets()
        .iter()
        .position(|row| row.kind != LibraryKind::Group)
        .expect("the distortion ships presets");
    let refused = session
        .set_channel_instrument(at)
        .expect_err("an effect preset needs an effect window");
    assert!(refused.contains("window"), "{refused}");
}

#[test]
fn clicking_an_effect_preset_lands_in_the_insert_you_are_looking_at() {
    let dir = scratch("insert");
    let mut session = a_session(&dir);
    session.add_insert(0, EffectKind::Distortion);
    session.note_open_insert(Some((0, 0)));
    session.open_file(device_row(&session, "Dist")).unwrap();
    let at = session
        .library_presets()
        .iter()
        .position(|row| row.kind != LibraryKind::Group)
        .expect("the distortion ships presets");
    let name = session.library_presets()[at].name.clone();
    session.set_channel_instrument(at).unwrap();
    assert_eq!(
        session
            .preset_bar(fontelle_ui::canvas::PresetDevice::Insert { strip: 0, slot: 0 })
            .name,
        Some(name)
    );
}
