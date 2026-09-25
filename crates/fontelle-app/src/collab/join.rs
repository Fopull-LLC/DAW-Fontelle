//! The join, on disk (`docs/collab-plan.md` §4.2–4.5): which copy of a song
//! this studio already has, the question that asks what to do about it, and
//! the three things the answer can do to a projects folder.
//!
//! **The three rules** (§4.4), each held by a test in `tests/collab.rs`:
//! a join never deletes; a replace is a backup first; unsaved work is asked
//! about before anything else (that one is `Session::join`'s). Every write
//! here goes through `save_project`, which writes a temporary file and renames
//! it, so a join cut off half way leaves the old copy or the new one and never
//! half of either.

use std::path::{Path, PathBuf};

use fontelle_model::Project;
use fontelle_model::wire::{AssetEntry, ProjectHead};
use fontelle_types::PersistentId;

use super::{JoinAnswer, JoinQuestion, Relation};
use crate::projects::{BUNDLE_SUFFIX, SHARED_DIR};

/// One copy of the song already on this machine.
struct Local {
    path: PathBuf,
    project: Project,
}

/// The question a join asks before it writes anything (§4.3).
///
/// `me` is this studio's install id and `host_install` the host's: the
/// record of what two copies last agreed on names the other studio, not the
/// person (§4.5).
pub(crate) fn question(
    projects_dir: &Path,
    head: &ProjectHead,
    manifest: &[AssetEntry],
    host: &str,
    me: PersistentId,
    host_install: PersistentId,
) -> JoinQuestion {
    let found = crate::projects::find_by_id(projects_dir, head.id);
    match found.as_slice() {
        [] => copy_question(head, manifest, host),
        [path] => match fontelle_model::load_project(path) {
            Ok(project) => known_question(
                &Local {
                    path: path.clone(),
                    project,
                },
                head,
                host,
                me,
                host_install,
            ),
            // A copy that will not open is not a copy to update; it is left
            // exactly where it is and the song comes in beside it.
            Err(_) => copy_question(head, manifest, host),
        },
        many => which_question(many, head),
    }
}

/// Asks again, once somebody has picked which of several copies they mean.
pub(crate) fn question_for(
    path: &Path,
    head: &ProjectHead,
    host: &str,
    me: PersistentId,
    host_install: PersistentId,
) -> Result<JoinQuestion, String> {
    let project = fontelle_model::load_project(path).map_err(|e| e.to_string())?;
    Ok(known_question(
        &Local {
            path: path.to_path_buf(),
            project,
        },
        head,
        host,
        me,
        host_install,
    ))
}

fn copy_question(head: &ProjectHead, manifest: &[AssetEntry], host: &str) -> JoinQuestion {
    let size: u64 = manifest.iter().map(|entry| entry.size).sum();
    let what = match manifest.len() {
        0 => String::new(),
        1 => format!(" 1 file, {}.", megabytes(size)),
        n => format!(" {n} files, {}.", megabytes(size)),
    };
    JoinQuestion {
        song: head.name.clone(),
        from: host.to_string(),
        lines: vec![format!(
            "Copy \u{201c}{}\u{201d} from {host} into your Shared projects?{what}",
            head.name
        )],
        buttons: vec![
            (JoinAnswer::Copy, "Copy it".to_string()),
            (JoinAnswer::Cancel, "Cancel".to_string()),
        ],
        default: 0,
        relation: None,
    }
}

fn which_question(paths: &[PathBuf], head: &ProjectHead) -> JoinQuestion {
    let mut buttons: Vec<(JoinAnswer, String)> = paths
        .iter()
        .map(|path| (JoinAnswer::Pick(path.clone()), place_of(path)))
        .collect();
    buttons.push((JoinAnswer::Cancel, "Cancel".to_string()));
    JoinQuestion {
        song: head.name.clone(),
        from: String::new(),
        lines: vec![format!(
            "You have \u{201c}{}\u{201d} in more than one place. Which one is it?",
            head.name
        )],
        buttons,
        default: 0,
        relation: None,
    }
}

