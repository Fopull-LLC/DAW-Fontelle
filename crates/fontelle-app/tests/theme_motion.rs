//! Moving themes from the settings page's side (hub card 0366): how much a
//! backdrop may move, remembered; the OS's reduce-motion choice respected;
//! and a theme carrying shaders and a font travelling as one file.

mod common;

use std::path::PathBuf;

use fontelle_app::settings::Settings;
use fontelle_app::themes::ThemeLibrary;
use fontelle_ui::StudioHost;
use fontelle_ui::backdrop::{Effects, Motion};
use fontelle_ui::canvas::SettingControl;
use fontelle_ui::theme::{BackdropPanel, Layer, ShaderLayer, THEME_EXTENSION, Theme, ThemeFont};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-motion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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

#[test]
fn appearance_has_the_motion_rows_under_the_theme() {
    let dir = scratch("rows");
    let session = studio(&dir);
    let theme = row(&session, "Theme");
    let controls = session.setting_controls();
    for name in ["Effects", "Backdrop rate", "Backdrop resolution"] {
        let at = row(&session, name);
        assert!(at > theme, "{name} is in Appearance");
        assert!(
            matches!(controls[at], SettingControl::Choice { .. }),
            "{name}: {:?}",
            controls[at]
        );
    }
    let hold = row(&session, "Hold backdrops still while playing");
    assert_eq!(controls[hold], SettingControl::Switch { on: false });
    let unfocused = row(&session, "Keep backdrops moving when not focused");
    assert_eq!(controls[unfocused], SettingControl::Switch { on: true });
    let SettingControl::Choice { options, chosen } = &controls[row(&session, "Effects")] else {
        panic!()
    };
    assert_eq!(options, &["Moving", "Still", "Off"]);
    assert_eq!(options[*chosen], "Moving");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_motion_rows_reach_the_window_and_are_remembered() {
    let dir = scratch("remember");
    let mut session = studio(&dir);
    assert_eq!(session.backdrop_motion(), Motion::default());

    session.choose_setting(row(&session, "Effects"), 1);
    let SettingControl::Choice { options, .. } =
        session.setting_controls()[row(&session, "Backdrop rate")].clone()
    else {
        panic!()
    };
    let fifteen = options.iter().position(|o| o == "15 fps").expect("15 fps");
    session.choose_setting(row(&session, "Backdrop rate"), fifteen);
    let SettingControl::Choice { options, .. } =
        session.setting_controls()[row(&session, "Backdrop resolution")].clone()
    else {
        panic!()
    };
    let full = options.iter().position(|o| o == "Full").expect("Full");
    session.choose_setting(row(&session, "Backdrop resolution"), full);
    session.nudge_setting(row(&session, "Hold backdrops still while playing"), 1);

    session.nudge_setting(row(&session, "Keep backdrops moving when not focused"), 1);

    let wanted = Motion {
        effects: Effects::Still,
        fps: 15.0,
        scale: 1.0,
        hold_while_playing: true,
        when_unfocused: false,
    };
    assert_eq!(session.backdrop_motion(), wanted);
    let again = studio(&dir);
    assert_eq!(again.backdrop_motion(), wanted, "the next launch keeps it");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_reduce_motion_desktop_starts_fontelle_still() {
    let untouched = Settings::default();
    assert_eq!(untouched.backdrop_motion(true).effects, Effects::Still);
    assert_eq!(untouched.backdrop_motion(false).effects, Effects::Moving);
    // A choice made on the page outranks the desktop's.
    let chosen = Settings {
        backdrop_effects: Some(Effects::Moving),
        ..Settings::default()
    };
    assert_eq!(chosen.backdrop_motion(true).effects, Effects::Moving);
}

#[test]
fn a_settings_file_from_before_moving_themes_reads_as_the_defaults() {
    let dir = scratch("old-settings");
    let path = dir.join("settings.json");
    std::fs::write(&path, Settings::default().to_json()).unwrap();
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for key in [
        "backdrop_effects",
        "backdrop_fps",
        "backdrop_scale_percent",
        "hold_backdrops_while_playing",
        "backdrops_when_unfocused",
    ] {
        json.as_object_mut().unwrap().remove(key);
    }
    std::fs::write(&path, json.to_string()).unwrap();
    let (settings, error) = Settings::load_from(&path);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(settings.backdrop_motion(false), Motion::default());
    std::fs::remove_dir_all(&dir).unwrap();
}

fn open_sans() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fontelle-ui/fonts/OpenSans-Regular.ttf"
    ))
    .unwrap()
}

fn a_moving_theme(name: &str) -> Theme {
    let mut theme = Theme::dark_default();
    theme.name = name.to_string();
    theme
        .backdrops
        .layers_mut(BackdropPanel::Window)
        .push(Layer::Shader(ShaderLayer::new(
            "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n\
             return vec4<f32>(uv, fract(bd.time), 1.0);\n}\n",
        )));
    theme
        .backdrops
        .layers_mut(BackdropPanel::Mixer)
        .push(Layer::Shader(ShaderLayer::new("builtin:aurora")));
    theme
        .fonts
        .push(ThemeFont::from_bytes(&open_sans()).unwrap());
    theme.font.family = "Open Sans".to_string();
    theme
}

