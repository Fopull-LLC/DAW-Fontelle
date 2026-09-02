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

use fontelle_app::desktop::{elide_path, parse_picker_output, picker_candidates, reveal_command};

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
        run_picker(&[("/bin/false", Vec::new())]).expect("false is on every unix"),
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
