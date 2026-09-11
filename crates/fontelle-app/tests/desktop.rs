//! Reaching the desktop: opening a folder in the file manager, and asking the
//! user for one.
//!
//! Both are the same shape — pick a command for this platform, run it, read
//! what comes back — so the *choice* of command and the *reading* of the answer
//! are pure functions and are tested here. Only the `spawn` in the middle needs
//! a desktop, and it is three lines.
//!
//! **No new crate for this.** The obvious dependency is `rfd`, and on Linux its
//! default backend is GTK, which is LGPL and therefore banned by `deny.toml`
//! (TDD §3.4). Its portal backend needs an async runtime this project does not
//! otherwise have. Spawning the desktop's own picker costs nothing, links
//! nothing, and degrades to a clear message on a machine that has none.

use std::path::{Path, PathBuf};

use fontelle_app::desktop::{
    elide_path, open_url_command, parse_picker_output, picker_candidates, reveal_command,
};

#[test]
fn the_file_manager_is_opened_with_the_platforms_own_opener() {
    let (program, args) = reveal_command(Path::new("/music/sf2"));
    if cfg!(target_os = "linux") {
        assert_eq!(program, "xdg-open");
    } else if cfg!(target_os = "macos") {
        assert_eq!(program, "open");
    } else if cfg!(target_os = "windows") {
        assert_eq!(program, "explorer");
    }
    assert!(
        args.iter().any(|a| a == "/music/sf2"),
        "the folder has to actually be in the arguments: {args:?}"
    );
}

#[test]
fn there_is_more_than_one_folder_picker_to_try() {
    let candidates = picker_candidates("Soundfont folder", None);
    // On Linux, where which desktop this is cannot be assumed. macOS and
    // Windows each have one dialog and it is always there.
    #[cfg(target_os = "linux")]
    assert!(
        candidates.len() >= 2,
        "a desktop with no zenity is ordinary; there has to be a fallback"
    );
    // Every candidate has to actually name a program.
    for (program, _) in &candidates {
        assert!(!program.is_empty());
    }
}

#[test]
fn a_picker_is_started_in_the_folder_it_was_given() {
    let candidates = picker_candidates("Soundfont folder", Some(Path::new("/music/sf2")));
    assert!(
        candidates
            .iter()
            .all(|(_, args)| args.iter().any(|a| a.contains("/music/sf2"))),
        "a picker that opens in the home directory when we know where the bank \
         is makes the user navigate to it every time: {candidates:?}"
    );
    // And with nowhere to start, no candidate carries an empty path argument.
    let bare = picker_candidates("Soundfont folder", None);
    assert!(
        bare.iter()
            .all(|(_, args)| args.iter().all(|a| !a.is_empty()))
    );
}

#[test]
fn what_a_picker_prints_becomes_a_path_or_a_cancel() {
    assert_eq!(
        parse_picker_output("/music/sf2\n"),
        Some(PathBuf::from("/music/sf2")),
        "every one of these prints a trailing newline"
    );
    // zenity can return several selections separated by a pipe; the first is
    // the one that was asked for.
    assert_eq!(
        parse_picker_output("/music/sf2|/music/other\n"),
        Some(PathBuf::from("/music/sf2"))
    );
    // A cancel prints nothing at all.
    assert_eq!(parse_picker_output(""), None);
    assert_eq!(parse_picker_output("   \n"), None);
}

#[test]
fn a_long_path_is_shortened_from_the_left_so_the_end_stays_readable() {
    // A 248-pixel panel cannot show
    // `/home/someone/.local/share/fontelle/soundfonts`, and the half worth
    // showing is the end of it.
    let path = Path::new("/home/someone/.local/share/fontelle/soundfonts");
    assert_eq!(elide_path(path, 2), "…/fontelle/soundfonts");
    assert_eq!(elide_path(path, 3), "…/share/fontelle/soundfonts");

    // Short enough already: no ellipsis, and no lie about there being more.
    assert_eq!(elide_path(Path::new("/sf2"), 2), "/sf2");
    assert_eq!(elide_path(Path::new("/music/sf2"), 2), "/music/sf2");
    assert_eq!(elide_path(Path::new(""), 2), "");
}

