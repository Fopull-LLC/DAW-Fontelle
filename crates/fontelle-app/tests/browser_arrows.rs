//! The arrows walk a browser list and you hear each row as they land on it
//! (Ty, 2026-11-12: *"it would also be good if when you used arrow keys in
//! the browser to go between presets it played the sound currently you have
//! to click this is with soundfonts and stuff"*).
//!
//! A click on a row is a listen; walking to a row with the arrows is the same
//! listen, so a list can be heard from top to bottom without the mouse. Enter
//! is still the choice, as a double-click is.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, FolderKind};
use fontelle_ui::canvas::{BrowserMode, PresetDevice};
use fontelle_ui::document::{LibraryKind, StudioHost};
use fontelle_ui::layout::Rect;
use winit::keyboard::{Key, NamedKey};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-browser-arrows-{name}-{}-{:?}",
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

fn window(session: Session) -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 800),
    )
}

fn middle_of(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// Waits out the audition a click started, so the next one is the arrow's.
fn let_it_end(w: &mut fontelle_ui::Windowless) {
    for _ in 0..40 {
        if w.auditioning().is_none() {
            return;
        }
        w.idle(Duration::from_millis(50));
    }
    panic!("the audition never ended");
}

#[test]
fn walking_the_presets_with_the_arrows_plays_each_one() {
    let dir = scratch("presets");
    let mut session = a_session(&dir);
    session.set_browser_mode(BrowserMode::Presets);
    let device = session
        .library_files()
        .iter()
        .position(|e| e.name == "Flopsynth")
        .expect("Flopsynth has presets");
    session.open_file(device).unwrap();
    let rows = session.library_presets();
    let first = rows
        .iter()
        .position(|r| r.kind != LibraryKind::Group)
        .expect("a preset row");
    let second = (first + 1..rows.len())
        .find(|i| rows[*i].kind != LibraryKind::Group)
        .expect("a second preset row");

    let mut w = window(session);
    w.show_browser_mode(BrowserMode::Presets);
    let at = w.browser_preset_row(first).expect("the first row is drawn");
    let (x, y) = middle_of(at);
    w.click_studio(x, y);
    assert!(w.auditioning().is_some(), "a click is a listen");
    let_it_end(&mut w);

    w.browser_key(Key::Named(NamedKey::ArrowDown));
    assert!(
        w.auditioning().is_some(),
        "the arrow landed on a row and it was not heard"
    );
    assert_eq!(
        w.document().preset_bar(PresetDevice::Instrument).name,
        Some(rows[second].name.clone()),
        "the row heard is the one the arrow moved to"
    );

    // A key has no button to come up: the note still ends on its own.
    let_it_end(&mut w);

    w.browser_key(Key::Named(NamedKey::ArrowUp));
    assert_eq!(
        w.document().preset_bar(PresetDevice::Instrument).name,
        Some(rows[first].name.clone()),
        "and back up"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A short tone, so its audition is over in a moment.
fn a_blip(dir: &Path, name: &str) {
    let samples: Vec<f32> = (0..2_400)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();
    std::fs::write(dir.join(name), build_wav(48_000, 1, &samples)).unwrap();
}

#[test]
fn the_import_list_walks_with_the_arrows_too_and_enter_imports_where_they_are() {
    let dir = scratch("import");
    let audio = dir.join("audio");
    std::fs::create_dir_all(&audio).unwrap();
    a_blip(&audio, "Alpha.wav");
    a_blip(&audio, "Bravo.wav");
    let mut session = a_session(&dir);
    session.set_import_folder(FolderKind::Audio, Some(audio.clone()));
    session.set_import_kind(FolderKind::Audio);
    session.set_browser_mode(BrowserMode::Import);
    let rows = session.import_files();
    let alpha = rows
        .iter()
        .position(|r| r.name.starts_with("Alpha"))
        .expect("Alpha is listed");
    let bravo = rows
        .iter()
        .position(|r| r.name.starts_with("Bravo"))
        .expect("Bravo is listed");
    assert_eq!(bravo, alpha + 1, "listed by name");

    let mut w = window(session);
    let clips_before = w.document().clips().len();
    let (x, y) = middle_of(w.browser_file_row(alpha).expect("Alpha is drawn"));
    w.click_studio(x, y);
    assert!(w.auditioning().is_some(), "a click is a listen");
    let_it_end(&mut w);

    w.browser_key(Key::Named(NamedKey::ArrowDown));
    assert!(w.auditioning().is_some(), "Bravo was not heard");
    let_it_end(&mut w);
    assert_eq!(
        w.document().clips().len(),
        clips_before,
        "walking to a file is a listen, not an import"
    );

    // An audio file comes in as a clip of its own.
    w.browser_key(Key::Named(NamedKey::Enter));
    let clips = w.document().clips();
    assert_eq!(clips.len(), clips_before + 1, "Enter imports");
    assert!(
        clips.iter().any(|clip| clip.name.contains("Bravo")),
        "the file the arrow is on: {:?}",
        clips.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(&dir).ok();
}