/// Where a copy is, the way a person would say it: its folder name, and
/// *Shared* when it came from somebody.
fn place_of(path: &Path) -> String {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let shared = path
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|parent| parent == SHARED_DIR);
    if shared {
        format!("{name} (Shared)")
    } else {
        name
    }
}

/// Which of two copies has moved since they last agreed (§4.5).
///
/// Each side keeps the song's hash as it was when the two last parted, and
/// which studio it parted from. A copy whose hash is still that is a copy
/// nobody has changed since; the other side's head says the same about its
/// own. With no record on either side all that is left to go on is which was
/// saved later, and the question says so rather than guessing harder.
pub(crate) fn relation(
    yours: &Project,
    head: &ProjectHead,
    me: PersistentId,
    host_install: PersistentId,
) -> Relation {
    let your_hash = yours.sync_hash();
    if your_hash == head.hash {
        return Relation::Same;
    }
    let yours_unchanged = yours
        .meta
        .shared_revision
        .is_some_and(|(peer, hash)| peer == host_install && hash == your_hash);
    let theirs_unchanged = head
        .shared_revision
        .is_some_and(|(peer, hash)| peer == me && hash == head.hash);
    let any_record = yours
        .meta
        .shared_revision
        .is_some_and(|(peer, _)| peer == host_install)
        || head.shared_revision.is_some_and(|(peer, _)| peer == me);
    match (yours_unchanged, theirs_unchanged) {
        (true, _) => Relation::YouAreBehind,
        (false, true) => Relation::TheyAreBehind,
        (false, false) if any_record => Relation::BothChanged,
        (false, false) => Relation::Unknown,
    }
}

fn known_question(
    local: &Local,
    head: &ProjectHead,
    host: &str,
    me: PersistentId,
    host_install: PersistentId,
) -> JoinQuestion {
    let relation = relation(&local.project, head, me, host_install);
    let meta = &local.project.meta;
    let theirs_newer = head.saved_at > meta.saved_at;
    let verdict = match relation {
        Relation::Same => "They are the same song.".to_string(),
        Relation::YouAreBehind => {
            format!("{host}\u{2019}s has changed since you last shared, and yours has not.")
        }
        Relation::TheyAreBehind => format!(
            "Yours has changed since you last shared and {host}\u{2019}s has not \u{2014} \
             updating would lose your changes."
        ),
        Relation::BothChanged => {
            format!("Yours and {host}\u{2019}s both have changed since you last shared.")
        }
        Relation::Unknown if theirs_newer => {
            format!("{host}\u{2019}s was saved more recently.")
        }
        Relation::Unknown => "Yours was saved more recently.".to_string(),
    };
    let default = match relation {
        Relation::Same | Relation::YouAreBehind => 0,
        Relation::Unknown if theirs_newer => 0,
        _ => 1,
    };
    JoinQuestion {
        song: head.name.clone(),
        from: host.to_string(),
        lines: vec![
            format!("You already have \u{201c}{}\u{201d}.", head.name),
            format!(
                "Yours: saved {} by {} (revision {}).",
                age_of(&meta.saved_at),
                who(&meta.saved_by),
                meta.saved_revision
            ),
            format!(
                "{host}\u{2019}s: saved {} by {} (revision {}).",
                age_of(&head.saved_at),
                who(&head.saved_by),
                head.saved_revision
            ),
            verdict,
            // What the two buttons do, here rather than on them: a button is
            // a few words (F62).
            format!("Update mine takes {host}\u{2019}s, and keeps a backup of yours."),
            "Keep both makes yours a song of its own.".to_string(),
        ],
        buttons: vec![
            (
                JoinAnswer::Update(local.path.clone()),
                "Update mine".to_string(),
            ),
            (
                JoinAnswer::KeepBoth(local.path.clone()),
                "Keep both".to_string(),
            ),
            (JoinAnswer::Cancel, "Cancel".to_string()),
        ],
        default,
        relation: Some(relation),
    }
}

fn who(name: &str) -> &str {
    if name.is_empty() { "someone" } else { name }
}

