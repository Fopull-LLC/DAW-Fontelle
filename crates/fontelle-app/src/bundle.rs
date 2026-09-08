//! Opening and saving a project, with its audio (TDD §17.1–17.4).
//!
//! `fontelle-model` owns the document format; this owns the other half — the
//! samples a reopened project's patches point at. Only this layer can do it:
//! it needs the model, the patch format in `fontelle-core`, and the decoder in
//! `fontelle-assets` at once.

use std::collections::BTreeMap;
use std::path::Path;

use fontelle_model::{Project, StorageError};
use fontelle_types::{AssetKind, AssetRef, ChannelId};

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
    pub channels: Vec<ChannelId>,
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

/// Reads the project at `path` and reloads the audio its patches name.
pub fn open_project(path: &Path) -> Result<OpenedProject, OpenError> {
    let project = fontelle_model::load_project(path)?;

    // Grouped by file: one read of a soundfont serves every sample any patch
    // takes from it, which on a 325 MB library is the difference between one
    // pass and one per layer.
    let mut wanted: BTreeMap<AssetRef, (Vec<u32>, Vec<ChannelId>)> = BTreeMap::new();
    for (id, channel) in project.channels.iter() {
        let Some(data) = &channel.patch_data else {
            continue;
        };
        let samples = fontelle_core::referenced_samples(data)
            .map_err(|error| OpenError::Patch { channel: id, error })?;
        for reference in samples {
            let entry = wanted.entry(reference.file).or_default();
            entry.0.push(reference.sample);
            if !entry.1.contains(&id) {
                entry.1.push(id);
            }
        }
    }

    let mut library = SampleLibrary::new();
    let mut missing = Vec::new();
    for (file, (samples, channels)) in wanted {
        let result = match file.kind {
            AssetKind::Sf2 | AssetKind::Sf3 => library.reload_sf2_samples(&file, &samples),
            // A plain audio file put on a channel as a sampler — see
            // `SampleLibrary::import_sample`. Without this arm a sampler built
            // by dropping a wav on the rack opens silent.
            AssetKind::Sample => library.reload_sample(&file),
            other => Err(fontelle_assets::ImportError(format!(
                "{other:?} assets cannot be loaded yet"
            ))),
        };
        if let Err(e) = result {
            missing.push(MissingAsset {
                file,
                channels,
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