/// The subprocess plumbing, driven with real programs.
///
/// Which picker gets chosen is tested above; this is the part in the middle —
/// spawn, read stdout, tell a cancel from an answer, fall through a program
/// that is not installed. It is the half that would otherwise only be checked
/// by clicking a dialog.
#[cfg(unix)]
#[test]
fn a_picker_that_prints_a_path_gives_a_path_and_one_that_fails_gives_a_cancel() {
    use fontelle_app::desktop::run_picker;

    // The answer comes back off stdout, trailing newline and all.
    let picked = run_picker(&[("/bin/echo", vec!["/music/sf2".to_string()])])
        .expect("echo is on every unix");
    assert_eq!(picked, Some(PathBuf::from("/music/sf2")));

    // A non-zero exit is how every one of these reports "the user cancelled".
    assert_eq!(
        // By name, not `/bin/false`: macOS keeps it in `/usr/bin`.
        run_picker(&[("false", Vec::new())]).expect("false is on every unix"),
        None
    );

    // A program that is not installed is not a cancel: try the next one.
    let fallen_through = run_picker(&[
        ("/definitely/not/installed", Vec::new()),
        ("/bin/echo", vec!["/music/sf2".to_string()]),
    ])
    .expect("the second candidate answers");
    assert_eq!(fallen_through, Some(PathBuf::from("/music/sf2")));

    // Nothing installed at all is an error worth showing, not silence.
    let nothing = run_picker(&[("/definitely/not/installed", Vec::new())]);
    assert!(nothing.is_err());
    assert!(
        nothing.unwrap_err().contains("--soundfonts"),
        "the message has to name the way out"
    );
}

#[test]
fn a_picker_says_which_folder_it_is_asking_for() {
    // Reported from using the window: *"if I select change to set my projects
    // folder and I select a folder it actually just changes my soundfonts
    // folder."* Half of that was a click handler that did not branch on the
    // panel's mode. The other half is here, and it is why it was *believable*:
    // every picker on every platform was titled "Soundfont folder", so the
    // dialog that came up to choose a projects folder said, in its own title
    // bar, that it was choosing the soundfont one.
    for title in ["Soundfont folder", "Projects folder"] {
        let candidates = picker_candidates(title, None);
        assert!(!candidates.is_empty());
        for (program, args) in &candidates {
            assert!(
                args.iter().any(|a| a.contains(title)),
                "{program} was not told it is asking for the {title}: {args:?}"
            );
        }
    }
}

#[test]
fn no_picker_is_titled_for_a_folder_it_is_not_asking_for() {
    // The specific failure, pinned: asking for the projects folder must not
    // put the word "soundfont" anywhere a person can read it.
    for (_, args) in picker_candidates("Projects folder", Some(Path::new("/music/projects"))) {
        for arg in &args {
            assert!(
                !arg.to_lowercase().contains("soundfont"),
                "the projects picker still says {arg:?}"
            );
        }
    }
}

#[test]
fn a_web_page_opens_in_the_desktops_own_browser() {
    // The start menu's footer links: fopull.com, the repository, a release
    // page. The same program that reveals a folder, handed a URL, on every
    // desktop but Windows — where `explorer` shows folders and `start` is
    // what opens a link.
    let (program, args) = open_url_command("https://fopull.com");
    assert!(args.iter().any(|a| a == "https://fopull.com"));
    if cfg!(target_os = "windows") {
        assert_eq!(program, "cmd");
    } else if cfg!(target_os = "macos") {
        assert_eq!(program, "open");
    } else {
        assert_eq!(program, "xdg-open");
    }
}

#[test]
fn only_a_web_address_is_handed_to_the_browser() {
    // Whatever ends up in a release's `html_url` field, it does not get run:
    // a string that is not `http(s)://` is refused before any program is
    // spawned.
    assert!(fontelle_app::desktop::open_url("file:///etc/passwd").is_err());
    assert!(fontelle_app::desktop::open_url("javascript:alert(1)").is_err());
    assert!(fontelle_app::desktop::open_url("-rf").is_err());
}

