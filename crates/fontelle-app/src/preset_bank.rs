//! Every preset on this machine, factory and user (`docs/flopsynth-plan.md`
//! §P.4).
//!
//! Modelled on [`crate::bank::FileBank`] rather than copied from it: the same
//! folder walk, the same list of what could not be read, the same
//! [`fuzzy_score`](crate::bank::fuzzy_score) behind the search. What is
//! different is that this bank has **two origins**, and almost every rule here
//! follows from that one fact.
//!
//! - **Factory presets are in the binary.** `build.rs` walks
//!   `assets/presets/` at compile time and writes a list of `(path, contents)`
//!   pairs, so a fresh install has presets with nothing to unpack. They are
//!   parsed once, at construction, because they are already in memory.
//! - **User presets are files somebody owns.** They are found by walking
//!   `Settings::user_preset_dir`, parsed on [`load`](PresetBank::load) and
//!   cached by modification time — the bar asks for one per device per
//!   revision to decide whether to draw its `*`, and re-reading a file for
//!   every repaint would be a disk in a loop.
//! - **A user preset named like a factory one is a second row**, tagged, not a
//!   shadow. The two are different files, and hiding one would be a preset
//!   that vanished the day somebody reused a name.
//!
//! The layout is `<root>/<device slug>/<category>/<name>.json` under both
//! origins, so a category *is* a folder and "Save as…" can make one. That is
//! also why a name that is a path is refused here rather than at the button:
//! this is the only place that knows what a name is about to become.

use fontelle_types::{DeviceKind, Preset, PresetOrigin, PresetRef};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

include!(concat!(env!("OUT_DIR"), "/factory_presets.rs"));

/// One preset the bank knows about, without its payload.
///
/// The payload is deliberately absent: a bank holds thousands of rows and a
/// browser draws forty of them, so the list is names and the sound is read
/// when somebody asks for it. The one exception is the factory bank, whose
/// files are in the binary anyway.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetEntry {
    pub device: DeviceKind,
    pub name: String,
    pub category: String,
    pub origin: PresetOrigin,
    /// Where it came from: a file for a user preset, and the path it was
    /// embedded under for a factory one. Never opened for a factory entry —
    /// it is an identity, and the one the browser's "reveal in folder" row is
    /// drawn disabled for.
    pub path: PathBuf,
}

impl PresetEntry {
    /// What a device stores to remember it was loaded from this.
    pub fn reference(&self) -> PresetRef {
        PresetRef::new(self.name.clone(), self.category.clone(), self.origin)
    }
}

/// How deep the user's own folder is walked.
///
/// Two levels is the layout — device, then category — and one more so that a
/// person who has nested a category inside a category still sees their files
/// rather than silently losing them. Bounded for `FileBank::rescan`'s reason:
/// a symlink loop, or a folder somebody pointed at their home directory,
/// must not turn a scan into a hang.
const MAX_DEPTH: usize = 4;

pub struct PresetBank {
    user_dir: Option<PathBuf>,
    entries: Vec<PresetEntry>,
    /// The factory presets, parsed once. Indexed the same way `entries` is —
    /// by the entry's `path`, which for a factory entry is its embedded one.
    factory: HashMap<PathBuf, Preset>,
    unreadable: Vec<(PathBuf, String)>,
    /// User presets read from disk, by path, with the modification time they
    /// were read at. A file that has been written since is read again.
    cache: RefCell<HashMap<PathBuf, (Option<SystemTime>, Preset)>>,
}

impl PresetBank {
    /// The factory bank, plus whatever is in `user_dir` if there is one.
    ///
    /// `None` is a real state and not an error: a machine with no writable
    /// data directory can still *browse* everything that ships, and only
    /// saving is refused.
    pub fn new(user_dir: Option<PathBuf>) -> Self {
        let mut bank = Self {
            user_dir,
            entries: Vec::new(),
            factory: HashMap::new(),
            unreadable: Vec::new(),
            cache: RefCell::new(HashMap::new()),
        };
        bank.read_factory();
        bank.rescan();
        bank
    }

    pub fn user_dir(&self) -> Option<&Path> {
        self.user_dir.as_deref()
    }

    /// Points the bank at a different folder and reads it.
    pub fn set_user_dir(&mut self, dir: Option<PathBuf>) {
        self.user_dir = dir;
        self.rescan();
    }

    pub fn entries(&self) -> &[PresetEntry] {
        &self.entries
    }

