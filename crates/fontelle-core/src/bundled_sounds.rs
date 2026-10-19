//! Every sound Fontelle ships, and the licence that lets it.
//!
//! > *"A licensing check IMPLEMENTED for any font bundled or re-hosted: the
//! > app carries each bundled font's licence text, and the build refuses a
//! > font whose licence does not explicitly allow redistribution."* — hub
//! > card 0430
//!
//! One entry per thing in the tree that is a sound someone could take out
//! of a download: a soundfont, or a folder of recordings the binary embeds
//! (`factory_samples`). Fontelle is going to sell packs, and a pack that
//! leans on a sound we had no right to pass on is a takedown, not a sale —
//! so the rule is written where it cannot be skipped:
//!
//! - **[`SoundLicence`] is the permitted list.** It has no variant for a
//!   licence we refuse, so an entry under one cannot be written.
//! - **Each entry `include_str!`s its licence text.** The text is in the
//!   binary, and a missing file is a compile error, not a forgotten copy.
//! - **[`FactorySampleSet::bundled`] is an exhaustive match**, so a new
//!   factory set does not compile until it has an entry.
//! - **[`unclaimed`] walks the tree** for what the compiler cannot see — a
//!   `.sf2` dropped into the repository, a new folder of recordings under
//!   `assets/` — and `tests/bundled_sounds.rs` fails CI on anything it finds.
//!
//! `licenses/SOUNDS.md` is the human-readable side of the same list, and the
//! release workflow copies `licenses/` into every archive.

use std::path::{Path, PathBuf};

use crate::factory_samples::FactorySampleSet;

/// A licence a bundled or re-hosted sound may be under. **Only these.**
///
/// Conservative on purpose: each one says in its own words that the work may
/// be copied, changed and passed on, commercially, by anyone, with at most a
/// credit asked in return. What is left out is left out for a reason
/// [`Refusal`] gives, and adding to this list is Ty's decision, not a
/// convenience for a sound somebody wants to ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundLicence {
    Mit,
    Cc0,
    /// Credit the author and say what was changed (CC BY 3.0 §4(b)).
    CcBy3,
    CcBy4,
    /// A written dedication to the public domain by the author. The entry's
    /// text is that statement; "I found it on a free site" is not one.
    PublicDomain,
    Apache2,
    /// S. Christian Collins's own licence for GeneralUser GS, which allows
    /// redistribution and use in commercial work. Named as itself rather
    /// than folded into another, and its text is the one the font ships.
    GeneralUserGs,
    /// Made in Fontelle from no outside material: the workspace's own
    /// licence (MIT OR Apache-2.0). The kits, and any pack built from our
    /// own recordings.
    Own,
}

impl SoundLicence {
    pub const PERMITTED: [Self; 8] = [
        Self::Mit,
        Self::Cc0,
        Self::CcBy3,
        Self::CcBy4,
        Self::PublicDomain,
        Self::Apache2,
        Self::GeneralUserGs,
        Self::Own,
    ];

