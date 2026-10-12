//! `fontelle-asio` stays a program apart (`docs/asio.md`, "Licences"):
//! nothing of Fontelle's depends on it, it depends on nothing of Fontelle's
//! but the MIT/Apache protocol crate, and its licence says so.

use std::path::Path;

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<this>")
}

/// The dependency names in a manifest's dependency tables.
fn dependencies(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
            continue;
        }
        if in_deps
            && let Some((name, _)) = line.split_once('=')
            && !line.starts_with('#')
        {
            names.push(name.trim().to_string());
        }
    }
    names
}

#[test]
fn nothing_in_the_workspace_depends_on_the_gpl_helper() {
    let mut checked = 0;
    for dir in ["crates", "xtask"] {
        let path = workspace().join(dir);
        let manifests: Vec<_> = if dir == "xtask" {
            vec![path.join("Cargo.toml")]
        } else {
            std::fs::read_dir(&path)
                .unwrap()
                .map(|e| e.unwrap().path().join("Cargo.toml"))
                .collect()
        };
        for manifest in manifests {
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            if manifest.parent().unwrap().ends_with("fontelle-asio-host") {
                continue;
            }
            checked += 1;
            assert!(
                !dependencies(&text)
                    .iter()
                    .any(|d| d == "fontelle-asio-host"),
                "{} depends on the GPL helper",
                manifest.display()
            );
        }
    }
    assert!(checked > 10, "only {checked} manifests found");
}

#[test]
fn the_helper_uses_nothing_of_fontelles_but_the_protocol() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let ours: Vec<_> = dependencies(&text)
        .into_iter()
        .filter(|d| d.starts_with("fontelle"))
        .collect();
    assert_eq!(ours, ["fontelle-audio-ipc"]);
    assert!(text.contains("license = \"GPL-3.0-only\""), "{text}");

    let protocol =
        std::fs::read_to_string(workspace().join("crates/fontelle-audio-ipc/Cargo.toml")).unwrap();
    assert!(protocol.contains("license.workspace = true"));
    let root = std::fs::read_to_string(workspace().join("Cargo.toml")).unwrap();
    assert!(root.contains("license = \"MIT OR Apache-2.0\""));
}

#[test]
fn the_licences_are_beside_it() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let gpl = std::fs::read_to_string(here.join("LICENSE")).unwrap();
    assert!(gpl.contains("GNU GENERAL PUBLIC LICENSE"));
    assert!(gpl.contains("Version 3, 29 June 2007"));
    let steinberg = std::fs::read_to_string(here.join("STEINBERG-ASIO-SDK-LICENSE.txt")).unwrap();
    assert!(steinberg.contains("General Public License (GPL) Version 3"));
}
