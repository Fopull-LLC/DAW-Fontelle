//! What an audio block's edges mean for the part of the take it plays.
//!
//! > *"if i edit my clips enough and dragging them in after cutting them it
//! > removes the content of the audio for the section after the cutoff if i
//! > try to expand it again its weird. please remove any odd interactions
//! > like this."*
//!
//! A clip whose file plays at its own rate, once, without repeating — a
//! **window** onto its take — has one rule here: its trim
//! (`source_start..source_end`) is exactly what its block shows. The right
//! edge dragged out reveals more of the file, up to its end; dragged in, the
//! trim follows it, and so does the fade-out that belongs to that edge. The
//! trim used to be set once, at a cut, and never again, so a half grown back
//! out drew nothing past the cut (and faded out in the middle of itself)
//! while the player went on reading the file.
//!
//! Everything else — a stretched clip, whose file fills its block, and a
//! repeating one, whose content comes round — keeps its trim when its block
//! changes, because there the block is not a window.

use fontelle_types::{AudioClipData, ClipLoopMode, ClipStretch, Sample, Tick};

use crate::{Clip, ClipSource, TempoMap};

/// Whether `clip` is an audio block that is a plain window onto its take:
/// not stretched, not repeating in the file, not looped on the arrangement.
pub fn is_window(clip: &Clip) -> bool {
    match &clip.source {
        ClipSource::Audio(data) => {
            data.stretch == ClipStretch::Off
                && data.loop_mode == ClipLoopMode::Once
                && !clip.loop_length.is_some_and(|period| period > 0)
        }
        _ => false,
    }
}

/// Whether `clip` comes round — looped on the arrangement, or in the file.
pub fn repeats(clip: &Clip) -> bool {
    match &clip.source {
        ClipSource::Audio(data) => {
            data.loop_mode == ClipLoopMode::Loop || clip.loop_length.is_some_and(|p| p > 0)
        }
        _ => clip.loop_length.is_some_and(|p| p > 0),
    }
}

/// How many **file** frames a window clip reads per song sample: the file's
/// rate against the device's, times its speed. The player's own arithmetic
/// (`AudioClipData::read_ratio` and `time_rate`), asked rather than repeated.
pub fn file_frames_per_sample(tempo: &TempoMap, data: &AudioClipData) -> f64 {
    data.read_ratio(data.sample_rate, tempo.sample_rate_hz(), 0) * data.time_rate()
}

/// How many file frames the stretch of block from `from` to `to` covers.
/// Signed: negative when `to` is before `from`.
pub fn file_frames_over(tempo: &TempoMap, data: &AudioClipData, from: Tick, to: Tick) -> Sample {
    let samples = tempo.tick_to_sample(to) - tempo.tick_to_sample(from);
    (samples as f64 * file_frames_per_sample(tempo, data)).round() as Sample
}

/// Makes a window clip's trim what its block shows: the far end of the trim
/// (the end for a forward clip, the start for a reversed one) set from the
/// block's length, and never past the file.
///
/// Anything that is not a window is left alone, as is a clip whose file's
/// rate is not known.
pub fn fit_window(tempo: &TempoMap, clip: &mut Clip) {
    if !is_window(clip) {
        return;
    }
    let (start, length) = (clip.start, clip.length);
    let ClipSource::Audio(data) = &mut clip.source else {
        return;
    };
    if data.sample_rate == 0 {
        return;
    }
    let frames = file_frames_over(tempo, data, start, start + length).max(0);
    if data.reverse {
        data.source_start = (data.source_end - frames).max(0).min(data.source_end);
    } else {
        let mut end = data.source_start + frames;
        if data.file_frames > 0 {
            end = end.min(data.file_frames);
        }
        data.source_end = end.max(data.source_start);
    }
}

/// Brings an audio clip written by an older build up to the rules above:
/// tells it how long its file is, when it did not know, and fits a window
/// clip's trim to its block.
///
/// Called on the clips of a project as it is opened. A clip grown back out
/// past a cut used to play the rest of its take and draw none of it; healed,
/// it draws what it plays.
pub fn heal(tempo: &TempoMap, clip: &mut Clip, file_frames: Option<Sample>) {
    if let (ClipSource::Audio(data), Some(frames)) = (&mut clip.source, file_frames)
        && data.file_frames <= 0
        && frames > 0
    {
        data.file_frames = frames;
    }
    fit_window(tempo, clip);
}

/// Where a clip's front may be dragged to, as close as it can get to
/// `wanted`: never before the song, never so far right that the block is
/// shorter than `shortest`, and never before the first frame of the take it
/// would uncover.
pub fn clamp_front(tempo: &TempoMap, clip: &Clip, wanted: Tick, shortest: Tick) -> Tick {
    let end = clip.start + clip.length;
    let latest = (end - shortest).max(clip.start);
    let mut front = wanted.max(0).min(latest);
    if front >= clip.start {
        return front;
    }
    let ClipSource::Audio(data) = &clip.source else {
        return front;
    };
    // How far left the take goes, in the units the edge moves through.
    if repeats(clip) {
        // A loop on the arrangement has a pass before every pass; one coming
        // round in the file has only the phase it has already come round.
        if clip.loop_length.is_some_and(|p| p > 0) {
            return front;
        }
        return front.max(clip.start - data.loop_phase.max(0));
    }
    let before: Sample = if data.reverse {
        if data.file_frames > 0 {
            (data.file_frames - data.source_end).max(0)
        } else {
            0
        }
    } else {
        data.source_start.max(0)
    };
    let fits = |at: Tick| -> bool {
        let needed = match data.stretch {
            ClipStretch::Off => -file_frames_over(tempo, data, clip.start, at),
            ClipStretch::Resample => proportional_frames(clip, data, clip.start - at),
        };
        needed <= before
    };
    if fits(front) {
        return front;
    }
    // The furthest left that still fits. Monotonic, so a bisection: a few
    // dozen steps at most, on a press.
    let (mut lo, mut hi) = (front, clip.start);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if fits(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    front = hi;
    front
}

/// A stretched clip's file frames under `ticks` of its block: its trim spread
/// evenly over it, which is what stretching is.
pub fn proportional_frames(clip: &Clip, data: &AudioClipData, ticks: Tick) -> Sample {
    if clip.length <= 0 {
        return 0;
    }
    (ticks as f64 / clip.length as f64 * data.source_frames() as f64).round() as Sample
}
