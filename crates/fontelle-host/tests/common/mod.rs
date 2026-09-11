#![allow(dead_code)]

//! Where the test bundle is.

use std::path::PathBuf;

/// The `fontelle-testplug` bundle cargo built beside this test binary.
///
/// Found by walking up from the running test rather than by a path written
/// down here, because `target/` moves: `CARGO_TARGET_DIR`, a workspace built
/// from elsewhere, and `cargo test --release` all put it somewhere different,
/// and all three put it exactly two levels above the test binary.
pub fn bundle() -> PathBuf {
    let mut path = std::env::current_exe().expect("a test binary knows where it is");
    path.pop(); // deps/
    path.pop(); // debug/ or release/
    let file = if cfg!(target_os = "windows") {
        "fontelle_testplug.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    };
    let built = path.join(file);
    assert!(
        built.exists(),
        "the test plugin has not been built: {} — run `cargo build -p fontelle-testplug`",
        built.display()
    );
    assert_bundle_is_fresh(&built);
    // Copied to the name a CLAP bundle actually has. Cargo names a `cdylib`
    // the way the platform names a shared library, and the scanner looks for
    // `.clap` — which is the real rule and worth testing against rather than
    // relaxing.
    //
    // Copied through a temporary and renamed, every time: rename is atomic, so
    // two test binaries running at once cannot read a half-written bundle, and
    // copying unconditionally is what stops a stale bundle from a previous
    // build being what the tests actually load.
    // Copied **once per test binary**, and through a rename. Once, because
    // tests inside a binary run in parallel and two of them writing the same
    // staging file is how a half-written bundle got loaded; through a rename,
    // because two test binaries run in parallel too and rename is atomic.
    static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let bundle = path.join("fontelle-testplug.clap");
            let staging = path.join(format!(
                "fontelle-testplug.{}.{:?}.tmp",
                std::process::id(),
                std::thread::current().id()
            ));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, &bundle);
            }
            let _ = std::fs::remove_file(&staging);
            bundle
        })
        .clone()
}

pub const GAIN: &str = "com.fopull.fontelle.testgain";
pub const SINE: &str = "com.fopull.fontelle.testsine";
/// The sine again, whose note port speaks **only** CLAP's own dialect — see
/// `fontelle_testplug::SINE_CLAP_ONLY`.
pub const SINE_CLAP_ONLY: &str = fontelle_testplug::SINE_CLAP_ONLY;

/// Refuses to run against a bundle older than the source it was built from.
///
/// Worth its own check because the failure it prevents is deeply confusing:
/// depending on `fontelle-testplug` builds its **rlib**, and the `.clap` a
/// host test loads is the **cdylib**, which only a build of that package
/// itself produces. So `cargo test -p ...` alone can run new tests against an
/// old plugin and fail for reasons that are nowhere in the diff.
fn assert_bundle_is_fresh(built: &std::path::Path) {
    let Ok(binary) = built.metadata().and_then(|m| m.modified()) else {
        return;
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fontelle-testplug/src");
    let Ok(entries) = std::fs::read_dir(&source) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(changed) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        assert!(
            changed <= binary,
            "{} is newer than the built plugin — run `cargo build -p fontelle-testplug`",
            entry.path().display()
        );
    }
}

// ------------------------------------------------------------------ LV2

pub const LV2_GAIN: &str = fontelle_testlv2::GAIN_URI;
pub const LV2_SINE: &str = fontelle_testlv2::SINE_URI;