// --- the desktop entry, so a Wayland compositor can find the icon ---

#[test]
fn the_desktop_entry_names_this_binary_and_the_app_id() {
    use fontelle_app::desktop::{APP_ID, desktop_entry};
    let text = desktop_entry(Path::new("/opt/fontelle/fontelle"));
    assert!(text.starts_with("[Desktop Entry]\n"));
    assert!(text.contains("Exec=/opt/fontelle/fontelle\n"));
    assert!(text.contains(&format!("Icon={APP_ID}\n")));
    // What ties an X11 window to this entry; Wayland ties by the app id
    // the window itself announces.
    assert!(text.contains(&format!("StartupWMClass={APP_ID}\n")));
    assert_eq!(APP_ID, "com.fopull.Fontelle");
}

#[test]
fn registering_writes_the_entry_and_the_icon_once_and_leaves_them_alone_after() {
    use fontelle_app::desktop::{APP_ID, register_desktop_entry};
    let home = std::env::temp_dir().join(format!("fontelle-desktop-{}", std::process::id()));
    std::fs::remove_dir_all(&home).ok();
    let data = home.join("share");
    let exe = home.join("bin").join("fontelle");
    let icon = b"not really a png";

    let wrote = register_desktop_entry(&data, &exe, icon).expect("a writable data dir");
    assert!(wrote, "the first run writes");
    let entry = data.join("applications").join(format!("{APP_ID}.desktop"));
    let png = data
        .join("icons/hicolor/256x256/apps")
        .join(format!("{APP_ID}.png"));
    assert!(entry.is_file());
    assert_eq!(std::fs::read(&png).unwrap(), icon);
    let first = std::fs::metadata(&entry).unwrap().modified().unwrap();

    // The same binary again: nothing to do, and nothing touched.
    let wrote = register_desktop_entry(&data, &exe, icon).unwrap();
    assert!(!wrote);
    assert_eq!(
        std::fs::metadata(&entry).unwrap().modified().unwrap(),
        first
    );

    // A binary somewhere else — a new build, a new install — rewrites it.
    let other = home.join("elsewhere").join("fontelle");
    assert!(register_desktop_entry(&data, &other, icon).unwrap());
    assert!(
        std::fs::read_to_string(&entry)
            .unwrap()
            .contains("elsewhere")
    );
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn a_new_entry_or_icon_is_announced_to_the_desktop_that_is_already_running() {
    // > *"it looks like the icon for the app is still showing the yellow w"*
    //
    // The entry and the icon were on disk. A compositor and a shell that
    // were started before they existed had already looked the app id up,
    // found nothing, and kept that answer — so the placeholder stayed until
    // the next login. Writing the files is half of it; the other half is
    // telling the desktop its icon caches are stale.
    use fontelle_app::desktop::{APP_ID, desktop_refresh_commands};
    let data = Path::new("/home/someone/.local/share");
    let commands = desktop_refresh_commands(data);
    let names: Vec<&str> = commands.iter().map(|(name, _)| *name).collect();

    // The entry: the freedesktop database, which the shells read.
    let db = commands
        .iter()
        .find(|(name, _)| *name == "update-desktop-database")
        .expect("the applications folder is re-indexed");
    // Spelled the way this platform spells a path, so the test holds on
    // Windows too.
    let applications = data.join("applications").to_string_lossy().into_owned();
    assert!(db.1.contains(&applications), "{:?}", db.1);
    // The icon: the theme is touched so every toolkit's watcher fires...
    assert!(names.contains(&"xdg-icon-resource"), "{names:?}");
    // ...and KDE's own loader is told outright, because KWin and the panel
    // keep an "icon not found" answer until somebody says otherwise.
    let kde = commands
        .iter()
        .find(|(name, _)| *name == "dbus-send")
        .expect("KDE's icon loader is told");
    assert!(
        kde.1.iter().any(|a| a == "org.kde.KIconLoader.iconChanged"),
        "{:?}",
        kde.1
    );
    assert!(
        !names.iter().any(|n| n.contains(APP_ID)),
        "these are the desktop's own tools, not ours"
    );
}
