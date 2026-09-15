//! The start menu's *Recent* list, and where it comes from.
//!
//! A project is recent when it was last opened, made or saved under a name —
//! the three ways a bundle path enters a session. The list lives in the
//! settings file (it is about this machine, not about any project), newest
//! first, one entry per path, and short: it is a menu, not a history.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::Session;
use fontelle_app::settings::{RECENT_PROJECTS, SETTINGS_FORMAT_VERSION, Settings};
use fontelle_ui::StudioHost;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-recent-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

// --- the settings ---

#[test]
fn remembering_puts_the_newest_first_and_keeps_one_of_each() {
    let mut settings = Settings::default();
    settings.remember_project(Path::new("/p/One.fontelle"));
    settings.remember_project(Path::new("/p/Two.fontelle"));
    settings.remember_project(Path::new("/p/One.fontelle"));
    assert_eq!(
        settings.recent_projects,
        vec![
            PathBuf::from("/p/One.fontelle"),
            PathBuf::from("/p/Two.fontelle")
        ]
    );
}

#[test]
fn the_list_is_capped_at_a_menus_worth() {
    let mut settings = Settings::default();
    for i in 0..(RECENT_PROJECTS + 5) {
        settings.remember_project(Path::new(&format!("/p/{i}.fontelle")));
    }
    assert_eq!(settings.recent_projects.len(), RECENT_PROJECTS);
    // The newest survive, the oldest go.
    assert_eq!(
        settings.recent_projects[0],
        PathBuf::from(format!("/p/{}.fontelle", RECENT_PROJECTS + 4))
    );
}

#[test]
fn forgetting_drops_one_entry_and_nothing_else() {
    let mut settings = Settings::default();
    settings.remember_project(Path::new("/p/One.fontelle"));
    settings.remember_project(Path::new("/p/Two.fontelle"));
    settings.forget_project(Path::new("/p/Two.fontelle"));
    assert_eq!(
        settings.recent_projects,
        vec![PathBuf::from("/p/One.fontelle")]
    );
    settings.forget_project(Path::new("/p/Nope.fontelle"));
    assert_eq!(settings.recent_projects.len(), 1);
}

#[test]
fn a_settings_file_from_before_the_list_existed_still_reads() {
    // The field is new; the file on somebody's disk is not. And the update
    // check is on unless they said otherwise — a person who never opens the
    // tab gets told about a new release, which is what a start menu is for.
    let old = r#"{"format_version": 5, "soundfont_dirs": [], "projects_dir": null, "theme": null}"#;
    let settings = Settings::from_json(old).expect("an older file reads");
    assert!(settings.recent_projects.is_empty());
    assert!(settings.check_for_updates);
    const { assert!(SETTINGS_FORMAT_VERSION >= 6, "the format grew two fields") };
}

#[test]
fn the_list_and_the_switch_round_trip_through_the_file() {
    let mut settings = Settings::default();
    settings.remember_project(Path::new("/p/One.fontelle"));
    settings.check_for_updates = false;
    let back = Settings::from_json(&settings.to_json()).unwrap();
    assert_eq!(back, settings);
}

// --- the session ---

#[test]
fn making_opening_and_saving_as_all_make_a_project_recent() {
    let dir = scratch("session");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    session.set_projects_dir(Some(dir.clone()));

    session
        .new_project_named("First")
        .expect("a new project in a chosen folder");
    session.new_project_named("Second").expect("and another");
    let recent = session.recent_projects();
    assert_eq!(
        recent.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["Second", "First"],
        "newest first"
    );
    assert!(recent.iter().all(|r| r.exists));

    // Opening First again moves it to the top rather than listing it twice.
    let first = recent[1].path.clone();
    session
        .open_project_path(&first)
        .expect("a bundle that exists opens");
    let names: Vec<String> = session
        .recent_projects()
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, vec!["First", "Second"]);

    // And the list survives the session: it is in the settings file.
    let (settings, _) =
        Settings::load_from(session.settings_path().expect("a test session has one"));
    assert_eq!(settings.recent_projects.len(), 2);
    assert_eq!(settings.recent_projects[0], first);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_that_has_gone_is_listed_dead_not_dropped() {
    let dir = scratch("gone");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    session.set_projects_dir(Some(dir.clone()));
    session.new_project_named("Keep").unwrap();
    session.new_project_named("Lose").unwrap();
    let lose = session.recent_projects()[0].path.clone();
    std::fs::remove_dir_all(&lose).unwrap();

    let recent = session.recent_projects();
    assert_eq!(
        recent.len(),
        2,
        "still listed, so it can be forgotten on purpose"
    );
    assert!(!recent[0].exists);
    assert!(recent[1].exists);

    // Opening it says what happened rather than opening nothing.
    let err = session.open_project_path(&lose).unwrap_err();
    assert!(err.contains("Lose"), "{err}");
    // And forgetting it is the menu's own gesture.
    session.forget_recent(0);
    assert_eq!(session.recent_projects().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_window_reads_the_list_through_the_host_trait() {
    // The start menu is in `fontelle-ui`, which cannot see the settings
    // file; this is the seam it reads through.
    let dir = scratch("trait");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    session.set_projects_dir(Some(dir.clone()));
    session.new_project_named("Through the seam").unwrap();
    let host: &dyn StudioHost = &session;
    assert_eq!(
        StudioHost::recent_projects(host)[0].name,
        "Through the seam"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --- the switch, on the settings tab ---

#[test]
fn the_update_check_has_a_row_that_says_on_or_off() {
    use fontelle_app::settings::{SETTING_ROWS, SettingRow};
    let row = SettingRow::CheckForUpdates;
    assert!(
        SETTING_ROWS.contains(&row),
        "it has to be reachable from the tab"
    );
    let mut settings = Settings::default();
    assert_eq!(row.value(&settings), "On");
    settings.check_for_updates = false;
    assert_eq!(row.value(&settings), "Off");
}

#[test]
fn clicking_the_row_flips_the_switch_and_writes_it_down() {
    use fontelle_app::settings::{SettingRow, setting_rows};
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    // The window addresses rows by their position in the *displayed* list —
    // which now expands the Extensions heading into a row per catalogue
    // entry — so the test finds the switch the same way.
    let index = setting_rows(&Settings::default())
        .iter()
        .position(|r| *r == SettingRow::CheckForUpdates)
        .unwrap();
    assert!(session.checks_for_updates());
    session.nudge_setting(index, 1);
    assert!(!session.checks_for_updates());
    let (settings, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(!settings.check_for_updates, "it is in the file");
    // Either direction is a flip: a switch has two states, not a range.
    session.nudge_setting(index, -1);
    assert!(session.checks_for_updates());
}

// --- what the start menu asks before it prompts for a name ---

#[test]
fn the_start_menu_can_ask_whether_there_is_anywhere_to_put_a_new_project() {
    // > *"when making a new project from the start screen it doesnt prompt
    // > me to name it first"* — and a name is only worth asking for once
    // there is a folder for it to go in (INVARIANT 10: no folder is guessed).
    let dir = scratch("has-folder");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    assert!(!<Session as StudioHost>::has_projects_dir(&session));
    session.set_projects_dir(Some(dir.clone()));
    assert!(<Session as StudioHost>::has_projects_dir(&session));
    std::fs::remove_dir_all(&dir).ok();
}
