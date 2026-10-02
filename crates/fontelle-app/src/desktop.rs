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
//! A machine with none is an ordinary GNOME, so on Linux the desktop portal
//! is asked after them, over the blocking `dbus` crate the engine's rtkit
//! request already brings — no runtime, nothing new in the graph.
//!
//! Everything except the `spawn` is a pure function, so the command shapes and
//! the answer-reading are tested rather than hoped for.

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
mod win_dialogs;

/// A Windows dialog's filter for `pattern` (`*.json`): the name it shows and
/// the pattern itself. Nothing asked for is every file.
pub fn windows_filter(pattern: &str) -> (String, String) {
    if pattern.trim().is_empty() {
        ("All files".to_string(), "*.*".to_string())
    } else {
        (format!("Files ({pattern})"), pattern.to_string())
    }
}

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

/// The programs that put text on the desktop's clipboard, in the order to
/// try them — each given the text on its standard input, never as an
/// argument. Wayland's own first: under XWayland an X11 tool writes a
/// clipboard the Wayland programs may never see.
pub fn copy_commands() -> Vec<(&'static str, Vec<String>)> {
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect();
    if cfg!(target_os = "macos") {
        vec![("pbcopy", Vec::new())]
    } else if cfg!(target_os = "windows") {
        vec![("clip", Vec::new())]
    } else {
        vec![
            ("wl-copy", Vec::new()),
            ("xclip", args(&["-selection", "clipboard"])),
            ("xsel", args(&["--clipboard", "--input"])),
        ]
    }
}

/// Puts `text` on the desktop's clipboard (the Share panel's Copy,
/// `docs/collab-plan.md` §10.1), through the first of [`copy_commands`] this
/// machine has.
///
/// Waits for it: `wl-copy` forks to serve the clipboard and returns at once,
/// and the others are done as soon as they have read their input.
pub fn copy_text(text: &str) -> Result<(), String> {
    use std::io::Write;
    for (program, args) in copy_commands() {
        let Ok(mut child) = Command::new(program)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }
    Err(NO_CLIPBOARD.to_string())
}

/// What a machine with none of [`copy_commands`] or [`paste_commands`] is
/// told: the package that brings the Wayland pair, which is what a Fedora
/// or Ubuntu desktop is.
const NO_CLIPBOARD: &str = "there is no clipboard program here (wl-copy, xclip or xsel) — \
     install wl-clipboard: `sudo dnf install wl-clipboard` or \
     `sudo apt install wl-clipboard`";

/// The programs that read the desktop's clipboard, in [`copy_commands`]'
/// order and one for each, printing the text on their standard output.
pub fn paste_commands() -> Vec<(&'static str, Vec<String>)> {
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect();
    if cfg!(target_os = "macos") {
        vec![("pbpaste", Vec::new())]
    } else if cfg!(target_os = "windows") {
        // `clip` only writes. PowerShell reads, told to answer in UTF-8
        // rather than the console's code page.
        vec![(
            "powershell",
            args(&[
                "-NoProfile",
                "-Command",
                "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-Clipboard -Raw",
            ]),
        )]
    } else {
        vec![
            ("wl-paste", args(&["--no-newline", "--type", "text"])),
            ("xclip", args(&["-selection", "clipboard", "-o"])),
            ("xsel", args(&["--clipboard", "--output"])),
        ]
    }
}

/// The text on the desktop's clipboard, for a field's Ctrl+V — so a share
/// code copied from a chat can be pasted.
pub fn paste_text() -> Result<String, String> {
    paste_text_with(&paste_commands())
}

