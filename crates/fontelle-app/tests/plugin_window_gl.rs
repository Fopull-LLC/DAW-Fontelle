//! A plugin window that would take the studio down is refused, and the
//! setting that makes it draw.
//!
//! Reported (2026-10-05): Vital's editor, CLAP and VST 3, came up black and
//! aborted Fontelle — *"BGFX FATAL ... Failed to create surface"* — on
//! NVIDIA's EGL under X11. Mesa's EGL draws it. See `fontelle_host::alpha_egl`
//! for the probe, and `fontelle_host::gui::egl_vendor_for` for the setting.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::{SETTINGS_FORMAT_VERSION, SettingRow, Settings};
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_host::alpha_egl::{AlphaEgl, EditorGate, NeedsAlphaEgl};
use fontelle_types::{CompiledTimeline, PluginState};
use fontelle_ui::document::StudioHost;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-plugin-window-gl-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

// ------------------------------------------------------------ the setting

#[test]
fn the_setting_is_off_until_chosen_and_survives_a_round_trip() {
    let settings = Settings::default();
    assert!(!settings.compatible_plugin_graphics, "off by default");
    assert!(
        !settings.to_json().contains("compatible_plugin_graphics"),
        "an untouched file does not mention it"
    );
    let on = Settings {
        compatible_plugin_graphics: true,
        ..Settings::default()
    };
    let json = on.to_json();
    assert!(
        json.contains("\"compatible_plugin_graphics\": true"),
        "{json}"
    );
    assert_eq!(Settings::from_json(&json).unwrap(), on);
}

#[test]
fn the_settings_file_says_it_is_a_newer_one() {
    // An older build handed this file says "upgrade Fontelle" rather than
    // "unknown field".
    const { assert!(SETTINGS_FORMAT_VERSION >= 10) };
    // And an older file, without it, reads as off.
    let old = Settings::default().to_json().replace(
        &format!("\"format_version\": {SETTINGS_FORMAT_VERSION}"),
        "\"format_version\": 9",
    );
    assert!(old.contains("\"format_version\": 9"), "{old}");
    assert!(
        !Settings::from_json(&old)
            .expect("a version 9 file reads")
            .compatible_plugin_graphics
    );
}

#[test]
fn the_row_is_a_switch_under_plugins_on_linux_only() {
    let rows = fontelle_app::settings::setting_rows(&Settings::default());
    let at = rows
        .iter()
        .position(|r| *r == SettingRow::CompatibleGraphics);
    if !cfg!(target_os = "linux") {
        assert_eq!(at, None, "plugin windows are X11 only on Linux");
        return;
    }
    let at = at.expect("listed on Linux");
    let rescan = rows
        .iter()
        .position(|r| *r == SettingRow::RescanPlugins)
        .unwrap();
    assert_eq!(at, rescan + 1, "under the plugin rows");
    let row = SettingRow::CompatibleGraphics;
    assert_eq!(
        row.label(&Settings::default()),
        "Compatible plugin graphics"
    );
    assert_eq!(
        row.control_kind(),
        fontelle_app::settings::SettingControlKind::Switch
    );
    let help = row.help();
    assert!(help.contains("Mesa"), "{help}");
    assert!(help.contains("restart"), "{help}");
}

#[test]
fn the_value_says_when_it_takes_effect_and_why_it_cannot() {
    use fontelle_app::settings::compatible_graphics_value;
    assert_eq!(compatible_graphics_value(false, false, true), "Off");
    assert_eq!(compatible_graphics_value(true, true, true), "On");
    assert_eq!(
        compatible_graphics_value(true, false, true),
        "On after restart"
    );
    assert_eq!(
        compatible_graphics_value(false, true, true),
        "Off after restart"
    );
    assert_eq!(
        compatible_graphics_value(false, false, false),
        "Unavailable: Mesa's EGL is not installed"
    );
}

#[cfg(target_os = "linux")]
fn row_index(session: &Session) -> Option<usize> {
    session
        .settings()
        .iter()
        .position(|r| r.name == "Compatible plugin graphics")
}

#[cfg(target_os = "linux")]
#[test]
fn flipping_it_writes_it_down_and_says_it_applies_after_restart() {
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR))
        .with_mesa_egl(Some(PathBuf::from(
            "/usr/share/glvnd/egl_vendor.d/50_mesa.json",
        )));
    let index = row_index(&session).expect("the row");
    assert_eq!(
        session.setting_controls()[index],
        fontelle_ui::canvas::SettingControl::Switch { on: false }
    );
    session.nudge_setting(index, 1);
    assert_eq!(
        session.setting_controls()[index],
        fontelle_ui::canvas::SettingControl::Switch { on: true }
    );
    let (settings, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(settings.compatible_plugin_graphics, "it is in the file");
    let said = session.take_message().unwrap_or_default();
    assert!(said.contains("applies after restart"), "{said}");
    assert_eq!(session.settings()[index].detail, "On after restart");
}

