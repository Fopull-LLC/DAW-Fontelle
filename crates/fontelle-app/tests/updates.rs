//! Checking for a newer Fontelle, and installing one.
//!
//! The start menu asks *is there a newer release* at launch and offers to
//! install it. Everything network-shaped goes through one injectable fetch
//! function so the whole path — the JSON GitHub answers with, the archive
//! naming the release workflow uses, the checksum, the swap of the running
//! binary — is exercised here against bytes in memory and files in a scratch
//! folder, never against the real repository.
//!
//! The one thing these tests cannot prove is that `curl` is on the machine;
//! `fetch_command` is checked for shape, and a missing `curl` is reported as
//! "could not check", never as a crash.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fontelle_app::updates::{
    Release, Updater, Version, asset_name, expected_sha256, fetch_command, sha256_hex,
    target_triple, tidy,
};
use fontelle_ui::UpdateStatus;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-updates-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// What the GitHub API says for `releases/latest`, trimmed to the fields that
/// matter, with the shape of the real answer (a tag with a `v`, a page URL,
/// assets with download URLs).
fn github_json(tag: &str, assets: &[(&str, &str)]) -> String {
    let assets: Vec<String> = assets
        .iter()
        .map(|(name, url)| {
            format!(
                r#"{{"name":"{name}","browser_download_url":"{url}","size":1234,"content_type":"application/octet-stream"}}"#
            )
        })
        .collect();
    format!(
        r#"{{"url":"https://api.github.com/repos/Fopull-LLC/DAW-Fontelle/releases/1","tag_name":"{tag}","name":"Fontelle {tag}","draft":false,"prerelease":false,"html_url":"https://github.com/Fopull-LLC/DAW-Fontelle/releases/tag/{tag}","assets":[{}]}}"#,
        assets.join(",")
    )
}

// --- versions ---

#[test]
fn a_version_reads_with_or_without_its_v() {
    assert_eq!(Version::parse("v0.2.0"), Some(Version::new(0, 2, 0)));
    assert_eq!(Version::parse("0.10.1"), Some(Version::new(0, 10, 1)));
    assert_eq!(
        Version::parse("1.2.3").map(|v| v.to_string()),
        Some("1.2.3".into())
    );
}

#[test]
fn what_is_not_a_release_version_is_not_offered() {
    // Two parts is not a version this project tags; a pre-release is not
    // something the start menu should push at people; junk is junk.
    assert_eq!(Version::parse("1.2"), None);
    assert_eq!(Version::parse("1.2.3-beta.1"), None);
    assert_eq!(Version::parse("banana"), None);
    assert_eq!(Version::parse(""), None);
}

#[test]
fn versions_order_numerically_not_textually() {
    assert!(Version::new(0, 10, 0) > Version::new(0, 9, 9));
    assert!(Version::new(1, 0, 0) > Version::new(0, 99, 99));
    assert!(Version::new(0, 1, 1) > Version::new(0, 1, 0));
}

#[test]
fn the_build_knows_its_own_version() {
    // The one the workspace declares, so the window, the tag and the
    // comparison are all the same number.
    let current = Version::parse(fontelle_app::updates::CURRENT).expect("the crate version parses");
    assert_eq!(current, Version::parse(env!("CARGO_PKG_VERSION")).unwrap());
}

// --- the release ---

#[test]
fn a_release_is_read_out_of_githubs_answer() {
    let json = github_json(
        "v0.2.0",
        &[
            ("SHA256SUMS", "https://example.test/SHA256SUMS"),
            (
                "fontelle-0.2.0-x86_64-unknown-linux-gnu.tar.gz",
                "https://example.test/linux.tar.gz",
            ),
        ],
    );
    let release = Release::from_github_json(&json).expect("a real answer parses");
    assert_eq!(release.version, Version::new(0, 2, 0));
    assert_eq!(
        release.page,
        "https://github.com/Fopull-LLC/DAW-Fontelle/releases/tag/v0.2.0"
    );
    assert_eq!(release.assets.len(), 2);
    assert_eq!(release.assets[1].url, "https://example.test/linux.tar.gz");
}

