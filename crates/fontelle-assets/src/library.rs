use std::path::PathBuf;

/// One indexed entry: a preset or instrument found inside a scanned soundfont
/// file. Search must reach *into* files — "marimba" finds the marimba preset
/// inside `GeneralUser.sf2`, not just files named marimba (TDD §17.5).
pub struct LibraryEntry {
    pub file: PathBuf,
    pub preset_name: String,
    pub instrument_names: Vec<String>,
    pub sample_count: u32,
    pub tags: Vec<String>,
    pub favourite: bool,
}

/// Background-scans the user's configured soundfont directories into a searchable
/// index, cached under the cache directory and kept live with `notify`
/// (TDD §17.5).
pub struct SoundfontLibrary {
    watched_dirs: Vec<PathBuf>,
    entries: Vec<LibraryEntry>,
}

impl SoundfontLibrary {
    pub fn new() -> Self {
        Self {
            watched_dirs: Vec::new(),
            entries: Vec::new(),
        }
    }

    pub fn rescan(&mut self) {
        let _ = &self.watched_dirs;
        todo!("walk watched_dirs, soundfont::parse each file's preset/instrument names")
    }

    pub fn search(&self, _query: &str) -> impl Iterator<Item = &LibraryEntry> {
        self.entries.iter().take(0)
    }
}

impl Default for SoundfontLibrary {
    fn default() -> Self {
        Self::new()
    }
}
