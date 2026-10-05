//! The files a plugin loaded, travelling with the song.
//!
//! > `docs/plugin-experience-backlog.md` §9: a sampler keeps the path of the
//! > file it loaded in its saved state, so a song moved to another computer,
//! > or sent to a friend, opened with the sampler empty.
//!
//! An LV2 plugin's state says which of its values are paths (`atom:Path`;
//! `fontelle_host::lv2_state` maps them identity, so they are absolute).
//! That is enough to do for them what §17.4 does for Fontelle's own samples:
//!
//! - **Saved relative** when the file is inside the song's folder
//!   ([`for_saving`]), and made absolute again against wherever the song is
//!   now when it opens ([`for_opening`]) — so a song and its files move as
//!   one.
//! - **Collected** into `assets/plugin-files/` with the rest of the song's
//!   files ([`collect`]) — what sharing a song does, and what makes a copy a
//!   copy.
//! - **Named** when one is not there as the song opens.
//!
//! A CLAP or VST 3 state is the plugin's own bytes, with nothing in it the
//! host can tell is a path; those are left exactly as they are.

use std::path::{Path, PathBuf};

use fontelle_host::Lv2State;
use fontelle_model::Project;
use fontelle_types::{PluginFormat, PluginState};

const ATOM_PATH: &str = "http://lv2plug.in/ns/ext/atom#Path";

/// Every plugin state the song holds: channels' and inserts'.
fn states_mut(project: &mut Project) -> impl Iterator<Item = &mut PluginState> {
    let channels = project
        .channels
        .values_mut()
        .filter_map(|channel| channel.plugin.as_mut());
    let inserts = project
        .mixer
        .tracks
        .values_mut()
        .flat_map(|track| track.inserts.iter_mut())
        .filter_map(|insert| insert.plugin.as_mut());
    channels.chain(inserts)
}

/// Runs `map` over every path an LV2 state holds; a `Some` replaces it.
/// Whether anything changed.
fn map_paths(state: &mut PluginState, mut map: impl FnMut(&Path) -> Option<PathBuf>) -> bool {
    if state.key.format != PluginFormat::Lv2 {
        return false;
    }
    let Some(mut decoded) = state
        .blob
        .as_deref()
        .and_then(fontelle_types::decode_base64)
        .and_then(|bytes| Lv2State::decode(&bytes))
    else {
        return false;
    };
    let mut changed = false;
    for property in &mut decoded.properties {
        if property.type_uri != ATOM_PATH {
            continue;
        }
        let terminated = property.value.last() == Some(&0);
        let text = property.value.strip_suffix(&[0]).unwrap_or(&property.value);
        let Ok(text) = std::str::from_utf8(text) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let Some(mapped) = map(Path::new(text)) else {
            continue;
        };
        let mut value = mapped.to_string_lossy().into_owned().into_bytes();
        if terminated {
            value.push(0);
        }
        if value != property.value {
            property.value = value;
            changed = true;
        }
    }
    if changed {
        state.blob = Some(fontelle_types::encode_base64(&decoded.encode()));
    }
    changed
}

/// The song as it is written to `bundle`: every path inside the bundle made
/// relative to it. `None` when there is nothing to change, so the common
/// save clones nothing.
pub(crate) fn for_saving(project: &Project, bundle: &Path) -> Option<Project> {
    let mut copy = project.clone();
    let mut changed = false;
    for state in states_mut(&mut copy) {
        changed |= map_paths(state, |path| {
            path.strip_prefix(bundle).ok().map(Path::to_path_buf)
        });
    }
    changed.then_some(copy)
}

/// A song just read from `home`: every relative path made absolute against
/// it. Answers the files that are not there.
pub(crate) fn for_opening(project: &mut Project, home: &Path) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = Vec::new();
    for state in states_mut(project) {
        map_paths(state, |path| {
            let absolute = if path.is_relative() {
                home.join(path)
            } else {
                path.to_path_buf()
            };
            if !absolute.exists() && !missing.contains(&absolute) {
                missing.push(absolute.clone());
            }
            path.is_relative().then_some(absolute)
        });
    }
    missing
}

/// Copies every file a plugin's state names from outside `bundle` into
/// `bundle/assets/plugin-files/`, and points the state at the copy. Whether
/// anything was copied.
///
/// Named after the file, with a number when two differ — a sampler's file
/// list stays readable. A directory is left where it is: a whole sample
/// library is not collected by accident.
pub(crate) fn collect(project: &mut Project, bundle: &Path) -> Result<bool, String> {
    let folder = bundle.join("assets").join("plugin-files");
    let mut copied: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut failed: Option<String> = None;
    let mut any = false;
    for state in states_mut(project) {
        any |= map_paths(state, |path| {
            if failed.is_some() || path.starts_with(bundle) || !path.is_file() {
                return None;
            }
            if let Some((_, to)) = copied.iter().find(|(from, _)| from == path) {
                return Some(to.clone());
            }
            let name = path.file_name()?.to_string_lossy().into_owned();
            let mut target = folder.join(&name);
            let mut n = 2;
            while target.exists() && !same_bytes(path, &target) {
                target = folder.join(format!("{n} {name}"));
                n += 1;
            }
            if !target.exists() {
                let made = std::fs::create_dir_all(&folder).and_then(|()| {
                    // Beside and renamed, so a copy cut off half way is never
                    // mistaken for the file.
                    let part = target.with_extension("part");
                    std::fs::copy(path, &part)?;
                    std::fs::rename(&part, &target)
                });
                if let Err(e) = made {
                    failed = Some(format!("could not copy {}: {e}", path.display()));
                    return None;
                }
            }
            copied.push((path.to_path_buf(), target.clone()));
            Some(target)
        });
    }
    match failed {
        Some(why) => Err(why),
        None => Ok(any),
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
