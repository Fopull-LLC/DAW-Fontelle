//! Opening and saving a project, with its audio (TDD §17.1–17.4).
//!
//! `fontelle-model` owns the document format; this owns the other half — the
//! samples a reopened project's patches point at. Only this layer can do it:
//! it needs the model, the patch format in `fontelle-core`, and the decoder in
//! `fontelle-assets` at once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fontelle_model::{Project, StorageError};
use fontelle_types::{AssetKind, AssetRef, ChannelId, ClipId};

use crate::library::SampleLibrary;

/// A project, its audio, and whatever could not be found.
pub struct OpenedProject {
    pub project: Project,
    pub library: SampleLibrary,
    /// Files a patch pointed at that could not be read (TDD §17.4).
    ///
    /// **Not an error.** §17.4 is explicit that broken links are a normal
    /// condition: the project opens and plays with placeholders, and it does
    /// not lose the references on the next save. Every layer whose file is
    /// listed here renders silence; the list is what a relink dialog works on.
    pub missing: Vec<MissingAsset>,
}

#[derive(Debug, Clone)]
pub struct MissingAsset {
    pub file: AssetRef,
    /// Which channels are affected, so a message can name the instrument
    /// rather than the file nobody recognises.
    ///
    /// **Empty for an audio clip's file**, which belongs to no channel: a clip
    /// carries its own audio (`ClipSource::Audio`) and is routed by the clip,
    /// not by an instrument. `clips` is the list to name in that case.
    pub channels: Vec<ChannelId>,
    /// And which audio clips are, for the same reason.
    pub clips: Vec<ClipId>,
    pub why: String,
}

#[derive(Debug)]
pub enum OpenError {
    Storage(StorageError),
    /// A channel's stored patch could not be read at all — a project from a
    /// newer build, or a corrupt one. Distinct from a missing *file*, which
    /// is a normal condition.
    Patch {
        channel: ChannelId,
        error: fontelle_core::PatchFormatError,
    },
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "{e}"),
            Self::Patch { error, .. } => {
                write!(f, "a channel's instrument could not be read: {error}")
            }
        }
    }
}

impl std::error::Error for OpenError {}

impl From<StorageError> for OpenError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

/// Writes a project into the bundle at `path`.
///
/// **Assets are referenced, not copied.** §17.4's policy is to ask once and
/// remember the answer, with Shift to override for one import; there is no
/// dialog to ask from here, and referencing is the answer that cannot
/// surprise anybody by silently duplicating a 325 MB soundfont into their
/// project folder. The `assets/` directory is created regardless, because it
/// is part of the bundle's shape and the copy path lands in it.
pub fn save_project(project: &Project, path: &Path) -> Result<(), StorageError> {
    fontelle_model::save_project(project, path)
}

/// Where the bytes `file` names are on this machine, if they are anywhere
/// (`docs/collab-plan.md` §7.1).
///
/// **The one reader of a reference's path.** A path inside the bundle is
/// written relative to it (a collected file, `assets/<hash>.wav`); any other
/// is where the song was made. And when neither is there — a soundfont a
/// friend's song names at a place on *their* disk — the file is looked for by
/// what is in it: a collected copy in this bundle, then this machine's
/// soundfont folders (`banks`). A song shared between two machines names
/// files that are in different places on each; this is what makes that a
/// non-event.
pub fn resolve(bundle: Option<&Path>, file: &AssetRef, banks: &[PathBuf]) -> Option<PathBuf> {
    if file.path.is_relative() {
        let here = bundle?.join(&file.path);
        if here.is_file() {
            return Some(here);
        }
    } else if file.path.is_file() {
        return Some(file.path.clone());
    }
    if file.content_hash == 0 {
        return None;
    }
    // A collected copy is called by its whole digest, which ends with the
    // sixteen hex digits the reference carries.
    let tail = format!("{:016x}", file.content_hash);
    if let Some(bundle) = bundle
        && let Ok(listing) = std::fs::read_dir(bundle.join("assets"))
    {
        let found = listing.flatten().map(|entry| entry.path()).find(|path| {
            path.file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with(&tail))
        });
        if found.is_some() {
            return found;
        }
    }
    fontelle_assets::content_hash::find_by_hash(banks, file.content_hash, file.size)
}

/// Reads the project at `path` and reloads the audio its patches name.
///
/// With no soundfont folders to look in: see [`open_project_with`].
pub fn open_project(path: &Path) -> Result<OpenedProject, OpenError> {
    open_project_with(path, &[])
}

