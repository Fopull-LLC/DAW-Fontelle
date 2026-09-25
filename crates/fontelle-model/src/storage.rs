//! The project bundle on disk (TDD §17.1–17.2).

use std::io::Write;
use std::path::{Path, PathBuf};

use fontelle_types::PersistentId;

use crate::project::{Project, ProjectMeta};

/// The revision of the document format this build writes.
///
/// Separate from a patch's own version (`fontelle-core`): a project can gain a
/// field without every preset in the world needing a new version stamp, and a
/// patch's format can change without invalidating projects that hold one.
///
/// **1** (2026-09-25, `docs/collab-plan.md` §4.1): a project has an id,
/// markers are an arena, and every insert and send has an id of its own.
pub const PROJECT_FORMAT_VERSION: u32 = 1;

/// The document inside a bundle.
pub const PROJECT_FILE: &str = "project.json";

/// The directories a bundle carries (TDD §17.1).
///
/// Created on the first save rather than on demand, so nothing later has to
/// decide whether it is allowed to make a directory — which under INVARIANT 10
/// is exactly the decision that should never be made in passing.
pub const BUNDLE_DIRS: [&str; 5] = ["assets", "recordings", "renders", "backups", "cache"];

#[derive(Debug)]
pub enum StorageError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Written by a build newer than this one — not a damaged project, and
    /// worth telling apart so the message can say "upgrade Fontelle" rather
    /// than implying data loss.
    FromTheFuture {
        found: u32,
        newest: u32,
    },
    Format(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::FromTheFuture { found, newest } => write!(
                f,
                "this project is in format version {found}, and this build of Fontelle \
                 understands up to version {newest} — upgrade Fontelle to open it"
            ),
            Self::Format(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for StorageError {}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StorageError + '_ {
    move |source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Writes `project` into the bundle at `bundle`, creating it if it does not
/// exist.
///
/// **Atomic** (§17.1): the document goes to a temporary file in the same
/// directory, is flushed to disk, and is then renamed over the real one.
/// A rename within a directory is atomic on every filesystem this runs on, so
/// a crash mid-save leaves either the old document or the new one and never
/// half of either. The directory itself is synced afterwards, because the
/// rename is only durable once the directory entry is.
pub fn save_project(project: &Project, bundle: &Path) -> Result<(), StorageError> {
    std::fs::create_dir_all(bundle).map_err(io(bundle))?;
    for dir in BUNDLE_DIRS {
        let path = bundle.join(dir);
        std::fs::create_dir_all(&path).map_err(io(&path))?;
    }

    let mut project = project.clone();
    project.meta.format_version = PROJECT_FORMAT_VERSION;
    project.meta.app_version = env!("CARGO_PKG_VERSION").to_string();
    if project.meta.created.is_empty() {
        project.meta.created = now_iso8601();
    }

    // Pretty-printed on purpose: §17.2 chose JSON to be diffable, inspectable,
    // greppable and recoverable by hand, and one enormous line is none of
    // those.
    let text = serde_json::to_string_pretty(&project)
        .map_err(|e| StorageError::Format(format!("could not write the document: {e}")))?;

    let temp = bundle.join(format!("{PROJECT_FILE}.tmp"));
    {
        let mut file = std::fs::File::create(&temp).map_err(io(&temp))?;
        file.write_all(text.as_bytes()).map_err(io(&temp))?;
        file.sync_all().map_err(io(&temp))?;
    }
    let final_path = bundle.join(PROJECT_FILE);
    std::fs::rename(&temp, &final_path).map_err(io(&final_path))?;
    // Best effort: a filesystem that will not open a directory for syncing is
    // not a reason to report a failed save of a file that is already written.
    if let Ok(dir) = std::fs::File::open(bundle) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Reads the document out of the bundle at `bundle`.
///
/// The version is read before the body, so a project from a newer build is
/// refused by version rather than by whichever field happened to change shape
/// first.
pub fn load_project(bundle: &Path) -> Result<Project, StorageError> {
    let path = bundle.join(PROJECT_FILE);
    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        StorageError::Format(format!("{} is not readable JSON: {e}", path.display()))
    })?;

    let found = json
        .get("meta")
        .and_then(|meta| meta.get("format_version"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            StorageError::Format(format!(
                "{} has no meta.format_version — it is not a Fontelle project",
                path.display()
            ))
        })? as u32;
    if found > PROJECT_FORMAT_VERSION {
        return Err(StorageError::FromTheFuture {
            found,
            newest: PROJECT_FORMAT_VERSION,
        });
    }
    let json = migrate(json, found)?;

    serde_json::from_value(json)
        .map_err(|e| StorageError::Format(format!("{} could not be read: {e}", path.display())))
}

/// Reads only what the bundle at `bundle` says about itself — its id, its
/// name, its save stamps — without building the song.
///
/// What a join walks the projects folder with (`docs/collab-plan.md` §4.2):
/// two hundred bundles at join time is two hundred of these. The file is
/// still read through, but the body is skipped rather than built. A format-0
/// bundle answers with the id its migration would give it, so a peek and a
/// load always agree.
pub fn peek_meta(bundle: &Path) -> Result<ProjectMeta, StorageError> {
    #[derive(serde::Deserialize)]
    struct Head {
        meta: serde_json::Value,
    }
    let path = bundle.join(PROJECT_FILE);
    let file = std::fs::File::open(&path).map_err(io(&path))?;
    let head: Head = serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| {
        StorageError::Format(format!("{} is not a Fontelle project: {e}", path.display()))
    })?;
    let mut meta = head.meta;
    let found = meta
        .get("format_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            StorageError::Format(format!(
                "{} has no meta.format_version — it is not a Fontelle project",
                path.display()
            ))
        })? as u32;
    if found > PROJECT_FORMAT_VERSION {
        return Err(StorageError::FromTheFuture {
            found,
            newest: PROJECT_FORMAT_VERSION,
        });
    }
    if found == 0 {
        meta_v0_to_v1(&mut meta);
    }
    serde_json::from_value(meta)
        .map_err(|e| StorageError::Format(format!("{} could not be read: {e}", path.display())))
}

