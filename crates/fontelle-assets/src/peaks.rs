//! The waveform you see inside an audio clip (TDD §15.3).
//!
//! *"i should be able to see the waveform of the audio inside the clip."*
//!
//! Drawing a waveform is not drawing the samples. A four-bar clip at 48 kHz is
//! about four hundred thousand frames and the block it sits in is a few hundred
//! pixels wide, so what is actually drawn is **the loudest and quietest sample
//! in each pixel's worth of time**. Computing that per frame per repaint is
//! what makes a timeline crawl, so it is computed once, at several resolutions,
//! and the renderer asks for the one that fits ([`PeakData::level_for`]).
//!
//! Two properties do the work, and both are easy to get subtly wrong:
//!
//! - **A bucket's extremes are the extremes of the samples in it** — not their
//!   average. Averaging is a waveform that flattens as you zoom out, which is a
//!   lie about how loud the take was.
//! - **Every level covers the whole clip.** A coarser level that drops the last
//!   partial bucket draws a clip that stops before it ends.
//!
//! Pure: no file, no thread, no cache. §17.1 puts the cached form under the
//! project's `cache/` directory because it is regenerable, and regenerating it
//! is this function.

use fontelle_types::AssetId;

/// How many frames one bucket of the finest level covers.
///
/// A pixel of a clip zoomed in far enough to see individual transients is
/// somewhere around a hundred frames; 64 is under that with room to spare, and
/// it keeps the finest level to about one and a half per cent of the sample
/// data — small enough to hold for every clip in a project.
pub const PEAK_BUCKET: usize = 64;

/// Peak files generated at multiple zoom levels and cached under the project's
/// `cache/` directory (regenerable, safe to delete — TDD §15.3, §17.1).
/// Display must stay responsive while peaks are still generating: draw what
/// exists, fill in progressively.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakData {
    pub asset: AssetId,
    /// How many **frames** of audio these levels cover.
    ///
    /// Kept here rather than asked of the caller: a reader mapping a position
    /// in the file to a bucket needs both, and two numbers travelling
    /// separately is the usual reason a waveform draws the wrong part of a
    /// take.
    pub frames: usize,
    /// One `Vec<(min, max)>` per zoom level, **coarsest first** — so
    /// `levels.last()` is the most detailed and `levels[0]` is the summary.
    pub levels: Vec<Vec<(f32, f32)>>,
}

impl PeakData {
    /// Which level to draw `frames` frames into `width` pixels.
    ///
    /// The finest level whose buckets are no *wider* than a pixel would be
    /// wasted detail; the coarsest that still has at least one bucket per pixel
    /// is what is wanted, so a zoomed-out clip reads a short summary and a
    /// zoomed-in one reads real samples. Falls back to the finest there is
    /// rather than to nothing: a clip drawn from too little is a clip drawn.
    pub fn level_for(&self, frames: usize, width: usize) -> usize {
        if self.levels.is_empty() {
            return 0;
        }
        let last = self.levels.len() - 1;
        if width == 0 || frames == 0 {
            return 0;
        }
        for (index, level) in self.levels.iter().enumerate() {
            if level.len() >= width {
                return index;
            }
        }
        last
    }
}

/// Builds the levels for `samples`, which are **interleaved** across
/// `channels`.
///
/// One waveform per clip rather than one per channel: a lane is fourteen pixels
/// tall at the height it ships at, and two waveforms in that are neither. The
/// extremes are taken across every channel of a frame, so a spike in one
/// channel is a spike in the picture — which is what you are looking at a
/// waveform to find.
pub fn generate_peaks(asset: AssetId, samples: &[f32], channels: u16) -> PeakData {
    let channels = channels.max(1) as usize;
    let frames = samples.len() / channels;
    if frames == 0 {
        return PeakData {
            asset,
            frames: 0,
            levels: Vec::new(),
        };
    }

    // The finest level, straight off the samples.
    let mut finest: Vec<(f32, f32)> = Vec::with_capacity(frames.div_ceil(PEAK_BUCKET));
    let mut frame = 0;
    while frame < frames {
        let end = (frame + PEAK_BUCKET).min(frames);
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for value in &samples[frame * channels..end * channels] {
            lo = lo.min(*value);
            hi = hi.max(*value);
        }
        finest.push((lo, hi));
        frame = end;
    }

    // And each coarser level by folding pairs of the one below — which is what
    // makes "a coarse level keeps the extremes the fine one found" true by
    // construction rather than by a second pass that could disagree.
    let mut levels = vec![finest];
    while levels[levels.len() - 1].len() > 1 {
        let below = &levels[levels.len() - 1];
        let above: Vec<(f32, f32)> = below
            .chunks(2)
            .map(|pair| {
                pair.iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |acc, b| {
                        (acc.0.min(b.0), acc.1.max(b.1))
                    })
            })
            .collect();
        levels.push(above);
    }
    levels.reverse();
    PeakData {
        asset,
        frames,
        levels,
    }
}
