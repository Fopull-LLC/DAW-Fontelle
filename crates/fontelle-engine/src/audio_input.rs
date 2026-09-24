//! A microphone, into the program (TDD §15.4).
//!
//! Reported from using the window:
//!
//! > *"i go in the mixer make a new track, name it to like mic or something
//! > then i click a input button that lets my select my mic input to feed to
//! > that mixer track. when its recording its going through that track and
//! > recording into the arrangement as an audio clip."*
//!
//! # The ring, and why it is the whole of this file
//!
//! §15.4 is explicit: recording *"stream[s] from the RT thread through a
//! lock-free ring to the disk thread. The RT thread never touches the
//! filesystem."* An input callback is an RT thread like any other — it may not
//! allocate, may not lock and may not wait — so what it does is push its block
//! into a ring and return. Everything else happens on a thread that is allowed
//! to be slow.
//!
//! When the ring fills, the newest samples are **dropped and counted**. That is
//! the only honest answer available on the audio thread: waiting is out of the
//! question, and growing the ring is an allocation. A non-zero count is a take
//! with a hole in it, which the person recording has to be told about — a
//! recording that quietly lost audio is worse than one that failed.

/// The audio thread's half of a capture.
pub struct InputWriter {
    producer: rtrb::Producer<f32>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

/// The other thread's half.
pub struct InputReader {
    consumer: rtrb::Consumer<f32>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

/// Opens a ring `capacity` samples long.
///
/// Samples, not frames: what arrives from a device is interleaved and the ring
/// carries it exactly as it arrived. [`InputCapture`] is where the two numbers
/// are told apart.
pub fn input_capture_channel(capacity: usize) -> (InputWriter, InputReader) {
    let (producer, consumer) = rtrb::RingBuffer::new(capacity.max(1));
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        InputWriter {
            producer,
            dropped: dropped.clone(),
        },
        InputReader { consumer, dropped },
    )
}

impl InputWriter {
    /// **RT.** Pushes `block` into the ring and returns how much of it fitted.
    ///
    /// Never blocks, never allocates. What did not fit is counted rather than
    /// waited for — see this module's own note.
    pub fn write(&mut self, block: &[f32]) -> usize {
        let mut written = 0;
        for sample in block {
            if self.producer.push(*sample).is_err() {
                break;
            }
            written += 1;
        }
        let lost = block.len() - written;
        if lost > 0 {
            self.dropped
                .fetch_add(lost, std::sync::atomic::Ordering::Relaxed);
        }
        written
    }

    /// How many samples the ring has had to drop because nobody emptied it.
    pub fn dropped(&self) -> usize {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl InputReader {
    /// Moves everything captured so far into `out`, keeping what is there.
    ///
    /// Off the RT thread, so growing `out` is fine.
    pub fn drain_into(&mut self, out: &mut Vec<f32>) {
        while let Ok(sample) = self.consumer.pop() {
            out.push(sample);
        }
    }

    /// How many samples the ring had to drop. Not zero is a take with a hole
    /// in it.
    pub fn dropped(&self) -> usize {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// A take as it accumulates: interleaved samples, and how many channels they
/// are interleaved across.
///
/// Two numbers that could disagree is exactly where a stereo take ends up half
/// as long as it should be, so the frame count is derived here rather than
/// counted anywhere else.
#[derive(Debug, Clone, PartialEq)]
pub struct InputCapture {
    samples: Vec<f32>,
    channels: u16,
}

impl InputCapture {
    pub fn new(channels: u16) -> Self {
        Self {
            samples: Vec::new(),
            channels: channels.max(1),
        }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn push(&mut self, block: &[f32]) {
        self.samples.extend_from_slice(block);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Forgets the first `frames` frames — what arrived during a count-in.
    pub fn drop_front(&mut self, frames: usize) {
        let samples = (frames * usize::from(self.channels.max(1))).min(self.samples.len());
        self.samples.drain(..samples);
    }

    /// Hands the take over, leaving nothing behind.
    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}