    pub fn unreadable(&self) -> &[(PathBuf, String)] {
        &self.unreadable
    }

    /// Reads the user's folder again, keeping the factory bank as it is.
    ///
    /// The factory bank cannot change while the program runs — it is in the
    /// binary — so a rescan that re-parsed it would be work for nothing.
    pub fn rescan(&mut self) {
        self.entries.retain(|e| e.origin == PresetOrigin::Factory);
        self.unreadable
            .retain(|(path, _)| self.factory.contains_key(path));
        self.cache.borrow_mut().clear();
        let Some(root) = self.user_dir.clone() else {
            return;
        };
        let mut found = Vec::new();
        walk(&root, &root, 0, &mut found, &mut self.unreadable);
        found.sort_by_key(order);
        self.entries.extend(found);
    }

    /// Every preset for one device, factory first and then the user's own,
    /// each run ordered by category and then by name.
    pub fn for_device(&self, device: &DeviceKind) -> Vec<&PresetEntry> {
        self.entries
            .iter()
            .filter(|e| e.device == *device)
            .collect()
    }

    /// The categories this device has presets in, in order and without
    /// repeats. What the Presets page's left column lists.
    pub fn categories(&self, device: &DeviceKind) -> Vec<String> {
        let mut out: Vec<String> = self
            .for_device(device)
            .into_iter()
            .map(|e| e.category.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Every device the bank has a preset for, in slug order. What the
    /// browser's Presets tab lists at its top level.
    pub fn devices(&self) -> Vec<DeviceKind> {
        let mut out: Vec<DeviceKind> = Vec::new();
        for entry in &self.entries {
            if !out.contains(&entry.device) {
                out.push(entry.device.clone());
            }
        }
        out.sort_by_key(|device| device.slug());
        out
    }

    /// The entry a device's [`PresetRef`] names, if the bank still has it.
    ///
    /// **The origin is part of the question.** A user preset named like a
    /// factory one is a different preset, and a bar that drew the wrong one's
    /// name clean would be telling somebody their edits were saved.
    pub fn find(&self, device: &DeviceKind, reference: &PresetRef) -> Option<&PresetEntry> {
        self.entries.iter().find(|entry| {
            entry.device == *device
                && entry.origin == reference.origin
                && entry.name == reference.name
                && entry.category == reference.category
        })
    }

    /// The preset itself.
    pub fn load(&self, entry: &PresetEntry) -> Result<Preset, String> {
        if entry.origin == PresetOrigin::Factory {
            return self
                .factory
                .get(&entry.path)
                .cloned()
                .ok_or_else(|| format!("{} is not in this build", entry.path.display()));
        }
        let modified = modified_at(&entry.path);
        if let Some((at, preset)) = self.cache.borrow().get(&entry.path)
            && *at == modified
        {
            return Ok(preset.clone());
        }
        let text = std::fs::read_to_string(&entry.path).map_err(|e| e.to_string())?;
        let preset = parse(&text)?;
        self.cache
            .borrow_mut()
            .insert(entry.path.clone(), (modified, preset.clone()));
        Ok(preset)
    }

    /// Writes a preset into the user's bank and says what it is now called.
    ///
    /// Atomically — a temporary file beside it and a rename — because the one
    /// moment a preset file is most likely to be read is straight after it was
    /// written, and half a file is worse than no file. `overwrite` is the
    /// whole of the difference between **Save** and **Save as…**.
    pub fn save(&mut self, preset: &Preset, overwrite: bool) -> Result<PresetRef, String> {
        let Some(root) = self.user_dir.clone() else {
            return Err(
                "there is nowhere to save presets — set a preset folder in Settings".to_string(),
            );
        };
        let name = portable(&preset.name)
            .ok_or_else(|| format!("\"{}\" is not a name a file can have", preset.name))?;
        let category = portable(&preset.category)
            .ok_or_else(|| format!("\"{}\" is not a name a folder can have", preset.category))?;
        if !preset.is_consistent() {
            return Err(format!(
                "\"{}\" is not a preset for a {}",
                preset.name,
                preset.device.label()
            ));
        }
        let reference = PresetRef::new(name.clone(), category.clone(), PresetOrigin::User);
        if !overwrite && self.find(&preset.device, &reference).is_some() {
            return Err(format!("you already have a preset called \"{name}\""));
        }
        let folder = root.join(preset.device.slug()).join(&category);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = folder.join(format!("{name}.json"));
        let mut stored = preset.clone();
        stored.format_version = fontelle_types::PRESET_FORMAT_VERSION;
        stored.name = name.clone();
        stored.category = category.clone();
        let text = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
        let temporary = folder.join(format!(".{name}.json.part"));
        std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        self.rescan();
        Ok(reference)
    }

    /// Removes a user preset's file.
    ///
    /// A factory one is refused **here** rather than by a disabled button: a
    /// disabled button is a courtesy, and read-only is a rule.
    pub fn delete(&mut self, entry: &PresetEntry) -> Result<(), String> {
        if entry.origin == PresetOrigin::Factory {
            return Err(format!(
                "\"{}\" is a factory preset — those are read-only",
                entry.name
            ));
        }
        std::fs::remove_file(&entry.path).map_err(|e| e.to_string())?;
        // The folder it was in is left alone even when it is now empty: an
        // empty category is somewhere somebody was about to save, and a bank
        // that tidied it away would delete a folder nobody asked it to.
        self.rescan();
        Ok(())
    }

    /// Every preset whose name matches `query`, best first, across **every**
    /// device — you go looking for "hall" without first deciding it is a
    /// reverb.
    pub fn search(&self, query: &str) -> Vec<&PresetEntry> {
        let mut hits: Vec<(i32, &PresetEntry)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                crate::bank::fuzzy_score(query, &entry.name).map(|score| (score, entry))
            })
            .collect();
        // By score, and by the bank's own order among equals — which for an
        // empty query is every row scoring zero, so the whole bank comes back
        // in the order it is listed in.
        hits.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        hits.into_iter().map(|(_, entry)| entry).collect()
    }

