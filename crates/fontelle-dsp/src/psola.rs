//! Moving a pitch without moving the voice (`docs/tune-plan.md` §3.4).
//!
//! Time-domain pitch-synchronous overlap-add — the algorithm behind every
//! low-latency vocal tuner. Chosen over a phase vocoder because its latency is
//! a *period* rather than a frame, it has no phasiness, and it keeps the
//! spectral envelope by construction rather than by a correction step.
//!
//! # How it works
//!
//! An **analysis** mark walks through the delayed input by the detected period
//! `P`; a **synthesis** mark walks through the output by `P / ratio`. Each
//! synthesis mark takes the analysis mark nearest it in time and lays that
//! mark's grain — the input, windowed, centred on the mark — at the synthesis
//! position. A ratio over one repeats grains and one under it skips them, and
//! neither changes how long the sound lasts.
//!
//! Marks are **synthetic**: they are spaced by the period from a running
//! phase rather than estimated at glottal closures. A real-time tuner has no
//! look-ahead to find closures with, and the window's width makes the phase of
//! a mark within its period inaudible.
//!
//! At `ratio == 1` with a Hann grain two periods wide at one-period spacing,
//! the overlap-add is an identity: **the output is the delayed input**, sample
//! for sample. That is why there is no voiced/unvoiced switch and no crossfade
//! at a consonant — unvoiced is this same code at ratio one.
//!
//! # RT
//!
//! Every ring is sized in [`prepare`](PsolaShifter::prepare); `process`
//! allocates nothing (INVARIANT 1).

// Every loop here walks a frame and a channel over several parallel arrays —
// the bus, the input ring, the accumulator. Clippy's `needless_range_loop`
// wants each as an iterator chain, which for two indices over four arrays is
// less readable than the loop it replaces, and this is the shape of the whole
// job (the same allow `fontelle-fx` takes crate-wide).
#![allow(clippy::needless_range_loop)]

/// How a grain is shaped and where it is placed — the *character* of the
/// shift, which is the thing autotune plug-ins decide for you.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrainEngine {
    /// Pitch-synchronous, Hann, two periods wide. The transparent one.
    #[default]
    Smooth,
    /// Pitch-synchronous, Tukey (flat-topped), one and a half periods wide.
    /// Not quite constant-overlap-add, and the faint buzz that leaves at the
    /// period rate is the metallic edge.
    Hard,
    /// **Not** pitch-synchronous: fixed grains at half-grain spacing, read at
    /// the ratio. A plain granular varispeed — phasy, smeared, modulated at
    /// the grain rate.
    ///
    /// On this engine the pitch and the formants are the same knob, because on
    /// a resampler they are the same operation. That is not a limitation to be
    /// worked around; it is *why* this one sounds cheap, and the formant
    /// factor is an offset on top of it rather than a correction of it.
    Grain,
}

/// The longest grain the granular engine will use, in milliseconds. The rings
/// are sized for it, so the knob can move without reaching for memory.
pub const MAX_GRAIN_MS: f32 = 60.0;
/// And the shortest. Under this a grain is a period of a bass note.
pub const MIN_GRAIN_MS: f32 = 5.0;

/// The most channels one shifter carries. A mixer bus is stereo.
const MAX_CHANNELS: usize = 2;

/// A pitch-synchronous overlap-add shifter, one mark schedule for every
/// channel it carries.
///
/// **One schedule, several channels**: a stereo source keeps its image because
/// the same grain positions are read from both sides, so an inter-channel
/// phase offset survives by construction (§3.7).
pub struct PsolaShifter {
    channels: usize,
    sample_rate: f32,
    /// The reported and real delay from input to output, in samples.
    latency: u32,

    /// The input as it arrived, one flat ring per channel.
    input: Vec<Vec<f32>>,
    /// Where the next input frame is written, and how many have ever been.
    input_write: usize,
    input_count: u64,

    /// The overlap-add accumulator, one per channel, and the window sum they
    /// share — the marks are the same for every channel, so the divisor is too.
    ola: Vec<Vec<f32>>,
    window_sum: Vec<f32>,
    /// Absolute output index of the oldest slot in the output rings.
    output_count: u64,

