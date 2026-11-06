//! Fontelle on Windows is a window, not a console program (hub card 0645).
//!
//! W, from v0.33.0: *"The PE subsystem of `fontelle.exe` is 3 (CONSOLE) … A
//! black console window opens behind the studio, and closing it kills
//! Fontelle."* And every console program the studio starts — `curl` for the
//! update check and the install ping, `tar`, `reg`, the plugin probe, the
//! ASIO helper — opens one of its own over a windowed parent unless it is
//! started with `CREATE_NO_WINDOW`.

use std::path::Path;

fn source(path: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )
    .unwrap_or_else(|e| panic!("{path}: {e}"))
    // The Windows runner's checkout ends lines `\r\n`.
    .replace("\r\n", "\n")
}

#[test]
fn the_binary_asks_windows_for_no_console() {
    let main = source("crates/fontelle-app/src/main.rs");
    assert!(
        main.contains("#![cfg_attr(windows, windows_subsystem = \"windows\")]"),
        "main.rs must say it is a windowed program"
    );
    // Run from a terminal, `--render-wav` and the rest still print there.
    assert!(
        main.contains("AttachConsole"),
        "main.rs attaches to the parent's console"
    );
}

/// Every program Fontelle starts goes through `gui::no_console` — directly,
/// or by way of `gui::as_the_desktop_has_it`, which calls it.
#[test]
fn every_program_fontelle_starts_is_started_without_a_console() {
    let host_gui = source("crates/fontelle-host/src/gui.rs");
    let desktop = host_gui
        .split_once("pub fn as_the_desktop_has_it")
        .expect("as_the_desktop_has_it")
        .1;
    let desktop = &desktop[..desktop.find("\n}\n").unwrap_or(desktop.len())];
    assert!(desktop.contains("no_console(command)"), "{desktop}");

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut missing = Vec::new();
    for dir in [
        "crates/fontelle-app/src",
        "crates/fontelle-host/src",
        "crates/fontelle-engine/src",
        "crates/fontelle-net/src",
    ] {
        for entry in walk(&root.join(dir)) {
            let text = std::fs::read_to_string(&entry).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if !line.contains("Command::new(") || line.trim_start().starts_with("//") {
                    continue;
                }
                let near = lines[i.saturating_sub(2)..(i + 10).min(lines.len())].join("\n");
                if !near.contains("no_console(") && !near.contains("as_the_desktop_has_it(") {
                    missing.push(format!("{}:{}", entry.display(), i + 1));
                }
            }
        }
    }
    assert!(missing.is_empty(), "started with a console: {missing:#?}");
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// On the Windows runner: the built `fontelle.exe` says subsystem 2
/// (`IMAGE_SUBSYSTEM_WINDOWS_GUI`) in its optional header.
#[cfg(windows)]
#[test]
fn the_built_exe_is_a_windows_gui_program() {
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_fontelle")).unwrap();
    assert_eq!(pe_subsystem(&bytes), Some(2));
}

/// The PE optional header's `Subsystem`: `e_lfanew` at 0x3C, then "PE\0\0",
/// the 20-byte file header, and `Subsystem` 68 bytes into the optional one.
#[allow(dead_code)]
fn pe_subsystem(bytes: &[u8]) -> Option<u16> {
    let at = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if bytes.get(at..at + 4)? != b"PE\0\0" {
        return None;
    }
    let field = at + 4 + 20 + 68;
    Some(u16::from_le_bytes(
        bytes.get(field..field + 2)?.try_into().ok()?,
    ))
}