    /// Parses the tree `build.rs` embedded.
    fn read_factory(&mut self) {
        let mut entries = Vec::new();
        for (relative, text) in FACTORY_PRESETS {
            let path = PathBuf::from(relative);
            match parse(text) {
                Ok(preset) => {
                    entries.push(PresetEntry {
                        device: preset.device.clone(),
                        name: preset.name.clone(),
                        category: preset.category.clone(),
                        origin: PresetOrigin::Factory,
                        path: path.clone(),
                    });
                    self.factory.insert(path, preset);
                }
                Err(why) => self.unreadable.push((path, why)),
            }
        }
        entries.sort_by_key(order);
        self.entries = entries;
    }
}

/// What an entry sorts by within its origin: its device, then its category,
/// then its name.
fn order(entry: &PresetEntry) -> (String, String, String) {
    (
        entry.device.slug(),
        entry.category.clone(),
        entry.name.clone(),
    )
}

/// Reads one preset, refusing the two things a file can be wrong about before
/// its fields are believed.
fn parse(text: &str) -> Result<Preset, String> {
    let preset: Preset =
        serde_json::from_str(text).map_err(|e| format!("this is not a preset: {e}"))?;
    if preset.is_from_the_future() {
        return Err(format!(
            "written by a newer version of Fontelle (format {})",
            preset.format_version
        ));
    }
    if !preset.is_consistent() {
        return Err(format!(
            "says it is for a {} and holds something else",
            preset.device.label()
        ));
    }
    Ok(preset)
}

/// Whether `name` is something this bank is willing to put on the filesystem.
///
/// Trimmed, then refused if it is empty, if it is one of the two names every
/// filesystem already means something by, or if it carries a separator — a
/// name reaches the disk, so a name that is a path is a preset that writes
/// somewhere nobody asked for.
fn portable(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }
    if trimmed
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_control())
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// When a file was last written, or `None` if that cannot be asked — which is
/// a cache that always misses rather than one that goes stale.
fn modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Every `.json` under `dir`, as entries, with what could not be read.
///
/// The device and category come from the **file's own fields**, not from the
/// folders it sits in. The folders are how a person organises the bank in
/// their file manager; the file is what it says it is, and a preset moved into
/// the wrong folder by hand should still load as what it is.
fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<PresetEntry>,
    unreadable: &mut Vec<(PathBuf, String)>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(root, &path, depth + 1, out, unreadable);
            continue;
        }
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let read = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&text));
        match read {
            Ok(preset) => out.push(PresetEntry {
                device: preset.device,
                name: preset.name,
                category: preset.category,
                origin: PresetOrigin::User,
                path,
            }),
            Err(why) => unreadable.push((path, why)),
        }
    }
    let _ = root;
}
