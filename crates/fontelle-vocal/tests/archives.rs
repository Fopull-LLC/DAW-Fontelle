//! Voicebank packages beyond the zip: `.7z` (how many UTAU and DiffSinger
//! banks are shared) and `.uar` (UTAU's own installer archive: a zip under
//! another name, with an `install.txt` naming the bank's folder). What a
//! file is comes from its first bytes, not its name, so a `.7z` saved as
//! `.zip` installs too; a RAR says plainly that it cannot be read.

mod common;

use std::path::{Path, PathBuf};

use common::zip::{Entry, write};
use fontelle_vocal::voicebanks::{InstallError, PackageKind, install, list};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-archives-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const OTO: &[u8] = b"a.wav=- a,20,100,-200,50,30\n";

/// A `.7z` holding `files` (name, bytes); names ending in `/` are folders.
fn seven_zip(path: &Path, files: &[(&str, &[u8])]) {
    let mut w = ArchiveWriter::create(path).unwrap();
    for (name, data) in files {
        if let Some(dir) = name.strip_suffix('/') {
            w.push_archive_entry::<&[u8]>(ArchiveEntry::new_directory(dir), None)
                .unwrap();
        } else {
            w.push_archive_entry(ArchiveEntry::new_file(name), Some(*data))
                .unwrap();
        }
    }
    w.finish().unwrap();
}

#[test]
fn a_seven_zip_utau_bank_installs_under_its_own_folder() {
    let dir = scratch("7z");
    let banks = dir.join("voicebanks");
    let src = dir.join("teto.7z");
    seven_zip(
        &src,
        &[
            ("重音テト/", b""),
            ("重音テト/oto.ini", OTO),
            ("重音テト/あ.wav", b"RIFF...."),
            (
                "重音テト/character.txt",
                "name=重音テト\nimage=teto.bmp\n".as_bytes(),
            ),
            ("重音テト/teto.bmp", b"BM"),
            (
                "重音テト/readme.txt",
                "利用規約: 営利目的の利用は可".as_bytes(),
            ),
        ],
    );
    let bank = install(&src, &banks).unwrap();
    assert_eq!(bank.kind, PackageKind::Utau);
    assert_eq!(bank.name, "重音テト");
    assert!(bank.path.join("あ.wav").is_file());
    assert_eq!(bank.info.portrait, Some(bank.path.join("teto.bmp")));
    assert!(bank.terms.unwrap().contains("営利"));
    assert_eq!(list(&banks).len(), 1);
}

#[test]
fn a_seven_zip_diffsinger_bank_with_nested_folders_installs() {
    let dir = scratch("7z-ds");
    let banks = dir.join("voicebanks");
    let src = dir.join("qixuan.7z");
    seven_zip(
        &src,
        &[
            ("release/Qixuan/dsconfig.yaml", b"phonemes: phonemes.txt\n"),
            ("release/Qixuan/character.yaml", b"name: Qixuan\n"),
            ("release/Qixuan/dsdur/dsconfig.yaml", b"phonemes: x\n"),
            ("release/readme.txt", b"wrapping readme"),
        ],
    );
    let bank = install(&src, &banks).unwrap();
    assert_eq!(bank.kind, PackageKind::DiffSinger);
    assert_eq!(bank.name, "Qixuan");
    assert!(bank.path.join("dsdur/dsconfig.yaml").is_file());
}

#[test]
fn a_seven_zip_naming_a_file_outside_its_folder_is_refused_whole() {
    let dir = scratch("7z-slip");
    let banks = dir.join("voicebanks");
    let src = dir.join("evil.7z");
    seven_zip(
        &src,
        &[
            ("bank/oto.ini", OTO),
            ("bank/a.wav", b"RIFF"),
            ("../../escaped.txt", b"gotcha"),
        ],
    );
    assert!(matches!(
        install(&src, &banks),
        Err(InstallError::Unsafe(_))
    ));
    assert!(!dir.join("escaped.txt").exists());
    assert!(!banks.join("escaped.txt").exists());
    assert!(list(&banks).is_empty());
    // Nothing half-installed is left: only the (empty) folder itself.
    let left: Vec<_> = std::fs::read_dir(&banks).unwrap().flatten().collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn what_an_archive_is_comes_from_its_bytes_not_its_name() {
    let dir = scratch("sniff");
    let banks = dir.join("voicebanks");
    let src = dir.join("misnamed.zip");
    seven_zip(&src, &[("bank/oto.ini", OTO), ("bank/a.wav", b"RIFF")]);
    let bank = install(&src, &banks).unwrap();
    assert!(bank.path.join("a.wav").is_file());
}

#[test]
fn a_uar_is_a_zip_whose_install_txt_names_the_bank() {
    let dir = scratch("uar");
    let banks = dir.join("voicebanks");
    let src = dir.join("defoko.uar");
    let bytes = write(&[
        Entry::utf8(
            "install.txt",
            b"type=voiceset\r\nfolder=uta\r\ncontentsdir=voice\r\ndescription=Defoko\r\n",
        ),
        // The bank's own files under contentsdir; a stray oto elsewhere is
        // not the bank.
        Entry::utf8("voice/oto.ini", OTO),
        Entry::utf8("voice/a.wav", b"RIFF"),
        Entry::utf8("voice/character.txt", b"name=Defoko\n"),
        Entry::utf8("extras/oto.ini", OTO),
    ]);
    std::fs::write(&src, bytes).unwrap();
    let bank = install(&src, &banks).unwrap();
    assert_eq!(bank.kind, PackageKind::Utau);
    assert_eq!(bank.name, "Defoko");
    assert!(bank.path.join("a.wav").is_file());
    assert!(!bank.path.join("extras").exists());
    // Installed under install.txt's folder name.
    assert_eq!(bank.id, "uta");
}

#[test]
fn a_rar_says_it_cannot_be_read_and_what_to_do() {
    let dir = scratch("rar");
    let banks = dir.join("voicebanks");
    let src = dir.join("bank.rar");
    std::fs::write(&src, b"Rar!\x1a\x07\x01\x00rest of a rar").unwrap();
    match install(&src, &banks) {
        Err(InstallError::Archive(why)) => {
            assert!(why.contains("RAR"), "{why}");
            assert!(why.contains("folder"), "{why}");
        }
        other => panic!("{other:?}"),
    }
}
