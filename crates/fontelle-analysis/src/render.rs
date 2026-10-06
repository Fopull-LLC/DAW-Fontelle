//! A study's pitch edits, rendered (`docs/analyze-musically-plan.md` §2.4,
//! §3.7, §3.10).
//!
//! The two rules that make an edit sound natural, whatever the engine:
//!
//! 1. **Only edited spans are resynthesised.** Each note is moved with
//!    30 ms of padding either side and spliced back over 10 ms, at the
//!    quietest point of the padding (between two of the voice's pulses).
//!    Everything else is the source, bit for bit — "fix one note in a sung
//!    take" means one note.
//! 2. **The contour is edited, not a ratio.** The pitch of an edited note is
//!    `f0 · 2^(e(t)·(shift − flatten·drift(t) + (vibrato − 1)·vib(t))/1200)`,
//!    `e(t)` easing in over the glide in and out over the glide out. The
//!    drift and vibrato are the note's own, read from the audio here exactly
//!    as the analysis reads them (`mono::note_over`), so the singer's scoops
//!    and vibrato survive a move.
//!
//! **The splice is a constant-gain crossfade** (raised cosine), not an
//! equal-power one: at the seam the move has eased back to nothing, so the
//! two sides are the same signal give or take a rounding, and an
//! equal-power fade of two coherent signals is a 3 dB swell.
//!
//! Pure and offline: the caller's thread, never the RT thread.

use std::ops::Range;

use fontelle_types::PitchEdit;

use crate::mono::{F0Track, PyinParams, note_over, pyin};
use crate::resynth::{Resynth, SpanRequest};

/// Padding each side of an edited note that is resynthesised with it.
const PAD_SECONDS: f64 = 0.030;
/// The splice's length.
const FADE_SECONDS: f64 = 0.010;
/// Audio either side of the padding the pitch tracker reads, so its first
/// and last frames are not guesses.
const CONTEXT_SECONDS: f64 = 0.060;

/// One edit of a group moved together: its span inside the audio, and it.
type Member<'a> = (Range<usize>, &'a PitchEdit);

/// What a render made.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// The whole audio, interleaved as it came in, the same length.
    pub audio: Vec<f32>,
    /// The frames that are not the source's own, in order.
    pub spans: Vec<Range<usize>>,
    /// Where each splice is centred, in frames: two per span (fewer where a
    /// span meets the audio's ends).
    pub seams: Vec<usize>,
}

