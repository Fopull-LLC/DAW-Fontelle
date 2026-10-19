//! Fontelle installed by Steam (hub card 0429), and the Supporter Pack.
//!
//! Two promises are checked here. A copy Steam installed is the same program
//! as the one from the release page except that **it does not update
//! itself**: Steam owns the install folder, checks it file by file, and a
//! binary swapped in place by the start menu is a failed verification. And
//! the Supporter Pack is **content, never features**: a folder of soundfonts
//! the bank lists read-only, found beside the program (where Steam puts DLC)
//! or in Fontelle's data folder (where anybody can drop it), and nothing in
//! the program changes because it is there.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fontelle_app::distribution::{Distribution, STEAM_MARKER, install_dir};
use fontelle_app::settings::Settings;
use fontelle_app::supporter_pack::{MANIFEST, PACK_FOLDER, SupporterPack, find_packs, pack_places};
use fontelle_app::updates::Updater;
use fontelle_ui::{StudioHost, UpdateStatus};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-steam-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Everything under `dir`, relative to it, sorted — what an install folder
/// holds, for "nothing was written here".
fn listing(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A Linux or Windows install: the program at the top of its folder.
fn a_flat_install(root: &Path) -> PathBuf {
    let exe = root.join("Fontelle").join("fontelle");
    write(&exe, "the program");
    exe
}

/// A macOS install: the program inside `Fontelle.app`, which sits at the top
/// of the folder Steam installs into.
fn a_mac_install(root: &Path) -> PathBuf {
    let exe = root
        .join("Fontelle")
        .join("Fontelle.app")
        .join("Contents")
        .join("MacOS")
        .join("fontelle");
    write(&exe, "the program");
    exe
}

fn a_pack(dir: &Path, name: &str, soundfonts: &[&str]) {
    write(
        &dir.join(MANIFEST),
        &format!(r#"{{"format": 1, "name": "{name}", "version": "1.0.0"}}"#),
    );
    for file in soundfonts {
        write(&dir.join("soundfonts").join(file), "RIFF");
    }
}

// ------------------------------------------------------- which copy is it ---

#[test]
fn a_copy_without_the_marker_is_the_one_from_the_release_page() {
    let root = scratch("direct");
    let exe = a_flat_install(&root);
    assert_eq!(Distribution::of_install(&exe), Distribution::Direct);
    assert!(Distribution::Direct.updates_itself());
}

#[test]
fn the_marker_beside_the_program_says_steam_installed_it() {
    let root = scratch("steam-flat");
    let exe = a_flat_install(&root);
    write(&exe.with_file_name(STEAM_MARKER), "steam");
    assert_eq!(Distribution::of_install(&exe), Distribution::Steam);
    assert!(!Distribution::Steam.updates_itself());
}

#[test]
fn on_macos_the_marker_is_in_the_bundles_resources() {
    // Never in `Contents/MacOS`: codesign treats everything there as code,
    // and a text file in it is a bundle that will not sign.
    let root = scratch("steam-mac");
    let exe = a_mac_install(&root);
    assert_eq!(Distribution::of_install(&exe), Distribution::Direct);
    let resources = exe.parent().unwrap().parent().unwrap().join("Resources");
    write(&resources.join(STEAM_MARKER), "steam");
    assert_eq!(Distribution::of_install(&exe), Distribution::Steam);
}

#[test]
fn the_install_folder_is_the_programs_or_the_one_holding_the_app() {
    let root = scratch("install-dir");
    let flat = a_flat_install(&root.join("a"));
    assert_eq!(install_dir(&flat), flat.parent().map(Path::to_path_buf));
    let mac = a_mac_install(&root.join("b"));
    assert_eq!(install_dir(&mac), Some(root.join("b").join("Fontelle")));
}

// ------------------------------------------------- a copy Steam updates ---

#[test]
fn a_steam_copy_says_updates_come_through_steam_and_never_asks_github() {
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = Arc::clone(&asked);
    let updater = Updater::with_fetcher(Box::new(move |url: &str, _| {
        log.lock().unwrap().push(url.to_string());
        Err("no network in a test".to_string())
    }))
    .managed_by("Steam");
    let managed = UpdateStatus::ByStore {
        store: "Steam".to_string(),
    };
    assert_eq!(updater.status(), managed);
    updater.check();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(updater.status(), managed);
    assert!(asked.lock().unwrap().is_empty(), "{:?}", asked.lock());
}

#[test]
fn a_steam_copy_never_writes_beside_its_program() {
    // The start menu's button is gone in a Steam copy, but the call behind
    // it must be harmless too: the updater writes its download, a staging
    // folder and the `.old` binary into the install folder.
    let root = scratch("no-swap");
    let exe = a_flat_install(&root);
    let before = listing(&root);
    let updater = Updater::with_fetcher(Box::new(|_: &str, _| Ok(Vec::new()))).managed_by("Steam");
    updater.upgrade(exe.clone());
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(listing(&root), before);
    assert!(matches!(updater.status(), UpdateStatus::ByStore { .. }));
}

#[test]
fn a_steam_copys_settings_page_has_no_update_switches() {
    let labels = |session: &fontelle_app::Session| -> Vec<String> {
        session.settings().into_iter().map(|row| row.name).collect()
    };
    let direct = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    let direct_rows = labels(&direct);
    assert!(direct_rows.iter().any(|r| r == "Check at launch"));
    assert!(direct_rows.iter().any(|r| r == "Beta updates"));

    let steam = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR))
        .with_updater(Updater::new().managed_by("Steam"));
    let steam_rows = labels(&steam);
    assert!(!steam_rows.iter().any(|r| r == "Check at launch"));
    assert!(!steam_rows.iter().any(|r| r == "Beta updates"));
    assert!(!steam_rows.iter().any(|r| r == "Updates"));
    // Only those three: every other row is the same row.
    assert_eq!(steam_rows.len() + 3, direct_rows.len());
}

