//! The extensions catalogue and its install path (`docs/vst-plan.md` §4).

use std::sync::{Arc, Mutex};

use fontelle_app::extensions::{self, Extension, ExtensionAction, ExtensionKind, ExtensionState};
use fontelle_app::updates::{Fetcher, Version, sha256_hex};

#[test]
fn the_catalogue_offers_vst2() {
    let vst2 = extensions::find("vst2").expect("vst2 is in the catalogue");
    assert_eq!(vst2.name, "VST 2 plugins");
    assert!(vst2.summary.to_lowercase().contains("vst 2"));
    assert!(vst2.repo.starts_with("Fopull-LLC/"));
    assert!(matches!(vst2.kind, ExtensionKind::Bridge { .. }));
}

#[test]
fn the_catalogue_is_code_not_a_network_resource() {
    // Every entry is a bridge whose ABI this build knows — the whole point
    // of a compiled-in catalogue (§4.3).
    assert!(!extensions::CATALOGUE.is_empty());
    for extension in extensions::CATALOGUE {
        assert!(!extension.id.is_empty());
        assert!(!extension.repo.is_empty());
    }
}

#[test]
fn an_asset_is_named_the_way_the_updater_names_fontelles_own() {
    let vst2 = extensions::find("vst2").unwrap();
    let name = vst2.asset_name(Version::new(0, 1, 0), "x86_64-unknown-linux-gnu");
    assert_eq!(name, "fontelle-vst2-0.1.0-x86_64-unknown-linux-gnu.tar.gz");
    let windows = vst2.asset_name(Version::new(0, 1, 0), "x86_64-pc-windows-msvc");
    assert!(windows.ends_with(".zip"), "{windows}");
}

#[test]
fn a_loadable_extension_not_installed_offers_to_install() {
    let vst2 = extensions::find("vst2").unwrap();
    assert!(vst2.loadable(), "the catalogue's own ABI is this build's");
    let state = ExtensionState::of(vst2, false, None);
    assert_eq!(state, ExtensionState::NotInstalled);
    assert_eq!(extensions::action_for(&state), ExtensionAction::Install);
}

#[test]
fn an_installed_extension_offers_to_remove() {
    let vst2 = extensions::find("vst2").unwrap();
    let state = ExtensionState::of(vst2, true, None);
    assert_eq!(state, ExtensionState::Installed);
    assert_eq!(extensions::action_for(&state), ExtensionAction::Remove);
}

#[test]
fn a_bridge_from_a_later_fontelle_is_named_but_not_offered() {
    // A catalogue entry whose ABI this build cannot load.
    let future = Extension {
        id: "future",
        name: "A later format",
        summary: "",
        repo: "Fopull-LLC/whatever",
        kind: ExtensionKind::Bridge { abi: u32::MAX },
    };
    assert!(!future.loadable());
    let state = ExtensionState::of(&future, false, None);
    assert_eq!(state, ExtensionState::NeedsNewerFontelle);
    assert_eq!(extensions::action_for(&state), ExtensionAction::None);
}

// --- the install path, against a fake network and a real archive ---

type Served = Arc<Mutex<Vec<(String, Result<Vec<u8>, String>)>>>;

// `FONTELLE_BRIDGES` is process-global, so the two tests that set it must
// not run at once.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fetcher(served: &Served) -> Fetcher {
    let served = Arc::clone(served);
    Box::new(
        move |url: &str, progress: &mut dyn FnMut(u64, Option<u64>)| {
            let answer = served
                .lock()
                .unwrap()
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, a)| a.clone())
                .unwrap_or_else(|| Err(format!("no such url: {url}")));
            if let Ok(bytes) = &answer {
                progress(bytes.len() as u64, Some(bytes.len() as u64));
            }
            answer
        },
    )
}

/// A `.tar.gz` holding one file named like the bridge's library, so the
/// install path has something real to unpack and rename.
fn a_bridge_archive(dir: &std::path::Path, library: &str) -> Vec<u8> {
    std::fs::create_dir_all(dir).unwrap();
    let payload = dir.join(library);
    std::fs::write(&payload, b"a pretend shared library").unwrap();
    let tarball = dir.join("archive.tar.gz");
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(dir)
        .arg(library)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(&tarball).unwrap()
}