/// `audio` (interleaved, `channels` wide, at `sample_rate`) with `edits`
/// applied through `engine`. Edit spans are in frames of `audio`, and may
/// hang off either end. Edits that change nothing are skipped; edits whose
/// padded spans meet are moved together.
pub fn render_edits(
    audio: &[f32],
    channels: usize,
    sample_rate: u32,
    edits: &[PitchEdit],
    engine: &dyn Resynth,
) -> Rendered {
    let channels = channels.max(1);
    let frames = audio.len() / channels;
    let sr = f64::from(sample_rate.max(1));
    let secs = |s: f64| (s * sr).round() as usize;
    let (pad, fade, context) = (secs(PAD_SECONDS), secs(FADE_SECONDS), secs(CONTEXT_SECONDS));

    // The edits that do something, inside the audio, in order.
    let mut live: Vec<(Range<usize>, &PitchEdit)> = edits
        .iter()
        .filter(|e| !e.is_identity())
        .filter_map(|e| {
            let a = e.span.0.clamp(0, frames as i64) as usize;
            let b = e.span.1.clamp(0, frames as i64) as usize;
            (b > a).then_some((a..b, e))
        })
        .collect();
    live.sort_by_key(|(span, _)| span.start);

    // Grouped where the padded spans meet: one resynthesis each.
    let mut groups: Vec<(Range<usize>, Vec<Member<'_>>)> = Vec::new();
    for (span, edit) in live {
        let region = span.start.saturating_sub(pad)..(span.end + pad).min(frames);
        match groups.last_mut() {
            Some((r, members)) if region.start <= r.end => {
                r.end = r.end.max(region.end);
                members.push((span, edit));
            }
            _ => groups.push((region, vec![(span, edit)])),
        }
    }

    let mut out = audio.to_vec();
    let mut spans = Vec::new();
    let mut seams = Vec::new();
    let mut moved = Vec::new();
    for (region, members) in groups {
        let lo = region.start.saturating_sub(context);
        let hi = (region.end + context).min(frames);
        let mono: Vec<f32> = (lo..hi)
            .map(|f| audio[f * channels..(f + 1) * channels].iter().sum::<f32>() / channels as f32)
            .collect();
        let track = pyin(&mono, sample_rate, &PyinParams::default());
        let ratio = edited_ratio(&track, lo, sr, &members);
        let formant = members
            .iter()
            .map(|(_, e)| e.formant_cents)
            .find(|c| c.abs() > 0.05)
            .unwrap_or(0.0);
        let request = SpanRequest {
            input: audio,
            channels,
            sample_rate,
            f0: &track,
            track_start: lo,
            ratio: &ratio,
            formant_cents: formant,
            span: region.clone(),
        };
        if engine.render_span(&request, &mut moved).is_err()
            || moved.len() != region.len() * channels
        {
            continue;
        }

        // The seams: the quietest point of each padding, where the move
        // has eased back to nothing.
        let first = members.first().map_or(region.start, |(s, _)| s.start);
        let last = members.last().map_or(region.end, |(s, _)| s.end);
        let left = (region.start > 0).then(|| {
            quietest(
                &mono,
                lo,
                region.start + fade / 2,
                first.saturating_sub(fade / 2),
            )
        });
        let right = (region.end < frames).then(|| {
            quietest(
                &mono,
                lo,
                last + fade / 2,
                region.end.saturating_sub(fade / 2),
            )
        });
        let from = left.map_or(region.start, |s| s.saturating_sub(fade / 2));
        let to = right.map_or(region.end, |s| (s + fade / 2).min(frames));
        for f in from..to {
            // 0 is the source, 1 the moved audio.
            let w = match (left, right) {
                (Some(s), _) if f < s + fade / 2 => ramp(f, s, fade),
                (_, Some(s)) if f + fade / 2 >= s => 1.0 - ramp(f, s, fade),
                _ => 1.0,
            };
            for c in 0..channels {
                let at = f * channels + c;
                let m = moved[(f - region.start) * channels + c];
                out[at] = if w >= 1.0 {
                    m
                } else {
                    audio[at] + (m - audio[at]) * w
                };
            }
        }
        seams.extend(left);
        seams.extend(right);
        spans.push(from..to);
    }
    // Each note's own gain (the Pitch card's GAIN), over its span, eased in
    // and out over the splice's fade so a level change never clicks.
    for edit in edits.iter().filter(|e| e.gain_db.abs() >= 1e-3) {
        let a = edit.span.0.clamp(0, frames as i64) as usize;
        let b = edit.span.1.clamp(0, frames as i64) as usize;
        if b <= a {
            continue;
        }
        let g = 10f32.powf(edit.gain_db / 20.0);
        let half = fade / 2;
        let (from, to) = (a.saturating_sub(half), (b + half).min(frames));
        for f in from..to {
            let w = ramp(f, a, fade).min(1.0 - ramp(f, b, fade));
            let gain = 1.0 + (g - 1.0) * w;
            for c in 0..channels {
                out[f * channels + c] *= gain;
            }
        }
        spans.push(from..to);
    }
    Rendered {
        audio: out,
        spans,
        seams,
    }
}

/// A raised-cosine rise from 0 to 1 across `fade` frames centred on `seam`.
fn ramp(f: usize, seam: usize, fade: usize) -> f32 {
    let x = (f as f64 - (seam as f64 - fade as f64 / 2.0)) / fade.max(1) as f64;
    (0.5 - 0.5 * (std::f64::consts::PI * x.clamp(0.0, 1.0)).cos()) as f32
}

/// The quietest frame between `from` and `to` (inclusive), by the energy of
/// the millisecond around it: the gap between two of the voice's pulses.
/// `mono` starts at frame `lo`. Where the range is empty, its middle.
fn quietest(mono: &[f32], lo: usize, from: usize, to: usize) -> usize {
    if to <= from {
        return (from + to) / 2;
    }
    let half = 22usize;
    let energy = |f: usize| -> f32 {
        let at = f.saturating_sub(lo);
        let a = at.saturating_sub(half);
        let b = (at + half + 1).min(mono.len());
        mono[a.min(b)..b].iter().map(|v| v * v).sum()
    };
    (from..=to)
        .min_by(|a, b| energy(*a).total_cmp(&energy(*b)))
        .unwrap_or(from)
}

/// The factor each of the track's frames is moved by: 1 outside every
/// member's span, and inside it the note's contour edited (the module
/// doc's formula), eased by its glides.
fn edited_ratio(track: &F0Track, track_start: usize, sr: f64, members: &[Member<'_>]) -> Vec<f32> {
    let per_frame = track.hop * sr;
    let frame_of = |input: usize| ((input as f64 - track_start as f64) / per_frame).round();
    let mut cents = vec![0.0f32; track.len()];
    for (span, edit) in members {
        let first = frame_of(span.start).max(0.0) as usize;
        let end = (frame_of(span.end).max(0.0) as usize).min(track.len());
        if end <= first {
            continue;
        }
        let note = note_over(track, first, end);
        let length = (span.end - span.start) as f64 / sr * 1000.0;
        let (mut g_in, mut g_out) = (
            f64::from(edit.glide_in_ms.max(0.0)),
            f64::from(edit.glide_out_ms.max(0.0)),
        );
        if g_in + g_out > length {
            let scale = length / (g_in + g_out);
            g_in *= scale;
            g_out *= scale;
        }
        for (k, slot) in cents.iter_mut().enumerate().take(end).skip(first) {
            let ms = (track_start as f64 + k as f64 * per_frame - span.start as f64) / sr * 1000.0;
            let to_end = length - ms;
            let rise = if g_in > 0.0 {
                0.5 - 0.5 * (std::f64::consts::PI * (ms / g_in).clamp(0.0, 1.0)).cos()
            } else {
                1.0
            };
            let fall = if g_out > 0.0 {
                0.5 - 0.5 * (std::f64::consts::PI * (to_end / g_out).clamp(0.0, 1.0)).cos()
            } else {
                1.0
            };
            let ease = rise.min(fall) as f32;
            let (drift, vib) = note
                .as_ref()
                .map(|n| {
                    let j = k - n.first;
                    (
                        n.drift.get(j).copied().unwrap_or(0.0),
                        n.vibrato.get(j).copied().unwrap_or(0.0),
                    )
                })
                .unwrap_or((0.0, 0.0));
            *slot = ease
                * (edit.shift_cents - edit.flatten.clamp(0.0, 1.0) * drift
                    + (edit.vibrato.max(0.0) - 1.0) * vib);
        }
    }
    cents.into_iter().map(|c| 2f32.powf(c / 1200.0)).collect()
}