    /// The SPDX identifier, or a `LicenseRef-` where SPDX has none.
    pub fn spdx(self) -> &'static str {
        match self {
            Self::Mit => "MIT",
            Self::Cc0 => "CC0-1.0",
            Self::CcBy3 => "CC-BY-3.0",
            Self::CcBy4 => "CC-BY-4.0",
            Self::PublicDomain => "LicenseRef-Public-Domain",
            Self::Apache2 => "Apache-2.0",
            Self::GeneralUserGs => "LicenseRef-GeneralUser-GS",
            Self::Own => "MIT OR Apache-2.0",
        }
    }

    /// What a person reads.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mit => "MIT License",
            Self::Cc0 => "CC0 1.0 (public domain dedication)",
            Self::CcBy3 => "Creative Commons Attribution 3.0",
            Self::CcBy4 => "Creative Commons Attribution 4.0",
            Self::PublicDomain => "Public domain (author's dedication)",
            Self::Apache2 => "Apache License 2.0",
            Self::GeneralUserGs => "GeneralUser GS License",
            Self::Own => "Fontelle's own (MIT OR Apache-2.0)",
        }
    }

    /// A phrase the licence's own text contains — so an entry that points
    /// at the wrong file, or at an empty one, is caught rather than shipped
    /// as though it were the licence it claims.
    pub fn marker(self) -> &'static str {
        match self {
            Self::Mit | Self::Own => "Permission is hereby granted, free of charge",
            Self::Cc0 => "CC0 1.0 Universal",
            Self::CcBy3 => "Attribution 3.0",
            Self::CcBy4 => "Attribution 4.0 International",
            Self::PublicDomain => "public domain",
            Self::Apache2 => "Apache License",
            Self::GeneralUserGs => "GeneralUser GS",
        }
    }

    /// Whether a licence, as somebody wrote it, is one a sound may be
    /// shipped under — and if not, why not.
    ///
    /// The refusals are read **before** the permitted names, so a licence
    /// that names a permitted one and then takes something back (`CC-BY-NC`
    /// begins `CC-BY`) is refused. Anything not recognised is
    /// [`Refusal::Unknown`]: the default answer to "may we pass this on?"
    /// is no until somebody has read the terms. The in-app library and a
    /// pack exporter ask this same question (`docs/soundfont-suite.md`).
    pub fn judge(licence: &str) -> Result<Self, Refusal> {
        let said = licence.trim().to_ascii_uppercase();
        let has = |word: &str| said.contains(word);
        if said.is_empty() {
            return Err(Refusal::Unknown);
        }
        if has("-NC") || has("NONCOMMERCIAL") || has("NON-COMMERCIAL") || has("NON COMMERCIAL") {
            return Err(Refusal::NonCommercial);
        }
        if has("PERSONAL") {
            return Err(Refusal::PersonalUseOnly);
        }
        if has("-ND") || has("NODERIV") || has("NO DERIV") {
            return Err(Refusal::NoDerivatives);
        }
        if has("GPL") {
            return Err(Refusal::Copyleft);
        }
        if has("-SA") || has("SHAREALIKE") || has("SHARE-ALIKE") || has("SHARE ALIKE") {
            return Err(Refusal::ShareAlike);
        }
        if let Some(licence) = Self::PERMITTED
            .into_iter()
            .find(|l| l.spdx().eq_ignore_ascii_case(&said))
        {
            return Ok(licence);
        }
        if has("GENERALUSER") {
            return Ok(Self::GeneralUserGs);
        }
        Err(Refusal::Unknown)
    }
}

/// Why a licence is not on [`SoundLicence::PERMITTED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// `NC`: Fontelle is sold, and so are its packs.
    NonCommercial,
    /// `ND`: every factory sound is trimmed, resampled or rebuilt, and a
    /// pack is a changed work by definition.
    NoDerivatives,
    /// `SA`: allowed to pass on, but it may pull a pack built on it under
    /// the same licence. Ty's call, per sound, not a default.
    ShareAlike,
    /// GPL/LGPL: the same pull, onto a program that is MIT OR Apache-2.0.
    Copyleft,
    /// "Free for personal use" grants no right to pass it on at all.
    PersonalUseOnly,
    /// Unrecognised, or "freeware" with no terms: no until someone has read
    /// them and added a variant.
    Unknown,
}

impl Refusal {
    pub fn reason(self) -> &'static str {
        match self {
            Self::NonCommercial => "non-commercial only, and Fontelle and its packs are sold",
            Self::NoDerivatives => "no derivatives, and shipping a sound in Fontelle changes it",
            Self::ShareAlike => "share-alike could bind a pack built on it to the same licence",
            Self::Copyleft => "copyleft could bind work built on it to the same licence",
            Self::PersonalUseOnly => "personal use only, which grants no right to pass it on",
            Self::Unknown => "no recognised licence that allows redistribution",
        }
    }
}

