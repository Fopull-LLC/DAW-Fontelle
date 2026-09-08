//! Hearing the microphone through the track it is recording on (TDD §15.4).
//!
//! > *"please also make it so i can monitor my inputs so it should work like
//! > fl, tracks are already automatically routed to master so i should be able
//! > to hear routed input playing even when song isnt playing or im not
//! > recording."*
//!
//! # Why this is a second ring and not the one that already exists
//!
//! [`crate::InputWriter`]'s ring carries a take from the input callback to the
//! **disk** thread. Monitoring carries the same samples from the input
//! callback to the **output** callback. They cannot be one ring: two consumers
//! draining at different rates on different threads would each be stealing the
//! other's samples, and the one that lost would be the take.
//!
//! So the input callback writes both, and this is the second — atomics on both
//! ends, the shape [`crate::SpectrumTap`] already uses, for the same reason:
//! two RT threads, no lock, no allocation (INVARIANT 1). One producer, one
//! consumer, and neither ever waits for the other.
//!
//! # The two clocks
//!
//! An input device and an output device are two crystals and nothing keeps
//! them in step, so the ring drifts. Both directions have to be handled and
//! they need opposite answers:
//!
//! - **Towards empty**, the node runs dry. It goes silent and re-primes. A
//!   hole is honest; the last block played again is a stutter that sounds like
//!   the microphone rather than like the software.
//! - **Towards full**, the ring would reach its end and drop *every* block from
//!   then on — a glitch per block, forever. So the reader catches up once,
//!   dropping what it is too far behind to be worth playing, and carries on.
//!
//! The slack the node holds before it plays anything is what makes the first
//! kind rare, and it is latency: [`MonitorNode::latency_samples`] reports it,
//! because latency nothing reports is latency nothing can ever compensate for.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// The input callback's samples, on their way to the audio graph.
///
/// Shared through an `Arc`, like [`crate::TrackControls`]: the input stream
/// holds one end for as long as it is open, the node in the graph holds the
/// other, and it survives a graph rebuild for the same reason the analyser's
/// tap does — a monitor that went silent every time somebody added a channel
/// would be unusable.
pub struct InputMonitor {
    /// `f32` bits. Atomics rather than a `Vec<f32>` behind a lock, because
    /// both ends are audio threads and a lock on one is INVARIANT 1's whole
    /// subject.
    samples: Vec<AtomicU32>,
    /// Total samples ever written and ever read. Counters rather than indices
    /// so that "how much is in it" is one subtraction and needs no third
    /// field to tell full from empty apart.
    written: AtomicUsize,
    read: AtomicUsize,
    dropped: AtomicUsize,
    /// What the device opened at. Not negotiable — a microphone runs at what
    /// its interface runs at — so the reader converts. See [`MonitorNode`].
    rate: AtomicU32,
    channels: AtomicU32,
    live: AtomicBool,
    /// The largest block the device has handed over, in **frames**.
    ///
    /// The number the reader's slack has to be measured against, and the one
    /// it cannot guess: the output stream is pinned to `BLOCK_SIZE`, and an
    /// input stream takes whatever its device offers — routinely a thousand
    /// frames or more on ALSA and PipeWire. Measured rather than assumed
    /// because cpal will not always say, and because a device that changes its
    /// mind mid-stream should be followed rather than argued with.
    block: AtomicU32,
    /// Bumped every time a stream opens or closes, so the reader can tell "the
    /// same input, still running" from "a different microphone" without being
    /// told. It is what makes choosing another device re-prime rather than
    /// play the tail of the last one.
    generation: AtomicU32,
}