/// [`paste_text`] with the programs handed in. One that is missing or fails
/// (`wl-paste` on a clipboard with no text in it, `xclip` with no X server)
/// passes to the next.
pub fn paste_text_with(commands: &[(&str, Vec<String>)]) -> Result<String, String> {
    let mut found_one = false;
    for (program, args) in commands {
        let mut command = Command::new(program);
        command.args(args).stdin(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console flashing up over the studio for a paste.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let Ok(output) = command.output() else {
            continue;
        };
        found_one = true;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
    }
    if found_one {
        Ok(String::new())
    } else {
        Err(NO_CLIPBOARD.to_string())
    }
}

/// The programs that put `message` up in a box titled `title`, best first.
/// None on Windows, where [`show_alert`] calls `MessageBoxW` itself.
pub fn alert_commands(title: &str, message: &str) -> Vec<(&'static str, Vec<String>)> {
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect();
    if cfg!(target_os = "windows") {
        Vec::new()
    } else if cfg!(target_os = "macos") {
        // Handed to AppleScript as arguments, never spliced into its source.
        vec![(
            "osascript",
            args(&[
                "-e",
                "on run argv",
                "-e",
                "display alert (item 1 of argv) message (item 2 of argv) as critical",
                "-e",
                "end run",
                title,
                message,
            ]),
        )]
    } else {
        vec![
            ("kdialog", args(&["--title", title, "--error", message])),
            (
                "zenity",
                args(&[
                    "--error",
                    "--no-markup",
                    &format!("--title={title}"),
                    &format!("--text={message}"),
                ]),
            ),
            (
                "notify-send",
                args(&["--app-name=Fontelle", title, message]),
            ),
        ]
    }
}

/// Puts `message` in front of somebody who may have no terminal — a studio
/// launched from the desktop's menu that is about to exit. Best effort: the
/// first program that starts is left to show it, and the caller does not
/// wait for anybody to read it, except on Windows, where the box is the
/// process's own and it waits for OK.
pub fn show_alert(title: &str, message: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let wide = |s: &str| {
            s.encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let (title, message) = (wide(title), wide(message));
        // SAFETY: both are NUL-terminated and outlive the call; no owner.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    for (program, args) in alert_commands(title, message) {
        let started = Command::new(program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if started.is_ok() {
            return;
        }
    }
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

/// The save-file pickers to try, best first — the counterpart to
/// [`picker_candidates`] for the one direction that folder picking cannot do:
/// asking the user *where to write a new file* and under *what name*.
///
/// `default_name` is the name the file is offered under (e.g. `song.mid`);
/// `start` is the folder to open in. The answer is read the same way a folder
/// pick is — [`parse_picker_output`], through [`run_picker`] — so all the
/// subprocess plumbing is shared and only this command-shaping is new.
pub fn save_file_candidates(
    title: &str,
    default_name: &str,
    start: Option<&Path>,
) -> Vec<(&'static str, Vec<String>)> {
    let start = start.map(|p| p.to_string_lossy().into_owned());

    if cfg!(target_os = "macos") {
        // `choose file name` errors on cancel, which `run_picker` already reads
        // as a cancel; the default location is only added when we have one.
        let script = match &start {
            Some(dir) => format!(
                "POSIX path of (choose file name with prompt \"{title}\" \
                 default name \"{default_name}\" \
                 default location POSIX file \"{dir}\")"
            ),
            None => format!(
                "POSIX path of (choose file name with prompt \"{title}\" \
                 default name \"{default_name}\")"
            ),
        };
        return vec![("osascript", vec!["-e".to_string(), script])];
    }

    if cfg!(target_os = "windows") {
        let dir = start.clone().unwrap_or_default();
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.SaveFileDialog; \
             $d.Title = '{title}'; \
             $d.FileName = '{default_name}'; \
             $d.InitialDirectory = '{dir}'; \
             if ($d.ShowDialog() -eq 'OK') {{ $d.FileName }}"
        );
        return vec![(
            "powershell",
            vec!["-NoProfile".to_string(), "-Command".to_string(), script],
        )];
    }

    let mut candidates: Vec<(&'static str, Vec<String>)> = Vec::new();

    // The full path the dialog opens on: the start folder, or the home
    // directory, with the offered name already filled in.
    let where_to = |sep: char| {
        let dir = start.clone().unwrap_or_else(home_dir);
        format!("{dir}{sep}{default_name}")
    };

    // KDE's own. `--getsavefilename <startpath>` preselects the name; the
    // trailing `*.mid` is the filter, which keeps the dialog on MIDI files.
    let kdialog = vec![
        "--title".to_string(),
        title.to_string(),
        "--getsavefilename".to_string(),
        where_to('/'),
        "*.mid".to_string(),
    ];
    candidates.push(("kdialog", kdialog));

    let zenity = vec![
        "--file-selection".to_string(),
        "--save".to_string(),
        "--confirm-overwrite".to_string(),
        format!("--title={title}"),
        format!("--filename={}", where_to('/')),
    ];
    candidates.push(("zenity", zenity));

    candidates
}

/// The command lines that ask for an **existing** file — a preset pack to
/// import — under `title`, opening in `start`, showing files matching
/// `filter` (`*.json`). [`save_file_candidates`]' programs, asked to open.
pub fn open_file_candidates(
    title: &str,
    start: Option<&Path>,
    filter: &str,
) -> Vec<(&'static str, Vec<String>)> {
    let start = start.map(|p| p.to_string_lossy().into_owned());

    if cfg!(target_os = "macos") {
        let script = match &start {
            Some(dir) => format!(
                "POSIX path of (choose file with prompt \"{title}\" \
                 default location POSIX file \"{dir}\")"
            ),
            None => format!("POSIX path of (choose file with prompt \"{title}\")"),
        };
        return vec![("osascript", vec!["-e".to_string(), script])];
    }

    if cfg!(target_os = "windows") {
        let dir = start.clone().unwrap_or_default();
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.OpenFileDialog; \
             $d.Title = '{title}'; \
             $d.Filter = 'Files ({filter})|{filter}'; \
             $d.InitialDirectory = '{dir}'; \
             if ($d.ShowDialog() -eq 'OK') {{ $d.FileName }}"
        );
        return vec![(
            "powershell",
            vec!["-NoProfile".to_string(), "-Command".to_string(), script],
        )];
    }

    let dir = start.unwrap_or_else(home_dir);
    vec![
        (
            "kdialog",
            vec![
                "--title".to_string(),
                title.to_string(),
                "--getopenfilename".to_string(),
                dir.clone(),
                filter.to_string(),
            ],
        ),
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                format!("--title={title}"),
                format!("--filename={dir}/"),
                format!("--file-filter={filter}"),
            ],
        ),
    ]
}

