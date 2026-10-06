//! Comping (plan §3.1 Record page, P5): the best of several takes, one
//! span from each, crossfaded where they meet.
//!
//! Pure: takes in, one buffer out. Every take is on the same timeline —
//! frame 0 of each is the same moment — which is what a set of takes
//! recorded against one start (or one song position) is.

use crate::edit::FadeShape;

/// One span of the comp: frames `start..end` come from take `take`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompSpan {
    pub take: usize,
    pub start: usize,
    pub end: usize,
}

/// The comp's crossfade, the plan's 10 ms, in frames at `sample_rate`.
pub fn default_crossfade(sample_rate: u32) -> usize {
    crate::edit::ms_to_samples(10.0, sample_rate)
}

/// The comp of `takes` over `spans`, `len` frames long.
///
/// Spans are taken in start order; where one ends and the next begins
/// (the same frame), the two takes cross over `crossfade` frames centred on
/// that frame, `shape` up for the incoming take and down for the outgoing
/// one. Frames no span covers, or past a take's end, are silence. Outside
/// the crossfades a frame is exactly its take's.
pub fn comp(
    takes: &[&[f32]],
    spans: &[CompSpan],
    len: usize,
    crossfade: usize,
    shape: FadeShape,
) -> Vec<f32> {
    let mut spans: Vec<CompSpan> = spans
        .iter()
        .copied()
        .filter(|s| s.end > s.start && s.start < len)
        .collect();
    spans.sort_by_key(|s| s.start);
    let frame = |take: usize, i: usize| {
        takes
            .get(take)
            .and_then(|t| t.get(i))
            .copied()
            .unwrap_or(0.0)
    };
    let mut out = vec![0.0f32; len];
    for span in &spans {
        for (i, slot) in out
            .iter_mut()
            .enumerate()
            .take(span.end.min(len))
            .skip(span.start)
        {
            *slot = frame(span.take, i);
        }
    }
    // Each seam: two spans that meet on a frame, crossed over `crossfade`
    // frames centred on it. Half of it either side, so a seam at the very
    // start of a span still has the outgoing take to fade from.
    let half = crossfade / 2;
    for pair in spans.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if from.end != to.start || crossfade == 0 || from.take == to.take {
            continue;
        }
        let seam = to.start;
        let lo = seam.saturating_sub(half).max(from.start);
        let hi = (seam + (crossfade - half)).min(to.end).min(len);
        let width = (seam + (crossfade - half)) - seam.saturating_sub(half);
        for (i, slot) in out.iter_mut().enumerate().take(hi).skip(lo) {
            // Position through the whole fade, so a fade clipped by a short
            // span keeps the same curve rather than squeezing it.
            let t = (i + half - seam) as f32 / width as f32;
            *slot = frame(from.take, i) * shape.gain(1.0 - t) + frame(to.take, i) * shape.gain(t);
        }
    }
    out
}