/// One sound Fontelle ships: where it is, whose it is, and under what.
#[derive(Debug)]
pub struct BundledSound {
    /// Stable, and what `licenses/SOUNDS.md` lists it by.
    pub id: &'static str,
    pub title: &'static str,
    pub author: &'static str,
    /// Where it came from; empty only for [`SoundLicence::Own`].
    pub source_url: &'static str,
    pub licence: SoundLicence,
    /// The licence text's path from the repository root. Under
    /// `licenses/`, which every release archive carries, or one of the two
    /// licence files at the top that the archives carry too.
    pub licence_file: &'static str,
    /// That file, compiled in.
    pub licence_text: &'static str,
    /// Repository paths this entry covers: files, or folders whose every
    /// file it covers.
    pub paths: &'static [&'static str],
}

/// The manifest. Add to it in the same commit as the sound, with the
/// licence text beside it under `licenses/sounds/<id>/` and a row in
/// `licenses/SOUNDS.md`.
pub static BUNDLED_SOUNDS: &[BundledSound] = &[SALAMANDER_GRAND, FONTELLE_KITS];

const SALAMANDER_GRAND: BundledSound = BundledSound {
    id: "salamander-grand-piano",
    title: "Salamander Grand Piano (Yamaha C5), cut for the factory Grand Piano",
    author: "Alexander Holm",
    source_url: "https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html",
    licence: SoundLicence::CcBy3,
    licence_file: "licenses/sounds/salamander-grand-piano/LICENSE",
    licence_text: include_str!("../../../licenses/sounds/salamander-grand-piano/LICENSE"),
    paths: &["assets/flopsynth/samples/grand"],
};

const FONTELLE_KITS: BundledSound = BundledSound {
    id: "fontelle-kits",
    title: "Studio and 808 kits, recorded from Fontelle's drum machine",
    author: "Fopull LLC",
    source_url: "",
    licence: SoundLicence::Own,
    licence_file: "LICENSE-MIT",
    licence_text: include_str!("../../../LICENSE-MIT"),
    paths: &["assets/flopsynth/samples/kit"],
};

impl FactorySampleSet {
    /// This set's entry in [`BUNDLED_SOUNDS`]. Exhaustive, so a new set is
    /// a compile error until somebody has said whose it is.
    pub fn bundled(self) -> &'static BundledSound {
        match self {
            Self::GrandSoft | Self::GrandHard => &BUNDLED_SOUNDS[0],
            Self::KitStudio | Self::Kit808 => &BUNDLED_SOUNDS[1],
        }
    }
}

/// Soundfont containers, wherever they sit: a soundfont in the tree is one
/// somebody may ship, test fixture or not.
const SOUNDFONTS: [&str; 5] = ["sf2", "sf3", "sfz", "sfark", "dls"];
/// Recordings, which count under `assets/` — what the binary embeds and the
/// archives carry. A test's own audio (`crates/*/tests/data`) ships nowhere.
const RECORDINGS: [&str; 7] = ["wav", "flac", "ogg", "mp3", "opus", "aif", "aiff"];
/// Not part of any download: builds, version control, and the agents' own
/// worktrees (each a whole second copy of this tree).
const SKIPPED: [&str; 4] = ["target", ".git", ".claude", "node_modules"];

/// Of `paths` — every file in the tree, relative to its root — the sounds no
/// [`BUNDLED_SOUNDS`] entry covers. Empty is the only passing answer.
///
/// Names in rather than a folder to walk: INVARIANT 4 keeps files out of
/// this crate, and the question is about names anyway.
/// `tests/bundled_sounds.rs` walks the repository and asks.
pub fn unclaimed<P: AsRef<Path>>(paths: impl IntoIterator<Item = P>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .filter(|path| is_sound(path))
        .filter(|path| {
            !BUNDLED_SOUNDS
                .iter()
                .flat_map(|entry| entry.paths)
                .any(|covered| path.starts_with(covered))
        })
        .collect()
}

fn is_sound(relative: &Path) -> bool {
    if relative
        .components()
        .any(|part| SKIPPED.iter().any(|skipped| part.as_os_str() == *skipped))
    {
        return false;
    }
    let Some(ext) = relative.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    SOUNDFONTS.contains(&ext.as_str())
        || (relative.starts_with("assets") && RECORDINGS.contains(&ext.as_str()))
}
