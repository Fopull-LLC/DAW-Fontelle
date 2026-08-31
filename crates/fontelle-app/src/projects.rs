//! The projects folder: what is in it, and where a new project goes
//! (TDD §17.1, §17.3).
//!
//! Reported from using the window: *"I want to be able to select any folder on
//! my disk as my projects folder where new projects will be made by default,
//! and when browsing my projects in the app it will show me projects from that
//! folder."* `Settings::projects_dir` has existed since the settings file did
//! and nothing had ever read it.
//!
//! # INVARIANT 10 shapes the whole thing
//!
//! > Fontelle writes nothing outside locations the user has explicitly
//! > configured, except its own config directory.
//!
//! So there is **no default projects folder**. `None` means *ask* — never a
//! guess at `~/Documents` or `~/Music`, which belong to the user and not to
//! this program. Scanning a folder that is not there **reports** rather than
//! creating it: the folder somebody named is configured, one that does not
//! exist yet is a typo, and the soundfont bank is the one deliberate exception
//! to that (see [`crate::settings::Settings`]).
//!
//! Everything here except the two filesystem walks is a pure function, so the
//! naming rule and the ordering are tested without a folder.

use std::path::{Path, PathBuf};

/// The suffix a Fontelle project bundle carries (TDD §17.1).
pub const BUNDLE_SUFFIX: &str = "fontelle";

/// The file inside a bundle that makes it one.
const MANIFEST: &str = "project.json";

/// One project, as the browser draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    /// The bundle's name without its suffix — what the panel writes.
    pub name: String,
    pub path: PathBuf,
    /// When it was last written, as something a person can read. Empty when
    /// the filesystem would not say.
    pub modified: String,
    /// The raw timestamp, for ordering. Not shown.
    modified_at: std::time::SystemTime,
}

/// How the list is sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectOrder {
    /// Alphabetical, which is how you look something up.
    #[default]
    Name,
    /// Newest first, which is how you get back to what you were doing five
    /// minutes ago.
    Recent,
}

/// The projects folder, and what is in it.
#[derive(Debug)]
pub struct ProjectLibrary {
    dir: Option<PathBuf>,
    entries: Vec<ProjectEntry>,
    order: ProjectOrder,
    /// What to say when the list is empty, which on a first run it is.
    status: String,
}

impl Default for ProjectLibrary {
    /// A library with no folder — a real state, and the one a first run is in.
    ///
    /// Derived, this left `status` empty, and an empty panel that says nothing
    /// about why it is empty is the whole failure mode this feature exists to
    /// avoid. `rescan` writes the sentence.
    fn default() -> Self {
        let mut library = Self {
            dir: None,
            entries: Vec::new(),
            order: ProjectOrder::default(),
            status: String::new(),
        };
        library.rescan();
        library
    }
}

impl ProjectLibrary {
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn entries(&self) -> &[ProjectEntry] {
        &self.entries
    }

    pub fn order(&self) -> ProjectOrder {
        self.order
    }

    /// One line about where the projects are and what is in there — the thing
    /// to say when the list is empty, which on a first run it is.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Points the library at a folder and reads it. `None` forgets the one it
    /// had, list and all.
    pub fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.dir = dir;
        self.rescan();
    }

    pub fn set_order(&mut self, order: ProjectOrder) {
        if self.order == order {
            return;
        }
        self.order = order;
        self.sort();
    }

    /// Reads the folder again. What the window calls after saving.
    pub fn rescan(&mut self) {
        self.entries.clear();
        let Some(dir) = self.dir.clone() else {
            // Short enough for the 248-pixel panel it is drawn in: the longer
            // wording was clipped mid-word to `... "Change..." pi`, which is a
            // sentence nobody can finish.
            self.status = "no projects folder — \"Change...\" picks one".to_string();
            return;
        };
        let listing = match std::fs::read_dir(&dir) {
            Ok(listing) => listing,
            Err(e) => {
                // Reported, not created: see this module's own note on
                // INVARIANT 10.
                self.status = format!("{}: {e}", dir.display());
                return;
            }
        };

        for entry in listing.flatten() {
            let path = entry.path();
            if !is_bundle(&path) {
                continue;
            }
            let modified_at = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            self.entries.push(ProjectEntry {
                name: bundle_name(&path),
                modified: age(modified_at),
                path,
                modified_at,
            });
        }
        self.sort();
        self.status = match self.entries.len() {
            0 => format!("{} \u{2014} no projects yet", dir.display()),
            1 => format!("{} \u{2014} 1 project", dir.display()),
            n => format!("{} \u{2014} {n} projects", dir.display()),
        };
    }

    fn sort(&mut self) {
        match self.order {
            ProjectOrder::Name => self.entries.sort_by_key(|e| e.name.to_lowercase()),
            // Newest first, hence the reversed key.
            ProjectOrder::Recent => self
                .entries
                .sort_by_key(|e| std::cmp::Reverse(e.modified_at)),
        }
    }

    /// Where a new project called `name` would go, or `None` when there is no
    /// projects folder to put it in.
    ///
    /// Refused rather than guessed at: that is the one case INVARIANT 10 will
    /// not let this module invent an answer for.
    ///
    /// The name is made unique against what is already listed — two
    /// `Untitled`s in one folder is a mess somebody has to clean up by hand,
    /// and silently overwriting the first one is worse.
    pub fn new_project_path(&self, name: &str) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let taken: Vec<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        Some(dir.join(format!("{}.{BUNDLE_SUFFIX}", unique_name(name, &taken))))
    }
}