    /// Where the analysis mark sits **relative to the synthesis mark's own
    /// time in the input**, in samples.
    ///
    /// The mark schedule is kept as this offset rather than as an absolute
    /// position, and that is what makes a ratio of one an exact identity. An
    /// absolute mark advanced by the period is a grid anchored where the
    /// stream started, and the moment the detected period changes the grid
    /// moves under it — the nearest mark to the target is then up to half a
    /// period away, and it *jitters* with every re-estimate. Held as an
    /// offset, a ratio of one keeps it at zero however the period moves, and a
    /// ratio that is not one lets it walk by whole analysis periods, which for
    /// a periodic signal is the same waveform: repeating a mark is where a
    /// shift up gets its pitch and skipping one is where a shift down gets its.
    phase: f64,
    /// The running synthesis mark, in absolute output samples.
    synthesis: f64,
    /// Whether the marks have been started. They are placed on the first block
    /// rather than in `reset`, because where they start depends on the latency.
    running: bool,

    period: f32,
    ratio: f32,
    formant: f32,
    engine: GrainEngine,
    texture: f32,
    grain_ms: f32,
    /// The granular engine's jitter, advanced once per grain.
    jitter: u32,
    /// How many grains cover an output sample at the current settings —
    /// `half / hop`. The window sum is divided by this before it divides the
    /// accumulator, so the normalisation takes out the *ripple* and leaves the
    /// *overlap gain* alone.
    ///
    /// That distinction is the whole of the level control. At a ratio of two
    /// the synthesis marks are twice as close, every sample is covered twice,
    /// and dividing that away would be six decibels of an octave-up shift
    /// nobody asked for — the doubling is where the octave comes from.
    overlap_gain: f32,
    /// And the other end: how much of each output sample a grain covers when
    /// the marks are *further* apart than the grain is wide.
    ///
    /// On the way down the grains stop overlapping and the output is a run of
    /// windowed bursts — which is exactly where the lower octave comes from,
    /// and is not something to normalise away. What it does cost is level: a
    /// Hann-windowed burst carries three eighths of the power of the samples
    /// under it. This puts that back, on a curve that is unity at a ratio of
    /// one — where the overlap is coherent and there is nothing to put back —
    /// and reaches two decibels at an octave down.
    underlap: f32,
}

impl PsolaShifter {
    pub fn new(channels: usize) -> Self {
        Self {
            channels: channels.clamp(1, MAX_CHANNELS),
            sample_rate: 48_000.0,
            latency: 0,
            input: Vec::new(),
            input_write: 0,
            input_count: 0,
            ola: Vec::new(),
            window_sum: Vec::new(),
            output_count: 0,
            phase: 0.0,
            synthesis: 0.0,
            running: false,
            period: 240.0,
            ratio: 1.0,
            formant: 1.0,
            engine: GrainEngine::Smooth,
            texture: 0.0,
            grain_ms: 25.0,
            jitter: 0x1234_5678,
            overlap_gain: 1.0,
            underlap: 1.0,
        }
    }

    /// Off-RT: sizes every ring for the longest period, the longest grain and
    /// the longest block this shifter will ever see, and fixes the latency.
    ///
    /// `latency` is the contract the graph compensates (§3.8) and is not
    /// derived here: the document computes it from the range and the mode, and
    /// one function answering for everybody is what keeps the compensation and
    /// the sound in step.
    pub fn prepare(&mut self, sample_rate: f32, p_max: u32, max_block: u32, latency: u32) {
        self.sample_rate = sample_rate.max(1.0);
        self.latency = latency;
        let p_max = p_max.max(2) as usize;
        let grain = (MAX_GRAIN_MS / 1000.0 * self.sample_rate).ceil() as usize;
        let block = max_block.max(1) as usize;
        // The input has to reach back past the latency and past the widest
        // grain's own reach, which the formant read can double.
        let input = latency as usize + 4 * p_max + 2 * grain + block + 16;
        self.input = (0..self.channels).map(|_| vec![0.0; input]).collect();
        // The output rings only have to hold what is still being written into:
        // a block, plus the reach of the grains that overlap its end.
        let output = 4 * p_max + 2 * grain + block + 16;
        self.ola = (0..self.channels).map(|_| vec![0.0; output]).collect();
        self.window_sum = vec![0.0; output];
        self.reset();
    }

