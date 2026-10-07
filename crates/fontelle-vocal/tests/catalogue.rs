//! Find singers: the catalogue index (a JSON file of free voicebanks, each
//! with its kind, languages, portrait, sample, the author's terms in brief
//! and where to get it), a download checked against the size and sha256
//! the index records, and the Downloads folder watched for a bank the
//! author's own page handed out.

mod common;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use fontelle_vocal::catalogue::{
    Catalogue, CatalogueKind, DownloadWatch, Get, Permission, downloads_dir_from, verify,
};

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("fontelle-catalogue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const INDEX: &str = r#"{
  "format": 1,
  "updated": "2026-10-07",
  "singers": [
    {
      "id": "example.direct",
      "name": "Direct Example",
      "kind": "diffsinger",
      "languages": ["zh", "ja"],
      "author": "Somebody",
      "portrait": "https://example.org/p.png",
      "sample": "https://example.org/s.ogg",
      "terms": {
        "summary": "Free, credit required, no commercial use.",
        "url": "https://example.org/terms",
        "commercial": "no",
        "ai_training": "no",
        "credit": "yes"
      },
      "download": {
        "url": "https://example.org/bank.zip",
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "size": 1
      },
      "page": "https://example.org/bank",
      "something_new": 42
    },
    {
      "id": "example.page",
      "name": "Page Example",
      "kind": "utau",
      "languages": ["ja"],
      "terms": { "summary": "See the page.", "url": "https://example.org/rules" },
      "page": "https://example.org/utau"
    },
    {
      "id": "example.broken",
      "name": "No Way To Get It",
      "kind": "utau",
      "languages": ["ja"],
      "terms": { "summary": "x", "url": "https://example.org" }
    }
  ]
}"#;

#[test]
fn an_index_reads_and_says_how_each_singer_is_got() {
    let catalogue = Catalogue::parse(INDEX).unwrap();
    assert_eq!(catalogue.singers.len(), 2, "the broken entry is left out");
    assert_eq!(catalogue.skipped.len(), 1);
    assert!(catalogue.skipped[0].contains("No Way To Get It"));
    let direct = &catalogue.singers[0];
    assert_eq!(direct.kind, CatalogueKind::DiffSinger);
    assert_eq!(direct.languages, vec!["zh", "ja"]);
    assert_eq!(direct.terms.commercial, Permission::No);
    assert_eq!(direct.terms.credit, Permission::Yes);
    assert_eq!(direct.terms.ai_training, Permission::No);
    match direct.get() {
        Get::Download(d) => {
            assert_eq!(d.size, 1);
            assert_eq!(d.sha256.len(), 64);
        }
        other => panic!("{other:?}"),
    }
    let page = &catalogue.singers[1];
    assert_eq!(page.terms.commercial, Permission::Unknown);
    assert_eq!(page.get(), Get::Page("https://example.org/utau".into()));
}

#[test]
fn a_download_with_a_bad_checksum_or_no_https_is_not_offered() {
    let bad = INDEX.replace("e3b0c442", "zzzzzzzz");
    let c = Catalogue::parse(&bad).unwrap();
    // Still reachable through its page, but not as a direct install.
    assert!(matches!(c.singers[0].get(), Get::Page(_)));
    let plain = INDEX.replace(
        "https://example.org/bank.zip",
        "http://example.org/bank.zip",
    );
    let c = Catalogue::parse(&plain).unwrap();
    assert!(matches!(c.singers[0].get(), Get::Page(_)));
    assert!(Catalogue::parse("{\"format\": 99, \"singers\": []}").is_err());
    assert!(Catalogue::parse("not json").is_err());
}

#[test]
fn a_download_is_checked_against_its_size_and_sha256() {
    let dir = scratch("verify");
    let file = dir.join("bank.zip");
    std::fs::write(&file, b"abc").unwrap();
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert!(verify(&file, sha, 3).is_ok());
    assert!(verify(&file, sha, 4).unwrap_err().contains("size"));
    let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(verify(&file, wrong, 3).unwrap_err().contains("sha256"));
}

#[test]
fn the_shipped_index_is_valid_and_every_entry_reachable() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/singers-index.json");
    let text = std::fs::read_to_string(&path).expect("docs/singers-index.json");
    let c = Catalogue::parse(&text).unwrap();
    assert!(c.skipped.is_empty(), "{:?}", c.skipped);
    for s in &c.singers {
        assert!(!s.terms.summary.is_empty(), "{}", s.name);
        assert!(s.terms.url.starts_with("https://"), "{}", s.name);
        assert!(s.page.as_deref().is_some_and(|p| p.starts_with("https://")));
    }
}

#[test]
fn a_bank_landing_in_downloads_is_found_once_it_stops_growing() {
    let dir = scratch("watch");
    std::fs::write(dir.join("old.zip"), b"old").unwrap();
    let mut watch = DownloadWatch::new(&dir, SystemTime::now() - Duration::from_millis(1), None);
    assert!(watch.poll().is_none(), "what was there before is not it");
    // A browser's partial file is not it either.
    std::fs::write(dir.join("teto.zip.crdownload"), b"part").unwrap();
    std::fs::write(dir.join("teto.zip.part"), b"part").unwrap();
    assert!(watch.poll().is_none());
    std::fs::write(dir.join("teto.zip"), b"whole").unwrap();
    // Seen once: it may still be growing.
    assert!(watch.poll().is_none());
    assert_eq!(watch.poll(), Some(dir.join("teto.zip")));
    // And only once.
    assert!(watch.poll().is_none());
    // Not a voicebank's kind of file: ignored.
    std::fs::write(dir.join("photo.jpg"), b"x").unwrap();
    assert!(watch.poll().is_none() && watch.poll().is_none());
}

#[test]
fn a_watch_that_knows_the_checksum_takes_only_that_file() {
    let dir = scratch("watch-sha");
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string();
    let mut watch = DownloadWatch::new(
        &dir,
        SystemTime::now() - Duration::from_millis(1),
        Some(sha),
    );
    std::fs::write(dir.join("other.zip"), b"xyz").unwrap();
    std::fs::write(dir.join("bank.zip"), b"abc").unwrap();
    let _ = watch.poll();
    assert_eq!(watch.poll(), Some(dir.join("bank.zip")));
}

#[test]
fn downloads_is_where_the_desktop_says() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    };
    let dirs = "XDG_DOWNLOAD_DIR=\"$HOME/Telechargements\"\n";
    assert_eq!(
        downloads_dir_from(&env(&[("HOME", "/home/ty")]), false, Some(dirs)),
        Some(PathBuf::from("/home/ty/Telechargements"))
    );
    assert_eq!(
        downloads_dir_from(&env(&[("HOME", "/home/ty")]), false, None),
        Some(PathBuf::from("/home/ty/Downloads"))
    );
    assert_eq!(
        downloads_dir_from(&env(&[("USERPROFILE", "C:\\Users\\ty")]), true, None),
        Some(PathBuf::from("C:\\Users\\ty").join("Downloads"))
    );
}
