//! Finding plugins on the machine.

mod common;

use fontelle_host::{PluginScan, scan_bundle, search_paths};
use fontelle_types::PluginFormat;

#[test]
fn a_bundle_reports_every_plugin_in_it() {
    let found = scan_bundle(&common::bundle()).expect("the test bundle loads");
    // The gain, the sine, the face, and the sine again speaking only CLAP.
    assert_eq!(found.len(), 4, "{found:#?}");
    let ids: Vec<_> = found.iter().map(|p| p.key.id.as_str()).collect();
    assert!(ids.contains(&common::GAIN), "{ids:?}");
    assert!(ids.contains(&common::SINE), "{ids:?}");
    assert!(ids.contains(&common::SINE_CLAP_ONLY), "{ids:?}");
}

#[test]
fn a_plugin_carries_what_a_menu_needs_to_show_it() {
    let found = scan_bundle(&common::bundle()).unwrap();
    let gain = found.iter().find(|p| p.key.id == common::GAIN).unwrap();
    assert_eq!(gain.name, "Fontelle Test Gain");
    assert_eq!(gain.vendor, "Fopull LLC");
    assert_eq!(gain.version, "1.0.0");
    assert_eq!(gain.key.format, PluginFormat::Clap);
    assert_eq!(gain.path, common::bundle());
}

#[test]
fn what_a_plugin_can_be_is_read_off_what_it_says_it_is() {
    let found = scan_bundle(&common::bundle()).unwrap();
    let gain = found.iter().find(|p| p.key.id == common::GAIN).unwrap();
    let sine = found.iter().find(|p| p.key.id == common::SINE).unwrap();

    assert!(gain.is_effect(), "{gain:#?}");
    assert!(!gain.is_instrument(), "{gain:#?}");
    assert!(sine.is_instrument(), "{sine:#?}");
    assert!(!sine.is_effect(), "{sine:#?}");
}

#[test]
fn something_that_is_not_a_plugin_is_a_refusal_rather_than_a_crash() {
    let text = std::env::temp_dir().join("fontelle-not-a-plugin.clap");
    std::fs::write(&text, b"this is not a shared library").unwrap();
    assert!(scan_bundle(&text).is_err());
    let _ = std::fs::remove_file(&text);
}