#[test]
fn installing_downloads_verifies_and_puts_the_library_in_place() {
    // Point the bridges folder at a scratch dir so the test does not touch
    // the real one.
    let scratch = std::env::temp_dir().join(format!("fontelle-ext-test-{}", std::process::id()));
    let _guard = ENV_LOCK.lock().unwrap();
    let _ = std::fs::remove_dir_all(&scratch);
    let bridges = scratch.join("bridges");
    unsafe {
        std::env::set_var("FONTELLE_BRIDGES", &bridges);
    }

    let vst2 = extensions::find("vst2").unwrap();
    let target = "x86_64-unknown-linux-gnu";
    let asset_name = vst2.asset_name(Version::new(0, 1, 0), target);
    let library = "libfontelle_vst2.so";
    let archive = a_bridge_archive(&scratch, library);
    let sums = format!("{}  {asset_name}\n", sha256_hex(&archive));
    let json = format!(
        r#"{{"tag_name":"v0.1.0","html_url":"https://example.test/rel","assets":[
            {{"name":"{asset_name}","browser_download_url":"https://example.test/a"}},
            {{"name":"SHA256SUMS","browser_download_url":"https://example.test/sums"}}
        ]}}"#
    );
    let served: Served = Arc::new(Mutex::new(vec![
        (vst2.latest_url(), Ok(json.into_bytes())),
        (
            "https://example.test/sums".to_string(),
            Ok(sums.into_bytes()),
        ),
        ("https://example.test/a".to_string(), Ok(archive)),
    ]));
    let fetch = fetcher(&served);

    assert!(!extensions::is_installed(vst2));
    let mut seen = 0u64;
    extensions::install(vst2, target, &fetch, &mut |done, _| seen = seen.max(done))
        .expect("the extension installs");
    assert!(seen > 0, "progress was reported");
    assert!(extensions::is_installed(vst2), "it is on disk now");

    // And removing it takes it away.
    extensions::remove(vst2).expect("removes");
    assert!(!extensions::is_installed(vst2));

    unsafe {
        std::env::remove_var("FONTELLE_BRIDGES");
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_download_whose_checksum_is_wrong_is_not_installed() {
    let scratch = std::env::temp_dir().join(format!("fontelle-ext-bad-{}", std::process::id()));
    let _guard = ENV_LOCK.lock().unwrap();
    let _ = std::fs::remove_dir_all(&scratch);
    unsafe {
        std::env::set_var("FONTELLE_BRIDGES", scratch.join("bridges"));
    }
    let vst2 = extensions::find("vst2").unwrap();
    let target = "x86_64-unknown-linux-gnu";
    let asset_name = vst2.asset_name(Version::new(0, 1, 0), target);
    let archive = a_bridge_archive(&scratch, "libfontelle_vst2.so");
    let sums = format!("{}  {asset_name}\n", sha256_hex(b"something else"));
    let json = format!(
        r#"{{"tag_name":"v0.1.0","assets":[
            {{"name":"{asset_name}","browser_download_url":"https://example.test/a"}},
            {{"name":"SHA256SUMS","browser_download_url":"https://example.test/sums"}}
        ]}}"#
    );
    let served: Served = Arc::new(Mutex::new(vec![
        (vst2.latest_url(), Ok(json.into_bytes())),
        (
            "https://example.test/sums".to_string(),
            Ok(sums.into_bytes()),
        ),
        ("https://example.test/a".to_string(), Ok(archive)),
    ]));
    let fetch = fetcher(&served);
    let result = extensions::install(vst2, target, &fetch, &mut |_, _| {});
    assert!(result.is_err(), "a bad checksum is refused");
    assert!(!extensions::is_installed(vst2));
    unsafe {
        std::env::remove_var("FONTELLE_BRIDGES");
    }
    let _ = std::fs::remove_dir_all(&scratch);
}
