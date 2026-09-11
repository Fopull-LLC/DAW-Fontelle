//! The two things Fontelle asks the desktop to do: show a folder, and choose
//! one.
//!
//! # Why there is no file-dialog crate here
//!
//! The obvious answer is `rfd`. On Linux its default backend is GTK, which is
//! LGPL and therefore banned outright by `deny.toml` (TDD §3.4); its portal
//! backend needs an async runtime this workspace does not otherwise have, for
//! two dialogs. Running the desktop's own picker as a subprocess links nothing,
//! adds nothing to the dependency graph, works on every desktop that has one,
//! and says so clearly on a machine that has none.
//!
//! Everything except the `spawn` is a pure function, so the command shapes and
//! the answer-reading are tested rather than hoped for.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The program and arguments that show `dir` in the desktop's file manager.
pub fn reveal_command(dir: &Path) -> (&'static str, Vec<String>) {
    let path = dir.to_string_lossy().into_owned();
    if cfg!(target_os = "macos") {
        ("open", vec![path])
    } else if cfg!(target_os = "windows") {
        ("explorer", vec![path])
    } else {
        ("xdg-open", vec![path])
    }
}

/// Shows `dir` in the file manager, creating it first if it is not there.
///
/// Creating it is the point: the whole reason to press this button is that the
/// bank folder is empty and you want to put something in it, and a file manager
/// opening on a folder that does not exist is not an answer.
pub fn reveal(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let (program, args) = reveal_command(dir);
    Command::new(program)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {}: {program} {e}", dir.display()))
}

/// The program and arguments that open `url` in the default browser.
///
/// `xdg-open` and `open` take a URL as readily as a folder. Windows does not:
/// `explorer` shows folders, and a link goes through `cmd /c start` — with an
/// empty title first, because `start` reads its first quoted argument as a
/// window title and a URL is quoted.
pub fn open_url_command(url: &str) -> (&'static str, Vec<String>) {
    if cfg!(target_os = "macos") {
        ("open", vec![url.to_string()])
    } else if cfg!(target_os = "windows") {
        (
            "cmd",
            vec![
                "/c".to_string(),
                "start".to_string(),
                String::new(),
                url.to_string(),
            ],
        )
    } else {
        ("xdg-open", vec![url.to_string()])
    }
}

/// Opens `url` in the default browser.
///
/// Only a web address: the strings that reach here include a release's
/// `html_url` as GitHub returned it, and a program that hands whatever it
/// was given to `xdg-open` is a program that opens `file://` paths and runs
/// whatever `cmd` makes of a stray `&`. Anything that is not `http(s)://`
/// is refused before a process is spawned.
pub fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("{url:?} is not a web address"));
    }
    let (program, args) = open_url_command(url);
    Command::new(program)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {url}: {program} {e}"))
}

/// The application id the window announces and the desktop entry is named
/// by. **One string, used in three places** — the Wayland app id and the X11
/// `WM_CLASS` the window sets, the `.desktop` file's name and
/// `StartupWMClass`, and the icon's file name — because that is how a
/// compositor finds an icon for a window: it matches the id the window
/// announces to an entry of that name and takes the icon that entry names.
pub const APP_ID: &str = "com.fopull.Fontelle";

/// The desktop entry for the binary at `exe`.
///
/// The same words as `packaging/linux/com.fopull.Fontelle.desktop`, with the
/// full path in `Exec`: a desktop session does not always have `~/.local/bin`
/// on its PATH, and an entry that names a program it cannot find fails
/// silently.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Fontelle\n\
         GenericName=Digital Audio Workstation\n\
         Comment=A SoundFont-first digital audio workstation by Fopull LLC\n\
         Exec={}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;Music;\n\
         Keywords=DAW;music;soundfont;sf2;sequencer;synth;\n\
         StartupWMClass={APP_ID}\n",
        exe.display()
    )
}