#[test]
fn an_answer_without_a_tag_is_an_error_not_a_release() {
    assert!(Release::from_github_json(r#"{"message":"Not Found"}"#).is_err());
    assert!(Release::from_github_json("not json").is_err());
}

#[test]
fn the_archive_is_named_by_version_and_target() {
    // The release workflow produces exactly these names; a mismatch here is a
    // release the updater cannot find.
    assert_eq!(
        asset_name(Version::new(0, 2, 0), "x86_64-unknown-linux-gnu"),
        "fontelle-0.2.0-x86_64-unknown-linux-gnu.tar.gz"
    );
    assert_eq!(
        asset_name(Version::new(0, 2, 0), "x86_64-pc-windows-msvc"),
        "fontelle-0.2.0-x86_64-pc-windows-msvc.zip"
    );
    assert_eq!(
        asset_name(Version::new(1, 0, 0), "aarch64-apple-darwin"),
        "fontelle-1.0.0-aarch64-apple-darwin.tar.gz"
    );
}

#[test]
fn the_release_finds_this_machines_archive_and_the_checksums() {
    let json = github_json(
        "v0.2.0",
        &[
            ("SHA256SUMS", "https://example.test/SHA256SUMS"),
            (
                "fontelle-0.2.0-aarch64-apple-darwin.tar.gz",
                "https://example.test/mac.tar.gz",
            ),
            (
                "fontelle-0.2.0-x86_64-unknown-linux-gnu.tar.gz",
                "https://example.test/linux.tar.gz",
            ),
        ],
    );
    let release = Release::from_github_json(&json).unwrap();
    assert_eq!(
        release
            .asset_for("x86_64-unknown-linux-gnu")
            .map(|a| a.url.as_str()),
        Some("https://example.test/linux.tar.gz")
    );
    assert_eq!(release.asset_for("x86_64-pc-windows-msvc"), None);
    assert_eq!(
        release.checksums().map(|a| a.url.as_str()),
        Some("https://example.test/SHA256SUMS")
    );
}

#[test]
fn the_target_triple_is_this_machines() {
    let triple = target_triple();
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(triple, "x86_64-unknown-linux-gnu");
    }
    // Whatever the machine, it has the three parts an asset name needs.
    assert!(triple.split('-').count() >= 3, "{triple}");
}

// --- checksums ---

#[test]
fn sha256_matches_the_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn the_expected_checksum_is_read_from_the_sums_file() {
    // `sha256sum`'s own format: hex, two spaces, file name — which is what
    // the release workflow writes.
    let sums = "\
0000000000000000000000000000000000000000000000000000000000000001  fontelle-0.2.0-aarch64-apple-darwin.tar.gz
0000000000000000000000000000000000000000000000000000000000000002  fontelle-0.2.0-x86_64-unknown-linux-gnu.tar.gz
";
    assert_eq!(
        expected_sha256(sums, "fontelle-0.2.0-x86_64-unknown-linux-gnu.tar.gz").as_deref(),
        Some("0000000000000000000000000000000000000000000000000000000000000002")
    );
    assert_eq!(
        expected_sha256(sums, "fontelle-0.2.0-x86_64-pc-windows-msvc.zip"),
        None
    );
}

// --- the fetch ---

#[test]
fn the_fetch_is_curl_that_fails_loudly_and_gives_up_in_time() {
    let (program, args) = fetch_command("https://example.test/x");
    assert_eq!(program, "curl");
    assert!(args.iter().any(|a| a == "https://example.test/x"));
    // An HTTP error must be an error, not a page of HTML mistaken for JSON.
    assert!(args.iter().any(|a| a == "--fail"));
    // And a machine with no route must not hold the start menu hostage.
    assert!(args.iter().any(|a| a == "--max-time"));
    assert!(
        args.iter().any(|a| a == "-L"),
        "a release asset is a redirect"
    );
}

#[test]
fn what_curl_said_is_turned_into_a_sentence() {
    use fontelle_app::updates::plain_curl_error;
    // The case every launch meets until the first release is cut.
    assert_eq!(
        plain_curl_error("curl: (22) The requested URL returned error: 404", Some(22)),
        "no release has been published yet"
    );
    assert_eq!(
        plain_curl_error("curl: (6) Could not resolve host: api.github.com", Some(6)),
        "no connection"
    );
    // Anything else keeps its words, without the prefix nobody needs.
    assert_eq!(
        plain_curl_error("curl: (60) SSL certificate problem", Some(60)),
        "SSL certificate problem"
    );
    assert!(plain_curl_error("", Some(1)).contains("curl"));
}

// --- the swap ---

/// A tarball shaped like a release: one folder named like the archive, with
/// `fontelle` inside it alongside the licences the workflow packs beside it.
fn a_release_archive(dir: &Path, binary_text: &str) -> PathBuf {
    let stem = "fontelle-9.9.9-x86_64-unknown-linux-gnu";
    let stage = dir.join("stage").join(stem);
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("fontelle"), binary_text).unwrap();
    std::fs::write(stage.join("LICENSE-MIT"), "mit").unwrap();
    std::fs::write(stage.join("install.sh"), "#!/bin/sh").unwrap();
    let archive = dir.join(format!("{stem}.tar.gz"));
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(dir.join("stage"))
        .arg(stem)
        .status()
        .expect("tar is on this machine");
    assert!(status.success());
    std::fs::remove_dir_all(dir.join("stage")).unwrap();
    archive
}

