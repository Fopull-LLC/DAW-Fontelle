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
    with_face_keeping_settings(dir, gate)
}

/// [`with_face`] over whatever `settings.json` the last session left — the
/// next session on the same machine.
fn with_face_keeping_settings(dir: &Path, gate: EditorGate) -> Session {
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
    // And a window that opens offers nothing: Compatible plugin graphics is
    // asked about only when it would help.
    assert_eq!(session.session_question(), None);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------- asking, the first time
//
// Ty: *"we should make it detect if the user should need that setting
// enabled and ask them if they want it if weve never asked them, then theyll
// dismiss forever or chose yes to enable it without having to go in
// settings. im just worried about users not knowing it exists and just
// thinking the daw is broken."*

use fontelle_app::settings::offer_compatible_graphics;

#[test]
fn the_offer_is_made_only_when_it_would_help_and_nobody_has_answered() {
    // refused, on, never, asked, mesa
    assert!(offer_compatible_graphics(true, false, false, false, true));
    assert!(
        !offer_compatible_graphics(false, false, false, false, true),
        "nothing refused"
    );
    assert!(
        !offer_compatible_graphics(true, true, false, false, true),
        "already on"
    );
    assert!(
        !offer_compatible_graphics(true, false, true, false, true),
        "never again"
    );
    assert!(
        !offer_compatible_graphics(true, false, false, true, true),
        "asked this session"
    );
    assert!(
        !offer_compatible_graphics(true, false, false, false, false),
        "nothing to turn on"
    );
}

#[test]
fn dont_ask_again_is_written_down_and_an_older_file_has_never_answered() {
    let settings = Settings::default();
    assert!(!settings.never_ask_compatible_graphics);
    assert!(!settings.to_json().contains("never_ask_compatible_graphics"));
    let never = Settings {
        never_ask_compatible_graphics: true,
        ..Settings::default()
    };
    assert_eq!(Settings::from_json(&never.to_json()).unwrap(), never);
    const { assert!(SETTINGS_FORMAT_VERSION >= 11) };
}

#[test]
fn a_restart_reopens_the_same_song_in_the_window() {
    let args = fontelle_app::relaunch::args_for(Some(Path::new("/songs/My Song")));
    assert_eq!(
        args,
        vec![
            std::ffi::OsString::from("--open"),
            std::ffi::OsString::from("/songs/My Song"),
            std::ffi::OsString::from("--window"),
        ]
    );
    assert!(
        fontelle_app::relaunch::args_for(None).is_empty(),
        "a song never saved: the plain studio"
    );
}

fn refusing_gate() -> EditorGate {
    EditorGate::with(vec![FACE], false, || AlphaEgl::Fails {
        vendor: "NVIDIA".to_string(),
        why: "EGL_BAD_CONFIG".to_string(),
    })
}

fn with_mesa(session: Session) -> Session {
    session.with_mesa_egl(Some(PathBuf::from(
        "/usr/share/glvnd/egl_vendor.d/50_mesa.json",
    )))
}

#[test]
fn a_refused_window_asks_once_with_three_answers() {
    let dir = scratch("asks");
    let mut session = with_mesa(with_face(&dir, refusing_gate()));
    assert_eq!(session.session_question(), None);
    assert!(!open_any(&mut session));
    let question = session.session_question().expect("it asks");
    let said = question.lines.join(" ");
    assert!(said.contains("Compatible plugin graphics"), "{said}");
    assert!(said.contains("restarts"), "{said}");
    assert_eq!(
        question.buttons,
        ["Turn on and restart", "Don't ask again", "Not now"]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn not_now_asks_again_next_session_and_not_again_in_this_one() {
    let dir = scratch("not-now");
    let mut session = with_mesa(with_face(&dir, refusing_gate()));
    open_any(&mut session);
    session.answer_session_question(2).unwrap();
    assert_eq!(session.session_question(), None);
    open_any(&mut session);
    assert_eq!(session.session_question(), None, "not twice in a session");
    let (settings, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(!settings.never_ask_compatible_graphics);
    assert!(!settings.compatible_plugin_graphics);
    assert!(!session.take_restart_request());
    drop(session);

    // The next session — the same settings file.
    let mut again = with_mesa(with_face_keeping_settings(&dir, refusing_gate()));
    open_any(&mut again);
    assert!(again.session_question().is_some(), "asked again");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dont_ask_again_is_never_asked_again() {
    let dir = scratch("never");
    let mut session = with_mesa(with_face(&dir, refusing_gate()));
    open_any(&mut session);
    session.answer_session_question(1).unwrap();
    let (settings, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(settings.never_ask_compatible_graphics);
    assert!(!settings.compatible_plugin_graphics, "and it stays off");
    drop(session);
    let mut again = with_mesa(with_face_keeping_settings(&dir, refusing_gate()));
    open_any(&mut again);
    assert_eq!(again.session_question(), None);
    // The status line still says why the window did not open.
    let said = again.take_message().unwrap_or_default();
    assert!(said.contains("Compatible"), "{said}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn turn_on_and_restart_writes_the_setting_and_asks_the_window_to_restart() {
    let dir = scratch("turn-on");
    let mut session = with_mesa(with_face(&dir, refusing_gate()));
    open_any(&mut session);
    session.answer_session_question(0).unwrap();
    let (settings, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(settings.compatible_plugin_graphics, "on, in the file");
    assert_eq!(session.session_question(), None);
    assert!(
        session.take_restart_request(),
        "the window is asked to restart"
    );
    assert!(!session.take_restart_request(), "once");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn with_no_mesa_there_is_nothing_to_offer() {
    let dir = scratch("no-mesa");
    let mut session = with_face(&dir, refusing_gate()).with_mesa_egl(None);
    open_any(&mut session);
    assert_eq!(session.session_question(), None);
    std::fs::remove_dir_all(&dir).ok();
}
