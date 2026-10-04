//! A scan that a broken plugin cannot take down, and that does not leave
//! every plugin on the machine loaded in the studio.
//!
//! > *"sometimes they'll just revert back to the init preset ... I find I
//! > have trouble rendering midi to audio"*
//!
//! Sweeping every installed instrument through the studio found the scan
//! itself crashing Fontelle: it loaded every CLAP bundle on the machine into
//! its own process, one after another, and unloaded each. ZamHeadX2's entry
//! sets up FFTW; FFTW kept a pointer into a library that had already been
//! unloaded; the process died inside a plugin nobody had asked for. Which
//! plugins are installed decides whether that happens — the shape of a bug
//! that is on one person's machine and nobody else's.
//!
//! So a bundle that has to be loaded to be read is read by a **child
//! process** (`BundleProber`): one that crashes or hangs is a failure in the
//! list, and the studio goes on. What was read is remembered by the file's
//! size and time, so the next scan asks nobody.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use fontelle_host::{Bridges, BundleProber, PluginScan};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-scan-isolation-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn prober() -> BundleProber {
    BundleProber::new(PathBuf::from(env!("CARGO_BIN_EXE_fontelle-scan-probe")))
        .with_timeout(Duration::from_secs(3))
}

/// A folder with the good bundle and the bad ones beside it.
fn a_folder_with(name: &str, bad: &[&str]) -> PathBuf {
    let folder = scratch(name);
    std::fs::copy(common::bundle(), folder.join("good.clap")).unwrap();
    for spelling in bad {
        std::fs::copy(common::bundle(), folder.join(format!("{spelling}.clap"))).unwrap();
    }
    folder
}

fn keys(scan: &PluginScan) -> Vec<String> {
    let mut keys: Vec<String> = scan.plugins.iter().map(|p| p.key.id.clone()).collect();
    keys.sort();
    keys
}

#[test]
fn a_bundle_that_crashes_as_it_loads_is_a_failure_and_the_scan_goes_on() {
    let folder = a_folder_with("crash", &[fontelle_testplug::CRASHES_ON_LOAD]);
    let scan = PluginScan::of_probed(std::slice::from_ref(&folder), &Bridges::none(), &prober());
    assert!(
        keys(&scan).contains(&common::GAIN.to_string()),
        "{:?}",
        keys(&scan)
    );
    let failure = scan
        .failures
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .contains(fontelle_testplug::CRASHES_ON_LOAD)
        })
        .expect("the crashing bundle is listed as a failure");
    assert!(failure.why.contains("crashed"), "{}", failure.why);
}

#[test]
fn a_bundle_that_never_answers_is_given_up_on() {
    let folder = a_folder_with("hang", &[fontelle_testplug::HANGS_ON_LOAD]);
    let started = std::time::Instant::now();
    let scan = PluginScan::of_probed(&[folder], &Bridges::none(), &prober());
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(keys(&scan).contains(&common::GAIN.to_string()));
    let failure = scan
        .failures
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .contains(fontelle_testplug::HANGS_ON_LOAD)
        })
        .expect("the hanging bundle is listed as a failure");
    assert!(failure.why.contains("did not answer"), "{}", failure.why);
}

#[test]
fn a_probed_scan_finds_what_one_in_this_process_finds() {
    let folder = a_folder_with("same", &[]);
    let probed = PluginScan::of_probed(std::slice::from_ref(&folder), &Bridges::none(), &prober());
    let here = PluginScan::of(&[folder]);
    assert_eq!(probed.plugins, here.plugins);
    assert!(probed.failures.is_empty(), "{:?}", probed.failures);
}

