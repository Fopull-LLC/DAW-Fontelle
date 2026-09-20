//! The modulation sources `docs/flopsynth-next.md` §4.2 adds beside the
//! envelopes and the LFOs: a chaotic attractor, a random walk, a follower
//! of the voice's own level, and two step sequencers. Each is a small piece
//! of per-voice state advanced once a modulation step (`voice::MOD_STEP`),
//! and the settings that shape them live on the patch ([`Chaos`],
//! [`RandomWalk`], [`StepSequencer`]) beside the LFOs'.
//!
//! Here in `fontelle-core` and not `fontelle-dsp` for the reason the LFO
//! is: a sequencer's division is [`fontelle_types::NoteDivision`], and the
//! DSP crate sees nothing but numbers (INVARIANT 4).

use fontelle_types::NoteDivision;

/// A Lorenz attractor's settings: how fast it is run.
///
/// The attractor is Serum 2's chaos source and the one everybody means by
/// it — a continuous, bounded, never-repeating wander between two lobes.
/// Per voice, so a chord is four orbits.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Chaos {
    /// Orbits a second, roughly: at 1 the attractor swings between its
    /// lobes about once a second.
    pub rate_hz: f32,
}

impl Default for Chaos {
    fn default() -> Self {
        Self { rate_hz: 1.0 }
    }
}

/// A random walk's settings: how often it picks somewhere new to go, and
/// how long it takes getting there.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RandomWalk {
    /// New targets a second.
    pub rate_hz: f32,
    /// 0..1: at 0 the value jumps to each target (a sample & hold), at 1
    /// it takes the whole interval to arrive (a glide).
    pub smooth: f32,
}

impl Default for RandomWalk {
    fn default() -> Self {
        Self {
            rate_hz: 2.0,
            smooth: 0.5,
        }
    }
}

/// The most steps a sequencer holds.
pub const SEQ_STEPS: usize = 16;

/// One of the patch's two step sequencers: sixteen values, how many of
/// them play, and how fast.
///
/// **State, not events** (Ty's §9.3): the value at a step is what the
/// matrix reads, and nothing here emits a note. Synced, the position is a
/// fact about the transport — every voice reads the same step, and the
/// same bar plays the same steps every time — and free, in hertz, it
/// starts from the first step with the note, as a retriggered LFO does.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StepSequencer {
    /// Each −1..=1.
    pub steps: [f32; SEQ_STEPS],
    /// How many of `steps` play before it comes round, 1..=16.
    pub length: u8,
    /// Steps a second, when not synced.
    pub rate_hz: f32,
    pub sync: bool,
    /// The length of one step, when synced.
    pub division: NoteDivision,
    /// 0..1: how much of a step it takes to reach each new value.
    pub smooth: f32,
}

impl Default for StepSequencer {
    fn default() -> Self {
        Self {
            steps: [0.0; SEQ_STEPS],
            length: SEQ_STEPS as u8,
            rate_hz: 4.0,
            sync: true,
            division: NoteDivision::Sixteenth,
            smooth: 0.0,
        }
    }
}

impl StepSequencer {
    /// Steps a second, at `bpm`.
    pub fn steps_per_second(&self, bpm: f32) -> f32 {
        if self.sync {
            (bpm.max(1.0) / 60.0) / self.division.beats().max(1e-4)
        } else {
            self.rate_hz.max(0.0)
        }
    }
}

/// A Lorenz attractor in flight: per voice, seeded from the note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChaosState {
    x: f32,
    y: f32,
    z: f32,
}

impl Default for ChaosState {
    fn default() -> Self {
        Self {
            x: 1.0,
            y: 1.0,
            z: 20.0,
        }
    }
}

impl ChaosState {
    /// Starts a note somewhere on the attractor that depends on `seed`,
    /// so two notes are two orbits and the same note is the same one twice.
    pub fn reset(&mut self, seed: u32) {
        let mut x = seed.wrapping_mul(0x9e37_79b9) | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let a = (x & 0xffff) as f32 / 65_535.0;
        let b = ((x >> 16) & 0xffff) as f32 / 65_535.0;
        // Inside the attractor's box rather than at its origin, which is an
        // unstable fixed point every orbit takes a while to leave.
        self.x = -12.0 + 24.0 * a;
        self.y = -12.0 + 24.0 * b;
        self.z = 10.0 + 25.0 * (a + b) * 0.5;
    }

    /// Advances `seconds` at `rate_hz` and returns `x`, scaled to −1..=1.
    ///
    /// Lorenz's own constants (σ 10, ρ 28, β 8/3); one orbit is about 0.7
    /// of its time units, so a rate of 1 Hz runs 0.7 units a second.
    /// Euler in sub-steps short enough to stay on the attractor whatever
    /// the rate: the step is bounded at 0.002 units, which keeps the
    /// integration well inside the stable region (the tell of a step too
    /// long is an orbit that flies off to infinity).
    pub fn advance(&mut self, rate_hz: f32, seconds: f32) -> f32 {
        let total = rate_hz.max(0.0) * 0.7 * seconds;
        if total > 0.0 {
            let pieces = (total / 0.002).ceil().clamp(1.0, 64.0);
            let dt = total / pieces;
            for _ in 0..pieces as usize {
                let dx = 10.0 * (self.y - self.x);
                let dy = self.x * (28.0 - self.z) - self.y;
                let dz = self.x * self.y - (8.0 / 3.0) * self.z;
                self.x += dx * dt;
                self.y += dy * dt;
                self.z += dz * dt;
            }
        }
        (self.x / 20.0).clamp(-1.0, 1.0)
    }
}

