//! An unquoted path containing spaces has now cost two sessions: soundfont
//! libraries live in directories like `FL 2026 Linux/`, the shell splits the
//! argument into fragments, and Fontelle reported "no such file:
//! /mnt/disks/3tb/Apps/FL" — a path the user never typed. These tests pin the
//! diagnosis so the error explains itself instead of needing PROGRESS.md.

use std::path::{Path, PathBuf};

use fontelle_app::{Sf2PathError, resolve_sf2_path};

const REAL: &str = "/sounds/FL 2026 Linux/Secret_of_Mana.sf2";

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Stands in for the filesystem: only `REAL` exists.
fn only_real(path: &Path) -> bool {
    path == Path::new(REAL)
}

#[test]
fn a_quoted_path_resolves_directly() {
    let argv = args(&["fontelle", "--play-sf2", REAL]);
    assert_eq!(resolve_sf2_path(&argv, only_real), Ok(PathBuf::from(REAL)));
}

#[test]
fn a_shell_split_path_is_diagnosed_with_the_rejoined_suggestion() {
    // Exactly what fish hands us for an unquoted `/sounds/FL 2026 Linux/...`.
    let argv = args(&[
        "fontelle",
        "--play-sf2",
        "/sounds/FL",
        "2026",
        "Linux/Secret_of_Mana.sf2",
    ]);

    let err = resolve_sf2_path(&argv, only_real).expect_err("the fragment doesn't exist");
    assert_eq!(
        err,
        Sf2PathError::NotFound {
            tried: PathBuf::from("/sounds/FL"),
            rejoined: Some(PathBuf::from(REAL)),
        }
    );

    // The message has to be actionable on its own — that's the whole point.
    let text = err.to_string();
    assert!(
        text.contains(REAL),
        "message should show the real path: {text}"
    );
    assert!(
        text.contains("Quote it"),
        "message should say what to do: {text}"
    );
}

#[test]
fn rejoining_stops_at_the_next_flag() {
    let argv = args(&[
        "fontelle",
        "--play-sf2",
        "/sounds/FL",
        "2026",
        "Linux/Secret_of_Mana.sf2",
        "--key",
        "48",
    ]);
    let err = resolve_sf2_path(&argv, only_real).unwrap_err();
    assert_eq!(
        err,
        Sf2PathError::NotFound {
            tried: PathBuf::from("/sounds/FL"),
            rejoined: Some(PathBuf::from(REAL)),
        },
        "`--key 48` must not be swallowed into the rejoined path"
    );
}

#[test]
fn a_genuinely_missing_single_path_offers_no_bogus_suggestion() {
    let argv = args(&["fontelle", "--play-sf2", "/nope/missing.sf2"]);
    assert_eq!(
        resolve_sf2_path(&argv, only_real),
        Err(Sf2PathError::NotFound {
            tried: PathBuf::from("/nope/missing.sf2"),
            rejoined: None,
        })
    );
}

/// Fragments that rejoin into something that *still* doesn't exist must not
/// claim to have found the user's file.
#[test]
fn fragments_that_rejoin_into_nothing_real_offer_no_suggestion() {
    let argv = args(&["fontelle", "--play-sf2", "/nope/some", "other", "thing.sf2"]);
    assert_eq!(
        resolve_sf2_path(&argv, only_real),
        Err(Sf2PathError::NotFound {
            tried: PathBuf::from("/nope/some"),
            rejoined: None,
        })
    );
}

#[test]
fn a_missing_flag_or_value_is_reported_as_not_requested() {
    assert_eq!(
        resolve_sf2_path(&args(&["fontelle"]), only_real),
        Err(Sf2PathError::NotRequested)
    );
    assert_eq!(
        resolve_sf2_path(&args(&["fontelle", "--play-sf2"]), only_real),
        Err(Sf2PathError::NotRequested)
    );
}
