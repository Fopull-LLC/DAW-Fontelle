//! Finding a file by what is in it.
//!
//! > *"when users have the same soundfonts downloaded sometimes it still
//! > doesnt recognize it"* (2026-10-18). One of the causes: the search did
//! > not follow symbolic links, so a soundfont that was a link — or a folder
//! > of them linked into the bank — was invisible.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use fontelle_assets::content_hash::{find_by_hash, hash_file};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-content-hash-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn a_file(at: &Path, bytes: &[u8]) -> (u64, u64) {
    std::fs::write(at, bytes).unwrap();
    (hash_file(at).unwrap().low, bytes.len() as u64)
}

#[test]
fn a_linked_file_is_found() {
    let dir = scratch("linked-file");
    let elsewhere = dir.join("elsewhere");
    let bank = dir.join("bank");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(&bank).unwrap();
    let (hash, size) = a_file(&elsewhere.join("real.sf2"), b"a soundfont, honestly");
    std::os::unix::fs::symlink(elsewhere.join("real.sf2"), bank.join("default.sf2")).unwrap();
    assert_eq!(
        find_by_hash(std::slice::from_ref(&bank), hash, size),
        Some(bank.join("default.sf2"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_linked_folder_is_searched() {
    let dir = scratch("linked-folder");
    let collection = dir.join("collection");
    let bank = dir.join("bank");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&bank).unwrap();
    let (hash, size) = a_file(&collection.join("piano.sf2"), b"a grand piano, in bytes");
    std::os::unix::fs::symlink(&collection, bank.join("my collection")).unwrap();
    assert!(find_by_hash(std::slice::from_ref(&bank), hash, size).is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_folder_that_links_back_into_itself_ends() {
    let dir = scratch("loop");
    let bank = dir.join("bank");
    std::fs::create_dir_all(&bank).unwrap();
    std::os::unix::fs::symlink(&bank, bank.join("again")).unwrap();
    assert_eq!(find_by_hash(std::slice::from_ref(&bank), 1, 1), None);
    std::fs::remove_dir_all(&dir).ok();
}
