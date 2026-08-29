use std::sync::Arc;

use fontelle_types::AssetId;

/// Decoded, ready-to-play PCM for one sample — mono, `f32`, at its native rate.
/// Fontelle inverts the usual SF2 relationship (TDD §7.1): once this buffer
/// exists, it has no live link back to the source file. `fontelle-assets` decodes
/// and inserts it; `fontelle-core` only ever reads it back by the `AssetId`
/// `insert` mints.
///
/// Ref-counted (`Arc`) so every patch/layer referencing the same file shares one
/// copy in memory (TDD §7.7) — cloning a `SampleBuffer` clones the handle, not the
/// audio.
#[derive(Debug, Clone)]
pub struct SampleBuffer {
    pub data: Arc<[f32]>,
    pub sample_rate: u32,
}

/// **M0 scope note:** this is the fully-resident case only. TDD §7.7 also
/// specifies a streamed case (files over a configurable threshold: only the
/// first N ms resident, the remainder read live from the disk thread into
/// per-voice ring buffers). That needs `fontelle-engine`'s disk thread to exist
/// first, so it isn't implemented yet — every `SampleBuffer` here is fully
/// resident regardless of size. Tracked in `PROGRESS.md`.
/// `Clone` is cheap: the map holds `Arc` handles, so cloning it duplicates the
/// index rather than the audio. That is what lets a library importing a new
/// soundfont copy-on-write out from under a graph the audio thread is already
/// holding, instead of having to stop playback first.
#[derive(Debug, Default, Clone)]
pub struct SampleStore {
    samples: slotmap::SlotMap<AssetId, SampleBuffer>,
}

impl SampleStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mints a fresh `AssetId` for `buffer` and stores it.
    pub fn insert(&mut self, buffer: SampleBuffer) -> AssetId {
        self.samples.insert(buffer)
    }

    pub fn get(&self, asset: AssetId) -> Option<&SampleBuffer> {
        self.samples.get(asset)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_get_round_trips_the_same_data() {
        let mut store = SampleStore::new();
        let id = store.insert(SampleBuffer {
            data: Arc::from(vec![0.0, 0.5, 1.0, 0.5]),
            sample_rate: 44_100,
        });

        let got = store
            .get(id)
            .expect("just-inserted sample must be retrievable");
        assert_eq!(&*got.data, &[0.0, 0.5, 1.0, 0.5][..]);
        assert_eq!(got.sample_rate, 44_100);
    }

    #[test]
    fn distinct_inserts_get_distinct_ids_and_dont_collide() {
        let mut store = SampleStore::new();
        let a = store.insert(SampleBuffer {
            data: Arc::from(vec![1.0]),
            sample_rate: 8_000,
        });
        let b = store.insert(SampleBuffer {
            data: Arc::from(vec![2.0]),
            sample_rate: 16_000,
        });

        assert_ne!(a, b);
        assert_eq!(store.get(a).unwrap().data[0], 1.0);
        assert_eq!(store.get(b).unwrap().data[0], 2.0);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn unknown_id_returns_none_rather_than_panicking() {
        use slotmap::Key;

        let store = SampleStore::new();
        assert!(store.get(AssetId::null()).is_none());
    }
}