/// A random walk in flight: where it is, where it is going, and the clock
/// that says when to pick again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkState {
    value: f32,
    target: f32,
    /// Fraction of the interval elapsed, 0..1.
    phase: f32,
    rng: u32,
}

impl Default for WalkState {
    fn default() -> Self {
        Self {
            value: 0.0,
            target: 0.0,
            phase: 1.0,
            rng: 0x2545_f491,
        }
    }
}

impl WalkState {
    pub fn reset(&mut self, seed: u32) {
        self.rng = seed.wrapping_mul(0x9e37_79b9).wrapping_add(0x85eb_ca6b) | 1;
        self.phase = 1.0;
        self.target = self.next_random();
        self.value = self.target;
    }

    fn next_random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / 8_388_608.0 - 1.0
    }

    /// Advances `seconds` and returns the value, −1..=1.
    ///
    /// A new target every `1 / rate_hz` seconds; between targets the value
    /// slews towards the current one through a one-pole whose time
    /// constant is `smooth` intervals — at 0 it is there at once, at 1 it
    /// spends the whole interval arriving.
    pub fn advance(&mut self, config: &RandomWalk, seconds: f32) -> f32 {
        let rate = config.rate_hz.max(1e-3);
        self.phase += rate * seconds;
        if self.phase >= 1.0 {
            self.phase = self.phase.rem_euclid(1.0);
            self.target = self.next_random();
        }
        let smooth = config.smooth.clamp(0.0, 1.0);
        if smooth <= 0.0 {
            self.value = self.target;
        } else {
            // τ = smooth / rate seconds; the one-pole's step is 1 − e^(−dt/τ).
            let tau = smooth / rate;
            let coefficient = 1.0 - (-seconds / tau).exp();
            self.value += (self.target - self.value) * coefficient;
        }
        self.value
    }
}

/// A follower of the voice's own level: fast up, slower down, like the
/// detector in a compressor. Reads 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FollowerState {
    level: f32,
}

impl FollowerState {
    pub fn reset(&mut self) {
        self.level = 0.0;
    }

    /// Feeds the peak of one step and returns the level.
    ///
    /// Five milliseconds up and fifty down: quick enough to catch a pluck,
    /// slow enough not to ripple at the note's own frequency.
    pub fn advance(&mut self, peak: f32, seconds: f32) -> f32 {
        let tau = if peak > self.level { 0.005 } else { 0.05 };
        let coefficient = 1.0 - (-seconds / tau).exp();
        self.level += (peak.clamp(0.0, 1.0) - self.level) * coefficient;
        self.level
    }

    pub fn level(&self) -> f32 {
        self.level
    }
}

/// A step sequencer in flight.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SeqState {
    /// Position in steps, free-running; the step is its floor modulo the
    /// length.
    position: f64,
    smoothed: f32,
    started: bool,
}

impl SeqState {
    pub fn reset(&mut self) {
        self.position = 0.0;
        self.smoothed = 0.0;
        self.started = false;
    }

    /// Which step is playing, 0..length.
    pub fn step(&self, config: &StepSequencer) -> usize {
        let length = usize::from(config.length.clamp(1, SEQ_STEPS as u8));
        (self.position.floor().max(0.0) as usize) % length
    }

    /// Advances `seconds` at `bpm` and returns the value, −1..=1.
    ///
    /// `clock_seconds` is where the transport is, for a synced sequencer,
    /// which reads its position off the clock the way a free-running LFO
    /// does; a free one accumulates from the note.
    pub fn advance(
        &mut self,
        config: &StepSequencer,
        bpm: f32,
        seconds: f32,
        clock_seconds: Option<f64>,
    ) -> f32 {
        let per_second = config.steps_per_second(bpm);
        if config.sync
            && let Some(at) = clock_seconds
        {
            self.position = at * f64::from(per_second);
        }
        // The value at the top of the step, before advancing.
        let raw = config.steps[self.step(config)].clamp(-1.0, 1.0);
        let smooth = config.smooth.clamp(0.0, 1.0);
        let value = if smooth > 0.0 && per_second > 0.0 && self.started {
            let tau = smooth / per_second;
            let coefficient = 1.0 - (-seconds / tau).exp();
            self.smoothed += (raw - self.smoothed) * coefficient;
            self.smoothed
        } else {
            self.smoothed = raw;
            raw
        };
        self.started = true;
        self.position += f64::from(per_second * seconds);
        value
    }
}
