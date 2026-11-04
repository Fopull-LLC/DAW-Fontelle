//! An installed update restarts the studio by itself, the moment nothing
//! would be lost by it.
//!
//! Ty, 2026-11-03 (hub card 0620): *"lets make it immediately automatically
//! restart after finishing installing instead of the user having to read
//! where it says restart to use and then do it, ive seen some users just
//! press install update and then go on with their day not realizing."*
//!
//! The rule itself (`canvas::update_restart_due`) is pinned in
//! `fontelle-ui/tests/welcome.rs`; these hold the window to it end to end.
//! One file, one lock: the relaunch waiting for `main` is process-wide.

mod common;

use std::sync::Mutex;

use fontelle_ui::document::DocumentHost;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn studio(session: fontelle_app::Session) -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
}

/// A studio whose update has just finished installing.
fn installed() -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(4))
        .with_updater(fontelle_app::updates::Updater::already_installed("9.9.9"))
}

#[test]
fn on_the_start_menu_it_restarts_as_soon_as_the_install_is_done() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    fontelle_app::relaunch::take();
    let mut w = studio(installed());
    w.open_welcome();
    w.tick();
    assert!(w.quitting(), "the window leaves by itself");
    let relaunch = fontelle_app::relaunch::take().expect("and leaves a relaunch for main");
    // The binary as it was launched — after an upgrade `current_exe()` names
    // the renamed-away `.old (deleted)`, which would start nothing.
    assert_eq!(
        Some(relaunch.program),
        fontelle_app::updates::launched_exe()
    );
}

#[test]
fn with_unsaved_changes_it_waits_and_asks_nothing() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    fontelle_app::relaunch::take();
    let mut session = installed();
    DocumentHost::set_tempo(&mut session, 97.0);
    let mut w = studio(session);
    w.tick();
    assert!(!w.quitting(), "unsaved work is never restarted away");
    assert!(!w.save_prompt_open(), "and no save prompt pops up over it");
    assert!(fontelle_app::relaunch::take().is_none());
}

#[test]
fn nothing_installed_nothing_restarts() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    fontelle_app::relaunch::take();
    let mut w = studio(common::a_session_for(common::a_clip_project(4)));
    w.open_welcome();
    w.tick();
    assert!(!w.quitting());
    assert!(fontelle_app::relaunch::take().is_none());
}

#[test]
fn any_restart_starts_the_binary_that_was_launched() {
    let relaunch = fontelle_app::relaunch::this_binary_on(None).expect("this binary");
    assert_eq!(
        Some(relaunch.program),
        fontelle_app::updates::launched_exe()
    );
}