/// Reads the project at `path` and reloads **every file it names** — each
/// channel's samples and its A/B slot's, each audio clip's and each prefab's
/// audio — looking for any that are not where the song says in `banks`
/// (see [`resolve`]).
///
/// It used to read the channels' patches and the clips, and nothing else: a
/// prefab of an audio clip and a sound waiting in an A/B slot opened silent
/// (`docs/collab-plan.md` §18, F57).
pub fn open_project_with(path: &Path, banks: &[PathBuf]) -> Result<OpenedProject, OpenError> {
    let project = fontelle_model::load_project(path)?;

    // Grouped by file: one read of a soundfont serves every sample any patch
    // takes from it, which on a 325 MB library is the difference between one
    // pass and one per layer.
    let mut wanted: BTreeMap<AssetRef, (Vec<u32>, Vec<ChannelId>)> = BTreeMap::new();
    for (id, channel) in project.channels.iter() {
        for data in [&channel.patch_data, &channel.ab.other]
            .into_iter()
            .flatten()
        {
            let samples = fontelle_core::referenced_samples(data)
                .map_err(|error| OpenError::Patch { channel: id, error })?;
            for reference in samples {
                let entry = wanted.entry(reference.file).or_default();
                if !entry.0.contains(&reference.sample) {
                    entry.0.push(reference.sample);
                }
                if !entry.1.contains(&id) {
                    entry.1.push(id);
                }
            }
        }
    }

    let mut library = SampleLibrary::new();
    let mut missing = Vec::new();
    for (file, (samples, channels)) in wanted {
        let Some(from) = resolve(Some(path), &file, banks) else {
            missing.push(MissingAsset {
                file,
                channels,
                clips: Vec::new(),
                why: "it is not on this machine".to_string(),
            });
            continue;
        };
        let result = match file.kind {
            AssetKind::Sf2 | AssetKind::Sf3 => library.reload_sf2_samples(&file, &from, &samples),
            // A plain audio file put on a channel as a sampler — see
            // `SampleLibrary::import_sample`. Without this arm a sampler built
            // by dropping a wav on the rack opens silent.
            AssetKind::Sample => library.reload_sample(&file, &from),
            other => Err(fontelle_assets::ImportError(format!(
                "{other:?} assets cannot be loaded yet"
            ))),
        };
        if let Err(e) = result {
            missing.push(MissingAsset {
                file,
                channels,
                clips: Vec::new(),
                why: e.to_string(),
            });
        }
    }

    // **And every audio clip's own file** (TDD §15).
    //
    // > *"audio clips, after closing the project and re opening, often would
    // > just be blank after that point."*
    //
    // This walked `project.channels` and stopped, so a take or a loop — whose
    // audio hangs off the *clip* and not off any instrument — was never read
    // back at all. The library came up without it, the block drew no waveform
    // because §15.3's peaks are keyed by asset, and the player found nothing
    // under the id, so the clip was blank and silent and said nothing about
    // why: only a file somebody *tried* to load can be reported missing.
    //
    // Grouped by reference so one file behind eight rows is one read, and
    // reloaded **under the id the project wrote down** — see
    // `SampleLibrary::reload_audio` for why a clip cannot be given a fresh one
    // the way a patch's samples can.
    let mut takes: BTreeMap<AssetRef, Vec<ClipId>> = BTreeMap::new();
    for (id, clip) in project.clips.iter() {
        if let fontelle_model::ClipSource::Audio(data) = &clip.source {
            takes.entry(data.asset.clone()).or_default().push(id);
        }
    }
    // A prefab's audio is a take nobody has placed yet: it plays wherever a
    // place of the prefab is put, so it is read back like any other.
    for prefab in project.prefabs.values() {
        if let fontelle_model::ClipSource::Audio(data) = &prefab.source {
            takes.entry(data.asset.clone()).or_default();
        }
    }
    for (file, clips) in takes {
        let Some(from) = resolve(Some(path), &file, banks) else {
            missing.push(MissingAsset {
                file,
                channels: Vec::new(),
                clips,
                why: "it is not on this machine".to_string(),
            });
            continue;
        };
        if let Err(e) = library.reload_audio(&file, &from) {
            missing.push(MissingAsset {
                file,
                channels: Vec::new(),
                clips,
                why: e.to_string(),
            });
        }
    }

    Ok(OpenedProject {
        project,
        library,
        missing,
    })
}
