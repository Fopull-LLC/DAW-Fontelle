//! The decoded audio a project's patches point at, and the file references
//! that let a reopened project find it again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_assets::ImportError;
use fontelle_core::{Patch, SampleBuffer, SampleStore};
use fontelle_model::Arena;
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
    ///
    /// An [`Arena`] rather than a `SlotMap` for one operation: `insert_at`.
    /// A reopened project's clips name the ids the *previous* session minted
    /// ([`reload_audio`](Self::reload_audio)), and a fresh slotmap mints from
    /// index zero — so without claiming those slots the next import would be
    /// handed an id a clip is already using and would quietly replace its
    /// audio. Same reason the document's arenas are not slotmaps.
    audio_files: Arena<AssetId, PathBuf>,
    audio_by_path: HashMap<PathBuf, AssetId>,
    /// The waveform summary per audio asset (TDD §15.3), built once on import.
    ///
    /// Beside the audio rather than in the document: it is regenerable from
    /// the file, which is why §17.1 puts its cached form under `cache/` and not
    /// under `assets/`.
    audio_peaks: HashMap<AssetId, fontelle_assets::PeakData>,
    /// Where this library mints a clip's audio id — a joiner's own space while
    /// it shares a song, so an import here and one on another studio at the
    /// same moment are two ids (`docs/collab-plan.md` §18, F56). An audio
    /// clip's id is written into the song, unlike a patch sample's.
    mint_space: Option<u16>,
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
    ///
    /// `from` is where the bytes are on this machine, which is not always
    /// where `file` says: a collected file's path is inside the bundle, and a
    /// soundfont somebody shared is found in this machine's own bank by what
    /// is in it (`bundle::resolve`). The library stays keyed by `file`, the
    /// song's own name for it.
    pub fn reload_sf2_samples(
        &mut self,
        file: &AssetRef,
        from: &Path,
        wanted: &[u32],
    ) -> Result<(), ImportError> {
        let loaded = fontelle_assets::load_sf2_samples(from, wanted, self.store_mut())?;
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
        let hash = fontelle_assets::content_hash::hash_file(&path)
            .map_err(|e| ImportError(format!("{}: {e}", path.display())))?;
        let id = match self.audio_by_path.get(&path) {
            Some(id) => *id,
            None => {
                let id = fontelle_model::minting_in(self.mint_space, || {
                    self.audio_files.insert(path.clone())
                });
                self.audio_by_path.insert(path.clone(), id);
                self.audio_peaks.insert(
                    id,
                    fontelle_assets::generate_peaks(id, &decoded.samples, decoded.channels),
                );
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
                // What the file *is*, so a machine that already has it — a
                // shared song's other studio, a copy in a bank — finds it by
                // its contents (`docs/collab-plan.md` §7.1, F24).
                content_hash: hash.low,
                size: hash.size,
                kind: fontelle_types::AssetKind::Sample,
            },
            frames: decoded.frames,
            sample_rate: decoded.sample_rate,
            channels: decoded.channels,
        })
    }

    /// Brings a plain audio file in as a **sampler** sample (TDD §7.1).
    ///
    /// *"i cannot drag an audio clip from the audio import tab into the
    /// channel rack to turn it into a sampler."* The two stores are different
    /// and deliberately so — an audio clip's audio lives in the `AudioStore`
    /// and is stereo, and a sampler layer's lives in the `SampleStore` and is
    /// mono (see `fontelle_core::SampleBuffer`) — so bringing a file in as an
    /// instrument is not the same act as dropping it on the arrangement, and
    /// this is the other one.
    ///
    /// **Folded to mono by averaging**, not by taking the left channel: a
    /// stereo file whose sides differ would otherwise lose half of itself, and
    /// half of a stereo drum loop is a quieter, thinner drum loop rather than
    /// an obviously wrong one.
    ///
    /// Registered with the file it came from, so a patch built on it survives
    /// the round trip through a saved project — [`Self::reload_sample`] is the
    /// other half of that.
    pub fn import_sample(&mut self, path: &Path) -> Result<ImportedSample, ImportError> {
        let path = path.to_path_buf();
        let decoded = fontelle_assets::import_audio(&path)?;
        if decoded.frames == 0 || decoded.sample_rate == 0 {
            return Err(ImportError(format!(
                "there is no sound in {}",
                path.display()
            )));
        }
        let hash = fontelle_assets::content_hash::hash_file(&path)
            .map_err(|e| ImportError(format!("{}: {e}", path.display())))?;
        let file = SampleRef {
            file: AssetRef::unregistered(
                path.clone(),
                hash.low,
                hash.size,
                fontelle_types::AssetKind::Sample,
            ),
            // One file, one sample: a wav has no preset index to disambiguate.
            sample: 0,
        };
        // Already in: a file dropped twice is one sample, so two channels
        // built on it share the audio rather than decoding it again.
        if let Some(id) = self.by_file.get(&file).copied() {
            return Ok(ImportedSample {
                id,
                file,
                sample_rate: decoded.sample_rate,
                frames: decoded.frames,
            });
        }
        let buffer = mono(&decoded.samples, decoded.channels, decoded.sample_rate);
        let id = self.store_mut().insert(buffer);
        self.names.insert(
            id,
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Sample".to_string()),
        );
        self.by_file.insert(file.clone(), id);
        self.by_id.insert(id, file.clone());
        Ok(ImportedSample {
            id,
            file,
            sample_rate: decoded.sample_rate,
            frames: decoded.frames,
        })
    }

    /// Puts a sample a saved patch names back, out of the file it names.
    ///
    /// [`Self::reload_sf2_samples`]'s counterpart for a plain audio file, and
    /// what stops a sampler built by dropping a wav on the rack from opening
    /// silent (`bundle::open` dispatches on `AssetKind`).
    pub fn reload_sample(&mut self, file: &AssetRef, from: &Path) -> Result<(), ImportError> {
        let want = SampleRef {
            file: file.clone(),
            sample: 0,
        };
        let decoded = fontelle_assets::import_audio(from)?;
        let buffer = mono(&decoded.samples, decoded.channels, decoded.sample_rate);
        // A fresh id is fine — and is what `reload_sf2_samples` does too:
        // a patch stores its layers' **provenance** (`SampleRef`) and resolves
        // them through `SampleLibrary::resolve` on load, so what has to match
        // is the file it names, not the slot it happened to sit in.
        let id = self.store_mut().insert(buffer);
        self.names.insert(id, sound_name(&file.path));
        self.by_file.insert(want.clone(), id);
        self.by_id.insert(id, want);
        Ok(())
    }

    /// Brings a saved clip's audio back **under the id the project wrote
    /// down** (TDD §15, §17.4).
    ///
    /// > *"audio clips, after closing the project and re opening, often would
    /// > just be blank after that point."*
    ///
    /// The audio-clip counterpart of [`reload_sample`](Self::reload_sample),
    /// and the one way it differs is the whole of this function. A patch may
    /// be handed a **fresh** id, because it stores its layers' provenance and
    /// resolves them by file on load; an audio clip has no such indirection —
    /// `AudioClipData::asset` *is* an `AssetId`, and the waveform and the
    /// player both index by it. Load the file under any other id and the
    /// library holds the audio while every clip still points at nothing, which
    /// looks exactly like not having loaded it at all.
    ///
    /// The slot is claimed in `audio_files` as well as filled in the store, so
    /// a later import cannot be minted an id a clip is already using.
    ///
    /// Already-loaded ids and already-decoded paths both return without
    /// touching the decoder: a loop on eight rows is eight clips, one asset
    /// and one read.
    pub fn reload_audio(&mut self, file: &AssetRef, from: &Path) -> Result<(), ImportError> {
        if self.audio.get(file.id).is_some() {
            return Ok(());
        }
        // The same file under some other id — two sessions' imports of one
        // loop, or a clip copied between projects. `AudioBuffer` holds its
        // samples behind an `Arc`, so this shares them rather than copying.
        if let Some(known) = self.audio_by_path.get(&file.path).copied()
            && let Some(buffer) = self.audio.get(known).cloned()
        {
            let peaks = self.audio_peaks.get(&known).cloned();
            self.claim(file);
            Arc::make_mut(&mut self.audio).insert(file.id, buffer);
            if let Some(peaks) = peaks {
                self.audio_peaks.insert(file.id, peaks);
            }
            return Ok(());
        }
        let decoded = fontelle_assets::import_audio(from)?;
        self.claim(file);
        self.audio_peaks.insert(
            file.id,
            fontelle_assets::generate_peaks(file.id, &decoded.samples, decoded.channels),
        );
        Arc::make_mut(&mut self.audio).insert(
            file.id,
            fontelle_core::AudioBuffer {
                data: Arc::from(decoded.samples),
                sample_rate: decoded.sample_rate,
                channels: decoded.channels,
            },
        );
        Ok(())
    }

    /// Makes a clip's audio id minted in `space` from now on — see the field.
    pub fn set_mint_space(&mut self, space: Option<u16>) {
        self.mint_space = space;
    }

    /// Follows a song's file to where it has been moved: every entry that
    /// knew it at `from` knows it at `to`'s path, hash and size, under the
    /// same ids.
    ///
    /// What collecting a song into its bundle needs beside the document's
    /// own rewrite (`fontelle_model::RelocateAssets`): a patch's samples are
    /// found by the exact reference the patch stores, so a library still
    /// keyed by the old one would leave every sampler silent after the move.
    pub fn relocate(&mut self, from: &Path, to: &Path, content_hash: u64, size: u64) {
        let moved = |file: &AssetRef| AssetRef {
            path: to.to_path_buf(),
            content_hash,
            size,
            ..file.clone()
        };
        let keys: Vec<SampleRef> = self
            .by_file
            .keys()
            .filter(|key| key.file.path == from)
            .cloned()
            .collect();
        for key in keys {
            if let Some(id) = self.by_file.remove(&key) {
                let new = SampleRef {
                    file: moved(&key.file),
                    sample: key.sample,
                };
                self.by_id.insert(id, new.clone());
                self.by_file.insert(new, id);
            }
        }
        if let Some(id) = self.audio_by_path.remove(from) {
            self.audio_by_path.insert(to.to_path_buf(), id);
            if let Some(path) = self.audio_files.get_mut(id) {
                *path = to.to_path_buf();
            }
        }
    }

    /// Books a stored id out of the arena so nothing else is given it.
    ///
    /// A refused `insert_at` means the slot is already occupied — the file is
    /// then reachable under *its own* id and this one stays a dead reference,
    /// which is the honest outcome and not worth failing an open over.
    fn claim(&mut self, file: &AssetRef) {
        self.audio_files.insert_at(file.id, file.path.clone());
        self.audio_by_path
            .entry(file.path.clone())
            .or_insert(file.id);
    }

    /// Every audio clip's audio, in the form the graph takes.
    pub fn audio_store(&self) -> Arc<fontelle_core::AudioStore> {
        self.audio.clone()
    }

    /// One asset's waveform summary, if it has been decoded.
    ///
    /// `None` while a file is still being read — §15.3's *"draw what exists"*,
    /// which the arrangement answers by drawing nothing rather than a slab.
    pub fn audio_peaks(&self, asset: AssetId) -> Option<&fontelle_assets::PeakData> {
        self.audio_peaks.get(&asset)
    }

    fn store_mut(&mut self) -> &mut SampleStore {
        Arc::make_mut(&mut self.store)
    }
}