#[test]
fn a_folder_of_plugins_is_walked_and_what_failed_is_kept() {
    let dir = std::env::temp_dir().join("fontelle-scan-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Some Vendor")).unwrap();
    std::fs::copy(common::bundle(), dir.join("Some Vendor/Test.clap")).unwrap();
    std::fs::write(dir.join("Broken.clap"), b"nope").unwrap();
    std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();

    let scan = PluginScan::of(std::slice::from_ref(&dir));
    // The gain, the sine, the face, and the sine again speaking only CLAP —
    // see `fontelle_testplug::SINE_CLAP_ONLY`.
    assert_eq!(scan.plugins.len(), 4, "{:#?}", scan.plugins);
    assert_eq!(scan.failures.len(), 1, "{:#?}", scan.failures);
    assert!(scan.failures[0].path.ends_with("Broken.clap"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_scan_is_sorted_so_the_menu_does_not_reshuffle_itself() {
    let dir = std::env::temp_dir().join("fontelle-scan-order");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(common::bundle(), dir.join("Z.clap")).unwrap();
    std::fs::copy(common::bundle(), dir.join("A.clap")).unwrap();

    let scan = PluginScan::of(std::slice::from_ref(&dir));
    let names: Vec<_> = scan.plugins.iter().map(|p| p.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|name| name.to_lowercase());
    assert_eq!(names, sorted, "{names:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_plugin_found_twice_is_listed_once() {
    let dir = std::env::temp_dir().join("fontelle-scan-dupes");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(common::bundle(), dir.join("One.clap")).unwrap();
    std::fs::copy(common::bundle(), dir.join("Two.clap")).unwrap();

    let scan = PluginScan::of(std::slice::from_ref(&dir));
    assert_eq!(scan.plugins.len(), 4, "{:#?}", scan.plugins);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_that_is_not_there_is_not_a_failure() {
    let scan = PluginScan::of(&[std::env::temp_dir().join("fontelle-no-such-folder")]);
    assert!(scan.plugins.is_empty());
    assert!(scan.failures.is_empty());
}

#[test]
fn the_places_plugins_are_installed_are_looked_in_without_being_asked() {
    let paths = search_paths();
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.is_absolute()), "{paths:?}");
}

/// > Plugins installed by Fedora packages are never found.
///
/// Fedora, openSUSE and RHEL put 64-bit libraries under `lib64`, and their
/// plugin packages follow: `dnf install lsp-plugins-clap` lands in
/// `/usr/lib64/clap`. The CLAP list names only `lib`, which is Debian's and
/// Arch's layout, so on those systems every packaged plugin was missing.
#[cfg(target_os = "linux")]
#[test]
fn the_lib64_folders_fedora_installs_plugins_in_are_looked_in() {
    let paths = search_paths();
    for folder in [
        "/usr/lib/clap",
        "/usr/lib64/clap",
        "/usr/local/lib/clap",
        "/usr/local/lib64/clap",
        "/usr/lib64/lv2",
        "/usr/local/lib64/lv2",
        "/usr/lib64/vst3",
        "/usr/local/lib64/vst3",
    ] {
        assert!(
            paths.contains(&folder.into()),
            "{folder} missing: {paths:?}"
        );
    }
}

// --------------------------------------------- folders the way distros lay them
//
// Found by scanning every plugin installed on a CachyOS machine
// (`fontelle-app/tests/real_plugin_sessions.rs`, `the_studios_scan_of_this_machine`).

fn scratch(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("fontelle-scanning-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(target_os = "linux")]
fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// An LV2 bundle is a folder with a `manifest.ttl` in it; `.lv2` on the end
/// is a habit, not the rule. setBfree's `b_synth` has no suffix, and was not
/// found — its libraries were offered to the VST 2 bridge instead.
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_bundle_without_the_suffix_is_still_found() {
    let folder = scratch("lv2-suffix");
    copy_dir(&common::lv2_bundle(), &folder.join("fontelle_test"));
    let scan = PluginScan::of(&[folder]);
    assert!(
        scan.plugins.iter().any(|p| p.key.id == common::LV2_GAIN),
        "{:?} {:?}",
        scan.plugins,
        scan.failures
    );
    assert!(scan.failures.is_empty(), "{:?}", scan.failures);
}

/// On Linux and Windows a CLAP plugin is a **file**. A folder called
/// `Cardinal.clap` (how Cardinal is packaged: `Cardinal.clap/Cardinal.clap`,
/// `…/CardinalFX.clap`, `…/CardinalSynth.clap`) is a folder of them, and was
/// reported as a bundle that would not open.
#[cfg(not(target_os = "macos"))]
#[test]
fn a_folder_named_like_a_clap_bundle_is_a_folder_of_them() {
    let folder = scratch("clap-folder");
    let inner = folder.join("Suite.clap");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::copy(common::bundle(), inner.join("Suite.clap")).unwrap();
    let scan = PluginScan::of(&[folder]);
    assert!(
        scan.plugins.iter().any(|p| p.key.id == common::GAIN),
        "{:?} {:?}",
        scan.plugins,
        scan.failures
    );
    assert!(scan.failures.is_empty(), "{:?}", scan.failures);
}

/// `/usr/lib64` is a link to `/usr/lib` on Arch and the real folder on
/// Fedora; both are searched, and a folder reached twice is walked once —
/// a bundle that would not open was reported twice.
#[cfg(unix)]
#[test]
fn a_folder_reached_twice_is_walked_once() {
    let folder = scratch("twice");
    let real = folder.join("lib").join("clap");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("broken.clap"), b"not a library").unwrap();
    std::os::unix::fs::symlink(folder.join("lib"), folder.join("lib64")).unwrap();
    let scan = PluginScan::of(&[real, folder.join("lib64").join("clap")]);
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
}