/// Writes the desktop entry and the icon under `data` (the XDG data
/// directory) so the compositor can put a face on the window. `Ok(true)`
/// when something was written; `Ok(false)` when both were already as they
/// should be, which is every launch but the first and the one after a
/// rebuild.
///
/// **This is a write outside Fontelle's own directories**, and it is a
/// deliberate reading of INVARIANT 10 in the same spirit as the soundfont
/// bank: `~/.local/share/applications` and `~/.local/share/icons` are the
/// places the desktop *defines* for an application to say what it is, and
/// there is no other way for a Wayland window to have an icon at all —
/// Wayland has no per-window icon, only an app id the compositor looks up.
/// Without this, every `cargo run` shows the compositor's placeholder (a
/// yellow *W* on KDE), which is what was reported. Flag it to the owner if it
/// is the wrong reading; it is one call in `main`.
pub fn register_desktop_entry(data: &Path, exe: &Path, icon_png: &[u8]) -> Result<bool, String> {
    let entry = data.join("applications").join(format!("{APP_ID}.desktop"));
    let icon = data
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps")
        .join(format!("{APP_ID}.png"));
    let wanted = desktop_entry(exe);
    let mut wrote = false;
    if std::fs::read_to_string(&entry).ok().as_deref() != Some(wanted.as_str()) {
        write_file(&entry, wanted.as_bytes())?;
        wrote = true;
    }
    if std::fs::read(&icon).ok().as_deref() != Some(icon_png) {
        write_file(&icon, icon_png)?;
        wrote = true;
    }
    Ok(wrote)
}

/// The desktop's own tools to run after the entry or the icon has been
/// written, so a session that is already running notices.
///
/// > *"it looks like the icon for the app is still showing the yellow w"*
///
/// The entry and the icon were on disk and correct. What had not happened
/// was anybody telling the compositor and the panel — both started long
/// before the files existed, and both had already looked the app id up,
/// found nothing, and kept that answer. Three notices, each best-effort and
/// each harmless where it does not apply:
///
/// - `update-desktop-database` re-indexes the applications folder, which is
///   what the freedesktop shells read the entry through.
/// - `xdg-icon-resource forceupdate` touches the icon theme, which is the
///   change every toolkit's icon loader watches for.
/// - KDE's icon loader is told outright over D-Bus (`org.kde.KIconLoader`
///   `iconChanged`): KWin and Plasma keep an "icon not found" answer until
///   that signal, and nothing else clears it short of logging out.
pub fn desktop_refresh_commands(data: &Path) -> Vec<(&'static str, Vec<String>)> {
    vec![
        (
            "update-desktop-database",
            vec![data.join("applications").to_string_lossy().into_owned()],
        ),
        (
            "xdg-icon-resource",
            vec![
                "forceupdate".to_string(),
                "--theme".to_string(),
                "hicolor".to_string(),
            ],
        ),
        (
            "dbus-send",
            vec![
                "--session".to_string(),
                "--type=signal".to_string(),
                "/KIconLoader".to_string(),
                "org.kde.KIconLoader.iconChanged".to_string(),
                "int32:0".to_string(),
            ],
        ),
    ]
}

