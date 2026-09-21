//! The bank's previews (`docs/flopsynth-next.md` §5.2): every factory
//! synth preset's [`SoundVector`], kept in one file beside the settings so
//! *sounds like* answers on the frame and the bank is rendered once.
//!
//! A render is 1.5 seconds a row — the whole bank is forty seconds of
//! work — so it happens on a **worker thread**, never on the audio thread
//! and never on the frame: the index is asked what it has, the worker
//! fills what is missing, and the file is written when it is whole. A
//! preset whose file has changed (a re-voiced row after an update) is
//! rendered again, because the vector is keyed by the file's text.

use std::path::{Path, PathBuf};

use fontelle_core::preview::{SoundVector, nearest};

/// One preset's place in the index: its vector, and the hash of the file
/// it was rendered from.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndexedPreview {
    pub name: String,
    pub hash: u64,
    pub vector: SoundVector,
    /// The preview note's envelope, `Preview::COLUMNS` columns in 0..1 —
    /// the inspector's thumbnail (§5.2). Empty in a file written before
    /// it was kept, which `has` reads as not indexed.
    #[serde(default)]
    pub peaks: Vec<f32>,
}

/// The previews the bank has, by preset name.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PreviewIndex {
    pub entries: Vec<IndexedPreview>,
}

impl PreviewIndex {
    /// The file this lives in, beside the settings.
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join("previews.json")
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    /// A stable hash of a preset file's text, so a row that was re-voiced
    /// is rendered again.
    pub fn hash_of(text: &str) -> u64 {
        // FNV-1a: stable across builds, which `DefaultHasher` is not
        // promised to be.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    /// Whether `name` is indexed from a file hashing to `hash` — with its
    /// envelope, so an index from before the thumbnail is filled in once.
    pub fn has(&self, name: &str, hash: u64) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.name == name && entry.hash == hash && !entry.peaks.is_empty())
    }

    /// Records `name`'s vector, replacing an older one. For a test's
    /// seeding; the worker goes through [`insert_with_peaks`](Self::insert_with_peaks).
    pub fn insert(&mut self, name: &str, hash: u64, vector: SoundVector) {
        self.insert_with_peaks(name, hash, vector, vec![1.0]);
    }

    /// Records `name`'s vector and envelope, replacing an older one.
    pub fn insert_with_peaks(
        &mut self,
        name: &str,
        hash: u64,
        vector: SoundVector,
        peaks: Vec<f32>,
    ) {
        self.entries.retain(|entry| entry.name != name);
        self.entries.push(IndexedPreview {
            name: name.to_string(),
            hash,
            vector,
            peaks,
        });
    }

    /// `name`'s thumbnail: its envelope and its ten-band shape. `None`
    /// until it is rendered.
    pub fn thumbnail(&self, name: &str) -> Option<(Vec<f32>, [f32; 10])> {
        self.entries
            .iter()
            .find(|entry| entry.name == name && !entry.peaks.is_empty())
            .map(|entry| (entry.peaks.clone(), entry.vector.shape))
    }

    /// The `count` presets nearest `name` by sound, nearest first — empty
    /// until `name` itself is indexed.
    pub fn sounds_like(&self, name: &str, count: usize) -> Vec<String> {
        let Some(of) = self.entries.iter().position(|entry| entry.name == name) else {
            return Vec::new();
        };
        let vectors: Vec<SoundVector> = self.entries.iter().map(|e| e.vector.clone()).collect();
        nearest(&vectors, of, count)
            .into_iter()
            .map(|index| self.entries[index].name.clone())
            .collect()
    }

    /// The names, of those offered, that are not indexed at their current
    /// hash — what the worker has left to render.
    pub fn missing<'a>(&self, offered: &'a [(String, u64)]) -> Vec<&'a (String, u64)> {
        offered
            .iter()
            .filter(|(name, hash)| !self.has(name, *hash))
            .collect()
    }
}

