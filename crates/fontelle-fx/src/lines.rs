//! What the modulated-delay effects share: a ring read at a fractional
//! position, and a one-pole's coefficient. Lifted out of `chorus.rs` for
//! the flanger and hyper (`docs/flopsynth-next.md` §4.5), which are the
//! same line read differently.

/// Reads `line` at a fractional position, wrapping.
///
/// Linear interpolation, which is what makes a moving read head a slide
/// rather than a stair. Its high-frequency loss is real, and on a chorus it
/// is part of why the copies sit under the dry signal rather than on top of
/// it.
pub(crate) fn read_at(line: &[f32], position: f32) -> f32 {
    let length = line.len();
    let wrapped = position.rem_euclid(length as f32);
    // `rem_euclid` of a position a hair under zero comes back a hair under
    // `length`, and a hair under `length` rounds *to* `length` in `f32` once
    // the line is a few thousand samples long — which is an index one past
    // the end, on the audio thread, on the first block after a reset. The
    // wrap belongs here rather than in every caller.
    let index = wrapped as usize;
    let (index, fraction) = if index >= length {
        (0, 0.0)
    } else {
        (index, wrapped - index as f32)
    };
    let next = if index + 1 == length { 0 } else { index + 1 };
    line[index] + (line[next] - line[index]) * fraction
}

/// A one-pole low-pass's coefficient at `cutoff_hz`.
pub(crate) fn one_pole(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let w = std::f32::consts::TAU * cutoff_hz.max(1.0) / sample_rate.max(1.0);
    (1.0 - (-w).exp()).clamp(0.0, 1.0)
}