/// Asks the user for an existing file, **blocking** until they answer —
/// `Ok(None)` a cancel, `Err` a machine with no picker.
pub fn choose_open_file(
    title: &str,
    start: Option<&Path>,
    filter: &str,
) -> Result<Option<PathBuf>, String> {
    // The system's own, in-process, on Windows — see `win_dialogs`.
    #[cfg(windows)]
    return win_dialogs::ask(title, start, win_dialogs::Ask::Open { filter });
    #[cfg(not(windows))]
    ask_desktop(
        Pick::Open,
        &open_file_candidates(title, start, filter),
        title,
        start,
        filter,
    )
}

/// Asks the user where to save a new file, **blocking** until they answer.
///
/// `Ok(None)` is a cancel; `Err` is a machine with no picker at all. The
/// counterpart to [`choose_folder`], sharing its subprocess plumbing.
pub fn choose_save_file(
    title: &str,
    default_name: &str,
    start: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    return win_dialogs::ask(title, start, win_dialogs::Ask::Save { name: default_name });
    #[cfg(not(windows))]
    ask_desktop(
        Pick::Save,
        &save_file_candidates(title, default_name, start),
        title,
        start,
        default_name,
    )
}

/// What a picker is asked for, so a machine without one can be told which
/// kind it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Folder,
    Open,
    Save,
}

/// The home directory, spelled out: a picker is a program, not a shell, and
/// a `~` handed to one is a folder called `~`.
fn home_dir() -> String {
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .unwrap_or_else(|| "/".to_string())
}

/// A `file://` URI, as the desktop portal answers, read as a path.
pub fn portal_uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Anything before the path is the host, which for a local file is this one.
    let raw = &rest.as_bytes()[rest.find('/')?..];
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let escaped = (raw[i] == b'%')
            .then(|| raw.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                bytes.push(byte);
                i += 3;
            }
            None => {
                bytes.push(raw[i]);
                i += 1;
            }
        }
    }
    #[cfg(unix)]
    return Some(PathBuf::from(
        <std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(bytes),
    ));
    #[cfg(not(unix))]
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
}