#[cfg(target_os = "linux")]
#[test]
fn without_mesa_the_row_cannot_be_turned_on_and_says_why() {
    let mut session =
        common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR)).with_mesa_egl(None);
    let index = row_index(&session).expect("the row");
    assert_eq!(
        session.setting_controls()[index],
        fontelle_ui::canvas::SettingControl::Button {
            caption: String::new()
        },
        "nothing to press"
    );
    assert_eq!(
        session.settings()[index].detail,
        "Unavailable: Mesa's EGL is not installed"
    );
    session.nudge_setting(index, 1);
    let (settings, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(!settings.compatible_plugin_graphics);
}

// ------------------------------------------------------------ the refusal

/// The test bundle, alone in a folder — see `tests/plugin_ui.rs`.
fn plugin_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(if cfg!(target_os = "windows") {
        "fontelle_testplug.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    });
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testplug`",
        built.display()
    );
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = std::env::temp_dir().join("fontelle-app-plugin-window-gl-tests");
            let _ = std::fs::create_dir_all(&folder);
            let staging = folder.join(format!("staging.{}.tmp", std::process::id()));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, folder.join("fontelle-testplug.clap"));
            }
            let _ = std::fs::remove_file(&staging);
            folder
        })
        .clone()
}

/// The test face stands in for Vital: an editor of its own, in this gate's
/// table.
const FACE: NeedsAlphaEgl = NeedsAlphaEgl {
    name: "Fontelle Test Face",
    keys: &["clap:com.fopull.fontelle.testface"],
};

/// The face in an insert on the master track, editors on no screen, and
/// `gate` asked before one opens.
fn with_face(dir: &Path, gate: EditorGate) -> Session {
    std::fs::write(dir.join("settings.json"), Settings::default().to_json()).unwrap();
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let master = project.mixer.master.expect("a project has a master track");
    project.mixer.tracks[master]
        .inserts
        .push(fontelle_model::EffectSlot::hosting(PluginState::new(
            fontelle_types::PluginKey::clap("com.fopull.fontelle.testface"),
            "Face",
        )));
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
        fontelle_app::realise(&project, &library, options).expect("the project must realise");
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
    .with_settings_path(dir.join("settings.json"))
    .with_plugin_folders(vec![plugin_folder()])
    .with_headless_plugin_editors()
    .with_plugin_editor_gate(gate);
    session.pump();
    session
}

fn open_any(session: &mut Session) -> bool {
    (0..session.mixer_strips().len()).any(|strip| session.open_plugin_editor_for_insert(strip, 0))
}

/// The open is refused before the plugin is asked anything — its editor is
/// never created, so nothing of the plugin's runs — and the person is told
/// in words. The panel is what the window falls back to on `false`.
#[test]
fn an_editor_the_driver_cannot_draw_is_refused_before_the_plugin_is_called() {
    let dir = scratch("refused");
    let mut session = with_face(
        &dir,
        EditorGate::with(vec![FACE], false, || AlphaEgl::Fails {
            vendor: "NVIDIA".to_string(),
            why: "EGL_BAD_CONFIG".to_string(),
        }),
    );
    assert!(!open_any(&mut session), "no editor opened");
    let said = session.take_message().unwrap_or_default();
    assert_eq!(
        said,
        "Fontelle Test Face's window can't open with this graphics driver (NVIDIA on \
         X11). Its knobs are in Fontelle's panel. Turn on Settings \u{2192} Compatible \
         plugin graphics and restart to use its window."
    );
    assert_eq!(
        fontelle_host::guard::editor(std::time::Duration::from_secs(3600)),
        None,
        "the plugin's editor was never opened, so nothing was marked"
    );
    assert!(session.plugin_headers().is_empty(), "no window is up");
    std::fs::remove_dir_all(&dir).ok();

    // And where the driver can, the same editor opens. In this test rather
    // than its own, because opening one marks it, and the assertion above
    // is that nothing was marked.
    let dir = scratch("opens");
    let mut session = with_face(
        &dir,
        EditorGate::with(vec![FACE], false, || AlphaEgl::Works {
            vendor: "Mesa Project".to_string(),
        }),
    );
    assert!(open_any(&mut session), "the editor opened");
    std::fs::remove_dir_all(&dir).ok();
}