/// The same with no folder: the binary at the top level. Not what the
/// workflow makes, but what somebody repacking by hand might, and cheap to
/// accept.
fn a_flat_archive(dir: &Path, binary_text: &str) -> PathBuf {
    let stage = dir.join("flat");
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("fontelle"), binary_text).unwrap();
    let archive = dir.join("flat.tar.gz");
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&stage)
        .arg(".")
        .status()
        .expect("tar is on this machine");
    assert!(status.success());
    std::fs::remove_dir_all(&stage).unwrap();
    archive
}

#[test]
fn a_binary_at_the_top_of_the_archive_installs_too() {
    let dir = scratch("flat");
    let archive = a_flat_archive(&dir, "flat build");
    let exe = dir.join("bin").join("fontelle");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "old build").unwrap();
    fontelle_app::updates::install(&archive, &exe).expect("a flat archive installs");
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "flat build");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn installing_replaces_the_binary_and_leaves_nothing_behind() {
    let dir = scratch("install");
    let archive = a_release_archive(&dir, "new build");
    let exe = dir.join("bin").join("fontelle");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "old build").unwrap();

    fontelle_app::updates::install(&archive, &exe).expect("a writable folder installs");

    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new build");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "the new binary must be executable: {mode:o}"
        );
    }
    // Nothing else in the folder: the staging directory is gone, and the
    // licences that travel in the archive are not scattered next to the
    // binary.
    let left: Vec<String> = std::fs::read_dir(exe.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "fontelle")
        .collect();
    assert!(
        left.iter().all(|n| n == "fontelle.old"),
        "only the previous binary may remain, for a platform that cannot delete a running one: {left:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_written_is_an_error_with_the_path_in_it() {
    // `/usr/bin`, in effect: the binary is there, the folder is not ours. The
    // menu's answer is the release page, and the error is what tells it so.
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("readonly");
    let archive = a_release_archive(&dir, "new build");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("fontelle");
    std::fs::write(&exe, "old build").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes anywhere; there is nothing to prove on a machine running
    // tests as root.
    if std::fs::write(bin.join("probe"), "").is_ok() {
        std::fs::remove_file(bin.join("probe")).ok();
        return;
    }

    let err = fontelle_app::updates::install(&archive, &exe).unwrap_err();
    assert!(err.contains("bin"), "{err}");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "old build");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tidying_removes_the_previous_binary_left_by_the_last_upgrade() {
    let dir = scratch("tidy");
    let exe = dir.join("fontelle");
    std::fs::write(&exe, "current").unwrap();
    std::fs::write(dir.join("fontelle.old"), "previous").unwrap();
    tidy(&exe);
    assert!(!dir.join("fontelle.old").exists());
    assert!(exe.exists());
    // Nothing to tidy is not an error.
    tidy(&exe);
    std::fs::remove_dir_all(&dir).ok();
}

// --- the updater ---

type Served = Arc<Mutex<Vec<(String, Result<Vec<u8>, String>)>>>;

/// An updater whose network is a table of URL → answer.
fn updater_serving(served: &Served) -> Updater {
    let served = Arc::clone(served);
    Updater::with_fetcher(Box::new(move |url: &str| {
        let table = served.lock().unwrap();
        table
            .iter()
            .find(|(u, _)| u == url)
            .map(|(_, answer)| answer.clone())
            .unwrap_or_else(|| Err(format!("no such url in the test: {url}")))
    }))
}

