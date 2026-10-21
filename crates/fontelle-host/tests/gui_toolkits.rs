//! A plugin's editor drawn by Qt or GTK, on a Wayland desktop.
//!
//! > *"having some issues with drumkv1 i closed it and opened it and its
//! > black now."*
//!
//! Every editor this host embeds is put in an X11 window. drumkv1's Qt,
//! inside a studio on a Wayland desktop, chose Qt's Wayland platform, and an
//! X11 window id means nothing to it: the editor came up as a window of its
//! own and the studio's stayed black — every time, the first included; the
//! first time it was the loose window that was seen. GTK's `GDK_BACKEND` is
//! the same question for a GTK editor.
//!
//! So the toolkits are put on X11 where there is an X display, and given back
//! the desktop's own choice for every program the studio starts.

#![cfg(target_os = "linux")]

use std::ffi::OsString;

use fontelle_host::gui::{as_the_desktop_had_it, toolkit_platforms_for};

#[test]
fn with_an_x_display_qt_and_gtk_are_put_on_it_whatever_the_desktop_said() {
    assert_eq!(
        toolkit_platforms_for(true),
        vec![("QT_QPA_PLATFORM", "xcb"), ("GDK_BACKEND", "x11")]
    );
}

#[test]
fn with_no_x_display_nothing_is_changed() {
    assert!(toolkit_platforms_for(false).is_empty());
}

/// A file picker, the file manager, a browser: each is the desktop's, and
/// runs as the desktop had it — a value it had set back, one it had not
/// taken away.
#[test]
fn a_program_the_studio_starts_gets_the_desktops_own_choice_back() {
    let originals: Vec<(&str, Option<OsString>)> = vec![
        ("QT_QPA_PLATFORM", Some("wayland".into())),
        ("GDK_BACKEND", None),
    ];
    let mut command = std::process::Command::new("true");
    as_the_desktop_had_it(&mut command, &originals);
    let envs: Vec<(OsString, Option<OsString>)> = command
        .get_envs()
        .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
        .collect();
    assert!(
        envs.contains(&("QT_QPA_PLATFORM".into(), Some("wayland".into()))),
        "{envs:?}"
    );
    assert!(envs.contains(&("GDK_BACKEND".into(), None)), "{envs:?}");
}
