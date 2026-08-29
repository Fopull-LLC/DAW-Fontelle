//! The soundfont bank (TDD §17.5).
//!
//! One or more folders the user drops `.sf2` files into, scanned into a list
//! the browser panel searches. That is the whole of it today, and the shape is
//! deliberately the shape §17.5 describes so the rest can be added without
//! moving anything: a `notify` watch, a cached index carrying preset names and
//! tags, and cross-file preset search all hang off [`SoundfontBank::rescan`]
//! and [`matches`].
//!
//! **What is not here yet, and is named in §17.5:** the background scan (this
//! one is synchronous, and a folder of a hundred files takes a millisecond
//! because it reads directory entries and not soundfonts), the on-disk index,
//! and fuzzy search across *preset names inside files* — which needs that
//! index, because answering it live would mean parsing every `.sf2` in the
//! collection on every keystroke. Searching presets inside the **open** file is
//! here, since those are already parsed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One soundfont in the bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankEntry {
    pub path: PathBuf,
    /// The file name without its extension — what the browser shows and what
    /// the search runs against.
    pub name: String,
    /// Carried because §2.6 of the plan says memory cost should be *visible*: a
    /// 325 MB soundfont is fully resident until streaming lands, and that is a
    /// decision the user is entitled to make knowingly.
    pub size_bytes: u64,
}

impl BankEntry {
    /// A bank entry for a file that is not on this machine — the shape without
    /// the filesystem, so the search can be tested against a fixed list.
    pub fn for_test(path: &str, size_bytes: u64) -> Self {
        let path = PathBuf::from(path);
        Self {
            name: display_name(&path),
            path,
            size_bytes,
        }
    }
}

/// Whether `path` is something [`crate::SampleLibrary::import_sf2`] can open.
///
/// Extension only, and case-insensitively, because the alternative is opening
/// every file in the folder to find out. `.sf3` is deliberately absent: it is
/// the same container with Vorbis-compressed samples, the importer cannot read
/// one yet, and listing files that fail to load is worse than not listing them.
pub fn is_soundfont(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("sf2"))
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string()
}

/// The folders, and what is in them.
#[derive(Debug, Default)]
pub struct SoundfontBank {
    dirs: Vec<PathBuf>,
    entries: Vec<BankEntry>,
    unreadable: Vec<(PathBuf, String)>,
}

/// How deep [`SoundfontBank::rescan`] walks.
///
/// Deep enough for how people actually organise a collection —
/// `soundfonts/Orchestral/Strings/`— and bounded so a symlink loop or a folder
/// somebody pointed at their whole home directory cannot turn the scan into a
/// hang. §17.5's `notify`-driven incremental index is what replaces this.
const MAX_DEPTH: usize = 6;

impl SoundfontBank {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            entries: Vec::new(),
            unreadable: Vec::new(),
        }
    }

    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    pub fn entries(&self) -> &[BankEntry] {
        &self.entries
    }

    /// Folders that could not be read — gone, or not permitted.
    ///
    /// Kept rather than swallowed: a browser that is empty because the folder
    /// has moved looks exactly like a browser that is empty because the
    /// collection is, and those need different things done about them.
    pub fn unreadable(&self) -> &[(PathBuf, String)] {
        &self.unreadable
    }

    /// Rebuilds the list from the filesystem.
    ///
    /// Sorted by name, case-insensitively, and de-duplicated by path: two
    /// configured folders that overlap — or one that is inside another — must
    /// not list the same soundfont twice.
    pub fn rescan(&mut self) {
        self.entries.clear();
        self.unreadable.clear();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for dir in self.dirs.clone() {
            if !dir.exists() {
                self.unreadable
                    .push((dir.clone(), "the folder is not there".to_string()));
                continue;
            }
            self.walk(&dir, 0, &mut seen);
        }

        self.entries.sort_by_key(|entry| entry.name.to_lowercase());
    }

    fn walk(&mut self, dir: &Path, depth: usize, seen: &mut HashSet<PathBuf>) {
        if depth > MAX_DEPTH {
            return;
        }
        let listing = match std::fs::read_dir(dir) {
            Ok(listing) => listing,
            Err(e) => {
                self.unreadable.push((dir.to_path_buf(), e.to_string()));
                return;
            }
        };
        // Collected before recursing so `self` is not borrowed across the walk.
        let mut subdirs = Vec::new();
        for entry in listing.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                subdirs.push(path);
            } else if is_soundfont(&path) {
                // Canonicalised for the duplicate check only: two configured
                // folders reaching the same file through different routes is
                // ordinary, and the path shown stays the one the user's own
                // folder leads to.
                let key = path.canonicalize().unwrap_or_else(|_| path.clone());
                if seen.insert(key) {
                    self.entries.push(BankEntry {
                        name: display_name(&path),
                        path,
                        size_bytes: meta.len(),
                    });
                }
            }
        }
        for sub in subdirs {
            self.walk(&sub, depth + 1, seen);
        }
    }
}

