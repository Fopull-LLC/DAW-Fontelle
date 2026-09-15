//! The plugin folders another DAW already searches.
//!
//! > *"i want this to be a daw anyone could pick up and easily be able to use
//! > their tools in it so they could just point the plugins folder and stuff
//! > wherever they want"* — and, about FL Studio, *"sync it to their FL"*.
//!
//! What "sync" means in practice (`docs/vst-plan.md` §5): one folder both
//! programs read, and Fontelle finding it without being told. FL Studio
//! keeps its extra search folders in the registry, under
//! `HKCU\Software\Image-Line\Shared\Paths` as `VST plugins extra search
//! folder` (and `… 2`, `… 3` in newer versions). On Windows that is a `reg
//! query` away — through a child process for the reason the updater uses
//! `curl`: one call is not worth a crate. On Linux and macOS an FL Studio
//! lives in a Wine prefix, whose `user.reg` holds the same key in Wine's own
//! text form, and a Windows path in it is mapped back through the prefix's
//! drives. What the folder then holds is the user's business: under Wine
//! it is often the `.so` files `yabridge` made of Windows plugins, which is
//! exactly what a Linux Fontelle can load.
//!
//! Nothing here touches the settings. It answers *which folders*; the
//! settings row that asks adds them and rescans.

use std::path::{Path, PathBuf};

/// The registry key FL Studio keeps its search folders under, spelled the
/// way `reg query` wants it.
#[cfg_attr(not(windows), allow(dead_code))]
const FL_PATHS_KEY: &str = r"HKCU\Software\Image-Line\Shared\Paths";
/// The value name, which newer versions number: `… folder`, `… folder 2`.
const FL_VALUE_PREFIX: &str = "VST plugins extra search folder";

/// The folders FL Studio on this machine searches beyond the standard ones,
/// or an empty list when there is no FL Studio to ask.
pub fn fl_studio_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(output) = std::process::Command::new("reg")
            .args(["query", FL_PATHS_KEY])
            .output()
            && output.status.success()
        {
            folders.extend(fl_folders_from_reg_query(&String::from_utf8_lossy(
                &output.stdout,
            )));
        }
    }
    for prefix in wine_prefixes() {
        if let Ok(text) = std::fs::read_to_string(prefix.join("user.reg")) {
            folders.extend(fl_folders_from_user_reg(&text, &prefix));
        }
    }
    folders.retain(|folder| folder.is_dir());
    folders.dedup();
    folders
}

/// Where a Wine prefix might be: `WINEPREFIX`, and the default `~/.wine`.
fn wine_prefixes() -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    if let Some(set) = std::env::var_os("WINEPREFIX") {
        prefixes.push(PathBuf::from(set));
    }
    if let Some(home) = std::env::var_os("HOME") {
        prefixes.push(PathBuf::from(home).join(".wine"));
    }
    prefixes.dedup();
    prefixes
}

/// The folders in `reg query`'s printout of the key: every `REG_SZ` value
/// whose name is FL's, in the order printed.
pub fn fl_folders_from_reg_query(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, rest) = line.split_once("REG_SZ")?;
            let name = name.trim();
            if !name.starts_with(FL_VALUE_PREFIX) {
                return None;
            }
            let path = rest.trim();
            (!path.is_empty()).then(|| PathBuf::from(path))
        })
        .collect()
}

/// The folders in a Wine prefix's `user.reg`, mapped into `prefix`.
///
/// Wine writes a section per key, `[Software\\Image-Line\\Shared\\Paths]`,
/// and its values as `"name"="value"` with backslashes doubled. Only the
/// section for FL's key is read; a value whose drive the prefix does not
/// map is skipped rather than guessed.
pub fn fl_folders_from_user_reg(text: &str, prefix: &Path) -> Vec<PathBuf> {
    let wanted = r"[Software\\Image-Line\\Shared\\Paths]";
    let mut in_section = false;
    let mut folders = Vec::new();
    for line in text.lines() {
        if line.starts_with('[') {
            in_section = line.starts_with(wanted);
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"');
        if !name.starts_with(FL_VALUE_PREFIX) {
            continue;
        }
        let value = value.trim().trim_matches('"').replace("\\\\", "\\");
        if let Some(path) = windows_path_in_prefix(&value, prefix) {
            folders.push(path);
        }
    }
    folders
}

/// A Windows path as the prefix sees it: `C:` is `drive_c`, and every other
/// drive letter is the symlink under `dosdevices` Wine keeps for it.
pub fn windows_path_in_prefix(path: &str, prefix: &Path) -> Option<PathBuf> {
    let mut chars = path.chars();
    let drive = chars.next()?.to_ascii_lowercase();
    if !drive.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    let rest = chars.as_str().trim_start_matches(['\\', '/']);
    let rest = rest.replace('\\', "/");
    let root = if drive == 'c' {
        prefix.join("drive_c")
    } else {
        prefix.join("dosdevices").join(format!("{drive}:"))
    };
    Some(if rest.is_empty() {
        root
    } else {
        root.join(rest)
    })
}