    pub fn reset(&mut self) {
        for channel in &mut self.input {
            channel.fill(0.0);
        }
        for channel in &mut self.ola {
            channel.fill(0.0);
        }
        self.window_sum.fill(0.0);
        self.input_write = 0;
        self.input_count = 0;
        self.output_count = 0;
        self.phase = 0.0;
        self.synthesis = 0.0;
        self.running = false;
        self.jitter = 0x1234_5678;
    }

    /// The delay from input to output, in samples. Fixed by `prepare`.
    pub fn latency_samples(&self) -> u32 {
        self.latency
    }

    /// The period the analysis marks walk by, in samples. Held at the last
    /// voiced value over an unvoiced stretch, which with a ratio of one is the
    /// identity (§3.4).
    pub fn set_period(&mut self, period: f32) {
        self.period = period.clamp(2.0, self.sample_rate / 8.0);
    }

    /// What to multiply the pitch by.
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(0.25, 4.0);
    }

    /// The stride a grain is read at, which is what moves the spectral
    /// envelope: a grain read at 1.5 has its formants a fifth higher and its
    /// marks, and therefore its pitch, unchanged.
    ///
    /// Bounded to ±an octave. A grain read faster than twice is an aliasing
    /// question this build does not answer, and the knob's range says so.
    pub fn set_formant(&mut self, formant: f32) {
        self.formant = formant.clamp(0.5, 2.0);
    }

    /// Which engine, where on its own continuum, and how long its grains are
    /// when it has a length of its own.
    pub fn set_engine(&mut self, engine: GrainEngine, texture: f32, grain_ms: f32) {
        self.engine = engine;
        self.texture = texture.clamp(0.0, 1.0);
        self.grain_ms = grain_ms.clamp(MIN_GRAIN_MS, MAX_GRAIN_MS);
    }

    /// The grain's half-width in output samples, at the current settings.
    ///
    /// The grain's half-width in output samples, at the current settings.
    fn half_width(&self) -> f32 {
        match self.engine {
            // Two periods wide down to one and a fifth: narrower grains harden
            // the edges of a fast retune without breaking synchronism.
            GrainEngine::Smooth => self.period * (1.0 - 0.4 * self.texture),
            GrainEngine::Hard => self.period * 0.75,
            GrainEngine::Grain => self.grain_ms / 1000.0 * self.sample_rate * 0.5,
        }
        .max(2.0)
    }

    /// How far the synthesis marks are apart, in output samples.
    fn synthesis_step(&self) -> f32 {
        match self.engine {
            // The pitch comes from the mark spacing, which is what leaves the
            // grain — and so the spectral envelope — alone.
            GrainEngine::Smooth | GrainEngine::Hard => self.period / self.ratio,
            // Half a grain, whatever the note is: that is what "not
            // pitch-synchronous" means, and the comb it leaves at twice the
            // grain rate is the sound.
            GrainEngine::Grain => self.half_width(),
        }
        .max(1.0)
    }

    /// How far the analysis marks are apart, in input samples.
    fn analysis_step(&self) -> f32 {
        match self.engine {
            GrainEngine::Smooth | GrainEngine::Hard => self.period,
            GrainEngine::Grain => self.half_width(),
        }
        .max(1.0)
    }

    /// The stride a grain's samples are read at.
    fn read_stride(&self) -> f32 {
        match self.engine {
            GrainEngine::Smooth | GrainEngine::Hard => self.formant,
            // On a resampler the pitch *is* the stride — see [`GrainEngine`].
            GrainEngine::Grain => self.ratio * self.formant,
        }
    }

    /// The window at `t` in −1..=1 across the grain.
    fn window(&self, t: f32) -> f32 {
        let t = t.clamp(-1.0, 1.0);
        match self.engine {
            GrainEngine::Smooth | GrainEngine::Grain => {
                0.5 + 0.5 * (std::f32::consts::PI * t).cos()
            }
            GrainEngine::Hard => {
                // A Tukey: flat over the middle, a raised cosine at the edges.
                // At the top of the texture knob the taper is five per cent
                // and the grain is nearly rectangular, which is the rasp.
                let taper = 0.5 - 0.45 * self.texture;
                let a = t.abs();
                if a <= 1.0 - taper {
                    1.0
                } else {
                    let into = (a - (1.0 - taper)) / taper.max(1e-4);
                    0.5 + 0.5 * (std::f32::consts::PI * into).cos()
                }
            }
        }
    }

    /// One block, in place. Every channel must be the same length.
    pub fn process(&mut self, channels: &mut [&mut [f32]]) {
        if self.input.is_empty() || channels.is_empty() {
            return;
        }
        let used = channels.len().min(self.channels);
        let frames = channels
            .iter()
            .take(used)
            .map(|c| c.len())
            .min()
            .unwrap_or(0);
        if frames == 0 {
            return;
        }

        // The block in, first: a grain laid this block may read up to the
        // newest sample of it.
        let capacity = self.input[0].len();
        for frame in 0..frames {
            for channel in 0..used {
                self.input[channel][self.input_write] = channels[channel][frame];
            }
            self.input_write = (self.input_write + 1) % capacity;
        }
        self.input_count += frames as u64;

        if !self.running {
            // The first mark sits exactly one latency after the input's start,
            // so at a ratio of one the read lands on whole samples and the
            // overlap-add is an identity rather than an interpolation of one.
            self.synthesis = self.latency as f64;
            self.phase = 0.0;
            self.running = true;
        }

        // Only the gain *above* one is taken out. Under it the grains
        // underlap and the window sum is the whole correction; over it every
        // sample is covered more than once and dividing that away would take
        // the octave out of an octave-up shift — see `overlap_gain`.
        let coverage = (self.half_width() / self.synthesis_step()).clamp(0.05, 8.0);
        // How much of the overlap adds up rather than cancelling out.
        //
        // Two grains a synthesis step apart carry the same waveform offset by
        // that step, and what the sum does depends on where the step falls
        // within the waveform's own period *as it reads out* — `P / stride`.
        // A whole number of periods and they reinforce; half of one and they
        // cancel, which is exactly where an octave-up shift gets its octave
        // from. So the overlap gain is taken *out* only where the overlap
        // really added: a chipmunk — formants following, so the grain reads at
        // the ratio and every step lands on a whole period — is six decibels
        // hot without this, and an octave-up with the formants held is six
        // decibels quiet with it applied everywhere.
        let content_period = (self.period / self.read_stride().abs().max(1e-3)).max(1e-3);
        let phase = (self.synthesis_step() / content_period).fract();
        let coherence = (std::f32::consts::PI * phase).cos().abs();
        self.overlap_gain = 1.0 + (coverage.max(1.0) - 1.0) * (1.0 - coherence);
        self.underlap = (0.75 * coverage + 0.25).min(1.0).sqrt();
        let block_end = self.output_count + frames as u64;
        self.place_grains(block_end, used);

        // And out: the accumulator over the window sum, each slot cleared
        // behind the read head so the ring can be used again.
        let out_capacity = self.window_sum.len();
        for frame in 0..frames {
            let absolute = self.output_count + frame as u64;
            let slot = (absolute % out_capacity as u64) as usize;
            let sum = (self.window_sum[slot] / self.overlap_gain).max(1.0) * self.underlap;
            for channel in 0..used {
                channels[channel][frame] = self.ola[channel][slot] / sum;
                self.ola[channel][slot] = 0.0;
            }
            self.window_sum[slot] = 0.0;
        }
        self.output_count = block_end;
    }

    /// Lays down every grain that reaches into the block ending at `block_end`.
    fn place_grains(&mut self, block_end: u64, used: usize) {
        let out_capacity = self.window_sum.len() as f64;
        // A hard stop, so a period or a ratio that has gone strange cannot
        // spin here on the audio thread.
        let mut guard = 0;
        loop {
            let half = self.half_width();
            if self.synthesis - half as f64 >= block_end as f64 {
                break;
            }
            guard += 1;
            if guard > out_capacity as i32 {
                break;
            }
            // Where this synthesis mark's own time falls in the input, and the
            // analysis mark a whole number of periods from it.
            let target = self.synthesis - self.latency as f64;
            let p_in = self.analysis_step() as f64;
            let p_out = self.synthesis_step() as f64;
            let mark = match self.engine {
                GrainEngine::Grain => {
                    // ±half a grain of jitter at the top of the texture knob:
                    // at the bottom the modulation is a clean comb at the
                    // grain rate, at the top it is a cloud.
                    self.jitter = self
                        .jitter
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    let unit = (self.jitter >> 8) as f32 / 8_388_608.0 - 1.0;
                    target + self.phase + f64::from(unit * self.texture * half * 0.5)
                }
                _ => target + self.phase,
            };
            self.lay(mark, self.synthesis, half, used);
            // How many whole analysis periods to walk, to keep up with a
            // synthesis mark that has moved `p_out`. One is the ordinary case;
            // zero repeats a grain, which is a shift up, and two skips one,
            // which is a shift down.
            let steps = ((p_out - self.phase) / p_in).round().max(0.0);
            self.phase += steps * p_in - p_out;
            // Kept inside half a period, by whole periods — which for a
            // periodic signal is the same waveform, and is what stops a
            // changing period from walking the mark off the note.
            while self.phase > p_in * 0.5 {
                self.phase -= p_in;
            }
            while self.phase < -p_in * 0.5 {
                self.phase += p_in;
            }
            self.synthesis += p_out;
        }
    }

    /// One grain: the input around `mark`, windowed, added at `at`.
    fn lay(&mut self, mark: f64, at: f64, half: f32, used: usize) {
        let stride = self.read_stride();
        let first = (at - half as f64).ceil() as i64;
        let last = (at + half as f64).floor() as i64;
        let out_capacity = self.window_sum.len() as i64;
        let newest = self.input_count as f64 - 1.0;
        for n in first..=last {
            if (n as u64) < self.output_count {
                // Already emitted; a grain cannot reach into the past.
                continue;
            }
            let offset = n as f64 - at;
            let weight = self.window(offset as f32 / half);
            if weight <= 0.0 {
                continue;
            }
            // The read runs off the newest sample only at the very bottom of a
            // range with the formant knob at its top, where the grain is wider
            // than the look-ahead; holding the last sample there is a fraction
            // of one grain's tail rather than a click.
            let read = (mark + offset * stride as f64).min(newest);
            let slot = (n % out_capacity) as usize;
            for channel in 0..used {
                self.ola[channel][slot] += weight * self.read_input(channel, read);
            }
            self.window_sum[slot] += weight;
        }
    }

    /// The input of `channel` at a fractional absolute position, by Hermite —
    /// four points read through the ring rather than out of a slice, because
    /// the ring wraps and a slice would have to be copied to be contiguous.
    fn read_input(&self, channel: usize, position: f64) -> f32 {
        let capacity = self.input[channel].len() as i64;
        let base = position.floor();
        let t = (position - base) as f32;
        let base = base as i64;
        let oldest = self.input_count as i64 - capacity;
        let newest = self.input_count as i64 - 1;
        let at = |offset: i64| -> f32 {
            let index = (base + offset).clamp(oldest.max(0), newest.max(0));
            if index < 0 {
                return 0.0;
            }
            self.input[channel][(index % capacity) as usize]
        };
        let (x0, x1, x2, x3) = (at(-1), at(0), at(1), at(2));
        let c0 = x1;
        let c1 = 0.5 * (x2 - x0);
        let c2 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
        let c3 = 0.5 * (x3 - x0) + 1.5 * (x1 - x2);
        ((c3 * t + c2) * t + c1) * t + c0
    }
}