// ------------------------------------------------------------- the search ---

/// How well `needle` matches `haystack`, or `None` when it does not.
///
/// A case-insensitive **subsequence** match, not a substring one: that is what
/// makes "gus" find `GeneralUser GS`, which is the behaviour §17.5 is asking
/// for when it calls this a genuine differentiator. The score exists so the
/// list is ordered by how well it matched rather than alphabetically among the
/// hits, and it rewards exactly the two things a person means:
///
/// - **Runs.** Consecutive matched letters are worth more than scattered ones,
///   so "piano" prefers `Piano` to `Pizzicato And Others`.
/// - **Beginnings.** A letter at the start of the name, or of a word inside it,
///   is worth more than one in the middle — so "bass" prefers `Bass Guitar` to
///   `Contrabassoon`.
///
/// An empty needle matches everything with a score of zero, which is what keeps
/// an empty search box showing the whole bank in its own order.
pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().collect();
    let mut score = 0;
    let mut run = 0;
    let mut at = 0usize;

    for want in needle.chars().flat_map(|c| c.to_lowercase()) {
        let found = hay[at..]
            .iter()
            .position(|c| c.to_lowercase().next() == Some(want))?;
        let index = at + found;

        if found == 0 && at > 0 {
            // Immediately after the last match: a run.
            run += 1;
            score += 4 + run;
        } else {
            run = 0;
            score += 1;
        }
        // The start of the name, or the start of a word in it.
        let boundary = index == 0
            || hay[index - 1].is_whitespace()
            || matches!(hay[index - 1], '_' | '-' | '.' | '(' | '[');
        if boundary {
            score += 6;
        }
        at = index + 1;
    }

    // A short name that used most of itself is a better answer than a long one
    // that happened to contain the letters.
    score += (32 - hay.len().min(32)) as i32 / 4;
    Some(score)
}

/// The indices of `entries` matching `query`, best first.
///
/// An empty query returns every index in the bank's own (alphabetical) order —
/// scoring would otherwise reorder the whole list the moment the box is
/// cleared, which is the one time the user is looking for alphabetical.
pub fn matches(entries: &[BankEntry], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..entries.len()).collect();
    }
    let query = query.trim();
    let mut hits: Vec<(usize, i32)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| fuzzy_score(query, &entry.name).map(|score| (index, score)))
        .collect();
    // Stable on ties, and by index, so equally good matches keep the bank's
    // order instead of shuffling as the list is rebuilt.
    hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    hits.into_iter().map(|(index, _)| index).collect()
}

/// [`matches`], over anything with a name — the preset list inside the open
/// file, which is the other half of §17.5's search.
pub fn matches_names<S: AsRef<str>>(names: &[S], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..names.len()).collect();
    }
    let query = query.trim();
    let mut hits: Vec<(usize, i32)> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| fuzzy_score(query, name.as_ref()).map(|score| (index, score)))
        .collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    hits.into_iter().map(|(index, _)| index).collect()
}