#[test]
fn a_theme_with_shaders_and_a_font_imports_saves_and_renames_as_one_file() {
    let dir = scratch("one-file");
    let theme = a_moving_theme("Tide");
    let file = dir.join(format!("Tide.{THEME_EXTENSION}"));
    std::fs::write(&file, theme.to_json()).unwrap();

    let mut session = studio(&dir);
    assert_eq!(session.import_theme_from(&file).unwrap(), "Tide");
    assert_eq!(session.theme(), theme);
    // A second one by the same name keeps both.
    let library = ThemeLibrary::new(dir.join("themes"));
    assert_eq!(library.import(&file).unwrap(), "Tide 2");
    let second = library.load("Tide 2").unwrap();
    assert_eq!(second.backdrops, theme.backdrops);
    assert_eq!(second.fonts, theme.fonts);
    // And out again as one file that reads back whole.
    let out = dir.join("sent.fontelletheme");
    session.export_theme_to(&out).unwrap();
    assert_eq!(Theme::load_from_file(&out).unwrap(), theme);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn wearing_a_theme_whose_shader_does_not_compile_says_so_and_wears_it_anyway() {
    let dir = scratch("broken");
    let mut theme = Theme::dark_default();
    theme.name = "Cracked".to_string();
    theme
        .backdrops
        .layers_mut(BackdropPanel::Roll)
        .push(Layer::Shader(ShaderLayer::new(
            "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> { return oops; }",
        )));
    let file = dir.join(format!("Cracked.{THEME_EXTENSION}"));
    std::fs::write(&file, theme.to_json()).unwrap();
    let mut session = studio(&dir);
    session.import_theme_from(&file).unwrap();
    assert_eq!(
        session.theme().name,
        "Cracked",
        "the rest of the look is worn"
    );
    let said = session
        .take_settings_toast()
        .map(|(text, _)| text)
        .unwrap_or_default();
    assert!(said.contains("Piano roll"), "names the section: {said}");
    assert!(said.contains("oops"), "and naga's reason: {said}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_desktops_reduce_motion_switch_is_read_where_it_is_written() {
    use fontelle_app::desktop::{gnome_reduces_motion, kde_reduces_motion};
    // KDE: Settings ▸ Animation speed all the way to "Instant".
    let kde = "[General]\nColorScheme=BreezeDark\n\n[KDE]\nAnimationDurationFactor=0\n";
    assert_eq!(kde_reduces_motion(kde), Some(true));
    let kde = "[KDE]\nAnimationDurationFactor=0.5\nSingleClick=false\n";
    assert_eq!(kde_reduces_motion(kde), Some(false));
    assert_eq!(kde_reduces_motion("[General]\nfoo=1\n"), None);
    // Under another group the same key is not the desktop's.
    assert_eq!(
        kde_reduces_motion("[Other]\nAnimationDurationFactor=0\n"),
        None
    );
    // GNOME: Accessibility ▸ Reduce Animation.
    assert_eq!(gnome_reduces_motion("false\n"), Some(true));
    assert_eq!(gnome_reduces_motion("true\n"), Some(false));
    assert_eq!(gnome_reduces_motion("No such schema"), None);
}

fn a_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 4, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[200; 32]).unwrap();
    }
    out
}

/// Ty: *"the piano roll images stretch out. this should be configurable for
/// people making their themes how their images behaive"*. A section with a
/// picture gets three rows under it: how it fits, how big, and where.
#[test]
fn a_sections_picture_can_be_fitted_sized_and_placed_from_the_page() {
    let dir = scratch("placement");
    let picture = dir.join("wall.png");
    std::fs::write(&picture, a_png()).unwrap();
    let mut session = studio(&dir);
    assert!(
        !session
            .settings()
            .iter()
            .any(|r| r.name == "Piano roll picture fit"),
        "no picture, nothing to fit"
    );
    session
        .set_backdrop_from(BackdropPanel::Roll, Some(&picture))
        .unwrap();
    let fit = row(&session, "Piano roll picture fit");
    assert!(fit > row(&session, "Piano roll picture"));
    let SettingControl::Choice { options, .. } = session.setting_controls()[fit].clone() else {
        panic!()
    };
    assert_eq!(
        options,
        ["Cover", "Contain", "Stretch", "Tile", "Natural size"]
    );
    session.choose_setting(fit, 4);
    let place = row(&session, "Piano roll picture position");
    let SettingControl::Choice { options, .. } = session.setting_controls()[place].clone() else {
        panic!()
    };
    assert_eq!(options.len(), 9);
    let bottom_right = options.iter().position(|o| o == "Bottom right").unwrap();
    session.choose_setting(place, bottom_right);
    let size = row(&session, "Piano roll picture size");
    assert!(matches!(
        session.setting_controls()[size],
        SettingControl::Slider { .. }
    ));
    session.set_setting_fraction(size, 0.0);
    let b = session
        .theme()
        .backdrops
        .get(BackdropPanel::Roll)
        .cloned()
        .unwrap();
    assert_eq!(b.fit, fontelle_ui::theme::BackdropFit::Natural);
    assert_eq!(b.anchor, [1.0, 1.0]);
    assert!((b.scale - 0.1).abs() < 1e-3, "{}", b.scale);
    // Kept with the theme.
    let library = ThemeLibrary::new(dir.join("themes"));
    assert_eq!(
        library
            .load(&session.theme().name)
            .unwrap()
            .backdrops
            .get(BackdropPanel::Roll),
        Some(&b)
    );
    assert!(
        !session
            .settings()
            .iter()
            .any(|r| r.name == "Mixer picture fit"),
        "only the sections that have one"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