/// Brings a document written by an older build up to
/// [`PROJECT_FORMAT_VERSION`].
///
/// One step per revision, in order, each rewriting the document from `N` to
/// `N + 1`:
///
/// ```text
/// if version == 0 { json = v0_to_v1(json)?; version = 1; }
/// ```
///
/// Every step works on the JSON rather than on the types, because the types
/// are this build's and the file is not.
fn migrate(mut json: serde_json::Value, from: u32) -> Result<serde_json::Value, StorageError> {
    let mut version = from;
    if version == 0 {
        json = v0_to_v1(json)?;
        version = 1;
    }
    if version != PROJECT_FORMAT_VERSION {
        return Err(StorageError::Format(format!(
            "no migration from project format version {from} to {PROJECT_FORMAT_VERSION}"
        )));
    }
    Ok(json)
}

/// Format 0 to 1: the ids a song needs before two machines can share it
/// (`docs/collab-plan.md` §4.1, §5.2).
///
/// **Every id given out here is derived, never minted.** Two zipped copies of
/// one old project on two machines, or two loads of the same file on one,
/// must agree about who the song is and which insert is which; `now_v7()`
/// on load would make every open a different song. From format 1 on, ids are
/// minted when the thing is made and saved with it.
fn v0_to_v1(mut json: serde_json::Value) -> Result<serde_json::Value, StorageError> {
    let meta = json
        .get_mut("meta")
        .ok_or_else(|| StorageError::Format("a format-0 project with no meta".into()))?;
    meta_v0_to_v1(meta);
    let song = meta["id"].as_str().unwrap_or_default().to_string();

    // A plain list of markers becomes the arena's list of (key, marker)
    // pairs, keyed in the order they were in.
    if let Some(markers) = json.get_mut("markers").and_then(|m| m.as_array_mut()) {
        for (index, marker) in markers.iter_mut().enumerate() {
            let key = serde_json::json!({ "idx": index, "version": 1 });
            *marker = serde_json::json!([key, marker.take()]);
        }
    }

    // Each insert and send is named by its song, its strip's key and its
    // place, which is exactly as stable as the file is.
    if let Some(tracks) = json
        .pointer_mut("/mixer/tracks")
        .and_then(|t| t.as_array_mut())
    {
        for pair in tracks.iter_mut().filter_map(|p| p.as_array_mut()) {
            let [key, track] = pair.as_mut_slice() else {
                continue;
            };
            let strip = format!("{}.{}", key["idx"], key["version"]);
            for (list, what) in [("inserts", "insert"), ("sends", "send")] {
                let Some(slots) = track.get_mut(list).and_then(|l| l.as_array_mut()) else {
                    continue;
                };
                for (place, slot) in slots.iter_mut().enumerate() {
                    if let Some(slot) = slot.as_object_mut() {
                        let id = PersistentId::derived(&format!("{song}/{strip}/{what}/{place}"));
                        slot.entry("id").or_insert(serde_json::json!(id));
                    }
                }
            }
        }
    }
    Ok(json)
}

/// The meta half of [`v0_to_v1`], shared with [`peek_meta`] so a peek and a
/// load cannot disagree about an old song's id.
fn meta_v0_to_v1(meta: &mut serde_json::Value) {
    let created = meta["created"].as_str().unwrap_or_default();
    let name = meta["name"].as_str().unwrap_or_default();
    // A unit separator between the two, so "ab" + "c" and "a" + "bc" are two
    // songs.
    let id = PersistentId::derived(&format!("fontelle-project\u{1f}{created}\u{1f}{name}"));
    meta["id"] = serde_json::json!(id);
    meta["format_version"] = serde_json::json!(1);
}

/// The current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Hand-rolled rather than pulled in: the only thing that needs a calendar is
/// this one field, and the civil-from-days algorithm is short, exact and has
/// no dependency to keep up to date.
pub(crate) fn now_iso8601() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a proleptic
/// Gregorian date. Exact for every date this will ever see.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_matches_dates_a_person_can_check() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // 2000-02-29: the leap day of the century that is a leap year, which
        // is the case every naive implementation gets wrong.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 1900 was not a leap year, so 1900-03-01 is the day after 1900-02-28.
        assert_eq!(civil_from_days(-25_509), (1900, 2, 28));
        assert_eq!(civil_from_days(-25_508), (1900, 3, 1));
        assert_eq!(civil_from_days(20_693), (2026, 8, 28));
    }

    #[test]
    fn the_timestamp_has_the_shape_iso_8601_asks_for() {
        let now = now_iso8601();
        assert_eq!(now.len(), 20, "{now}");
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[10..11], "T");
        assert!(now.ends_with('Z'));
    }
}
