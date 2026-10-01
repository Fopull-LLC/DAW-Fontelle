//! The theme library and the settings page's Appearance section.
//!
//! > *"they all boil down to a single .fontelletheme file that can be easily
//! > shared and entered and saved into the app and added to the users theme
//! > library to be swapped between"* — Ty, 2026-10-01.
//!
//! The format is `fontelle-ui/tests/theme_files.rs`. This is where the files
//! live, how one arrives, and what the page does with them.

mod common;

use std::path::PathBuf;

use fontelle_app::themes::ThemeLibrary;
use fontelle_ui::StudioHost;
use fontelle_ui::canvas::SettingControl;
use fontelle_ui::theme::{BackdropPanel, THEME_EXTENSION, Theme};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-themes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn a_theme_file(dir: &std::path::Path, theme: &Theme) -> PathBuf {
    let path = dir.join(format!("{}.{THEME_EXTENSION}", theme.name));
    std::fs::write(&path, theme.to_json()).unwrap();
    path
}

fn a_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[10, 20, 30, 255]).unwrap();
    }
    out
}

/// A session whose settings file — and so whose theme library — is in `dir`.
fn studio(dir: &std::path::Path) -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(1)).with_settings_path(dir.join("settings.json"))
}

fn row(session: &fontelle_app::Session, name: &str) -> usize {
    session
        .settings()
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("no {name:?} row"))
}

// ---------------------------------------------------------- the library ---

