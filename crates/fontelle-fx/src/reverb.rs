//! The reverb's DSP: a feedback delay network. Its parameters are
//! `fontelle_types::ReverbConfig` — the document owns those, this owns the
//! network.
//!
//! Algorithmic rather than convolution (TDD §13.4): more tweakable, and no
//! impulse responses to license. What it costs is that "size" and "decay" have
//! to be made independent by construction rather than by measuring a room that
//! already had both.
//!
//! **What comes out is the tail and only the tail**, for the reason written at
//! the top of `delay.rs`.
//!
//! # How the three knobs stay three knobs
//!
//! - **Size** scales every line together. Longer lines mean a later first
//!   reflection and a sparser tail; on its own it would also make the tail
//!   last longer, because a sound goes round a long loop fewer times a second.
//! - **Decay** is therefore not a feedback gain. It is an RT60, and each
//!   line's gain is derived from *its own length*:
//!   `g = 10^(-3 L / (RT60 · fs))`, which is the gain that costs 60 dB in
//!   RT60 seconds however long the line is. That is what makes the size knob
//!   change the room and not the reverb time.
//! - **Damping** is a one-pole inside each line's feedback path, so the top
//!   comes off progressively rather than all at once. On the output it would
//!   be a tone control.
//!
//! And the matrix between the lines is a **Householder reflection**,
//! `y = x - (2/N)·Σx`, which is orthogonal: it moves energy between the lines
//! without creating or destroying any. A matrix that is not orthogonal either
//! kills the tail or runs away, and running away is the failure that reaches
//! the speakers.

use fontelle_types::{MAX_PRE_DELAY_MS, ReverbConfig};

const MAX_CHANNELS: usize = 2;

/// How many delay lines the network has. Eight is the usual floor for a tail
/// that reads as a room rather than as a set of distinct echoes: the echo
/// density of an FDN grows with the number of lines, and four is audibly a
/// flutter.
const LINES: usize = 8;

/// The lines' lengths in samples at unit scale, at any sample rate.
///
/// **Mutually prime**, which is the whole reason they are written out rather
/// than spread evenly: two lines whose lengths share a factor put their echoes
/// on top of each other at every common multiple, and a tail built out of
/// those is metallic. Distinct primes have no common multiple below their
/// product.
///
/// The spread — roughly 23 ms to 60 ms at 48 kHz — is the range of a
/// medium room's early reflections, which is what the network is standing in
/// for.
const BASE_LENGTHS: [usize; LINES] = [1123, 1381, 1607, 1867, 2129, 2371, 2617, 2879];

/// What the size knob scales the lines by, from smallest room to largest.
const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 2.0;

/// How fast the size knob's effect follows it, as a one-pole time constant.
/// Same reason as the delay's: a read pointer that jumps is a click.
const GLIDE_SECONDS: f32 = 0.2;

/// Feedback delay network, eight lines with Householder mixing (TDD §13.4).
pub struct FdnReverb {
    /// One ring per line, sized once in [`prepare`](Self::prepare) to the
    /// longest the size knob can ask for.
    lines: [Vec<f32>; LINES],
    /// The pre-delay. **One ring, not one per channel**: a room does not have
    /// a left and a right input — the input is summed before it reaches here,
    /// and the stereo in the tail comes from which lines each side is built
    /// out of, further down.
    pre: Vec<f32>,
    write: usize,
    pre_write: usize,
    /// Where the size knob has actually reached — see [`GLIDE_SECONDS`].
    scale: f32,
    started: bool,
    /// One damping pole per line.
    damp: [f32; LINES],
    sample_rate: f32,
}

impl FdnReverb {
    pub fn new() -> Self {
        Self {
            lines: std::array::from_fn(|_| Vec::new()),
            pre: Vec::new(),
            write: 0,
            pre_write: 0,
            scale: MAX_SCALE,
            started: false,
            damp: [0.0; LINES],
            sample_rate: 48_000.0,
        }
    }

    /// Sizes every line for the largest room the size knob can ask for.
    /// **The only place this type allocates.**
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        let rate = self.sample_rate / 48_000.0;
        // **Every line the same length**, sized for the longest one at the
        // largest room. They are read at different offsets, which is what
        // makes them different lines; they share one write pointer, so they
        // have to share one ring length — sizing each to its own need left the
        // long lines' pointers wrapping somewhere the writer never reached,
        // and read stale silence out of most of the network.
        let longest =
            (BASE_LENGTHS[LINES - 1] as f32 * rate * MAX_SCALE).ceil() as usize + 4;
        for line in &mut self.lines {
            line.clear();
            line.resize(longest, 0.0);
        }
        let pre = (MAX_PRE_DELAY_MS * self.sample_rate / 1000.0).ceil() as usize + 4;
        self.pre.clear();
        self.pre.resize(pre, 0.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.pre.fill(0.0);
        self.write = 0;
        self.pre_write = 0;
        self.damp = [0.0; LINES];
        self.started = false;
    }

