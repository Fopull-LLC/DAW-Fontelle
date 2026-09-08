//! Starred things: where they are kept, and how the window stars one.
//!
//! > *"make it so that i can favorite (star) plugins, instruments, effects,
//! > etc."*
//!
//! A favourite is a fact about the person rather than the project — the same
//! reverb is a favourite in every song — so it lives in the settings file,
//! beside the folders and the MIDI curve. The window half (which menus put
//! them where) is `fontelle-ui/tests/favorites.rs`; this is the file and the
//! seam through which a press on a star reaches it.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::{SETTINGS_FORMAT_VERSION, Settings};
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, EffectKind, Favorite, InstrumentKind, PluginKey};
use fontelle_ui::document::StudioHost;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-favorites-{name}-{}-{:?}",
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

// ------------------------------------------------------------ the file ---

#[test]
fn a_star_is_a_toggle_and_the_file_says_which_way_it_went() {
    let mut settings = Settings::default();
    let reverb = Favorite::Effect(EffectKind::Reverb);
    assert!(!settings.is_favorite(&reverb));
    assert!(settings.toggle_favorite(reverb.clone()), "now a favourite");
    assert!(settings.is_favorite(&reverb));
    assert!(!settings.toggle_favorite(reverb.clone()), "and now not");
    assert!(!settings.is_favorite(&reverb));
    assert!(settings.favorites.is_empty());
}

#[test]
fn starring_the_same_thing_twice_keeps_one_of_it() {
    // A list with the same reverb in it twice is a menu with the same reverb
    // in its favourites twice.
    let mut settings = Settings::default();
    let drums = Favorite::Instrument(InstrumentKind::DrumMachine);
    settings.favorites = vec![drums.clone(), drums.clone()];
    assert!(
        !settings.toggle_favorite(drums.clone()),
        "off, whichever copy"
    );
    assert!(settings.favorites.is_empty());
}

#[test]
fn favourites_survive_being_written_and_read_back() {
    let mut settings = Settings::default();
    settings.toggle_favorite(Favorite::Effect(EffectKind::Eq));
    settings.toggle_favorite(Favorite::Plugin(PluginKey::clap("com.u-he.diva")));
    let text = settings.to_json();
    assert!(text.contains("com.u-he.diva"), "{text}");
    let back = Settings::from_json(&text).expect("what was written reads back");
    assert_eq!(back.favorites, settings.favorites);
}

#[test]
fn a_settings_file_written_before_anything_could_be_starred_still_opens() {
    let json = r#"{"format_version":3,"soundfont_dirs":[],"projects_dir":null,"theme":null}"#;
    let read = Settings::from_json(json).expect("an older file is not a broken one");
    assert!(read.favorites.is_empty());
}

#[test]
fn the_file_format_moved_on_when_favourites_arrived() {
    // The rule the settings file follows: a new field bumps the version, so an
    // older build handed a newer file says "upgrade Fontelle" rather than
    // "unknown field `favorites`".
    const { assert!(SETTINGS_FORMAT_VERSION >= 4) };
}

// --------------------------------------------------------- the session ---

#[test]
fn a_press_on_a_star_reaches_the_file_and_the_menus_read_it_back() {
    let dir = scratch("press");
    let mut session = a_session(&dir);
    assert!(
        session.favorites().is_empty(),
        "nothing starred on a fresh install"
    );

    let reverb = Favorite::Effect(EffectKind::Reverb);
    session.toggle_favorite(reverb.clone());
    assert_eq!(session.favorites(), vec![reverb.clone()]);
    let message = session.take_message().unwrap_or_default();
    assert!(
        message.to_lowercase().contains("favorite") && message.contains("Reverb"),
        "the status line says what happened: {message:?}"
    );

    // On disk, not just in memory: the next session reads it from the file.
    let written = std::fs::read_to_string(dir.join("settings.json")).expect("saved");
    assert!(written.contains("Reverb"), "{written}");
    let again = a_session(&dir);
    assert_eq!(again.favorites(), vec![reverb.clone()]);

    // And the same press again takes it off.
    session.toggle_favorite(reverb.clone());
    assert!(session.favorites().is_empty());
    let written = std::fs::read_to_string(dir.join("settings.json")).expect("saved");
    assert!(!written.contains("Reverb"), "{written}");
}

#[test]
fn starring_is_not_an_edit_to_the_project() {
    // INVARIANT 8's spirit: a favourite is about the person, and the project
    // does not become dirty because they starred a compressor.
    use fontelle_ui::document::DocumentHost;
    let dir = scratch("clean");
    let mut session = a_session(&dir);
    let dirty_before = session.is_dirty();
    session.toggle_favorite(Favorite::Effect(EffectKind::Compressor));
    assert_eq!(session.is_dirty(), dirty_before);
}