fn wait_for(updater: &Updater, done: impl Fn(&UpdateStatus) -> bool) -> UpdateStatus {
    let start = Instant::now();
    loop {
        let status = updater.status();
        if done(&status) {
            return status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "stuck at {status:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn not_checking(status: &UpdateStatus) -> bool {
    !matches!(status, UpdateStatus::Checking | UpdateStatus::Unchecked)
}

#[test]
fn a_newer_release_is_offered() {
    let served: Served = Arc::new(Mutex::new(vec![(
        fontelle_app::updates::LATEST_URL.to_string(),
        Ok(github_json("v99.0.0", &[]).into_bytes()),
    )]));
    let updater = updater_serving(&served);
    assert_eq!(updater.status(), UpdateStatus::Unchecked);
    updater.check();
    let status = wait_for(&updater, not_checking);
    assert_eq!(
        status,
        UpdateStatus::Available {
            version: "99.0.0".to_string()
        }
    );
    assert_eq!(
        updater.release_page().as_deref(),
        Some("https://github.com/Fopull-LLC/DAW-Fontelle/releases/tag/v99.0.0")
    );
}

#[test]
fn the_same_or_an_older_release_means_up_to_date() {
    for tag in ["v0.0.1", &format!("v{}", fontelle_app::updates::CURRENT)] {
        let served: Served = Arc::new(Mutex::new(vec![(
            fontelle_app::updates::LATEST_URL.to_string(),
            Ok(github_json(tag, &[]).into_bytes()),
        )]));
        let updater = updater_serving(&served);
        updater.check();
        assert_eq!(
            wait_for(&updater, not_checking),
            UpdateStatus::UpToDate,
            "{tag}"
        );
    }
}

#[test]
fn no_network_is_a_sentence_not_a_crash() {
    let served: Served = Arc::new(Mutex::new(vec![(
        fontelle_app::updates::LATEST_URL.to_string(),
        Err("curl: (6) Could not resolve host".to_string()),
    )]));
    let updater = updater_serving(&served);
    updater.check();
    match wait_for(&updater, not_checking) {
        UpdateStatus::Failed(why) => assert!(why.contains("resolve host"), "{why}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn checking_can_be_switched_off() {
    // A person who does not want their DAW talking to GitHub at launch has a
    // setting for that, and the menu says so rather than saying nothing.
    let updater = Updater::disabled();
    updater.check();
    assert_eq!(updater.status(), UpdateStatus::Off);
}

#[test]
fn upgrading_downloads_verifies_and_swaps_the_binary() {
    let dir = scratch("upgrade");
    let archive = a_release_archive(&dir, "build 99");
    let bytes = std::fs::read(&archive).unwrap();
    let name = archive.file_name().unwrap().to_string_lossy().into_owned();
    let sums = format!("{}  {name}\n", sha256_hex(&bytes));
    let exe = dir.join("bin").join("fontelle");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "build 1").unwrap();

    let served: Served = Arc::new(Mutex::new(vec![
        (
            fontelle_app::updates::LATEST_URL.to_string(),
            Ok(github_json(
                "v9.9.9",
                &[
                    ("SHA256SUMS", "https://example.test/SHA256SUMS"),
                    (&name, "https://example.test/archive"),
                ],
            )
            .into_bytes()),
        ),
        (
            "https://example.test/SHA256SUMS".to_string(),
            Ok(sums.into_bytes()),
        ),
        ("https://example.test/archive".to_string(), Ok(bytes)),
    ]));
    let updater = updater_serving(&served).for_target("x86_64-unknown-linux-gnu");
    updater.check();
    wait_for(&updater, not_checking);
    updater.upgrade(exe.clone());
    let status = wait_for(&updater, |s| !matches!(s, UpdateStatus::Downloading { .. }));
    assert_eq!(
        status,
        UpdateStatus::Installed {
            version: "9.9.9".to_string()
        }
    );
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "build 99");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_checksum_that_does_not_match_leaves_the_binary_alone() {
    let dir = scratch("tampered");
    let archive = a_release_archive(&dir, "build 99");
    let bytes = std::fs::read(&archive).unwrap();
    let name = archive.file_name().unwrap().to_string_lossy().into_owned();
    let sums = format!("{}  {name}\n", sha256_hex(b"something else"));
    let exe = dir.join("bin").join("fontelle");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "build 1").unwrap();

    let served: Served = Arc::new(Mutex::new(vec![
        (
            fontelle_app::updates::LATEST_URL.to_string(),
            Ok(github_json(
                "v9.9.9",
                &[
                    ("SHA256SUMS", "https://example.test/SHA256SUMS"),
                    (&name, "https://example.test/archive"),
                ],
            )
            .into_bytes()),
        ),
        (
            "https://example.test/SHA256SUMS".to_string(),
            Ok(sums.into_bytes()),
        ),
        ("https://example.test/archive".to_string(), Ok(bytes)),
    ]));
    let updater = updater_serving(&served).for_target("x86_64-unknown-linux-gnu");
    updater.check();
    wait_for(&updater, not_checking);
    updater.upgrade(exe.clone());
    match wait_for(&updater, |s| !matches!(s, UpdateStatus::Downloading { .. })) {
        UpdateStatus::Failed(why) => assert!(why.contains("checksum"), "{why}"),
        other => panic!("expected a failure, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "build 1");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_release_with_no_archive_for_this_machine_says_so() {
    let served: Served = Arc::new(Mutex::new(vec![(
        fontelle_app::updates::LATEST_URL.to_string(),
        Ok(github_json(
            "v9.9.9",
            &[("SHA256SUMS", "https://example.test/SHA256SUMS")],
        )
        .into_bytes()),
    )]));
    let updater = updater_serving(&served).for_target("x86_64-unknown-linux-gnu");
    updater.check();
    wait_for(&updater, not_checking);
    updater.upgrade(PathBuf::from("/nonexistent/fontelle"));
    match wait_for(&updater, |s| !matches!(s, UpdateStatus::Downloading { .. })) {
        UpdateStatus::Failed(why) => assert!(why.contains("x86_64-unknown-linux-gnu"), "{why}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}
