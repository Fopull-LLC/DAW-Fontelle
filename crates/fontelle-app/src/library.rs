//! The decoded audio a project's patches point at, and the file references
//! that let a reopened project find it again.

use std::collections::HashMap;
use std::path::Path;
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
    /// How many synthetic samples have been registered, so each gets a
    /// distinct reference even when two are given the same name. Two samples
    /// sharing one reference is not a duplicate-name annoyance — it silently
    /// resolves one of them to the other's audio.
    synthetic_count: u32,
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

    /// Where each sample in the store came from — the argument
    /// `Patch::to_data` takes.
    pub fn provenance(&self) -> &HashMap<AssetId, SampleRef> {
        &self.by_id
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

    fn store_mut(&mut self) -> &mut SampleStore {
        Arc::make_mut(&mut self.store)
    }
}