#[test]
fn the_library_lists_the_built_in_looks_first_and_then_the_files() {
    let dir = scratch("list");
    let library = ThemeLibrary::new(dir.join("themes"));
    let names: Vec<String> = library.entries().into_iter().map(|e| e.name).collect();
    let builtins: Vec<String> = Theme::builtins().into_iter().map(|t| t.name).collect();
    assert_eq!(names, builtins, "an empty folder is the built-ins alone");

    let mut mine = Theme::paper();
    mine.name = "Lagoon".to_string();
    let source = a_theme_file(&dir, &mine);
    assert_eq!(library.import(&source).unwrap(), "Lagoon");
    let entries = library.entries();
    assert_eq!(entries.last().unwrap().name, "Lagoon");
    assert!(
        entries.last().unwrap().path.is_some(),
        "a file, not built in"
    );
    assert_eq!(library.load("Lagoon"), Some(mine));
    assert!(
        library
            .dir()
            .join(format!("Lagoon.{THEME_EXTENSION}"))
            .is_file(),
        "copied in: the file it came from can be deleted"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn importing_a_theme_whose_name_is_taken_keeps_both() {
    let dir = scratch("clash");
    let library = ThemeLibrary::new(dir.join("themes"));
    let source = a_theme_file(&dir, &Theme::midnight());
    let name = library.import(&source).unwrap();
    assert_eq!(name, "Midnight 2", "a built-in is never shadowed");
    assert_eq!(library.import(&source).unwrap(), "Midnight 3");
    assert_eq!(library.load("Midnight"), Some(Theme::midnight()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_that_is_not_a_theme_is_refused_and_says_why() {
    let dir = scratch("refuse");
    let library = ThemeLibrary::new(dir.join("themes"));
    let bad = dir.join("notes.fontelletheme");
    std::fs::write(&bad, "{ \"hello\": 1 }").unwrap();
    let why = library.import(&bad).unwrap_err();
    assert!(why.contains("theme"), "{why}");
    assert_eq!(library.entries().len(), Theme::builtins().len());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ------------------------------------------------------- the settings page ---

#[test]
fn the_page_has_an_appearance_section_with_a_theme_chooser() {
    let dir = scratch("page");
    let session = studio(&dir);
    let heading = row(&session, "Appearance");
    let theme = row(&session, "Theme");
    assert!(theme > heading);
    let controls = session.setting_controls();
    assert_eq!(controls[heading], SettingControl::Heading);
    let SettingControl::Choice { options, chosen } = &controls[theme] else {
        panic!("the theme row is a drop-down: {:?}", controls[theme]);
    };
    let builtins: Vec<String> = Theme::builtins().into_iter().map(|t| t.name).collect();
    assert_eq!(options, &builtins);
    assert_eq!(*chosen, 0, "the default look until one is chosen");
    for name in [
        "Corner rounding",
        "Window picture",
        "Transport picture",
        "Channels picture",
        "Browser picture",
        "Arrangement picture",
        "Piano roll picture",
        "Mixer picture",
        "Panel see-through",
        "Picture strength",
        "Import a theme",
        "Save this theme",
    ] {
        row(&session, name);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn choosing_a_theme_changes_the_window_and_is_remembered() {
    let dir = scratch("choose");
    let mut session = studio(&dir);
    let before = session.theme_revision();
    let theme = row(&session, "Theme");
    session.choose_setting(theme, 2);
    assert_eq!(session.theme().name, "Midnight");
    assert_ne!(session.theme_revision(), before, "the window is told");

    // The next launch opens in it.
    let again = studio(&dir);
    assert_eq!(again.theme().name, "Midnight");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn changing_a_built_in_look_saves_a_copy_of_it_rather_than_editing_it() {
    let dir = scratch("copy");
    let mut session = studio(&dir);
    session.choose_setting(row(&session, "Theme"), 2); // Midnight
    let rounding = row(&session, "Corner rounding");
    assert!(matches!(
        session.setting_controls()[rounding],
        SettingControl::Slider { .. }
    ));
    session.set_setting_fraction(rounding, 1.0);
    let theme = session.theme();
    assert_eq!(theme.name, "Midnight (mine)");
    assert!(theme.metrics.corner_radius > 8.0, "{:?}", theme.metrics);
    assert_eq!(
        Theme::midnight().metrics.corner_radius,
        0.0,
        "the built-in is as it was"
    );
    let library = ThemeLibrary::new(dir.join("themes"));
    assert_eq!(library.load("Midnight (mine)"), Some(theme.clone()));
    // A second change edits the copy, not a copy of the copy.
    session.set_setting_fraction(rounding, 0.0);
    assert_eq!(session.theme().name, "Midnight (mine)");
    assert_eq!(session.theme().metrics.corner_radius, 0.0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_picture_goes_behind_a_panel_inside_the_theme() {
    let dir = scratch("picture");
    let picture = dir.join("wall.png");
    std::fs::write(&picture, a_png()).unwrap();
    let mut session = studio(&dir);
    session
        .set_backdrop_from(BackdropPanel::Arrangement, Some(&picture))
        .unwrap();
    let theme = session.theme();
    assert_eq!(theme.name, "Fontelle Dark (mine)");
    let backdrop = theme
        .backdrops
        .get(BackdropPanel::Arrangement)
        .expect("set");
    assert_eq!(backdrop.image_bytes(), Some(a_png()));
    // In the file: the picture can be deleted and the theme keeps it.
    std::fs::remove_file(&picture).unwrap();
    let library = ThemeLibrary::new(dir.join("themes"));
    assert!(
        library
            .load("Fontelle Dark (mine)")
            .unwrap()
            .backdrops
            .arrangement
            .is_some()
    );
    // The strength slider sets how much shows.
    session.set_setting_fraction(row(&session, "Picture strength"), 0.25);
    let strength = session
        .theme()
        .backdrops
        .get(BackdropPanel::Arrangement)
        .unwrap()
        .opacity;
    assert!((strength - 0.25).abs() < 1e-3, "{strength}");
    // And it comes off again.
    session
        .set_backdrop_from(BackdropPanel::Arrangement, None)
        .unwrap();
    assert!(session.theme().backdrops.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_picture_that_is_not_one_is_refused() {
    let dir = scratch("not-picture");
    let text = dir.join("wall.png");
    std::fs::write(&text, "not a picture").unwrap();
    let mut session = studio(&dir);
    assert!(
        session
            .set_backdrop_from(BackdropPanel::Mixer, Some(&text))
            .is_err()
    );
    assert_eq!(session.theme().name, "Fontelle Dark", "nothing copied");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_theme_in_use_is_saved_as_one_file_to_send_to_someone() {
    let dir = scratch("export");
    let mut session = studio(&dir);
    session.choose_setting(row(&session, "Theme"), 3); // Ember
    let out = dir.join(format!("Ember.{THEME_EXTENSION}"));
    session.export_theme_to(&out).unwrap();
    assert_eq!(Theme::load_from_file(&out).unwrap(), Theme::ember());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_imported_theme_is_chosen_at_once() {
    let dir = scratch("import");
    let mut mine = Theme::ember();
    mine.name = "Rust".to_string();
    let file = a_theme_file(&dir, &mine);
    let mut session = studio(&dir);
    assert_eq!(session.import_theme_from(&file).unwrap(), "Rust");
    assert_eq!(session.theme(), mine);
    let SettingControl::Choice { options, chosen } =
        session.setting_controls()[row(&session, "Theme")].clone()
    else {
        panic!()
    };
    assert_eq!(options[chosen], "Rust");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// *"even have transparency for things that can overlap"*: the panels can
/// be made see-through, so a picture behind the whole window shows through
/// them. The ground under everything stays solid.
#[test]
fn panel_see_through_makes_the_panels_translucent_on_a_copy() {
    let dir = scratch("see-through");
    let mut session = studio(&dir);
    let row = row(&session, "Panel see-through");
    assert!(matches!(
        session.setting_controls()[row],
        SettingControl::Slider { .. }
    ));
    session.set_setting_fraction(row, 0.5);
    let theme = session.theme();
    assert_eq!(theme.name, "Fontelle Dark (mine)");
    let alpha = |c: fontelle_ui::theme::Color| c.0[3];
    assert!(alpha(theme.palette.panel) < 0xff && alpha(theme.palette.panel) > 0x40);
    assert_eq!(
        alpha(theme.palette.panel),
        alpha(theme.palette.panel_header)
    );
    assert_eq!(alpha(theme.palette.window), 0xff, "the ground stays solid");
    let SettingControl::Slider { fraction } = session.setting_controls()[row] else {
        panic!()
    };
    assert!((fraction - 0.5).abs() < 0.02, "{fraction}");
    std::fs::remove_dir_all(&dir).unwrap();
}