/// Whether `path` is a project bundle: a folder with a manifest in it.
///
/// The manifest and not just the suffix, so a folder somebody renamed by hand
/// is not offered as a project that then fails to open.
fn is_bundle(path: &Path) -> bool {
    path.is_dir()
        && path.extension().is_some_and(|e| e == BUNDLE_SUFFIX)
        && path.join(MANIFEST).is_file()
}

/// A bundle's name without its suffix.
fn bundle_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// `name`, or `name 2`, or `name 3` — the first one `taken` does not hold.
pub fn unique_name(name: &str, taken: &[&str]) -> String {
    let name = if name.trim().is_empty() {
        "Untitled"
    } else {
        name.trim()
    };
    if !taken.iter().any(|t| t.eq_ignore_ascii_case(name)) {
        return name.to_string();
    }
    // From two, because the first one is the bare name. Bounded so a folder
    // full of collisions cannot spin: past this something is wrong that
    // another number will not fix.
    for n in 2..10_000 {
        let candidate = format!("{name} {n}");
        if !taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
            return candidate;
        }
    }
    format!("{name} {}", std::process::id())
}

/// How long ago `when` was, in the coarsest unit that is still useful.
///
/// A date would need a calendar crate for one line of a list. What the
/// question actually is — "which of these was I working on" — is answered by
/// the age, and the age is arithmetic on a duration.
fn age(when: std::time::SystemTime) -> String {
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(when) else {
        // A file stamped in the future: a clock that moved, or a copy off
        // another machine. "just now" is closer than a negative number.
        return "just now".to_string();
    };
    let seconds = elapsed.as_secs();
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3_599 => plural(seconds / 60, "minute"),
        3_600..=86_399 => plural(seconds / 3_600, "hour"),
        86_400..=2_591_999 => plural(seconds / 86_400, "day"),
        2_592_000..=31_535_999 => plural(seconds / 2_592_000, "month"),
        _ => plural(seconds / 31_536_000, "year"),
    }
}

fn plural(n: u64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_name_is_used_as_it_is() {
        assert_eq!(unique_name("Untitled", &[]), "Untitled");
        assert_eq!(unique_name("Untitled", &["Other"]), "Untitled");
    }

    #[test]
    fn a_taken_name_counts_up_from_two() {
        assert_eq!(unique_name("Untitled", &["Untitled"]), "Untitled 2");
        assert_eq!(
            unique_name("Untitled", &["Untitled", "Untitled 2"]),
            "Untitled 3"
        );
    }

    #[test]
    fn case_does_not_make_a_name_free() {
        // On a case-insensitive filesystem it would collide anyway, and on a
        // case-sensitive one two projects a letter apart is a trap.
        assert_eq!(unique_name("Untitled", &["untitled"]), "Untitled 2");
    }

    #[test]
    fn an_empty_name_becomes_untitled_rather_than_a_folder_with_no_name() {
        assert_eq!(unique_name("   ", &[]), "Untitled");
        assert_eq!(unique_name("", &["Untitled"]), "Untitled 2");
    }

    #[test]
    fn an_age_reads_in_the_coarsest_unit_that_is_still_useful() {
        let now = std::time::SystemTime::now();
        let ago = |secs| now - std::time::Duration::from_secs(secs);
        assert_eq!(age(now), "just now");
        assert_eq!(age(ago(90)), "1 minute ago");
        assert_eq!(age(ago(60 * 5)), "5 minutes ago");
        assert_eq!(age(ago(3_600 * 2)), "2 hours ago");
        assert_eq!(age(ago(86_400 * 3)), "3 days ago");
    }

    #[test]
    fn a_file_stamped_in_the_future_reads_as_now_rather_than_as_nonsense() {
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3_600);
        assert_eq!(age(later), "just now");
    }
}