#[test]
fn a_bundle_read_once_is_not_read_again_until_it_changes() {
    let folder = a_folder_with("cache", &[fontelle_testplug::CRASHES_ON_LOAD]);
    let prober = prober();
    PluginScan::of_probed(std::slice::from_ref(&folder), &Bridges::none(), &prober);
    let asked = prober.probes();
    assert_eq!(asked, 2, "both bundles were read");
    let again = PluginScan::of_probed(std::slice::from_ref(&folder), &Bridges::none(), &prober);
    assert_eq!(prober.probes(), asked, "nothing was read twice");
    assert_eq!(again.failures.len(), 1, "a failure is remembered too");

    // Touched: read again.
    let good = folder.join("good.clap");
    let bytes = std::fs::read(&good).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&good, bytes).unwrap();
    PluginScan::of_probed(std::slice::from_ref(&folder), &Bridges::none(), &prober);
    assert_eq!(prober.probes(), asked + 1);

    // And a rescan somebody asked for tries the failures again.
    prober.forget_failures();
    PluginScan::of_probed(&[folder], &Bridges::none(), &prober);
    assert_eq!(prober.probes(), asked + 2);
}

#[test]
fn what_was_read_is_kept_on_disk_for_the_next_start() {
    let folder = a_folder_with("disk", &[]);
    let file = folder.join("..").join("fontelle-scan-isolation-cache.json");
    let _ = std::fs::remove_file(&file);
    let first = prober().with_cache_file(file.clone());
    PluginScan::of_probed(std::slice::from_ref(&folder), &Bridges::none(), &first);
    assert_eq!(first.probes(), 1);
    first.save();

    let next = prober().with_cache_file(file.clone());
    let scan = PluginScan::of_probed(&[folder], &Bridges::none(), &next);
    assert_eq!(next.probes(), 0, "the next start read nothing");
    assert!(keys(&scan).contains(&common::GAIN.to_string()));
    let _ = std::fs::remove_file(&file);
}

/// An LV2 bundle is read from its Turtle without loading its library, so it
/// is read here, as before.
#[cfg(target_os = "linux")]
#[test]
fn an_lv2_bundle_is_still_read_in_this_process() {
    let folder = scratch("lv2");
    let bundle = common::lv2_bundle();
    let to = folder.join(bundle.file_name().unwrap());
    copy_dir(&bundle, &to);
    let prober = prober();
    let scan = PluginScan::of_probed(&[folder], &Bridges::none(), &prober);
    assert!(keys(&scan).contains(&common::LV2_GAIN.to_string()));
    assert_eq!(prober.probes(), 0);
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

// ------------------------------------------------- nothing is unloaded

/// The other half of the same crash: a library unloaded while something it
/// set up is still in use. A plugin library, once loaded, stays for the
/// life of the process — a host dropped (a render's, a test's) no longer
/// takes it with it.
#[cfg(target_os = "linux")]
#[test]
fn a_plugin_library_stays_loaded_after_its_host_is_gone() {
    let mapped = |path: &std::path::Path| {
        let canonical = std::fs::canonicalize(path).unwrap();
        std::fs::read_to_string("/proc/self/maps")
            .unwrap()
            .contains(&*canonical.to_string_lossy())
    };
    let bundle = common::bundle();
    {
        let mut host = fontelle_host::PluginHost::new();
        let key = fontelle_types::PluginKey::clap(common::GAIN);
        let plugin = host.open(&bundle, &key).expect("opens");
        drop(plugin);
    }
    assert!(
        mapped(&bundle),
        "the CLAP library was unloaded with its host"
    );

    let vst3 = common::vst3_bundle();
    {
        let mut host = fontelle_host::PluginHost::new();
        let key =
            fontelle_types::PluginKey::new(fontelle_types::PluginFormat::Vst3, common::VST3_GAIN);
        let plugin = host.open(&vst3, &key).expect("opens");
        drop(plugin);
    }
    let library = std::fs::read_dir(vst3.join("Contents/x86_64-linux"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    assert!(
        mapped(&library),
        "the VST 3 library was unloaded with its host"
    );

    // Not an LV2 binary: lilv unloads it with its last instance, which is
    // when its plugin is finished with — and one kept to the end of the
    // process ran its static destructors after the libraries they used had
    // run theirs (Mephisto's Faust, after LLVM's).
}
