//! A desktop with no XSETTINGS manager, and the plugins that assume one.
//!
//! Reported: opening amsynth's editor (LV2, amsynth 2.0.0) took the studio
//! down — SIGSEGV in `amsynth_lv2ui.so`, at address 0x38, on the main
//! thread. On KDE Plasma under Wayland nobody owns `_XSETTINGS_S0` on
//! Xwayland, and amsynth 2.0.0 reads its scale off JUCE's XSETTINGS object
//! without asking whether there is one (fixed upstream after 2.0.0: *"Fix
//! crash if there are no XSETTINGS"*, amsynth issue #244). It reads
//! `GDK_SCALE` first and returns if it is set, so a studio that sets it —
//! to 1, which is what GTK uses on X11 when there are no XSETTINGS to say
//! otherwise — opens that editor in every amsynth 2.0.0 there is.

#![cfg(target_os = "linux")]

use fontelle_host::gui::{gdk_scale_for, xsettings_owner};
use x11rb::protocol::xproto::ConnectionExt as _;

#[test]
fn with_no_xsettings_manager_gtk_is_told_the_scale_it_would_use_anyway() {
    assert_eq!(gdk_scale_for(false, Some(false)), Some("1"));
}

#[test]
fn a_scale_somebody_set_is_theirs() {
    assert_eq!(gdk_scale_for(true, Some(false)), None);
    assert_eq!(gdk_scale_for(true, Some(true)), None);
}

#[test]
fn a_desktop_with_an_xsettings_manager_says_its_own_scale() {
    assert_eq!(gdk_scale_for(false, Some(true)), None);
}

#[test]
fn with_no_x_display_there_is_nothing_to_set() {
    assert_eq!(gdk_scale_for(false, None), None);
    assert_eq!(xsettings_owner(Some(":fontelle-no-such-display")), None);
}

/// Against the X server this runs under; skipped where there is none.
#[test]
fn the_owner_is_the_one_the_server_has() {
    let Ok((connection, screen)) = x11rb::connect(None) else {
        eprintln!("skipping: no X display");
        return;
    };
    let name = format!("_XSETTINGS_S{screen}");
    let atom = connection
        .intern_atom(false, name.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let owner = connection
        .get_selection_owner(atom)
        .unwrap()
        .reply()
        .unwrap()
        .owner;
    assert_eq!(xsettings_owner(None), Some(owner != 0));
}