/// The desktop portal's file chooser, `org.freedesktop.portal.FileChooser`:
/// the dialog a GNOME, a KDE or a Flatpak session draws itself, reached over
/// the session bus with nothing to install.
///
/// Tried only after kdialog and zenity, so a desktop that has one keeps the
/// dialog it has always had; this is for the one that has neither, which on
/// a fresh Fedora Workstation is the usual case.
#[cfg(target_os = "linux")]
mod portal {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::Duration;

    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::blocking::LocalConnection;
    use dbus::message::MatchRule;

    use super::Pick;

    /// A `Response` seen on the bus: the request it answers, its code, and
    /// the URIs picked.
    type Answer = (dbus::Path<'static>, u32, Vec<String>);

    /// `extra` is the offered name for a save, the filter for an open.
    pub fn ask(
        pick: Pick,
        title: &str,
        start: Option<&Path>,
        extra: &str,
    ) -> Result<Option<PathBuf>, String> {
        let bus = LocalConnection::new_session().map_err(|e| e.to_string())?;
        let token = format!("fontelle{}", std::process::id());

        // The answer is a signal on a request object, and every one seen is
        // kept until the call says which request is ours.
        let answers: Rc<RefCell<Vec<Answer>>> = Rc::default();
        let seen = Rc::clone(&answers);
        bus.add_match(
            MatchRule::new_signal("org.freedesktop.portal.Request", "Response"),
            move |(code, results): (u32, PropMap), _, message| {
                let uris = results
                    .get("uris")
                    .and_then(|v| v.0.as_iter())
                    .map(|list| {
                        list.filter_map(|u| u.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(path) = message.path() {
                    seen.borrow_mut().push((path.into_static(), code, uris));
                }
                true
            },
        )
        .map_err(|e| e.to_string())?;

        let mut options: PropMap = HashMap::new();
        let mut put = |key: &str, value: Box<dyn RefArg>| {
            options.insert(key.to_string(), Variant(value));
        };
        put("handle_token", Box::new(token));
        put("modal", Box::new(true));
        if let Some(dir) = start {
            // A byte string, NUL-terminated, as the portal's spec asks.
            let mut bytes = dir.as_os_str().as_encoded_bytes().to_vec();
            bytes.push(0);
            put("current_folder", Box::new(bytes));
        }
        match pick {
            Pick::Folder => put("directory", Box::new(true)),
            Pick::Save => put("current_name", Box::new(extra.to_string())),
            Pick::Open if !extra.is_empty() => put(
                "filters",
                Box::new(vec![(extra.to_string(), vec![(0u32, extra.to_string())])]),
            ),
            Pick::Open => {}
        }

        let method = if pick == Pick::Save {
            "SaveFile"
        } else {
            "OpenFile"
        };
        let proxy = bus.with_proxy(
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            Duration::from_secs(10),
        );
        let (handle,): (dbus::Path<'static>,) = proxy
            .method_call(
                "org.freedesktop.portal.FileChooser",
                method,
                ("", title, options),
            )
            .map_err(|e| e.to_string())?;

        loop {
            bus.process(Duration::from_millis(250))
                .map_err(|e| e.to_string())?;
            let found = answers.borrow().iter().find(|a| a.0 == handle).cloned();
            if let Some((_, code, uris)) = found {
                // 1 is a cancel, 2 a dialog closed some other way: neither
                // is an answer.
                return Ok((code == 0)
                    .then(|| uris.first().and_then(|u| super::portal_uri_path(u)))
                    .flatten());
            }
        }
    }
}

/// [`run_picker`] over `candidates`, and on Linux the desktop portal when
/// none of them is installed. The portal's own failure is not the message
/// worth showing: [`run_picker`]'s says what to install.
#[cfg(not(windows))]
fn ask_desktop(
    pick: Pick,
    candidates: &[(&str, Vec<String>)],
    title: &str,
    start: Option<&Path>,
    extra: &str,
) -> Result<Option<PathBuf>, String> {
    let answer = run_picker(pick, candidates);
    #[cfg(target_os = "linux")]
    if answer.is_err()
        && let Ok(found) = portal::ask(pick, title, start, extra)
    {
        return Ok(found);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (title, start, extra);
    answer
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
    #[cfg(windows)]
    return win_dialogs::ask(title, start, win_dialogs::Ask::Folder);
    #[cfg(not(windows))]
    ask_desktop(
        Pick::Folder,
        &picker_candidates(title, start),
        title,
        start,
        "",
    )
}

/// [`choose_folder`] with the candidate list handed in.
///
/// Split out so the subprocess half — spawn, read stdout, tell a cancel from an
/// answer, fall through a program that is not installed — can be driven with
/// `/bin/echo` and `/bin/false` instead of by clicking a dialog. Which picker
/// gets chosen is [`picker_candidates`]'s job and is tested separately.
pub fn run_picker(
    pick: Pick,
    candidates: &[(&str, Vec<String>)],
) -> Result<Option<PathBuf>, String> {
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
    // It said "no folder picker … start Fontelle with --soundfonts" to a
    // Fedora user asking to save a MIDI file: name what was asked for, and
    // the package that brings a picker.
    let what = if pick == Pick::Folder {
        "folder"
    } else {
        "file"
    };
    Err(format!(
        "no {what} picker on this machine (tried {}) — install zenity: \
         `sudo dnf install zenity` on Fedora, `sudo apt install zenity` on \
         Debian or Ubuntu",
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
    // Either separator: a Windows path split on `/` alone is one part, and
    // one part is never elided — which put the whole of
    // `C:\Users\…\AppData\Local\Temp\…` on a 248-pixel status line.
    let parts: Vec<&str> = text.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() <= keep {
        return text.into_owned();
    }
    let sep = std::path::MAIN_SEPARATOR;
    format!(
        "…{sep}{}",
        parts[parts.len() - keep..].join(&sep.to_string())
    )
}

// ------------------------------------------------------- reduce motion ---

/// Whether the desktop asks programs for less motion, where that can be
/// read: KDE's animation speed at "Instant", GNOME's *Reduce Animation*,
/// macOS's *Reduce motion*, Windows's *Show animations* off. `false` where
/// it cannot be read. Asked once, at launch: with Effects never chosen on
/// the settings page, it starts the theme's backdrops Still (hub card 0366).
pub fn reduce_motion() -> bool {
    #[cfg(target_os = "linux")]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home_dir()).join(".config"));
        if let Some(reduce) = std::fs::read_to_string(config.join("kdeglobals"))
            .ok()
            .and_then(|text| kde_reduces_motion(&text))
        {
            return reduce;
        }
        // Only where GNOME's settings are what the desktop reads: elsewhere
        // the key exists, unread, at whatever it was installed as.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if desktop.to_ascii_lowercase().contains("gnome")
            && let Ok(out) = Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "enable-animations"])
                .output()
        {
            return gnome_reduces_motion(&String::from_utf8_lossy(&out.stdout)).unwrap_or(false);
        }
        false
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("defaults")
            .args(["read", "com.apple.universalaccess", "reduceMotion"])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "1")
    }
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SystemParametersInfoW(
                action: u32,
                param: u32,
                value: *mut std::ffi::c_void,
                win_ini: u32,
            ) -> i32;
        }
        const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
        let mut on: i32 = 1;
        // SAFETY: the call writes one BOOL through a pointer to a live i32.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut on as *mut i32).cast(),
                0,
            )
        };
        ok != 0 && on == 0
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}

/// KDE's answer, from `kdeglobals`: `AnimationDurationFactor=0` under
/// `[KDE]` is the animation-speed slider at "Instant". `None` when the file
/// does not say.
pub fn kde_reduces_motion(kdeglobals: &str) -> Option<bool> {
    let mut in_kde = false;
    for line in kdeglobals.lines().map(str::trim) {
        if line.starts_with('[') {
            in_kde = line == "[KDE]";
            continue;
        }
        if !in_kde {
            continue;
        }
        if let Some(value) = line.strip_prefix("AnimationDurationFactor=") {
            return value.trim().parse::<f32>().ok().map(|f| f <= 0.0);
        }
    }
    None
}

/// GNOME's answer, from `gsettings get org.gnome.desktop.interface
/// enable-animations`: animations off is motion reduced.
pub fn gnome_reduces_motion(output: &str) -> Option<bool> {
    match output.trim() {
        "false" => Some(true),
        "true" => Some(false),
        _ => None,
    }
}
