//! Ty, 2026-10-02: *"can you make sure it lets users know that its out of
//! date if it is and they need to do that?"* An extension installed before
//! this Fontelle's release of it — the VST 2 bridge from before it took raw
//! MIDI — is said to be out of date when the studio opens, with where to
//! update it. Its own test binary, because it points `FONTELLE_BRIDGES` at a
//! scratch folder for the whole process.

mod common;

use fontelle_ui::document::StudioHost;

/// The two tests here each point `FONTELLE_BRIDGES` somewhere of their own,
/// and the variable is the process's: one at a time.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn an_out_of_date_extension_is_said_when_the_studio_opens() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = std::env::temp_dir().join(format!("fontelle-ext-notice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    unsafe {
        std::env::set_var("FONTELLE_BRIDGES", &scratch);
    }
    let vst2 = fontelle_app::extensions::find("vst2").unwrap();
    // An install from before installs recorded their version: the library
    // and nothing beside it. (Not a real bridge, so it never loads; the
    // notice is about the file, not the plugins.)
    let library = fontelle_app::extensions::installed_path(vst2).unwrap();
    std::fs::write(&library, b"not a real library").unwrap();

    let mut session = common::a_session_for(common::a_clip_project(1));
    session.announce_extension_notices();
    let said = session.take_message().unwrap_or_default();
    assert!(said.contains("out of date"), "{said:?}");
    assert!(said.contains("Settings"), "{said:?}");

    // A current one says nothing.
    std::fs::write(
        library.with_file_name(format!(
            "{}.version",
            library.file_name().unwrap().to_string_lossy()
        )),
        "0.2.0\n",
    )
    .unwrap();
    let mut session = common::a_session_for(common::a_clip_project(1));
    session.announce_extension_notices();
    assert!(
        !session
            .take_message()
            .unwrap_or_default()
            .contains("out of date")
    );

    unsafe {
        std::env::remove_var("FONTELLE_BRIDGES");
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// One line, so it follows whatever the launch already said rather than
/// replacing it: a note about how the last run ended keeps its place.
#[test]
fn the_notice_follows_what_the_launch_already_said() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let scratch =
        std::env::temp_dir().join(format!("fontelle-ext-notice-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    unsafe {
        std::env::set_var("FONTELLE_BRIDGES", &scratch);
    }
    let vst2 = fontelle_app::extensions::find("vst2").unwrap();
    let library = fontelle_app::extensions::installed_path(vst2).unwrap();
    std::fs::write(&library, b"not a real library").unwrap();

    let mut session = common::a_session_for(common::a_clip_project(1));
    session.announce("The last run was ended from outside");
    session.announce_extension_notices();
    let said = session.take_message().unwrap_or_default();
    assert!(said.starts_with("The last run"), "{said:?}");
    assert!(said.contains("out of date"), "{said:?}");

    unsafe {
        std::env::remove_var("FONTELLE_BRIDGES");
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The row's button updates an out-of-date install straight away. Only a
/// **remove** asks first — it cannot be taken back with a click — and an
/// update is not one (the press used to be held behind "Remove the VST 2
/// plugins extension?", which is not what Update says).
#[test]
fn update_does_not_ask_the_remove_question() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let scratch =
        std::env::temp_dir().join(format!("fontelle-ext-notice-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    unsafe {
        std::env::set_var("FONTELLE_BRIDGES", &scratch);
    }
    let vst2 = fontelle_app::extensions::find("vst2").unwrap();
    let library = fontelle_app::extensions::installed_path(vst2).unwrap();
    std::fs::write(&library, b"not a real library").unwrap();

    let session = common::a_session_for(common::a_clip_project(1));
    let rows = fontelle_app::settings::setting_rows(&fontelle_app::settings::Settings::default());
    let index = rows
        .iter()
        .position(|row| *row == fontelle_app::settings::SettingRow::Extension(0))
        .expect("the extension's row");
    assert_eq!(
        session.settings_confirm(index),
        None,
        "out of date: no question"
    );

    // Current, the button removes, and that asks.
    std::fs::write(
        library.with_file_name(format!(
            "{}.version",
            library.file_name().unwrap().to_string_lossy()
        )),
        "0.2.0\n",
    )
    .unwrap();
    assert!(session.settings_confirm(index).is_some());

    unsafe {
        std::env::remove_var("FONTELLE_BRIDGES");
    }
    let _ = std::fs::remove_dir_all(&scratch);
}
