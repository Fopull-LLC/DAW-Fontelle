//! Every sound Fontelle ships or re-hosts carries a licence that lets it.
//!
//! > *"A licensing check IMPLEMENTED for any font bundled or re-hosted: the
//! > app carries each bundled font's licence text, and the build refuses a
//! > font whose licence does not explicitly allow redistribution."* — hub
//! > card 0430
//!
//! The manifest is Rust (`fontelle_core::bundled_sounds`), so a licence
//! outside the permitted list cannot even be written into it, and each
//! entry's text is `include_str!`ed, so a missing file is a compile error.
//! What the compiler cannot see is the tree: a `.sf2` dropped into the
//! repository, or a new folder of recordings under `assets/`, with no entry
//! at all. These tests are that half, and CI runs them on every push.

use std::path::{Path, PathBuf};

use fontelle_core::bundled_sounds::{self, BUNDLED_SOUNDS, Refusal, SoundLicence};
use fontelle_core::factory_samples::FactorySampleSet;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file under `root`, relative to it, not following links. Builds,
/// version control and worktrees are skipped here only for speed:
/// `unclaimed` leaves them out by itself (the stray-soundfont test says so).
fn files(root: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, relative: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let path = relative.join(&name);
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && ![".git", "target", ".claude"].iter().any(|s| name == *s) {
                walk(root, &path, out);
            } else if kind.is_file() {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out);
    out
}

#[test]
fn the_permitted_list_is_the_conservative_one_and_nothing_else() {
    let ids: Vec<&str> = SoundLicence::PERMITTED.iter().map(|l| l.spdx()).collect();
    assert_eq!(
        ids,
        [
            "MIT",
            "CC0-1.0",
            "CC-BY-3.0",
            "CC-BY-4.0",
            "LicenseRef-Public-Domain",
            "Apache-2.0",
            "LicenseRef-GeneralUser-GS",
            "MIT OR Apache-2.0",
        ]
    );
    for licence in SoundLicence::PERMITTED {
        assert_eq!(SoundLicence::judge(licence.spdx()), Ok(licence));
        assert!(!licence.name().is_empty());
        assert!(!licence.marker().is_empty());
    }
    // Spelled the way people write them, too.
    assert_eq!(
        SoundLicence::judge("cc-by-3.0"),
        Ok(SoundLicence::CcBy3),
        "case is not a licence"
    );
    assert_eq!(
        SoundLicence::judge("GeneralUser GS License v2.0"),
        Ok(SoundLicence::GeneralUserGs)
    );
}

#[test]
fn what_cannot_be_sold_or_changed_or_passed_on_is_refused_with_its_reason() {
    for (id, why) in [
        ("CC-BY-NC-4.0", Refusal::NonCommercial),
        ("CC-BY-NC-SA-3.0", Refusal::NonCommercial),
        ("CC-BY-ND-4.0", Refusal::NoDerivatives),
        ("CC-BY-NC-ND-4.0", Refusal::NonCommercial),
        ("CC-BY-SA-4.0", Refusal::ShareAlike),
        ("GPL-2.0-or-later", Refusal::Copyleft),
        ("LGPL-3.0-only", Refusal::Copyleft),
        ("Free for personal use", Refusal::PersonalUseOnly),
        ("free for non-commercial use only", Refusal::NonCommercial),
        ("Freeware", Refusal::Unknown),
        ("", Refusal::Unknown),
    ] {
        assert_eq!(SoundLicence::judge(id), Err(why), "{id:?}");
        assert!(!why.reason().is_empty());
    }
}

#[test]
fn every_factory_sample_set_names_a_permitted_licence_whose_text_is_in_the_tree() {
    for set in FactorySampleSet::ALL {
        let entry = set.bundled();
        assert!(
            BUNDLED_SOUNDS.iter().any(|e| std::ptr::eq(e, entry)),
            "{set:?}'s entry is in the manifest"
        );
        assert!(SoundLicence::PERMITTED.contains(&entry.licence));
        let on_disk = std::fs::read_to_string(repo().join(entry.licence_file))
            .unwrap_or_else(|e| panic!("{}: {e}", entry.licence_file));
        assert_eq!(
            on_disk, entry.licence_text,
            "the binary carries the file itself"
        );
        assert!(
            entry.licence_text.contains(entry.licence.marker()),
            "{}'s text is the licence it claims",
            entry.id
        );
    }
    // The grand is Alexander Holm's Salamander, CC BY 3.0; the kits are ours.
    let grand = FactorySampleSet::GrandSoft.bundled();
    assert!(std::ptr::eq(grand, FactorySampleSet::GrandHard.bundled()));
    assert_eq!(grand.licence, SoundLicence::CcBy3);
    assert_eq!(grand.author, "Alexander Holm");
    let kits = FactorySampleSet::KitStudio.bundled();
    assert!(std::ptr::eq(kits, FactorySampleSet::Kit808.bundled()));
    assert_eq!(kits.licence, SoundLicence::Own);
}

