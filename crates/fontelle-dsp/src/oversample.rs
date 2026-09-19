//! Oversampling, for the things a mip pyramid cannot help.
//!
//! A table read at the right mip level has nothing above Nyquist to fold.
//! What is done *to* the read has: a hard sync's restart, a quantised phase,
//! FM at two cycles and a `tanh` in a ladder's loop all make harmonics past
//! the top of the band, and at the session's rate those come back down as
//! tones that are not the note's (`docs/flopsynth-next.md` §4.1; measured
//! in `tests/synth_alias.rs` — a sync at 8× on C7 was 18 dB of alias
//! against signal). The fix is the usual one: render at two or four times
//! the rate, where those harmonics have room, and come back down through a
//! low-pass that takes them out.
//!
//! The low-pass is a polyphase FIR: [`OVERSAMPLE_TAPS`] taps per
//! sub-sample, a Hamming-windowed sinc with its −6 dB point at 0.45 of the
//! session's rate. Sixteen taps per branch rather than the twelve the plan
//! sketched: twelve drooped a decibel at 18 kHz, sixteen holds it to half of
//! one and puts everything that would fold below 16 kHz under −58 dB
//! (worked out on the kernel's response before the table was cut; the
//! passband is held by `tests/oversample.rs`). The state per channel is the
//! sixteen accumulators, so an oscillator's stereo pair is `[f32; 32]` and
//! [`crate::SynthState`] stays `Copy`. The cost is `OVERSAMPLE_TAPS` multiply-adds
//! per sub-sample per channel — 64 per output sample at 4× — on top of
//! rendering the sub-samples themselves.
//!
//! **Off is a different branch, not a filter everything goes through.** An
//! oscillator or a filter at [`Oversampling::Off`] runs the code it always
//! ran, sample for sample; the tests hold that.

/// How many samples an oscillator (or a filter's nonlinearity) renders for
/// each one the session hears.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Oversampling {
    /// The session's rate. What every patch written before this had.
    #[default]
    Off,
    X2,
    X4,
}

impl Oversampling {
    /// In the order the chooser offers them.
    pub const ALL: [Oversampling; 3] = [Oversampling::Off, Oversampling::X2, Oversampling::X4];

    /// The sub-samples rendered per output sample: 1, 2 or 4.
    pub fn factor(self) -> usize {
        match self {
            Oversampling::Off => 1,
            Oversampling::X2 => 2,
            Oversampling::X4 => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Oversampling::Off => "Off",
            Oversampling::X2 => "2\u{d7}",
            Oversampling::X4 => "4\u{d7}",
        }
    }

    pub fn is_off(&self) -> bool {
        *self == Oversampling::Off
    }
}

/// Taps per polyphase branch — the kernel is this many times the factor
/// long, and this is what a [`Decimator`] or an [`Interpolator`] keeps per
/// channel.
pub const OVERSAMPLE_TAPS: usize = 16;

/// The most sub-samples one output sample is rendered from.
pub const MAX_OVERSAMPLE: usize = 4;

/// The kernel for `factor`, as its polyphase branches: branch `p` holds every
/// tap `p + j·N` of the `N·OVERSAMPLE_TAPS`-tap low-pass, so a sub-sample's
/// contribution to the next sixteen outputs is one contiguous dot product.
/// Generated from the formula `tests/oversample.rs` re-derives, and summing
/// to one so the level through a decimator is the level in.
pub fn oversample_kernel(factor: Oversampling) -> &'static [[f32; OVERSAMPLE_TAPS]] {
    match factor {
        Oversampling::Off => &[],
        Oversampling::X2 => &KERNEL_2X,
        Oversampling::X4 => &KERNEL_4X,
    }
}

const KERNEL_2X: [[f32; OVERSAMPLE_TAPS]; 2] = [
    [
        0.00012867278,
        0.0006444042,
        -0.003220503,
        0.009670843,
        -0.02204844,
        0.042768653,
        -0.078480974,
        0.17680712,
        0.41174975,
        -0.045827653,
        0.004551555,
        0.0074782036,
        -0.009186841,
        0.006826916,
        -0.0038151094,
        0.0019534063,
    ],
    [
        0.0019534063,
        -0.0038151094,
        0.006826916,
        -0.009186841,
        0.0074782036,
        0.004551555,
        -0.045827653,
        0.41174975,
        0.17680712,
        -0.078480974,
        0.042768653,
        -0.02204844,
        0.009670843,
        -0.003220503,
        0.0006444042,
        0.00012867278,
    ],
];

