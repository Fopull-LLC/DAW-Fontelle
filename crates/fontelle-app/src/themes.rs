//! The theme library: the looks a person can swap between.
//!
//! > *"they all boil down to a single .fontelletheme file that can be easily
//! > shared and entered and saved into the app and added to the users theme
//! > library to be swapped between"* — Ty, 2026-10-01.
//!
//! The built-in looks (`Theme::builtins`) first, then every
//! `.fontelletheme` in `<config>/themes/`. A theme is chosen by **name**,
//! which is why a name is never used twice: an import that clashes is
//! renamed, and a built-in is never shadowed or written over — changing one
//! saves a copy (`Session::own_theme`).

use std::path::{Path, PathBuf};

use fontelle_ui::theme::{THEME_EXTENSION, Theme};

/// One look in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeEntry {
    pub name: String,
    /// The file it is read from; `None` for a built-in.
    pub path: Option<PathBuf>,
}

/// The folder of the user's themes, with the built-ins in front of it.
#[derive(Debug, Clone)]
pub struct ThemeLibrary {
    dir: PathBuf,
}

impl ThemeLibrary {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every look, the built-ins first and then the folder's by name. A file
    /// that does not read as a theme is left out rather than listed broken.
    pub fn entries(&self) -> Vec<ThemeEntry> {
        let mut out: Vec<ThemeEntry> = Theme::builtins()
            .into_iter()
            .map(|t| ThemeEntry {
                name: t.name,
                path: None,
            })
            .collect();
        let mut own: Vec<ThemeEntry> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == THEME_EXTENSION))
            .filter_map(|path| {
                let theme = Theme::load_from_file(&path).ok()?;
                Some(ThemeEntry {
                    name: theme.name,
                    path: Some(path),
                })
            })
            .filter(|e| !out.iter().any(|b| b.name == e.name))
            .collect();
        own.sort_by_key(|e| e.name.to_lowercase());
        own.dedup_by(|a, b| a.name == b.name);
        out.extend(own);
        out
    }

    /// The look called `name`, built-in or from the folder.
    pub fn load(&self, name: &str) -> Option<Theme> {
        if let Some(theme) = Theme::builtins().into_iter().find(|t| t.name == name) {
            return Some(theme);
        }
        let entry = self.entries().into_iter().find(|e| e.name == name)?;
        Theme::load_from_file(&entry.path?).ok()
    }

    /// `wanted`, or `wanted 2`, `wanted 3`… — the first nobody has.
    pub fn unique_name(&self, wanted: &str) -> String {
        let taken: Vec<String> = self.entries().into_iter().map(|e| e.name).collect();
        if !taken.iter().any(|n| n == wanted) {
            return wanted.to_string();
        }
        (2..)
            .map(|n| format!("{wanted} {n}"))
            .find(|name| !taken.contains(name))
            .expect("an unbounded range finds one")
    }

    /// Copies the theme at `source` into the library, renamed if its name is
    /// taken, and answers the name it is listed under. Refused, with why, if
    /// it is not a theme.
    pub fn import(&self, source: &Path) -> Result<String, String> {
        let mut theme = Theme::load_from_file(source)
            .map_err(|e| format!("that is not a Fontelle theme: {e}"))?;
        theme.name = self.unique_name(theme.name.trim());
        if theme.name.is_empty() {
            theme.name = self.unique_name("Imported theme");
        }
        self.save(&theme)?;
        Ok(theme.name)
    }

    /// Writes `theme` into the folder under its name, over an earlier copy
    /// of the same name. A built-in's name is refused: those are not files.
    pub fn save(&self, theme: &Theme) -> Result<PathBuf, String> {
        if Theme::builtins().iter().any(|t| t.name == theme.name) {
            return Err(format!("{} is built in; save a copy", theme.name));
        }
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("could not make {}: {e}", self.dir.display()))?;
        // The file it was read from, if it has one; else a name made safe.
        let path = self
            .entries()
            .into_iter()
            .find(|e| e.name == theme.name)
            .and_then(|e| e.path)
            .unwrap_or_else(|| {
                self.dir
                    .join(format!("{}.{THEME_EXTENSION}", file_stem(&theme.name)))
            });
        std::fs::write(&path, theme.to_json())
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        Ok(path)
    }
}

/// A name as a file name: the characters a file system refuses, made dashes.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let stem = stem.trim().trim_matches('.').to_string();
    if stem.is_empty() {
        "theme".to_string()
    } else {
        stem
    }
}