impl InputMonitor {
    /// Room for `capacity` **samples** — interleaved, as the device delivers
    /// them, so a second of stereo at 48 kHz is 96 000 of them.
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: (0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            rate: AtomicU32::new(48_000),
            channels: AtomicU32::new(1),
            live: AtomicBool::new(false),
            block: AtomicU32::new(0),
            generation: AtomicU32::new(0),
        }
    }

    /// A stream has opened at `sample_rate` with `channels` channels.
    ///
    /// Whatever was in the ring is forgotten: it belongs to a device that is
    /// no longer the one being listened to.
    pub fn open(&self, sample_rate: u32, channels: u16) {
        self.read
            .store(self.written.load(Ordering::Acquire), Ordering::Release);
        self.rate.store(sample_rate.max(1), Ordering::Relaxed);
        self.channels
            .store(u32::from(channels.max(1)), Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
        // A different device delivers a different amount at a time, so the
        // measurement starts again with the stream.
        self.block.store(0, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.live.store(true, Ordering::Release);
    }

    /// The stream has closed. Nothing more is heard, and what was still in
    /// flight is not played through whatever is opened next.
    pub fn close(&self) {
        self.live.store(false, Ordering::Release);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether a stream is open. What the idle gate asks, and the node.
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }

    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Relaxed) as usize
    }

    /// The largest block this device has delivered, in frames. Zero before it
    /// has delivered any. See the field.
    pub fn device_block(&self) -> usize {
        self.block.load(Ordering::Relaxed) as usize
    }

    /// Changes every time a stream opens or closes. See the field.
    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Relaxed)
    }

    /// **RT**, on the input callback. Pushes one interleaved block and returns
    /// how much of it fitted.
    ///
    /// Never blocks, never allocates. What did not fit is counted rather than
    /// waited for — the same answer [`crate::InputWriter`] gives and for the
    /// same reason.
    pub fn write(&self, block: &[f32]) -> usize {
        // What this device hands over at a time, which is what the reader has
        // to hold slack against. The largest seen, because the smallest is
        // often a short first or last block.
        let frames = (block.len() / self.channels().max(1)) as u32;
        if frames > self.block.load(Ordering::Relaxed) {
            self.block.store(frames, Ordering::Relaxed);
        }
        let capacity = self.samples.len();
        let written = self.written.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let space = capacity - (written - read).min(capacity);
        let n = block.len().min(space);
        for (i, sample) in block.iter().take(n).enumerate() {
            self.samples[(written + i) % capacity].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.written.store(written + n, Ordering::Release);
        if n < block.len() {
            self.dropped.fetch_add(block.len() - n, Ordering::Relaxed);
        }
        n
    }

    /// **RT**, on the output callback. Takes up to `out.len()` samples and
    /// says how many it got. Short is an underrun, not an error.
    pub fn read(&self, out: &mut [f32]) -> usize {
        if !self.is_live() {
            return 0;
        }
        let capacity = self.samples.len();
        let read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        let n = out.len().min(written - read);
        for (i, slot) in out.iter_mut().take(n).enumerate() {
            *slot = f32::from_bits(self.samples[(read + i) % capacity].load(Ordering::Relaxed));
        }
        self.read.store(read + n, Ordering::Release);
        n
    }

    /// **RT**, on the output callback. Throws `samples` away without reading
    /// them — how the reader catches up when it has fallen too far behind to
    /// be worth playing what it missed. See this module's own note.
    pub fn skip(&self, samples: usize) -> usize {
        let read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        let n = samples.min(written - read);
        self.read.store(read + n, Ordering::Release);
        n
    }

    /// How many samples are waiting. Zero while no stream is open, whatever is
    /// physically still in the buffer: what a closed device left behind is not
    /// audio anybody asked for.
    pub fn available(&self) -> usize {
        if !self.is_live() {
            return 0;
        }
        self.written.load(Ordering::Acquire) - self.read.load(Ordering::Relaxed)
    }

    /// The same, in frames — which is what a reader actually reasons in.
    pub fn available_frames(&self) -> usize {
        self.available() / self.channels().max(1)
    }

    /// How many samples the ring had to drop because nobody emptied it. Not
    /// zero is monitoring with a hole in it.
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// How many channels a monitor is read across. The graph's buses are stereo;
/// an eight-input interface's other six are not monitored.
const MAX_CHANNELS: usize = 2;

/// The widest ratio between the input's rate and the graph's that is read
/// rather than refused.
///
/// 8 kHz into 192 kHz and back. Outside that the two are not the same kind of
/// stream and reading it would be a scratch noise rather than a mistake worth
/// making audible.
const MIN_RATIO: f64 = 0.25;
const MAX_RATIO: f64 = 4.0;

/// The node that plays a live input into the bus it is scheduled on
/// (TDD §15.4).
///
/// A **source**, like the sampler and the clip player: it adds into its output
/// rather than replacing it, because several things share a track's bus and a
/// node that overwrote would silence the instrument on the same strip.
///
/// It is scheduled at the head of the monitored track's chain, so what is
/// heard is the input through that track's inserts, its fader and its
/// routing — which is what makes *"i should be able to hear it because of it
/// routing my input track to master"* true, and what makes un-routing that
/// track silence the monitor without touching the recording.
pub struct MonitorNode {
    monitor: std::sync::Arc<InputMonitor>,
    /// One block's worth of input, read in one go so the ring is touched once
    /// per block rather than once per frame. Sized at `prepare`.
    scratch: Vec<f32>,
    /// The two input frames the interpolator is between, and where it is.
    /// See [`MonitorNode::process`].
    a: [f32; MAX_CHANNELS],
    b: [f32; MAX_CHANNELS],
    phase: f64,
    /// Whether the ring has been given a chance to fill. Cleared by an
    /// underrun and by a device change, which is what makes both of them
    /// recover rather than stutter.
    primed: bool,
    /// The generation this node last saw — see [`InputMonitor::generation`].
    generation: u32,
    sample_rate: f64,
    max_block: usize,
}

impl MonitorNode {
    pub fn new(monitor: std::sync::Arc<InputMonitor>) -> Self {
        Self {
            monitor,
            scratch: Vec::new(),
            a: [0.0; MAX_CHANNELS],
            b: [0.0; MAX_CHANNELS],
            phase: 0.0,
            primed: false,
            generation: 0,
            sample_rate: 48_000.0,
            max_block: crate::BLOCK_SIZE,
        }
    }

    /// The slack held before the first sample is played, in frames.
    ///
    /// > *"the audio monitoring sounds very flickery and weird."*
    ///
    /// **Measured against the input, not against the graph**, and that is the
    /// whole of that report. The output stream is pinned to `BLOCK_SIZE`; an
    /// input stream takes whatever its device offers, and on ALSA and PipeWire
    /// that is routinely a thousand frames or more. So a thousand frames land
    /// at once and are then drawn down 128 at a time over twenty-one
    /// milliseconds, during which nothing else arrives — and a reader holding
    /// the graph's own block ran dry eight times in between, went silent, and
    /// primed again, every single cycle.
    ///
    /// One input period plus two output blocks: enough to ride out the gap
    /// between deliveries, plus jitter either side of it. It is latency and it
    /// is reported as such by [`latency_samples`](Self::latency_samples).
    pub fn prime_frames(&self) -> usize {
        self.monitor.device_block().max(self.max_block) + self.max_block * 2
    }

    /// How far the reader is allowed to fall behind before it catches up, in
    /// frames.
    ///
    /// Four more input periods past the slack it means to hold. Far enough
    /// that a device delivering normally never trips it — the bug this
    /// replaced sized it off the *graph's* block, so an ordinary 1024-frame
    /// delivery looked like runaway drift and a block was thrown away every
    /// time one arrived — and near enough that a ring genuinely running away
    /// is trimmed long before it reaches its end.
    pub fn high_water_frames(&self) -> usize {
        self.prime_frames() + self.monitor.device_block().max(self.max_block) * 4
    }

    /// Pulls the next input frame into `b`, sliding `b` into `a`.
    /// `false` when the scratch has run out — an underrun.
    fn advance(&mut self, scratch_frames: usize, channels: usize, next: &mut usize) -> bool {
        if *next >= scratch_frames {
            return false;
        }
        self.a = self.b;
        for channel in 0..MAX_CHANNELS {
            // A mono input arrives on both sides: a microphone is a place in
            // the room, not a side of the field, and a take you can only hear
            // in one ear is nobody's idea of monitoring.
            let from = channel.min(channels - 1);
            self.b[channel] = self.scratch[*next * channels + from];
        }
        *next += 1;
        true
    }
}

impl crate::AudioNode for MonitorNode {
    fn prepare(&mut self, ctx: &crate::PrepareContext) {
        self.sample_rate = f64::from(ctx.sample_rate.max(1.0));
        self.max_block = (ctx.max_block_size as usize).max(1);
        // Enough for the widest ratio this reads at, plus the frame that
        // straddles the end of the block — so a block can never need more
        // input than the buffer holds and the read is always one call.
        let frames = (self.max_block as f64 * MAX_RATIO).ceil() as usize + 2;
        self.scratch.clear();
        self.scratch.resize(frames * MAX_CHANNELS, 0.0);
        self.primed = false;
    }

    fn process(&mut self, ctx: &mut crate::ProcessContext) {
        // A bounce is not a moment anybody is playing into. `realise` gives an
        // offline graph no monitor at all, so this is belt and braces — but a
        // microphone in an exported file is the kind of thing nobody notices
        // until it is somewhere public.
        if ctx.transport.state == crate::TransportState::Rendering {
            return;
        }
        if !self.monitor.is_live() {
            self.primed = false;
            return;
        }
        let generation = self.monitor.generation();
        if generation != self.generation {
            // A different device, or the same one reopened. Nothing held over.
            self.generation = generation;
            self.primed = false;
            self.a = [0.0; MAX_CHANNELS];
            self.b = [0.0; MAX_CHANNELS];
            self.phase = 0.0;
        }
        let frames = ctx.outputs.first().map_or(0, |o| o.len());
        if frames == 0 {
            return;
        }
        let channels = self.monitor.channels().clamp(1, MAX_CHANNELS);
        let ratio =
            (f64::from(self.monitor.sample_rate()) / self.sample_rate).clamp(MIN_RATIO, MAX_RATIO);

        // Drifting full: trim to the high-water mark rather than let the ring
        // reach its end, where it would drop a block and glitch on every one
        // from then on. See this module's own note.
        let high_water = self.high_water_frames();
        let waiting = self.monitor.available_frames();
        if waiting > high_water {
            self.monitor.skip((waiting - high_water) * channels);
        }

        // Priming: a block of slack before the first sample, or the first
        // block underruns on the difference between two clocks. It is also
        // what an underrun goes back to.
        if !self.primed {
            if self.monitor.available_frames() < self.prime_frames() {
                return;
            }
            self.primed = true;
            self.phase = 0.0;
            self.a = [0.0; MAX_CHANNELS];
            self.b = [0.0; MAX_CHANNELS];
            let mut next = 0usize;
            let got = self.monitor.read(&mut self.scratch[..2 * channels]) / channels;
            // Twice, so `a` holds the first frame and `b` the second: the
            // first sample out is the first sample in, at any ratio.
            self.advance(got, channels, &mut next);
            self.advance(got, channels, &mut next);
        }

        // Exactly what the loop below will consume, so nothing is read out of
        // the ring and thrown away: with `phase` in [0, 1), n output frames
        // advance the input by `floor(phase + n * ratio)` frames.
        let want = (self.phase + frames as f64 * ratio).floor() as usize;
        let capacity = self.scratch.len() / channels;
        let want = want.min(capacity);
        let got = self.monitor.read(&mut self.scratch[..want * channels]) / channels;

        let mut next = 0usize;
        let mut underrun = false;
        for frame in 0..frames {
            if underrun {
                break;
            }
            for (channel, out) in ctx.outputs.iter_mut().take(MAX_CHANNELS).enumerate() {
                let (a, b) = (self.a[channel], self.b[channel]);
                // Linear between the two input frames the output frame falls
                // between. At the common ratio of exactly one this is `a`
                // every time and costs a multiply nobody hears.
                out[frame] += a + (b - a) * self.phase as f32;
            }
            self.phase += ratio;
            while self.phase >= 1.0 {
                if !self.advance(got, channels, &mut next) {
                    underrun = true;
                    break;
                }
                self.phase -= 1.0;
            }
        }
        if underrun {
            // Silence for the rest of the block and a fresh prime, rather than
            // the last frame held or repeated: a hole sounds like the room
            // going quiet, a repeat sounds like a broken program.
            self.primed = false;
            self.a = [0.0; MAX_CHANNELS];
            self.b = [0.0; MAX_CHANNELS];
            self.phase = 0.0;
        }
    }

    /// A transport stop, a seek and a loop seam cut what the **song**
    /// started, and a microphone is not the song: somebody is playing into it
    /// right now and does not stop because the tape did. Re-priming here would
    /// put a block-long hole in the monitor at every loop seam.
    ///
    /// This is the case the trait's own note is about — *"only a source node
    /// that can be played by a person needs to override this"* — and a live
    /// input is the most literal example there is.
    fn reset_sequenced(&mut self) {}

    /// A full reset is a different question: a device torn down, a panic, a
    /// graph replaced. The frames in hand belong to a stream that is gone.
    fn reset(&mut self) {
        self.primed = false;
        self.a = [0.0; MAX_CHANNELS];
        self.b = [0.0; MAX_CHANNELS];
        self.phase = 0.0;
    }

    /// The slack held before the first sample is played, which is what
    /// monitoring costs. See [`prime_frames`](Self::prime_frames).
    fn latency_samples(&self) -> u32 {
        self.prime_frames() as u32
    }

    fn debug_name(&self) -> &'static str {
        "MonitorNode"
    }

    fn params(&self) -> &dyn crate::ParamSet {
        &crate::nodes::EmptyParams
    }
}
