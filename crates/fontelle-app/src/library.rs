//! The decoded audio a project's patches point at, and the file references
//! that let a reopened project find it again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_assets::ImportError;
use fontelle_core::{Patch, SampleBuffer, SampleStore};
use fontelle_types::{AssetId, AssetKind, AssetRef, SampleRef};

/// A `SampleStore` plus the two-way mapping between the ids it minted and the
/// files those samples came from.
///
/// A `Layer` names its audio by `AssetId`, and a *stored* layer names it by
/// `SampleRef` (TDD §8.3) — so both directions are needed, and nothing else in
/// the workspace is in a position to hold them: `fontelle-core` may not know
/// about files, `fontelle-model` may not know about `fontelle-core`, and
/// `fontelle-assets` only sees one import at a time.
#[derive(Debug, Default)]
pub struct SampleLibrary {
    /// Behind an `Arc` because that is the form `SamplerNode` takes, and
    /// `Arc::make_mut` is what lets a later import copy the *index* — never
    /// the audio, which is `Arc`-shared inside it — out from under a graph
    /// the audio thread is already holding.
    store: Arc<SampleStore>,
    by_id: HashMap<AssetId, SampleRef>,
    by_file: HashMap<SampleRef, AssetId>,
    /// What the file calls each sample.
    ///
    /// Kept beside the provenance rather than on the `Patch`, for the reason
    /// the provenance is kept here: it is a fact about the *file*, not about
    /// the user's instrument, and INVARIANT 8 keeps file-derived identity out
    /// of the document. Re-read on every import and on every reopen, so a
    /// drum kit is labelled in the piano roll whether the project was just
    /// built or just opened. See `crate::keymap`.
    names: HashMap<AssetId, String>,
    /// How many synthetic samples have been registered, so each gets a
    /// distinct reference even when two are given the same name. Two samples
    /// sharing one reference is not a duplicate-name annoyance — it silently
    /// resolves one of them to the other's audio.
    synthetic_count: u32,
    /// Every audio **clip's** decoded audio (TDD §15), which is a different
    /// store from `store` above because a `SampleBuffer` is mono and a take or
    /// a loop is not — see `fontelle_core::AudioBuffer`.
    ///
    /// Behind an `Arc` for the reason the soundfont store is: that is the form
    /// `AudioClipNode` takes, and `Arc::make_mut` lets an import copy the
    /// *index* out from under a graph the audio thread is already holding
    /// without copying a note of the audio.
    audio: Arc<fontelle_core::AudioStore>,
    /// The primary map the audio store is secondary to, so importing mints a
    /// real id — and, keyed by path, so a loop dropped on eight rows is one
    /// file rather than eight copies of it (TDD §7.7).
    audio_files: slotmap::SlotMap<AssetId, PathBuf>,
    audio_by_path: HashMap<PathBuf, AssetId>,
}

/// What [`SampleLibrary::import_audio`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedAudio {
    /// The reference a clip holds. Its `id` is what indexes
    /// [`SampleLibrary::audio_store`].
    pub asset: AssetRef,
    pub frames: usize,
    pub sample_rate: u32,
    pub channels: u16,
}

