//! Where Fontelle looks for plugins, and what the settings tab says about it
//! (TDD §8.4, §17.5).

use std::path::PathBuf;

use fontelle_app::daw_folders::fl_folders_from_reg_query;
// Reading a Wine prefix (its dosdevices/drive_c layout) is a Unix-only
// scenario; on native Windows FL's folders come straight from `reg query`.
#[cfg(unix)]
use fontelle_app::daw_folders::{fl_folders_from_user_reg, windows_path_in_prefix};
use fontelle_app::settings::{
    SETTING_ROWS, SETTINGS_FORMAT_VERSION, SettingRow, Settings, setting_rows,
};

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
    let label = SettingRow::PluginDir(0).label(&settings);
    assert!(label.contains("CLAP"), "got {label:?}");
}

/// The list is **visible**: one row per folder under the add button, each
/// named by its end, each a button that removes it. Before this a second
/// folder was invisible except as a count (`docs/vst-plan.md` §5).
#[test]
fn every_folder_is_its_own_row_with_a_remove() {
    let settings = Settings {
        plugin_dirs: vec![
            PathBuf::from("/one/CLAP"),
            PathBuf::from("/two/VST3"),
            PathBuf::from("/three/Plugins"),
        ],
        ..Settings::default()
    };
    let rows = setting_rows(&settings);
    let add = rows
        .iter()
        .position(|r| *r == SettingRow::PluginFolder)
        .expect("the add button is still there");
    assert_eq!(rows[add + 1], SettingRow::PluginDir(0));
    assert_eq!(rows[add + 2], SettingRow::PluginDir(1));
    assert_eq!(rows[add + 3], SettingRow::PluginDir(2));
    assert!(SettingRow::PluginDir(1).label(&settings).contains("VST3"));
    let value = SettingRow::PluginDir(1).value(&settings);
    assert!(value.to_lowercase().contains("remove"), "got {value:?}");
    assert!(SettingRow::PluginDir(0).is_plugin_row());
    // With nothing set, no folder rows — and the add button's value says so.
    let none = setting_rows(&Settings::default());
    assert!(!none.iter().any(|r| matches!(r, SettingRow::PluginDir(_))));
    // The static skeleton plus one row per catalogue extension, and no
    // plugin-folder rows when none are set.
    assert_eq!(
        none.len(),
        SETTING_ROWS.len() + fontelle_app::extensions::CATALOGUE.len()
    );
}

#[test]
fn the_add_button_stops_counting_once_the_folders_are_listed() {
    let settings = Settings {
        plugin_dirs: vec![PathBuf::from("/one/CLAP"), PathBuf::from("/two/VST3")],
        ..Settings::default()
    };
    let value = SettingRow::PluginFolder.value(&settings);
    assert!(
        value.to_lowercase().contains("click"),
        "the rows below say which; the button says what it does: {value:?}"
    );
}

/// *"sync it to their FL"*: one row reads the folders FL Studio searches
/// and adds them (`docs/vst-plan.md` §5).
#[test]
fn the_settings_tab_offers_fl_studios_folders() {
    let rows = setting_rows(&Settings::default());
    assert!(rows.contains(&SettingRow::ImportFlFolders));
    assert!(SettingRow::ImportFlFolders.is_plugin_row());
    let label = SettingRow::ImportFlFolders.label(&Settings::default());
    assert!(label.contains("FL Studio"), "{label:?}");
}

// ---------------------------------------------- reading FL Studio's settings

/// FL Studio keeps its extra search folders in the registry under
/// `HKCU/Software/Image-Line/Shared/Paths`; on Windows `reg query` prints
/// them like this.
#[test]
fn fl_studios_folders_are_read_off_a_reg_query() {
    let output = "\r\nHKEY_CURRENT_USER\\Software\\Image-Line\\Shared\\Paths\r\n    VST plugins extra search folder    REG_SZ    D:\\Audio\\Plugins\r\n    VST plugins extra search folder 2    REG_SZ    C:\\Users\\ty\\VST\r\n    Something else    REG_DWORD    0x1\r\n\r\n";
    let folders = fl_folders_from_reg_query(output);
    assert_eq!(
        folders,
        vec![
            PathBuf::from("D:\\Audio\\Plugins"),
            PathBuf::from("C:\\Users\\ty\\VST"),
        ]
    );
}

/// The same key in a Wine prefix's `user.reg`, where a Windows path is
/// mapped back through the prefix's drives.
#[cfg(unix)]
#[test]
fn fl_studios_folders_are_read_off_a_wine_user_reg() {
    let text = r#"WINE REGISTRY Version 2
;; All keys relative to \\User\\S-1-5-21-0-0-0-1000

[Software\\Image-Line\\Shared\\Paths] 1699999999
#time=1da1b2c3d4e5f60
"VST plugins extra search folder"="Z:\\home\\ty\\plugins"
"VST plugins extra search folder 2"="C:\\VST"

[Software\\Something] 1
"x"="y"
"#;
    let prefix = PathBuf::from("/home/ty/.wine");
    let folders = fl_folders_from_user_reg(text, &prefix);
    assert_eq!(
        folders,
        vec![
            PathBuf::from("/home/ty/.wine/dosdevices/z:/home/ty/plugins"),
            PathBuf::from("/home/ty/.wine/drive_c/VST"),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_windows_path_is_mapped_into_the_prefix_by_its_drive() {
    let prefix = PathBuf::from("/p");
    assert_eq!(
        windows_path_in_prefix("C:\\Program Files\\VstPlugins", &prefix),
        Some(PathBuf::from("/p/drive_c/Program Files/VstPlugins"))
    );
    assert_eq!(
        windows_path_in_prefix("d:\\x", &prefix),
        Some(PathBuf::from("/p/dosdevices/d:/x"))
    );
    assert_eq!(windows_path_in_prefix("not a drive", &prefix), None);
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

/// The Extensions heading is in the tab, and one row per catalogue entry
/// sits under it (`docs/vst-plan.md` §4.2).
#[test]
fn the_settings_tab_lists_the_extensions_catalogue() {
    let rows = setting_rows(&Settings::default());
    let heading = rows
        .iter()
        .position(|r| *r == SettingRow::Heading("Extensions"))
        .expect("an Extensions heading");
    for (i, _) in fontelle_app::extensions::CATALOGUE.iter().enumerate() {
        assert_eq!(rows[heading + 1 + i], SettingRow::Extension(i));
    }
    // The vst2 row names the extension and offers to install it.
    let vst2 = SettingRow::Extension(0);
    assert!(vst2.label(&Settings::default()).contains("VST 2"));
    let value = vst2.value(&Settings::default());
    assert!(
        value.to_lowercase().contains("install") || value.to_lowercase().contains("newer"),
        "got {value:?}"
    );
}
