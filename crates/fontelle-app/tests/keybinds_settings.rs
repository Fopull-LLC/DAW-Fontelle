//! The keymap's home in the settings file.
//!
//! The map itself is `fontelle-ui`'s (`canvas::keymap`); what this side owns
//! is remembering it. The window hands over only what differs from the
//! defaults, as `(action id, chord text)` pairs, and reads the same pairs
//! back on the next launch — so a settings file holds a person's few changes
//! and not a copy of every default that would go stale the day a default
//! moved.

mod common;

use fontelle_app::settings::Settings;
use fontelle_ui::document::StudioHost;

fn scratch(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("fontelle-keybinds-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

#[test]
fn a_session_starts_with_no_overrides_and_keeps_what_it_is_handed() {
    let dir = scratch("keep");
    let path = dir.join("settings.json");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR))
        .with_settings_path(path.clone());
    assert!(session.keymap_overrides().is_empty());

    session.set_keymap_overrides(vec![
        ("play".to_string(), "Ctrl+P".to_string()),
        ("save".to_string(), String::new()),
    ]);
    assert_eq!(
        session.keymap_overrides(),
        vec![
            ("play".to_string(), "Ctrl+P".to_string()),
            ("save".to_string(), String::new()),
        ]
    );

    // Written at once, so the next launch has it without a save being asked
    // for — a shortcut you changed and lost on quitting would be worse than
    // one you could not change.
    let (back, error) = Settings::load_from(&path);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(
        back.keybinds.get("play").map(String::as_str),
        Some("Ctrl+P")
    );
    assert_eq!(back.keybinds.get("save").map(String::as_str), Some(""));

    // And a session opened on that file reads it back the same way.
    let again = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR))
        .with_settings_path(path);
    let mut read = again.keymap_overrides();
    read.sort();
    assert_eq!(
        read,
        vec![
            ("play".to_string(), "Ctrl+P".to_string()),
            ("save".to_string(), String::new()),
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_settings_file_written_before_keybinds_existed_still_reads() {
    let older =
        r#"{"format_version": 1, "soundfont_dirs": [], "projects_dir": null, "theme": null}"#;
    let settings = Settings::from_json(older).expect("a version 1 file still reads");
    assert!(settings.keybinds.is_empty());
}