// --------------------------------------------------- the Supporter Pack ---

#[test]
fn the_pack_is_looked_for_beside_the_install_and_in_the_data_folder() {
    let root = scratch("places");
    let data = root.join("data");
    let flat = a_flat_install(&root.join("a"));
    assert_eq!(
        pack_places(Some(&flat), Some(&data)),
        vec![
            flat.parent().unwrap().join(PACK_FOLDER),
            data.join(PACK_FOLDER)
        ]
    );
    // Beside the app, never inside it: DLC added to a signed bundle is a
    // bundle whose signature no longer holds.
    let mac = a_mac_install(&root.join("b"));
    assert_eq!(
        pack_places(Some(&mac), None),
        vec![root.join("b").join("Fontelle").join(PACK_FOLDER)]
    );
    assert!(pack_places(None, None).is_empty());
}

#[test]
fn a_pack_is_a_folder_with_a_manifest() {
    let root = scratch("manifest");
    let pack = root.join(PACK_FOLDER);
    // Soundfonts with no manifest are somebody's folder, not a pack.
    write(&pack.join("soundfonts").join("Strings.sf2"), "RIFF");
    assert!(SupporterPack::read(&pack).is_none());
    a_pack(&pack, "Fontelle Supporter Pack", &["Strings.sf2"]);
    let read = SupporterPack::read(&pack).expect("a manifest makes it a pack");
    assert_eq!(read.name, "Fontelle Supporter Pack");
    assert_eq!(read.version, "1.0.0");
    assert_eq!(read.soundfonts(), pack.join("soundfonts"));
    assert_eq!(read.presets(), pack.join("presets"));
    assert_eq!(read.templates(), pack.join("templates"));
    // A manifest from a newer format is not guessed at.
    write(&pack.join(MANIFEST), r#"{"format": 99, "name": "Later"}"#);
    assert!(SupporterPack::read(&pack).is_none());
    write(&pack.join(MANIFEST), "not json");
    assert!(SupporterPack::read(&pack).is_none());
}

#[test]
fn both_places_are_found_and_the_same_folder_only_once() {
    let root = scratch("find");
    let a = root.join("install").join(PACK_FOLDER);
    let b = root.join("data").join(PACK_FOLDER);
    a_pack(&a, "Steam's", &["A.sf2"]);
    a_pack(&b, "Dropped in", &["B.sf2"]);
    let found = find_packs(&[a.clone(), root.join("nowhere"), b.clone(), a.clone()]);
    let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Steam's", "Dropped in"]);
}

#[test]
fn the_packs_soundfonts_are_in_the_bank_read_only_and_never_in_the_settings() {
    let root = scratch("bank");
    let mine = root.join("my soundfonts");
    write(&mine.join("Mine.sf2"), "RIFF");
    let pack_dir = root.join(PACK_FOLDER);
    a_pack(&pack_dir, "Fontelle Supporter Pack", &["Pack Strings.sf2"]);
    let pack = SupporterPack::read(&pack_dir).unwrap();

    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR))
        .with_supporter_packs(vec![pack]);
    session.add_soundfont_dir(&mine);
    session.open_bank();

    let bank = session.soundfont_bank();
    let names: Vec<_> = bank.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Mine", "Pack Strings"]);
    let theirs = pack_dir.join("soundfonts").join("Pack Strings.sf2");
    assert!(bank.is_read_only(&theirs));
    assert!(!bank.is_read_only(&mine.join("Mine.sf2")));
    // The pack is a folder of its own at the top, under its own name.
    assert!(
        bank.rows().iter().any(|row| matches!(
            row,
            fontelle_app::bank::BankRow::Folder { name, files: 1, .. }
                if name == "Fontelle Supporter Pack"
        )),
        "{:?}",
        bank.rows()
    );
    // The folders the person chose are still the ones the browser's buttons
    // act on, and the only ones written down: the pack's place is wherever
    // Steam put it this time.
    assert_eq!(session.library_dirs(), vec![mine.clone()]);
    let (saved, _) = Settings::load_from(session.settings_path().unwrap());
    assert_eq!(saved.soundfont_dirs, vec![mine.clone()]);
    // Changing the folders keeps the pack.
    session.set_library_dirs(vec![root.join("other")], true);
    assert!(session.soundfont_bank().is_read_only(&theirs));
    assert!(
        session
            .soundfont_bank()
            .entries()
            .iter()
            .any(|e| e.name == "Pack Strings")
    );
    let (saved, _) = Settings::load_from(session.settings_path().unwrap());
    assert!(
        !saved
            .soundfont_dirs
            .iter()
            .any(|d| d.starts_with(&pack_dir))
    );
}

