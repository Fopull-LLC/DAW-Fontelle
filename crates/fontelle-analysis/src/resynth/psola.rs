//! Offline TD-PSOLA with **real pitch marks** (plan §2.4, the "Standard"
//! engine).
//!
//! The realtime shifter (`fontelle_dsp::psola`) places its marks from a
//! running phase, because a live tuner has no look-ahead. Offline there is
//! the whole take, so the marks are where the voice's periods actually are:
//! one at the waveform's peak in each period, the next looked for a period on
//! (the pitch track says how far) within a fifth of a period either way. A
//! mark on a real period is what keeps a moved voice from buzzing — each
//! grain is one cycle of the voice through its own vocal tract, which is
//! also why the formants stay where they were.
//!
//! **Synthesis** walks the output a period at a time, the analysis spacing
//! divided by the ratio there, and lays the grain of the nearest analysis
//! mark at each step: an asymmetric Hann reaching back to the mark before and
//! on to the mark after. The overlap is normalised by the windows' sum, so
//! a ratio of one is the input back (the steps land on the marks themselves),
//! and a move up — grains closer together — is not louder for it.
//!
//! Unvoiced stretches are cut into 5 ms marks and never moved: a consonant
//! has no pitch to move, and this is the same code at a ratio of one.

use super::{Resynth, ResynthError, SpanRequest};

/// The engine. Stateless; one serves every study.
#[derive(Debug, Clone, Copy, Default)]
pub struct Psola;

/// Mark spacing where there is no pitch, in seconds.
const UNVOICED_STEP: f64 = 0.005;
/// How far from where the period says the next peak may be, as a share of
/// the period.
const SEARCH: f64 = 0.2;
/// Under this window sum a sample is not divided by it (a deep move down
/// leaves gaps between grains; dividing by nothing there would be a spike).
const MIN_WINDOW_SUM: f32 = 0.25;

impl Resynth for Psola {
    fn id(&self) -> &'static str {
        "psola"
    }

    fn render_span(
        &self,
        request: &SpanRequest<'_>,
        out: &mut Vec<f32>,
    ) -> Result<(), ResynthError> {
        let channels = request.channels.max(1);
        let frames = request.input.len() / channels;
        let span = request.span.start.min(frames)..request.span.end.min(frames);
        out.clear();
        if span.is_empty() {
            return Ok(());
        }
        let sr = f64::from(request.sample_rate.max(1));
        let track = Track {
            request,
            per_frame: request.f0.hop * sr,
        };
        // Grains reach a period either side, and the marks have to start
        // before the span to cover its first samples: two of the longest
        // period the tracker looks for.
        let reach = (sr / 60.0 * 2.0).ceil() as usize;
        let lo = span.start.saturating_sub(reach);
        let hi = (span.end + reach).min(frames);
        let mono: Vec<f32> = (lo..hi)
            .map(|f| {
                let at = f * channels;
                request.input[at..at + channels].iter().sum::<f32>() / channels as f32
            })
            .collect();
        let marks = place_marks(&mono, lo, &track, sr);
        if marks.len() < 2 {
            out.extend_from_slice(&request.input[span.start * channels..span.end * channels]);
            return Ok(());
        }

        let formant = 2f64.powf(f64::from(request.formant_cents) / 1200.0);
        let len = hi - lo;
        let mut acc = vec![0.0f32; len * channels];
        let mut sum = vec![0.0f32; len];
        let mut ts = marks[0].at as f64;
        let mut k = 0usize;
        while ts < hi as f64 {
            // The analysis mark nearest the synthesis point: no stretch, so
            // the output keeps the input's timing.
            while k + 1 < marks.len()
                && (marks[k + 1].at as f64 - ts).abs() <= (marks[k].at as f64 - ts).abs()
            {
                k += 1;
            }
            let mark = marks[k];
            let back = if k > 0 {
                mark.at - marks[k - 1].at
            } else {
                marks[1].at - marks[0].at
            };
            let ahead = if k + 1 < marks.len() {
                marks[k + 1].at - mark.at
            } else {
                back
            };
            let ratio = if mark.voiced {
                f64::from(track.ratio_at(ts)).clamp(0.25, 4.0)
            } else {
                1.0
            };
            let place = ts.floor();
            let frac = ts - place;
            let place = place as isize;
            for i in -(back as isize)..(ahead as isize) {
                let w = if i < 0 {
                    0.5 + 0.5 * (std::f64::consts::PI * i as f64 / back as f64).cos()
                } else {
                    0.5 + 0.5 * (std::f64::consts::PI * i as f64 / ahead as f64).cos()
                } as f32;
                let to = place + i;
                if to < lo as isize || to >= hi as isize {
                    continue;
                }
                // The grain's sample under output `to`: offset from the mark
                // by where `to` sits from the (fractional) synthesis point,
                // read wider or narrower for a formant shift.
                let from = mark.at as f64 + (i as f64 - frac) * formant;
                let slot = (to as usize) - lo;
                sum[slot] += w;
                for c in 0..channels {
                    acc[slot * channels + c] += w * read(request.input, channels, frames, from, c);
                }
            }
            ts += ahead as f64 / ratio;
        }

        out.reserve(span.len() * channels);
        for f in span.clone() {
            let slot = f - lo;
            let s = sum[slot];
            for c in 0..channels {
                out.push(if s > 0.0 {
                    acc[slot * channels + c] / s.max(MIN_WINDOW_SUM)
                } else {
                    request.input[f * channels + c]
                });
            }
        }
        Ok(())
    }
}