#[test]
fn the_manifest_is_whole_and_says_where_each_sound_came_from() {
    let mut ids: Vec<&str> = BUNDLED_SOUNDS.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), BUNDLED_SOUNDS.len(), "ids are unique");
    for entry in BUNDLED_SOUNDS {
        assert!(!entry.title.is_empty() && !entry.author.is_empty());
        assert!(entry.source_url.starts_with("https://") || entry.licence == SoundLicence::Own);
        assert!(!entry.paths.is_empty());
        for path in entry.paths {
            assert!(repo().join(path).exists(), "{}: {path}", entry.id);
        }
        assert!(SoundLicence::PERMITTED.contains(&entry.licence));
        assert!(entry.licence_text.contains(entry.licence.marker()));
    }
}

#[test]
fn a_soundfont_with_no_entry_anywhere_in_the_tree_is_found() {
    let tree = [
        "assets/flopsynth/samples/grand/soft/C4.wav",
        "assets/flopsynth/samples/grand/README.md",
        "assets/flopsynth/samples/kit/808/Kick.wav",
        "assets/soundfonts/Mystery.SF2",
        "crates/x/tests/data/fixture.sf3",
        "docs/old.sfz",
        "assets/new-set/Hit.flac",
        "assets/branding/logo.png",
        // Not shipped, so not this rule's: a test's own audio, and builds,
        // version control and the agents' worktrees.
        "crates/x/tests/data/tone.ogg",
        "target/debug/build/out.sf2",
        ".git/objects/pack.sf2",
        ".claude/worktrees/a/assets/x.wav",
    ];
    let mut found = bundled_sounds::unclaimed(tree.iter().map(Path::new));
    found.sort();
    assert_eq!(
        found,
        [
            "assets/new-set/Hit.flac",
            "assets/soundfonts/Mystery.SF2",
            "crates/x/tests/data/fixture.sf3",
            "docs/old.sfz",
        ]
        .map(PathBuf::from)
    );
}

#[test]
fn nothing_in_this_tree_is_bundled_without_an_entry() {
    let all = files(&repo());
    assert!(
        all.iter()
            .any(|p| p.starts_with("assets/flopsynth/samples/grand/soft")),
        "the walk reaches what the binary embeds, so a pass is not an empty walk"
    );
    let stray = bundled_sounds::unclaimed(all);
    assert!(
        stray.is_empty(),
        "sounds with no entry in fontelle_core::bundled_sounds (and licenses/SOUNDS.md): {stray:?}"
    );
}

#[test]
fn the_release_archives_and_the_credits_carry_every_licence() {
    let release = std::fs::read_to_string(repo().join(".github/workflows/release.yml")).unwrap();
    assert!(release.contains("cp -R licenses"), "the tar.gz archives");
    assert!(
        release.contains("Copy-Item -Recurse licenses"),
        "the Windows zip"
    );
    let index = std::fs::read_to_string(repo().join("licenses/SOUNDS.md")).unwrap();
    for entry in BUNDLED_SOUNDS {
        // Under `licenses/`, or one of the two the archives carry at the top.
        let top = ["LICENSE-MIT", "LICENSE-APACHE"].contains(&entry.licence_file)
            && release.contains("cp LICENSE-MIT LICENSE-APACHE")
            && release.contains("Copy-Item LICENSE-MIT, LICENSE-APACHE");
        assert!(
            entry.licence_file.starts_with("licenses/") || top,
            "{} travels with the program",
            entry.licence_file
        );
        for fact in [entry.id, entry.author, entry.licence.spdx()] {
            assert!(index.contains(fact), "licenses/SOUNDS.md names {fact}");
        }
    }
    let readme = std::fs::read_to_string(repo().join("README.md")).unwrap();
    assert!(
        readme.contains("licenses/SOUNDS.md"),
        "the README's licence section"
    );
}