const KERNEL_4X: [[f32; OVERSAMPLE_TAPS]; 4] = [
    [
        -0.00021907278,
        0.0007455062,
        -0.0023237038,
        0.0056867977,
        -0.011297652,
        0.019393628,
        -0.030904382,
        0.054651834,
        0.21985635,
        -0.0026461922,
        -0.008568912,
        0.009696307,
        -0.007647566,
        0.00480147,
        -0.0024317084,
        0.0010778584,
    ],
    [
        0.0003589392,
        -0.00019171728,
        -0.0006976574,
        0.0034867076,
        -0.009611068,
        0.02116363,
        -0.044020478,
        0.122898445,
        0.18389483,
        -0.036573734,
        0.011323063,
        -0.0017772763,
        -0.001630065,
        0.0020975284,
        -0.0014635632,
        0.0008718567,
    ],
    [
        0.0008718567,
        -0.0014635632,
        0.0020975284,
        -0.001630065,
        -0.0017772763,
        0.011323063,
        -0.036573734,
        0.18389483,
        0.122898445,
        -0.044020478,
        0.02116363,
        -0.009611068,
        0.0034867076,
        -0.0006976574,
        -0.00019171728,
        0.0003589392,
    ],
    [
        0.0010778584,
        -0.0024317084,
        0.00480147,
        -0.007647566,
        0.009696307,
        -0.008568912,
        -0.0026461922,
        0.21985635,
        0.054651834,
        -0.030904382,
        0.019393628,
        -0.011297652,
        0.0056867977,
        -0.0023237038,
        0.0007455062,
        -0.00021907278,
    ],
];

/// N samples in, one out: the low-pass, then every Nth sample — done as one
/// thing, so the samples that would be thrown away are never filtered.
///
/// Transposed form: each sub-sample is multiplied into the accumulators of
/// the next `OVERSAMPLE_TAPS` outputs as it arrives, and an output is the
/// accumulator that has heard all of its taps. One channel's worth; an
/// oscillator keeps two.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decimator {
    acc: [f32; OVERSAMPLE_TAPS],
}

impl Decimator {
    /// One output sample from `block`, which is the `factor.factor()`
    /// sub-samples rendered for it, oldest first.
    ///
    /// A block of the wrong length is trimmed or padded with silence rather
    /// than panicked over; nothing calls it that way, and the audio thread
    /// is not where to find out that something does.
    pub fn decimate(&mut self, block: &[f32], factor: Oversampling) -> f32 {
        let branches = oversample_kernel(factor);
        let n = branches.len();
        if n == 0 {
            return block.first().copied().unwrap_or(0.0);
        }
        for sub in 0..n {
            let x = block.get(sub).copied().unwrap_or(0.0);
            // The newest sub-sample of an output period is the kernel's
            // first tap, so sub-sample `i` reads branch `N−1−i`.
            let branch = &branches[n - 1 - sub];
            for (acc, tap) in self.acc.iter_mut().zip(branch) {
                *acc += tap * x;
            }
        }
        let out = self.acc[0];
        self.acc.copy_within(1.., 0);
        self.acc[OVERSAMPLE_TAPS - 1] = 0.0;
        out
    }
}

/// One sample in, N out: zero-stuffing through the same low-pass, which is
/// what puts a band-limited signal at the higher rate without the images a
/// held or linearly-drawn one would carry into a nonlinearity.
#[derive(Debug, Clone, Copy, Default)]
pub struct Interpolator {
    /// The last `OVERSAMPLE_TAPS` inputs, newest first.
    history: [f32; OVERSAMPLE_TAPS],
}

impl Interpolator {
    /// The `factor.factor()` sub-samples for `input`, into the front of
    /// `block`; the rest of it is left alone.
    pub fn interpolate(
        &mut self,
        input: f32,
        factor: Oversampling,
        block: &mut [f32; MAX_OVERSAMPLE],
    ) {
        let branches = oversample_kernel(factor);
        let n = branches.len();
        if n == 0 {
            block[0] = input;
            return;
        }
        self.history.copy_within(..OVERSAMPLE_TAPS - 1, 1);
        self.history[0] = input;
        // Zero-stuffing divides the level by N; the kernel sums to one, so
        // the gain goes back on here.
        let gain = n as f32;
        for (sub, branch) in branches.iter().enumerate() {
            let mut sum = 0.0;
            for (tap, past) in branch.iter().zip(&self.history) {
                sum += tap * past;
            }
            block[sub] = sum * gain;
        }
    }
}