/// The bank's factory synth rows as the worker wants them: each name with
/// its file's hash and the patch to render.
pub type PreviewJob = (String, u64, fontelle_core::Patch);

/// Renders `jobs` on a thread of its own, sending each result back as it
/// is done. The receiver is drained by the session's pump.
pub fn render_in_background(
    jobs: Vec<PreviewJob>,
) -> std::sync::mpsc::Receiver<(String, u64, fontelle_core::preview::Preview)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("preset previews".to_string())
        .spawn(move || {
            for (name, hash, patch) in jobs {
                let preview = fontelle_core::preview::preset_preview(&patch);
                if sender.send((name, hash, preview)).is_err() {
                    break;
                }
            }
        })
        .ok();
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector(scalar: [f32; 4]) -> SoundVector {
        SoundVector {
            scalar,
            shape: [0.1; 10],
        }
    }

    #[test]
    fn the_nearest_by_sound_are_the_nearest_by_the_vector() {
        let mut index = PreviewIndex::default();
        index.insert("Grand", 1, vector([0.0, 5.0, 1.0, 1.0]));
        index.insert("Felt", 1, vector([0.1, 5.1, 1.0, 1.0]));
        index.insert("Growl", 1, vector([2.0, 7.0, 3.0, 4.0]));
        index.insert("Organ", 1, vector([0.5, 5.2, 1.2, 0.9]));
        assert_eq!(index.sounds_like("Grand", 2), ["Felt", "Organ"]);
        assert!(index.sounds_like("Nobody", 2).is_empty());
    }

    #[test]
    fn a_changed_file_is_missing_again_and_a_saved_index_reads_back() {
        let mut index = PreviewIndex::default();
        index.insert("Grand", PreviewIndex::hash_of("v1"), vector([0.0; 4]));
        let offered = vec![
            ("Grand".to_string(), PreviewIndex::hash_of("v1")),
            ("Felt".to_string(), PreviewIndex::hash_of("x")),
        ];
        assert_eq!(index.missing(&offered).len(), 1);
        let changed = vec![("Grand".to_string(), PreviewIndex::hash_of("v2"))];
        assert_eq!(
            index.missing(&changed).len(),
            1,
            "re-voiced, so rendered again"
        );
        let dir = std::env::temp_dir().join(format!("fontelle-previews-{}", std::process::id()));
        let path = PreviewIndex::path_in(&dir);
        index.save(&path).unwrap();
        assert_eq!(PreviewIndex::load(&path), index);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The thumbnail (§5.2's inspector) is the preview's envelope beside
    /// its ten-band shape. An index written before the envelope was kept
    /// has the vector and no peaks; such a row is **missing** again, so
    /// the worker fills it in once rather than the column staying blank.
    #[test]
    fn a_row_without_its_envelope_is_rendered_again_and_a_thumbnail_is_both() {
        let mut index = PreviewIndex::default();
        let hash = PreviewIndex::hash_of("v1");
        index.entries.push(IndexedPreview {
            name: "Old".to_string(),
            hash,
            vector: vector([0.0; 4]),
            peaks: Vec::new(),
        });
        assert!(!index.has("Old", hash), "no envelope: render it again");
        assert!(index.thumbnail("Old").is_none());
        index.insert_with_peaks("Old", hash, vector([0.0; 4]), vec![0.2, 1.0, 0.5]);
        assert!(index.has("Old", hash));
        let (peaks, shape) = index.thumbnail("Old").expect("both halves");
        assert_eq!(peaks, [0.2, 1.0, 0.5]);
        assert_eq!(shape, [0.1; 10]);
        // A file from before reads back with empty peaks rather than failing.
        let text = r#"{"entries":[{"name":"X","hash":1,"vector":{"scalar":[0,0,0,0],"shape":[0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1]}}]}"#;
        let old: PreviewIndex = serde_json::from_str(text).unwrap();
        assert!(old.entries[0].peaks.is_empty());
    }
}
