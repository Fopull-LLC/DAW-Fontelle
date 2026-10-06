//! Starting Fontelle again, as itself, on the song that was open.
//!
//! Some settings change what the process starts with — Compatible plugin
//! graphics puts EGL on Mesa from the first line of `main` — so turning one
//! on means a restart. Ty: *"chose yes to enable it without having to go in
//! settings"*: the studio offers to do that restart itself. The window saves
//! (or asks about saving) as it would for a quit, the session leaves a
//! [`Relaunch`] here, and `main` starts it once the window is gone and
//! everything of the studio's own is written.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What to start: this binary, and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

static PENDING: Mutex<Option<Relaunch>> = Mutex::new(None);

/// The arguments that open `bundle` in the window, or none for the plain
/// studio — a song never saved has nothing on disk to reopen.
pub fn args_for(bundle: Option<&Path>) -> Vec<OsString> {
    match bundle {
        Some(bundle) => vec![
            OsString::from("--open"),
            bundle.as_os_str().to_owned(),
            OsString::from("--window"),
        ],
        None => Vec::new(),
    }
}

/// What starts this same binary on `bundle`, or why it cannot be known.
pub fn this_binary_on(bundle: Option<&Path>) -> Result<Relaunch, String> {
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(Relaunch {
        program,
        args: args_for(bundle),
    })
}

/// Leaves `relaunch` for `main` to start as the studio exits.
pub fn request(relaunch: Relaunch) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(relaunch);
}

/// What was left, taken.
pub fn take() -> Option<Relaunch> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Starts it, not waiting for it.
pub fn start(relaunch: &Relaunch) -> Result<(), String> {
    std::process::Command::new(&relaunch.program)
        .args(&relaunch.args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", relaunch.program.display()))
}
