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

// ------------------------------------------------------ audio clip audio ---

/// Decoded audio for an **audio clip** (TDD §15), which is not the same thing
/// as a [`SampleBuffer`].
///
/// A `SampleBuffer` is mono, because every SF2 sample is and everything that
/// reads one assumes it. A take off a microphone is mono and a loop off disk
/// usually is not, so a clip needs a buffer that knows its channel count and
/// can be asked for a **fractional** position in one of them — fractional
/// because the player reads at whatever ratio the device asks for (§7.6), and
/// because that is why import does not resample.
///
/// Ref-counted for the reason `SampleBuffer` is: two clips over one file share
/// one copy, and cloning the index does not copy the audio.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Interleaved across [`channels`](Self::channels).
    pub data: Arc<[f32]>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioBuffer {
    /// How many whole frames there are. A trailing half-frame is not a frame.
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1) as usize
    }

    /// One sample.
    ///
    /// **Silent off either end**, because this is read on the audio thread and
    /// an index out of bounds there is not a wrong sound but a dead process.
    ///
    /// A **mono** buffer answers for every channel with its only one: a mono
    /// take on a stereo track has to be heard on both sides, and a file that
    /// came out of the left speaker alone is the classic version of getting
    /// this wrong.
    pub fn sample(&self, frame: usize, channel: u16) -> f32 {
        let channels = self.channels.max(1);
        if frame >= self.frames() {
            return 0.0;
        }
        let channel = if channels == 1 { 0 } else { channel };
        if channel >= channels {
            return 0.0;
        }
        self.data[frame * channels as usize + channel as usize]
    }

    /// The buffer at a fractional frame, linearly interpolated.
    ///
    /// Linear rather than §7.6's better kernels, and deliberately: an audio
    /// clip at its default speed reads whole positions and never interpolates
    /// at all, so the cost only appears when a clip has been pitched — where it
    /// is one multiply-add against a buffer read that already dominates.
    ///
    /// The **last** frame reads itself rather than interpolating towards a
    /// frame that is not there, which would halve it — a dip at every loop
    /// seam.
    pub fn at(&self, position: f64, channel: u16) -> f32 {
        let frames = self.frames();
        if frames == 0 || position < 0.0 {
            return 0.0;
        }
        let index = position.floor();
        if index >= frames as f64 {
            return 0.0;
        }
        let i = index as usize;
        let a = self.sample(i, channel);
        if i + 1 >= frames {
            return a;
        }
        let b = self.sample(i + 1, channel);
        let t = (position - index) as f32;
        a + (b - a) * t
    }
}

/// Every audio clip's decoded audio, by asset.
///
/// A `SecondaryMap` rather than a `HashMap`: the lookup happens per placement
/// per block on the audio thread, and a secondary map indexes by the key's own
/// slot rather than hashing it. `SampleStore` is the primary map for soundfont
/// samples; audio-clip assets are minted by the document's own table, so this
/// is the secondary form by construction.
#[derive(Debug, Default, Clone)]
pub struct AudioStore {
    clips: slotmap::SecondaryMap<AssetId, AudioBuffer>,
}

impl AudioStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, asset: AssetId, buffer: AudioBuffer) {
        self.clips.insert(asset, buffer);
    }

    /// **RT.** No allocation, no hashing, no lock.
    pub fn get(&self, asset: AssetId) -> Option<&AudioBuffer> {
        self.clips.get(asset)
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}
