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
