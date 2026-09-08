//! Where Fontelle looks for plugins, and what the settings tab says about it
//! (TDD §8.4, §17.5).

use std::path::PathBuf;

use fontelle_app::settings::{SETTING_ROWS, SETTINGS_FORMAT_VERSION, SettingRow, Settings};

#[test]
fn the_settings_tab_offers_a_plugin_folder_and_a_rescan() {
    assert!(
        SETTING_ROWS.contains(&SettingRow::PluginFolder),
        "there is nowhere to point Fontelle at a plugin folder"
    );
    assert!(SETTING_ROWS.contains(&SettingRow::RescanPlugins));
}

#[test]
fn with_no_folder_of_your_own_the_row_says_so_rather_than_being_blank() {
    let value = SettingRow::PluginFolder.value(&Settings::default());
    assert!(
        value.to_lowercase().contains("not set") || value.contains("choose"),
        "got {value:?}"
    );
}

#[test]
fn a_folder_that_is_set_is_named_by_its_end() {
    let settings = Settings {
        plugin_dirs: vec![PathBuf::from("/mnt/second-disk/Audio/CLAP")],
        ..Settings::default()
    };
    let value = SettingRow::PluginFolder.value(&settings);
    assert!(value.contains("CLAP"), "got {value:?}");
}

#[test]
fn several_folders_are_counted_rather_than_run_together() {
    let settings = Settings {
        plugin_dirs: vec![
            PathBuf::from("/one/CLAP"),
            PathBuf::from("/two/CLAP"),
            PathBuf::from("/three/CLAP"),
        ],
        ..Settings::default()
    };
    let value = SettingRow::PluginFolder.value(&settings);
    assert!(value.contains('3'), "got {value:?}");
}

#[test]
fn a_plugin_row_is_a_button_rather_than_a_value_to_step() {
    // Like the import folders: `folder()` is what tells the host a click opens
    // a picker, and these two are pressed rather than nudged.
    assert_eq!(SettingRow::PluginFolder.folder(), None);
    assert!(SettingRow::PluginFolder.is_plugin_row());
    assert!(SettingRow::RescanPlugins.is_plugin_row());
    assert!(!SettingRow::VelocityCurve.is_plugin_row());
}

#[test]
fn folders_survive_being_written_and_read_back() {
    let settings = Settings {
        format_version: SETTINGS_FORMAT_VERSION,
        plugin_dirs: vec![PathBuf::from("/mnt/second-disk/Audio/CLAP")],
        ..Settings::default()
    };
    let json = serde_json::to_string(&settings).unwrap();
    let read: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(read.plugin_dirs, settings.plugin_dirs);
}

#[test]
fn a_settings_file_written_before_plugins_could_be_hosted_still_opens() {
    let json = r#"{"format_version":1,"soundfont_dirs":[],"projects_dir":null,"theme":null}"#;
    let read: Settings = serde_json::from_str(json).unwrap();
    assert!(read.plugin_dirs.is_empty());
}