    /// Runs `channels` through the network in place, replacing them with the
    /// tail.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &ReverbConfig) {
        if self.lines[0].is_empty() || self.pre.is_empty() || channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);

        let target = MIN_SCALE + config.size.clamp(0.0, 1.0) * (MAX_SCALE - MIN_SCALE);
        if !self.started {
            self.scale = target;
            self.started = true;
        }
        let glide = 1.0 / (GLIDE_SECONDS * self.sample_rate).max(1.0);
        let rate = self.sample_rate / 48_000.0;
        let decay = config.decay_s.clamp(0.05, 60.0);
        let damping = one_pole(config.damping_hz, self.sample_rate);
        let width = config.width.clamp(0.0, 1.0);
        let pre_len = self.pre.len();
        let pre_behind = (config.pre_delay_ms.clamp(0.0, MAX_PRE_DELAY_MS) * self.sample_rate
            / 1000.0)
            .clamp(0.0, (pre_len - 2) as f32);

        // Injected and tapped at `1/√N` each, so one pass through the network
        // is unity: the wet output of a reverb somebody has just added should
        // be the same order of magnitude as the track that fed it.
        let spread = 1.0 / (LINES as f32).sqrt();

        for frame in 0..frames {
            self.scale += (target - self.scale) * glide;

            // Everything the room hears, as one signal. A room does not have a
            // left and a right input; the stereo in the tail comes from which
            // lines each side is built out of, further down.
            let mut input = 0.0;
            for channel in 0..used {
                input += channels[channel][frame];
            }
            input /= used as f32;

            // Through the pre-delay first: the gap before the room answers.
            self.pre[self.pre_write] = input;
            let input = read_at(&self.pre, self.pre_write as f32 - pre_behind);
            self.pre_write = (self.pre_write + 1) % pre_len;

            // Read every line, damp it, and note where each one's feedback
            // gain has to be for the tail to last as long as the knob says.
            let mut tapped = [0.0f32; LINES];
            for line in 0..LINES {
                let length = (BASE_LENGTHS[line] as f32 * rate * self.scale)
                    .clamp(2.0, (self.lines[line].len() - 2) as f32);
                let value = read_at(&self.lines[line], self.write as f32 - length);
                self.damp[line] += (value - self.damp[line]) * damping;
                // 10^(-3L/(RT60·fs)) is -60 dB over RT60 seconds for a loop
                // this long. Derived per line, which is what keeps the decay
                // time independent of the size.
                let gain = 10f32.powf(-3.0 * length / (decay * self.sample_rate));
                tapped[line] = self.damp[line] * gain;
            }

            // The Householder reflection. Orthogonal, so the network neither
            // gains nor loses energy of its own — only the per-line gains and
            // the damping take any out.
            let sum: f32 = tapped.iter().sum();
            let shared = 2.0 / LINES as f32 * sum;
            for line in 0..LINES {
                self.lines[line][self.write] = tapped[line] - shared + input * spread;
            }

            // Two sides built from different lines, which is where the stereo
            // in the tail comes from: alternate lines arrive at alternate
            // times, so the two sums are decorrelated by construction.
            let mut left = 0.0;
            let mut right = 0.0;
            for line in 0..LINES {
                if line % 2 == 0 {
                    left += tapped[line];
                } else {
                    right += tapped[line];
                }
            }
            let tap = spread * std::f32::consts::SQRT_2;
            let (left, right) = (left * tap, right * tap);

            // Width as a mid/side scale, so zero is the mono tail rather than
            // one side of it.
            let mid = (left + right) * 0.5;
            let side = (left - right) * 0.5 * width;
            let out = [mid + side, mid - side];
            for channel in 0..channels.len() {
                channels[channel][frame] = out[channel.min(MAX_CHANNELS - 1)];
            }

            self.write = (self.write + 1) % self.lines[0].len();
        }
    }
}

impl Default for FdnReverb {
    fn default() -> Self {
        Self::new()
    }
}

fn one_pole(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let w = std::f32::consts::TAU * cutoff_hz.max(1.0) / sample_rate.max(1.0);
    (1.0 - (-w).exp()).clamp(0.0, 1.0)
}

fn read_at(line: &[f32], position: f32) -> f32 {
    let length = line.len();
    let wrapped = position.rem_euclid(length as f32);
    let index = wrapped as usize;
    let fraction = wrapped - index as f32;
    let next = if index + 1 == length { 0 } else { index + 1 };
    line[index] + (line[next] - line[index]) * fraction
}