/// "10 minutes ago", from a `YYYY-MM-DDTHH:MM:SSZ` stamp.
fn age_of(stamp: &str) -> String {
    let Some(then) = unix_of(stamp) else {
        return "at a time it did not write down".to_string();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let ago = (now - then).max(0);
    match ago {
        0..60 => "just now".to_string(),
        60..120 => "a minute ago".to_string(),
        120..3_600 => format!("{} minutes ago", ago / 60),
        3_600..7_200 => "an hour ago".to_string(),
        7_200..86_400 => format!("{} hours ago", ago / 3_600),
        86_400..172_800 => "yesterday".to_string(),
        _ => format!("{} days ago", ago / 86_400),
    }
}

fn unix_of(stamp: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| stamp.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Howard Hinnant's `days_from_civil`: the inverse of the `civil_from_days`
/// the stamps were written with.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The song as this studio will keep it: the host's song, labelled for the
/// folder it is going into, stamped with who saved it last, and remembering
/// that it agrees with the host as of now.
fn as_kept(song: &Project, head: &ProjectHead, host_install: PersistentId, path: &Path) -> Project {
    let mut kept = song.clone();
    if let Some(stem) = path.file_stem() {
        use fontelle_model::Command as _;
        let _ = fontelle_model::RenameProject::new(stem.to_string_lossy()).apply(&mut kept);
    }
    kept.meta.id = head.id;
    kept.meta.saved_revision = head.saved_revision;
    kept.meta.saved_at = head.saved_at.clone();
    kept.meta.saved_by = head.saved_by.clone();
    kept.meta.shared_revision = Some((host_install, song.sync_hash()));
    kept
}

/// Copies the song into `<projects>/Shared`, under a folder name nothing
/// there already has (the name is a label; the id is the identity).
pub(crate) fn copy_in(
    projects_dir: &Path,
    song: &Project,
    head: &ProjectHead,
    host_install: PersistentId,
) -> Result<PathBuf, String> {
    let shared = projects_dir.join(SHARED_DIR);
    std::fs::create_dir_all(&shared).map_err(|e| format!("{}: {e}", shared.display()))?;
    let taken: Vec<String> = std::fs::read_dir(&shared)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            path.extension()
                .is_some_and(|e| e == BUNDLE_SUFFIX)
                .then(|| {
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                })
                .flatten()
        })
        .collect();
    let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
    let name = crate::projects::unique_name(&crate::projects::safe_name(&head.name), &taken);
    let path = shared.join(format!("{name}.{BUNDLE_SUFFIX}"));
    crate::save_project(&as_kept(song, head, host_install, &path), &path)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Rewrites the copy at `path` as the host's song, after copying what was
/// there into `backups/before-join-<date>_<time>/` in the same bundle
/// (§4.4, rule 2). Nothing is removed: a file the new version no longer uses
/// stays where it was (rule 1), which is also why the backup needs only the
/// manifest.
pub(crate) fn update(
    path: &Path,
    song: &Project,
    head: &ProjectHead,
    host_install: PersistentId,
) -> Result<PathBuf, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (date, time) = crate::logs::utc(now);
    let mut backup = path
        .join("backups")
        .join(format!("before-join-{date}_{}", time.replace(':', "-")));
    // Two joins in one second are two backups, not one overwriting the other.
    let mut n = 2;
    while backup.exists() {
        backup = path
            .join("backups")
            .join(format!("before-join-{date}_{}-{n}", time.replace(':', "-")));
        n += 1;
    }
    std::fs::create_dir_all(&backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    let manifest = path.join(fontelle_model::PROJECT_FILE);
    std::fs::copy(&manifest, backup.join(fontelle_model::PROJECT_FILE))
        .map_err(|e| format!("could not back up {}: {e}", manifest.display()))?;
    crate::save_project(&as_kept(song, head, host_install, path), path)
        .map_err(|e| e.to_string())?;
    Ok(backup)
}

/// Makes the copy at `path` a song of its own — a new id, remembering the
/// shared one as where it came from — so the host's can come in beside it
/// without either being mistaken for the other again (§4.3, *Keep both*).
pub(crate) fn fork(path: &Path) -> Result<(), String> {
    let mut project = fontelle_model::load_project(path).map_err(|e| e.to_string())?;
    project.meta.fork();
    crate::save_project(&project, path).map_err(|e| e.to_string())
}

fn megabytes(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