/// Channel `c` at fractional frame `at`, by a line between its neighbours;
/// silence outside the audio.
fn read(input: &[f32], channels: usize, frames: usize, at: f64, c: usize) -> f32 {
    if at < 0.0 {
        return 0.0;
    }
    let i = at.floor() as usize;
    if i >= frames {
        return 0.0;
    }
    let a = input[i * channels + c];
    let frac = (at - i as f64) as f32;
    if frac == 0.0 || i + 1 >= frames {
        return a;
    }
    a + (input[(i + 1) * channels + c] - a) * frac
}

/// The request's pitch track and ratios, read at input frames.
struct Track<'a> {
    request: &'a SpanRequest<'a>,
    /// Input frames a track frame.
    per_frame: f64,
}

impl Track<'_> {
    fn index(&self, frame: f64) -> f64 {
        (frame - self.request.track_start as f64) / self.per_frame.max(1e-9)
    }

    /// Hertz at an input frame; 0 where the nearest track frame is unvoiced.
    fn hz_at(&self, frame: f64) -> f64 {
        let hz = &self.request.f0.hz;
        if hz.is_empty() {
            return 0.0;
        }
        let x = self.index(frame).clamp(0.0, (hz.len() - 1) as f64);
        let i = x.floor() as usize;
        let j = (i + 1).min(hz.len() - 1);
        let f = x - i as f64;
        let (a, b) = (f64::from(hz[i]), f64::from(hz[j]));
        match (a > 0.0, b > 0.0) {
            (true, true) => a + (b - a) * f,
            (true, false) if f < 0.5 => a,
            (false, true) if f >= 0.5 => b,
            _ => 0.0,
        }
    }

    fn ratio_at(&self, frame: f64) -> f32 {
        let ratio = self.request.ratio;
        if ratio.is_empty() {
            return 1.0;
        }
        let x = self.index(frame).clamp(0.0, (ratio.len() - 1) as f64);
        let i = x.floor() as usize;
        let j = (i + 1).min(ratio.len() - 1);
        let f = (x - i as f64) as f32;
        ratio[i] + (ratio[j] - ratio[i]) * f
    }
}

#[derive(Debug, Clone, Copy)]
struct Mark {
    /// Input frame.
    at: usize,
    voiced: bool,
}

/// One mark a period through `mono` (which starts at input frame `lo`): at
/// the waveform's peak, each looked for a period after the last, within a
/// fifth of a period; 5 ms apart where nothing is voiced.
fn place_marks(mono: &[f32], lo: usize, track: &Track<'_>, sr: f64) -> Vec<Mark> {
    let n = mono.len();
    let unvoiced = (UNVOICED_STEP * sr).round().max(1.0) as usize;
    let peak_in = |from: usize, to: usize| -> usize {
        let (from, to) = (from.min(n - 1), to.clamp(from + 1, n));
        (from..to)
            .max_by(|a, b| mono[*a].total_cmp(&mono[*b]))
            .unwrap_or(from)
    };
    let mut marks: Vec<Mark> = Vec::new();
    let mut pos = 0usize;
    while pos < n {
        let hz = track.hz_at((lo + pos) as f64);
        if hz > 0.0 {
            let period = sr / hz;
            let at = match marks.last() {
                // Continuing a voiced run: a period on, within the search.
                Some(last) if last.voiced => {
                    let expected = (last.at - lo) as f64 + period;
                    let from =
                        (expected - SEARCH * period).max((last.at - lo) as f64 + 0.5 * period);
                    let to = expected + SEARCH * period;
                    if from as usize >= n {
                        break;
                    }
                    peak_in(from.round() as usize, to.round() as usize + 1)
                }
                // Starting one: the peak of the first period.
                _ => peak_in(pos, pos + period.round() as usize),
            };
            if marks.last().is_some_and(|m| m.at >= lo + at) {
                pos += 1;
                continue;
            }
            marks.push(Mark {
                at: lo + at,
                voiced: true,
            });
            pos = at + (period * (1.0 - SEARCH)).max(1.0) as usize;
        } else {
            if marks.last().is_some_and(|m| m.at >= lo + pos) {
                pos += 1;
                continue;
            }
            marks.push(Mark {
                at: lo + pos,
                voiced: false,
            });
            pos += unvoiced;
        }
    }
    marks
}