#[test]
fn the_pack_is_content_never_features() {
    // Whatever its manifest claims: a pack lists soundfonts and nothing
    // else about the studio moves. The settings page and the start menu's
    // update line are the same with or without one.
    let root = scratch("content");
    let pack_dir = root.join(PACK_FOLDER);
    a_pack(&pack_dir, "Pack", &["P.sf2"]);
    write(
        &pack_dir.join(MANIFEST),
        r#"{"format": 1, "name": "Pack", "version": "1.0.0", "unlocks": ["everything"], "pro": true}"#,
    );
    let pack = SupporterPack::read(&pack_dir).expect("unknown fields are ignored, not obeyed");

    let without = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR));
    let with = common::a_session_for(common::a_project_with_a_clip(4, 120.0, common::SR))
        .with_supporter_packs(vec![pack]);
    let rows = |s: &fontelle_app::Session| -> Vec<(String, String)> {
        s.settings()
            .into_iter()
            .map(|row| (row.name, row.detail))
            .collect()
    };
    assert_eq!(rows(&with), rows(&without));
    assert_eq!(with.update_status(), without.update_status());
}

// ------------------------------------- nothing lands in the install folder ---

/// Steam starts a game in its install folder, so a relative path anywhere in
/// the chain from the environment to a folder is a file written into the
/// folder Steam verifies. Every rule that finds one of Fontelle's folders
/// answers an absolute path or nothing.
fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
    }
}

#[test]
fn a_relative_home_is_not_a_home_for_fontelles_folders() {
    use fontelle_app::settings::Platform;
    let env = env_of(&[("HOME", "relative/home")]);
    let never = |_: &Path| false;
    for platform in [Platform::Unix, Platform::Windows] {
        assert_eq!(Settings::config_dir_on(&env, platform, &never), None);
        assert_eq!(Settings::data_dir_on(&env, platform, &never), None);
        assert_eq!(Settings::cache_dir_on(&env, platform, &never), None);
    }
    assert_eq!(
        fontelle_host::fontelle_data_dir_from(&env, false, &never),
        None
    );
    assert_eq!(
        fontelle_host::fontelle_data_dir_from(&env, true, &never),
        None
    );
    // An absolute one still is.
    let env = env_of(&[("HOME", "/home/somebody")]);
    assert_eq!(
        Settings::config_dir_on(&env, Platform::Unix, &never),
        Some(PathBuf::from("/home/somebody/.config/fontelle"))
    );
}

#[test]
fn with_no_data_folder_voicebanks_go_to_the_temporary_folder_not_the_working_one() {
    let nothing = |_: &str| None::<String>;
    let dir =
        fontelle_vocal::voicebanks::voicebanks_dir_or_scratch_from(&nothing, false, &|_| false);
    assert!(dir.is_absolute(), "{}", dir.display());
    assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());
    let env = env_of(&[("XDG_DATA_HOME", "/data")]);
    assert_eq!(
        fontelle_vocal::voicebanks::voicebanks_dir_or_scratch_from(&env, false, &|_| false),
        PathBuf::from("/data/fontelle/voicebanks")
    );
}