impl SampleLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Imports one preset of an SF2 file, keeping the provenance the returned
    /// patch will need in order to be saved.
    pub fn import_sf2(&mut self, path: &Path, preset: usize) -> Result<Patch, ImportError> {
        let imported = fontelle_assets::import_sf2_preset(path, preset, self.store_mut())?;
        for (id, file) in imported.samples {
            self.by_file.insert(file.clone(), id);
            self.by_id.insert(id, file);
        }
        self.names.extend(imported.names);
        Ok(imported.patch)
    }

    /// Registers audio that did not come out of a file this build read.
    ///
    /// `name` stands in for the path, so a patch built on it still survives
    /// the round trip through the document — a sample with no provenance at
    /// all loads back as a silent layer, which is right for a genuinely
    /// missing file and wrong for a test fixture. Real recorded audio gets a
    /// file in the project's `recordings/` and comes back through
    /// [`Self::import_sf2`]'s path instead.
    pub fn insert_synthetic(&mut self, name: &str, buffer: SampleBuffer) -> AssetId {
        let id = self.store_mut().insert(buffer);
        self.names.insert(id, name.to_string());
        let index = self.synthetic_count;
        self.synthetic_count += 1;
        let file = SampleRef {
            file: AssetRef::unregistered(Path::new("memory:").join(name), 0, 0, AssetKind::Sample),
            sample: index,
        };
        self.by_file.insert(file.clone(), id);
        self.by_id.insert(id, file);
        id
    }

    /// Reloads the samples a saved patch names, out of the file it names them
    /// in (TDD §17.4).
    ///
    /// Only the headers listed are decoded, and by the same code the importer
    /// uses, so a reopened project's audio is bit-identical to what it was
    /// saved from. Deliberately *not* a re-import of the preset the patch
    /// originally came from: a patch the user has edited to reach a second
    /// preset's sample would not survive that, and the whole product thesis is
    /// that the file supplies defaults rather than the final word.
    pub fn reload_sf2_samples(
        &mut self,
        file: &AssetRef,
        wanted: &[u32],
    ) -> Result<(), ImportError> {
        let loaded = fontelle_assets::load_sf2_samples(&file.path, wanted, self.store_mut())?;
        for (sample, found) in loaded {
            let reference = SampleRef {
                file: file.clone(),
                sample,
            };
            self.by_file.insert(reference.clone(), found.asset);
            self.by_id.insert(found.asset, reference);
            self.names.insert(found.asset, found.name);
        }
        Ok(())
    }

    /// Where each sample in the store came from — the argument
    /// `Patch::to_data` takes.
    pub fn provenance(&self) -> &HashMap<AssetId, SampleRef> {
        &self.by_id
    }

    /// What the file calls the sample under `id`, if this library knows.
    ///
    /// `None` for audio whose file this build never read — a broken link
    /// (TDD §17.4), which is a normal condition rather than an error.
    pub fn name(&self, id: AssetId) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// The live id for a stored reference, if this library has it — the
    /// resolver `Patch::from_data` takes. `None` is a broken link, which
    /// TDD §17.4 makes a normal condition rather than an error.
    pub fn resolve(&self, file: &SampleRef) -> Option<AssetId> {
        self.by_file.get(file).copied()
    }

    /// The store as the graph takes it.
    pub fn store(&self) -> Arc<SampleStore> {
        self.store.clone()
    }

    /// Decodes an audio file and keeps it, minting an asset for a clip to
    /// point at (TDD §15, §17.4).
    ///
    /// The audio-clip counterpart of [`import_sf2`](Self::import_sf2), and it
    /// **dedupes by path**: importing the same file twice hands back the same
    /// asset, so a loop dropped on eight rows is one copy of the audio in
    /// memory rather than eight.
    ///
    /// A file that is not a sound is refused and mints nothing — an asset with
    /// no audio behind it is a clip that draws and never plays.
    pub fn import_audio(&mut self, path: &Path) -> Result<ImportedAudio, ImportError> {
        let path = path.to_path_buf();
        let decoded = fontelle_assets::import_audio(&path)?;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let id = match self.audio_by_path.get(&path) {
            Some(id) => *id,
            None => {
                let id = self.audio_files.insert(path.clone());
                self.audio_by_path.insert(path.clone(), id);
                Arc::make_mut(&mut self.audio).insert(
                    id,
                    fontelle_core::AudioBuffer {
                        data: Arc::from(decoded.samples),
                        sample_rate: decoded.sample_rate,
                        channels: decoded.channels,
                    },
                );
                id
            }
        };
        Ok(ImportedAudio {
            asset: AssetRef {
                id,
                path,
                // Left at nothing rather than guessed: §17.4's relink hash is
                // the first megabyte plus the size, and nothing reads it yet.
                // A wrong hash is worse than an absent one.
                content_hash: 0,
                size,
                kind: fontelle_types::AssetKind::Sample,
            },
            frames: decoded.frames,
            sample_rate: decoded.sample_rate,
            channels: decoded.channels,
        })
    }

    /// Every audio clip's audio, in the form the graph takes.
    pub fn audio_store(&self) -> Arc<fontelle_core::AudioStore> {
        self.audio.clone()
    }

    fn store_mut(&mut self) -> &mut SampleStore {
        Arc::make_mut(&mut self.store)
    }
}
