//! A file dragged in from the desktop, under Wayland.
//!
//! > *"im noticing that on my particular setup at least i still cannot drag
//! > in files from my file explorer. im on cacheyos with kde plasma i use
//! > dolphin as my file browser."*
//!
//! `winit` 0.30's Wayland backend has no file drag-and-drop at all — only
//! its X11 backend speaks XDND — so on a Plasma Wayland session a drag out
//! of Dolphin never reached the window. `fontelle_ui::file_drag` speaks the
//! `wl_data_device` protocol itself, on the connection winit is using. The
//! protocol half needs a compositor and is looked at on a nested KWin; this
//! is the pure half — what the source hands over is a `text/uri-list`, and
//! turning that into paths has enough edges to be worth holding.

use std::path::PathBuf;

use fontelle_ui::file_drag::paths_of_uri_list;

#[test]
fn a_uri_list_is_one_file_per_line() {
    assert_eq!(
        paths_of_uri_list("file:///home/ty/Music/kick.wav\r\nfile:///home/ty/Music/snare.wav\r\n"),
        vec![
            PathBuf::from("/home/ty/Music/kick.wav"),
            PathBuf::from("/home/ty/Music/snare.wav")
        ]
    );
    // Bare newlines too: the RFC says CRLF, and not everything read it.
    assert_eq!(
        paths_of_uri_list("file:///a.wav\nfile:///b.wav"),
        vec![PathBuf::from("/a.wav"), PathBuf::from("/b.wav")]
    );
}

#[test]
fn spaces_and_unicode_come_back_out_of_their_percent_escapes() {
    assert_eq!(
        paths_of_uri_list("file:///home/ty/My%20Loops/Doll%20Break%20120.wav"),
        vec![PathBuf::from("/home/ty/My Loops/Doll Break 120.wav")]
    );
    assert_eq!(
        paths_of_uri_list("file:///home/ty/%C3%A9t%C3%A9.wav"),
        vec![PathBuf::from("/home/ty/été.wav")]
    );
    // An escape that is not one is kept as it is rather than eaten.
    assert_eq!(
        paths_of_uri_list("file:///x/100%25.wav\nfile:///x/50%.wav\nfile:///x/%zz.wav"),
        vec![
            PathBuf::from("/x/100%.wav"),
            PathBuf::from("/x/50%.wav"),
            PathBuf::from("/x/%zz.wav")
        ]
    );
}

#[test]
fn comments_blank_lines_and_other_schemes_are_left_out() {
    assert_eq!(
        paths_of_uri_list(
            "# dragged from Dolphin\r\n\r\nfile:///a.wav\r\nhttps://example.com/b.wav\r\nsmb://nas/c.wav\r\n   \r\n"
        ),
        vec![PathBuf::from("/a.wav")]
    );
    assert!(paths_of_uri_list("").is_empty());
}

#[test]
fn a_localhost_authority_is_this_machine_and_another_host_is_not() {
    assert_eq!(
        paths_of_uri_list("file://localhost/a.wav\nfile:///b.wav\nfile://otherbox/c.wav"),
        vec![PathBuf::from("/a.wav"), PathBuf::from("/b.wav")],
        "a file on another machine is not a path here"
    );
}