/// Runs [`desktop_refresh_commands`], quietly: a tool that is not installed
/// or a desktop that is not running is not a failure, and the window is
/// about to open either way.
pub fn refresh_desktop(data: &Path) {
    for (program, args) in desktop_refresh_commands(data) {
        let _ = std::process::Command::new(program)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The folder pickers to try, best first.
///
/// Ordered by how well each one fits in: a KDE session gets its own dialog, a
/// GNOME one gets zenity, and the AppleScript and PowerShell forms are there so
/// this does not become a Linux-only feature by accident.
pub fn picker_candidates(title: &str, start: Option<&Path>) -> Vec<(&'static str, Vec<String>)> {
    let start = start.map(|p| p.to_string_lossy().into_owned());

    if cfg!(target_os = "macos") {
        let script = match &start {
            Some(dir) => format!(
                "POSIX path of (choose folder with prompt \"{title}\" \
                 default location POSIX file \"{dir}\")"
            ),
            None => format!("POSIX path of (choose folder with prompt \"{title}\")"),
        };
        return vec![("osascript", vec!["-e".to_string(), script])];
    }

    if cfg!(target_os = "windows") {
        let root = start.clone().unwrap_or_default();
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = '{title}'; \
             $d.SelectedPath = '{root}'; \
             if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}"
        );
        return vec![(
            "powershell",
            vec!["-NoProfile".to_string(), "-Command".to_string(), script],
        )];
    }

    let mut candidates: Vec<(&'static str, Vec<String>)> = Vec::new();

    // KDE's own, which is what this project's own desktop has.
    let mut kdialog = vec![
        "--title".to_string(),
        title.to_string(),
        "--getexistingdirectory".to_string(),
    ];
    // Never an empty argument: `kdialog --getexistingdirectory ""` is not a
    // start directory, it is a parse error.
    kdialog.push(start.clone().unwrap_or_else(|| "/".to_string()));
    candidates.push(("kdialog", kdialog));

    let mut zenity = vec![
        "--file-selection".to_string(),
        "--directory".to_string(),
        format!("--title={title}"),
    ];
    // Zenity wants a trailing separator to read the value as a directory to
    // open rather than a file to preselect.
    zenity.push(format!(
        "--filename={}/",
        start.clone().unwrap_or_else(|| "/".to_string())
    ));
    candidates.push(("zenity", zenity));

    candidates
}

/// A picker's answer: the folder, or `None` for a cancel.
///
/// Every one of these prints a trailing newline, and zenity separates a
/// multiple selection with `|` even when only one was asked for.
pub fn parse_picker_output(stdout: &str) -> Option<PathBuf> {
    let first = stdout.split('|').next().unwrap_or_default().trim();
    (!first.is_empty()).then(|| PathBuf::from(first))
}

/// Asks the user for a folder, **blocking** until they answer.
///
/// `title` is what the dialog calls itself, and it is not decoration: every
/// picker here was titled "Soundfont folder" whatever it was asking for, so
/// the dialog that came up to choose a *projects* folder said in its own title
/// bar that it was choosing the soundfont one. That is half of *"if I select
/// change to set my projects folder it actually just changes my soundfonts
/// folder"* — the other half was a click handler that did not branch on the
/// panel's mode, and a wrong answer is much easier to believe when the dialog
/// agrees with it.
///
/// `Ok(None)` is a cancel. `Err` is a machine with no picker on it at all,
/// which is worth saying rather than looking like a cancel.
pub fn choose_folder(title: &str, start: Option<&Path>) -> Result<Option<PathBuf>, String> {
    run_picker(&picker_candidates(title, start))
}

/// [`choose_folder`] with the candidate list handed in.
///
/// Split out so the subprocess half — spawn, read stdout, tell a cancel from an
/// answer, fall through a program that is not installed — can be driven with
/// `/bin/echo` and `/bin/false` instead of by clicking a dialog. Which picker
/// gets chosen is [`picker_candidates`]'s job and is tested separately.
pub fn run_picker(candidates: &[(&str, Vec<String>)]) -> Result<Option<PathBuf>, String> {
    let mut tried = Vec::new();
    for (program, args) in candidates {
        match Command::new(program).args(args).output() {
            Ok(output) => {
                // A non-zero status is how all of these report a cancel.
                if !output.status.success() {
                    return Ok(None);
                }
                return Ok(parse_picker_output(&String::from_utf8_lossy(
                    &output.stdout,
                )));
            }
            // Not installed. Try the next one.
            Err(_) => tried.push(*program),
        }
    }
    Err(format!(
        "no folder picker on this machine (tried {}) — start Fontelle with \
         --soundfonts <folder> instead, and it will be remembered",
        tried.join(", ")
    ))
}

/// A path shortened from the left, keeping its last `keep` components.
///
/// The end of a path is the half worth showing in a 248-pixel panel:
/// `…/fontelle/soundfonts` says where you are, and
/// `/home/someone/.local/sha…` says nothing at all.
pub fn elide_path(path: &Path, keep: usize) -> String {
    let text = path.to_string_lossy();
    if text.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = text.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() <= keep {
        return text.into_owned();
    }
    format!("…/{}", parts[parts.len() - keep..].join("/"))
}