/// The `fontelle-testlv2` bundle, assembled beside the test binary.
///
/// An LV2 bundle is a **folder**: the shared library and the Turtle files
/// that describe it. Cargo only builds the library, so this writes the folder
/// the way an installer would — the `.so` under the name the manifest gives
/// it, and the two `.ttl` files beside it — and hands back its path.
///
/// The same rules as [`bundle`]: found relative to the test binary, refused
/// if the built library is older than its source, written once per test
/// binary and put in place by a rename so a parallel test never reads half a
/// bundle.
#[cfg(target_os = "linux")]
pub fn lv2_bundle() -> PathBuf {
    let mut path = std::env::current_exe().expect("a test binary knows where it is");
    path.pop();
    path.pop();
    let built = path.join("libfontelle_testlv2.so");
    assert!(
        built.exists(),
        "the LV2 test plugin has not been built: {} — run `cargo build -p fontelle-testlv2`",
        built.display()
    );
    assert_source_is_older(&built, "../fontelle-testlv2");

    static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let bundle = path.join("fontelle-testlv2.lv2");
            let staging = path.join(format!(
                "fontelle-testlv2.{}.{:?}.tmp",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).expect("a staging folder");
            std::fs::copy(&built, staging.join(fontelle_testlv2::BINARY_NAME))
                .expect("the library copies");
            std::fs::write(staging.join("manifest.ttl"), fontelle_testlv2::MANIFEST_TTL).unwrap();
            std::fs::write(staging.join("testlv2.ttl"), fontelle_testlv2::PLUGIN_TTL).unwrap();
            // A directory rename over an existing directory fails, so the old
            // bundle goes first; the window in which neither exists is one
            // this process's own tests cannot observe, because they all wait
            // on this `OnceLock`.
            let _ = std::fs::remove_dir_all(&bundle);
            std::fs::rename(&staging, &bundle).expect("the bundle lands");
            bundle
        })
        .clone()
}

/// [`assert_bundle_is_fresh`], for any fixture crate.
fn assert_source_is_older(built: &std::path::Path, crate_dir: &str) {
    let Ok(binary) = built.metadata().and_then(|m| m.modified()) else {
        return;
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(crate_dir);
    for sub in ["src", "bundle"] {
        let Ok(entries) = std::fs::read_dir(source.join(sub)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(changed) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            assert!(
                changed <= binary,
                "{} is newer than the built plugin — run `cargo build -p {}`",
                entry.path().display(),
                crate_dir.trim_start_matches("../")
            );
        }
    }
}

// -------------------------------------------------------------- bridges

pub const BRIDGED_GAIN: &str = fontelle_testbridge::GAIN_ID;
pub const BRIDGED_SINE: &str = fontelle_testbridge::SINE_ID;

/// The folder holding the built `fontelle-testbridge` library — what
/// Fontelle's own bridges folder looks like with one bridge installed.
#[cfg(target_os = "linux")]
pub fn bridge_folder() -> PathBuf {
    let mut path = std::env::current_exe().expect("a test binary knows where it is");
    path.pop();
    path.pop();
    let built = path.join("libfontelle_testbridge.so");
    assert!(
        built.exists(),
        "the test bridge has not been built: {} — run `cargo build -p fontelle-testbridge`",
        built.display()
    );
    assert_source_is_older(&built, "../fontelle-testbridge");
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = path.join("fontelle-test-bridges");
            let _ = std::fs::create_dir_all(&folder);
            let staging = folder.join(format!("staging.{}.tmp", std::process::id()));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, folder.join("libfontelle_testbridge.so"));
            }
            let _ = std::fs::remove_file(&staging);
            folder
        })
        .clone()
}

/// A bundle of the test bridge's format: a folder with the extension the
/// bridge names, holding the listing the bridge reads.
#[cfg(target_os = "linux")]
pub fn bridged_bundle() -> PathBuf {
    static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let mut path = std::env::current_exe().unwrap();
            path.pop();
            path.pop();
            // In a folder of its own, so a test that scans the folder the
            // bundle sits in scans one bundle and not all of `target/`.
            let bundle = path.join("fontelle-test-bridged").join(format!(
                "fontelle-testbridge.{}",
                fontelle_testbridge::EXTENSION
            ));
            let _ = std::fs::create_dir_all(&bundle);
            std::fs::write(
                bundle.join(fontelle_testbridge::MANIFEST),
                format!(
                    "{}\n{}\n",
                    fontelle_testbridge::GAIN_ID,
                    fontelle_testbridge::SINE_ID
                ),
            )
            .unwrap();
            bundle
        })
        .clone()
}