/// One audio file, brought in as a sampler sample.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSample {
    /// Where it landed in the sample store — what a `Source::Sample` names.
    pub id: AssetId,
    /// And where it came from, so a saved patch can find it again.
    pub file: SampleRef,
    pub sample_rate: u32,
    pub frames: usize,
}

/// Folds interleaved audio down to the one channel a sampler layer plays.
///
/// By **averaging**, so a stereo file keeps both sides rather than losing one
/// — see `SampleLibrary::import_sample`.
fn mono(samples: &[f32], channels: u16, sample_rate: u32) -> SampleBuffer {
    let channels = channels.max(1) as usize;
    let data: Vec<f32> = if channels == 1 {
        samples.to_vec()
    } else {
        samples
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };
    SampleBuffer {
        data: std::sync::Arc::from(data),
        sample_rate,
    }
}

/// What a sound is called, from its file: the file's own name without its
/// extension — and without the hash a collected copy carries.
///
/// A song's files are collected into its bundle as
/// `assets/<name>.<sixteen hex digits>.<ext>` (`Session::collect_assets`): the
/// digits are what the file *is*, so two machines agree about it, and the
/// name is so a clip still says "Take 3" rather than its hash.
pub fn sound_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Sample".to_string());
    match stem.rsplit_once('.') {
        Some((name, tail)) if tail.len() == 16 && tail.bytes().all(|b| b.is_ascii_hexdigit()) => {
            name.to_string()
        }
        _ => stem,
    }
}
