//! The folder and file dialogs on Windows are the system's own, in-process.
//!
//! > *"anything that says click doesn't work"* — about the settings page,
//! > whose folder rows say *"Not set — click"*.
//!
//! On Windows those rows used to run PowerShell to put up a WinForms dialog:
//! seconds of the studio frozen while PowerShell loaded .NET, and then a
//! dialog belonging to another process, which Windows' focus rules put
//! **behind** the window that asked for it — a window that was frozen until
//! it was answered, and so got marked *Not Responding*. Now the dialog is the
//! same one Explorer uses, opened on the studio's own thread and owned by its
//! window, so it is in front of it and modal to it.
//!
//! The dialogs themselves can only be seen on Windows; the ignored test
//! below opens one for a person (or a driver under Wine) to answer.

#![cfg(windows)]

use fontelle_app::desktop::windows_filter;

#[test]
fn a_filter_is_a_name_and_a_pattern() {
    assert_eq!(
        windows_filter("*.json"),
        ("Files (*.json)".to_string(), "*.json".to_string())
    );
    // Nothing asked for is everything.
    assert_eq!(
        windows_filter(""),
        ("All files".to_string(), "*.*".to_string())
    );
}

/// Opens each dialog in turn. Run by hand, or under Wine with a driver
/// pressing Escape then Enter:
/// `wine windows_dialogs-*.exe --ignored --nocapture`.
#[test]
#[ignore = "opens a dialog for a person to answer"]
fn the_dialogs_open_and_answer() {
    let start = std::env::temp_dir();
    println!(
        "folder: {:?}",
        fontelle_app::desktop::choose_folder("Choose a folder", Some(&start))
    );
    println!(
        "open: {:?}",
        fontelle_app::desktop::choose_open_file("Open a pack", Some(&start), "*.json")
    );
    println!(
        "save: {:?}",
        fontelle_app::desktop::choose_save_file("Save the song", "song.mid", Some(&start))
    );
}
