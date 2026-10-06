//! Monophonic pitch: offline pYIN (Mauch & Dixon 2014) on `fontelle-dsp`'s
//! `yin_cmndf`, and the F0 track cut into notes, each split into its centre,
//! its slow drift and its vibrato (plan §2.3).
//!
//! # How it differs from the paper
//!
//! - **Candidates** are pYIN's: YIN's "first dip under the threshold" asked
//!   at a hundred thresholds weighted by a Beta(2, 18) prior, each answer
//!   carrying its threshold's weight. Found at 8 kHz (the difference function
//!   costs the square of the longest lag), then refined at 22 050 Hz with a
//!   parabola through the plain difference, so a period is known to well
//!   under a cent.
//! - **Voicing** is its own two-state HMM over the candidates' total mass
//!   (switching costs 1 %), not pYIN's mirrored unvoiced pitch states. With
//!   those, a frame is voiced whenever its one candidate outweighs the
//!   unvoiced mass spread over every bin, which is nearly always; pYIN leans
//!   on noise's candidates jumping about to undo that. Here noise is
//!   unvoiced because its dips are shallow, and digital silence because it
//!   is under the gate. No candidate without a dip under some threshold
//!   (pYIN's 1 % "absolute minimum" fallback is left out for the same reason).
//! - **Pitch** is a Viterbi path through 20-cent bins inside each voiced
//!   run, moving at most 12 bins (240 cents) a frame.

use crate::resample::resample_mono;
use fontelle_dsp::{hz_to_cents, yin_cmndf};

/// The rate candidates are found at: eight samples across a period of the
/// highest note (the tracker's own rule) at 1 kHz.
const COARSE_RATE: u32 = 8_000;
/// The rate they are refined at.
const FINE_RATE: u32 = 22_050;
/// The shortest window the difference function integrates over, in coarse
/// samples (the tracker's).
const MIN_WINDOW: usize = 24;
/// Thresholds, 0.01 to 1.00.
const N_THRESHOLDS: usize = 100;
/// Pitch bins.
const BIN_CENTS: f32 = 20.0;
/// The furthest the pitch path moves in one frame, in bins.
const MAX_STEP_BINS: usize = 12;
/// The chance of switching between voiced and unvoiced from one frame to
/// the next.
const SWITCH: f64 = 0.01;
/// Under this level (RMS) a frame has no candidates: digital silence and
/// room tone do not get a pitch.
const GATE_RMS: f32 = 1e-3;
/// Where in time the coarse window's newest sample sits past the frame's
/// centre, in coarse samples: a lag of `τ` reads `3τ` samples back from it,
/// so this centres a 200 Hz note's read on the frame.
const LOOKAHEAD: usize = 60;

/// What [`pyin`] looks for.
#[derive(Debug, Clone, PartialEq)]
pub struct PyinParams {
    /// The pitch range, in hertz.
    pub f_min: f32,
    pub f_max: f32,
    /// Seconds between frames.
    pub hop: f64,
}

impl Default for PyinParams {
    fn default() -> Self {
        Self {
            f_min: 60.0,
            f_max: 1_100.0,
            hop: 0.005,
        }
    }
}

/// A fundamental-frequency track: one frame every `hop` seconds, frame `i`
/// centred at `i * hop`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct F0Track {
    pub hop: f64,
    /// Hertz; 0 where unvoiced.
    pub hz: Vec<f32>,
    /// How likely each frame is voiced at all, 0..1 (the candidates' mass).
    pub voicing: Vec<f32>,
    /// The frame's level, RMS.
    pub rms: Vec<f32>,
}

impl F0Track {
    pub fn len(&self) -> usize {
        self.hz.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hz.is_empty()
    }

    /// Where frame `i` sits, in seconds.
    pub fn time(&self, i: usize) -> f64 {
        i as f64 * self.hop
    }

    /// Frame `i`'s pitch in MIDI cents (6900 = A4), if voiced.
    pub fn cents(&self, i: usize) -> Option<f32> {
        self.hz
            .get(i)
            .filter(|hz| **hz > 0.0)
            .map(|hz| hz_to_cents(*hz))
    }
}

/// The Beta(2, 18) prior over the thresholds, normalised: mean 0.1.
fn threshold_prior() -> [f32; N_THRESHOLDS] {
    let mut prior = [0.0f32; N_THRESHOLDS];
    let mut sum = 0.0f64;
    let raw: Vec<f64> = (1..=N_THRESHOLDS)
        .map(|j| {
            let s = j as f64 / N_THRESHOLDS as f64;
            let w = s * (1.0 - s).powi(17);
            sum += w;
            w
        })
        .collect();
    for (p, w) in prior.iter_mut().zip(raw) {
        *p = (w / sum) as f32;
    }
    prior
}

/// A pitch candidate: hertz and the prior mass that chose it.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    hz: f32,
    probability: f32,
}

/// `signal[at]`, zero outside it.
fn sample(signal: &[f32], at: isize) -> f32 {
    if at < 0 {
        0.0
    } else {
        signal.get(at as usize).copied().unwrap_or(0.0)
    }
}

/// The period near `coarse_period * ratio` at the fine rate: the plain
/// difference at seven lags round it, through a parabola.
fn refine(fine: &[f32], centre: isize, around: f32) -> f32 {
    let p = around.round() as isize;
    let w = (2 * p).max(220);
    let start = centre - (w + p) / 2;
    let difference = |lag: isize| -> f32 {
        let mut sum = 0.0f32;
        for n in 0..w {
            let d = sample(fine, start + n) - sample(fine, start + n + lag);
            sum += d * d;
        }
        sum
    };
    let low = (p - 3).max(2);
    let high = p + 3;
    let (mut best, mut best_d) = (low, f32::MAX);
    for lag in low..=high {
        let d = difference(lag);
        if d < best_d {
            best = lag;
            best_d = d;
        }
    }
    if best > low && best < high {
        let dm = difference(best - 1);
        let dp = difference(best + 1);
        let denominator = dm - 2.0 * best_d + dp;
        if denominator.abs() > 1e-12 {
            return best as f32 + 0.5 * (dm - dp) / denominator;
        }
    }
    best as f32
}

/// The F0 track of mono audio at any rate.
pub fn pyin(audio: &[f32], sample_rate: u32, params: &PyinParams) -> F0Track {
    let hop = params.hop.max(1e-4);
    let seconds = audio.len() as f64 / f64::from(sample_rate.max(1));
    let n_frames = (seconds / hop).floor() as usize + usize::from(!audio.is_empty());
    let coarse = resample_mono(audio, sample_rate, COARSE_RATE);
    let fine = resample_mono(audio, sample_rate, FINE_RATE);

    let f_max = params.f_max.min(COARSE_RATE as f32 / 4.0);
    let f_min = params.f_min.clamp(20.0, f_max / 2.0);
    let tau_min = ((COARSE_RATE as f32 / f_max).floor() as usize).max(2);
    let tau_max = (COARSE_RATE as f32 / f_min).ceil() as usize;
    let max_window = 2 * tau_max;
    let span = max_window + tau_max;
    let prior = threshold_prior();
    let ratio = FINE_RATE as f32 / COARSE_RATE as f32;
    let rms_half = (0.015 * f64::from(FINE_RATE)) as isize;

    let mut window = vec![0.0f32; span];
    let mut d = vec![1.0f32; tau_max + 1];
    let mut candidates: Vec<Vec<Candidate>> = Vec::with_capacity(n_frames);
    let mut rms = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let centre_fine = (i as f64 * hop * f64::from(FINE_RATE)).round() as isize;
        let energy: f32 = (centre_fine - rms_half..centre_fine + rms_half)
            .map(|at| sample(&fine, at).powi(2))
            .sum();
        let level = (energy / (2 * rms_half) as f32).sqrt();
        rms.push(level);
        if level < GATE_RMS {
            candidates.push(Vec::new());
            continue;
        }

        let end = (i as f64 * hop * f64::from(COARSE_RATE)).round() as isize + LOOKAHEAD as isize;
        for (k, slot) in window.iter_mut().enumerate() {
            *slot = sample(&coarse, end - span as isize + k as isize);
        }
        yin_cmndf(&window, tau_max, MIN_WINDOW, max_window, &mut d);

        // Each threshold's answer: the first lag under it, walked down to its
        // own minimum.
        let mut chosen: Vec<(usize, f32)> = Vec::new();
        let mut tau = tau_min;
        for (j, weight) in prior.iter().enumerate().rev() {
            let threshold = (j + 1) as f32 / N_THRESHOLDS as f32;
            // Highest threshold first: as it falls, the first lag under it
            // can only move later, never back.
            while tau <= tau_max && d[tau] >= threshold {
                tau += 1;
            }
            if tau > tau_max {
                continue;
            }
            let mut at = tau;
            while at < tau_max && d[at + 1] < d[at] {
                at += 1;
            }
            match chosen.iter_mut().find(|(t, _)| *t == at) {
                Some((_, p)) => *p += weight,
                None => chosen.push((at, *weight)),
            }
        }

        let frame: Vec<Candidate> = chosen
            .into_iter()
            .filter_map(|(tau, probability)| {
                // A parabola through the coarse minimum first, then the
                // fine-rate refine round it.
                let coarse_period = if tau > tau_min && tau < tau_max {
                    let (dm, d0, dp) = (d[tau - 1], d[tau], d[tau + 1]);
                    let denominator = dm - 2.0 * d0 + dp;
                    if denominator.abs() > 1e-12 {
                        tau as f32 + (0.5 * (dm - dp) / denominator).clamp(-1.0, 1.0)
                    } else {
                        tau as f32
                    }
                } else {
                    tau as f32
                };
                let period = refine(&fine, centre_fine, coarse_period * ratio);
                let hz = FINE_RATE as f32 / period;
                (hz >= f_min * 0.97 && hz <= f_max * 1.03).then_some(Candidate { hz, probability })
            })
            .collect();
        candidates.push(frame);
    }

    let voicing: Vec<f32> = candidates
        .iter()
        .map(|c| c.iter().map(|c| c.probability).sum::<f32>().min(1.0))
        .collect();
    let voiced = decode_voicing(&voicing);
    let hz = decode_pitch(&candidates, &voiced, f_min, f_max);
    F0Track {
        hop,
        hz,
        voicing,
        rms,
    }
}

/// The two-state voicing HMM: voiced frames see the candidates' mass,
/// unvoiced frames the rest.
fn decode_voicing(voicing: &[f32]) -> Vec<bool> {
    let n = voicing.len();
    if n == 0 {
        return Vec::new();
    }
    let floor = 1e-6f64;
    let emit = |p: f32, voiced: bool| {
        let p = f64::from(p).clamp(0.0, 1.0);
        (if voiced { p } else { 1.0 - p }).max(floor).ln()
    };
    let (stay, switch) = ((1.0 - SWITCH).ln(), SWITCH.ln());
    let mut score = [
        emit(voicing[0], false) + 0.5f64.ln(),
        emit(voicing[0], true) + 0.5f64.ln(),
    ];
    let mut back = vec![[0u8; 2]; n];
    for t in 1..n {
        let mut next = [0.0; 2];
        for (state, slot) in next.iter_mut().enumerate() {
            let from_same = score[state] + stay;
            let from_other = score[1 - state] + switch;
            let (best, from) = if from_same >= from_other {
                (from_same, state)
            } else {
                (from_other, 1 - state)
            };
            back[t][state] = from as u8;
            *slot = best + emit(voicing[t], state == 1);
        }
        score = next;
    }
    let mut state = usize::from(score[1] > score[0]);
    let mut out = vec![false; n];
    for t in (0..n).rev() {
        out[t] = state == 1;
        state = usize::from(back[t][state]);
    }
    out
}

/// The pitch path through each voiced run: Viterbi over 20-cent bins, the
/// candidates' mass as evidence, a triangular step of at most 12 bins.
fn decode_pitch(
    candidates: &[Vec<Candidate>],
    voiced: &[bool],
    f_min: f32,
    f_max: f32,
) -> Vec<f32> {
    let base = hz_to_cents(f_min);
    let n_bins = ((hz_to_cents(f_max) - base) / BIN_CENTS).ceil() as usize + 1;
    let bin_of = |hz: f32| {
        (((hz_to_cents(hz) - base) / BIN_CENTS).round().max(0.0) as usize).min(n_bins - 1)
    };
    let step: Vec<f64> = (0..=MAX_STEP_BINS)
        .map(|d| ((MAX_STEP_BINS + 1 - d) as f64 / (MAX_STEP_BINS + 1) as f64).ln())
        .collect();
    let floor = 1e-4f64;

    let mut hz = vec![0.0f32; candidates.len()];
    let mut t = 0;
    while t < candidates.len() {
        if !voiced[t] {
            t += 1;
            continue;
        }
        let first = t;
        while t < candidates.len() && voiced[t] {
            t += 1;
        }
        let run = first..t;

        // Evidence per frame and bin, in logs.
        let evidence: Vec<Vec<f64>> = run
            .clone()
            .map(|f| {
                let mut e = vec![0.0f64; n_bins];
                for c in &candidates[f] {
                    e[bin_of(c.hz)] += f64::from(c.probability);
                }
                e.into_iter().map(|p| (p + floor).ln()).collect()
            })
            .collect();
        let mut score = evidence[0].clone();
        let mut back = vec![vec![0u32; n_bins]; run.len()];
        for k in 1..run.len() {
            let mut next = vec![f64::NEG_INFINITY; n_bins];
            for (b, slot) in next.iter_mut().enumerate() {
                let lo = b.saturating_sub(MAX_STEP_BINS);
                let hi = (b + MAX_STEP_BINS).min(n_bins - 1);
                let (mut best, mut from) = (f64::NEG_INFINITY, b);
                for prev in lo..=hi {
                    let s = score[prev] + step[prev.abs_diff(b)];
                    if s > best {
                        best = s;
                        from = prev;
                    }
                }
                back[k][b] = from as u32;
                *slot = best + evidence[k][b];
            }
            score = next;
        }
        let mut bin = (0..n_bins)
            .max_by(|a, b| score[*a].total_cmp(&score[*b]))
            .unwrap_or(0);
        for k in (0..run.len()).rev() {
            let f = first + k;
            // The candidate the path chose, refined; or the nearest one a bin
            // away; or, crossing a frame with no evidence, the bin itself.
            let near = candidates[f]
                .iter()
                .filter(|c| bin_of(c.hz).abs_diff(bin) <= 1)
                .max_by(|a, b| a.probability.total_cmp(&b.probability));
            hz[f] = match near {
                Some(c) => c.hz,
                None => fontelle_dsp::cents_to_hz(base + bin as f32 * BIN_CENTS),
            };
            bin = back[k][bin] as usize;
        }
    }
    hz
}

/// One sung note: a run of voiced frames with one pitch centre.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MonoNote {
    /// Its frames in the track, `[first, end)`, and the same in seconds.
    pub first: usize,
    pub end: usize,
    pub start_time: f64,
    pub end_time: f64,
    /// The pitch it is heard at, MIDI cents: the median of its middle 60 %.
    pub centre: f32,
    /// Per frame of the note: the slow movement (under 3 Hz) about the
    /// centre, and the vibrato (3 to 9 Hz), both in cents.
    pub drift: Vec<f32>,
    pub vibrato: Vec<f32>,
}

impl MonoNote {
    /// The nearest key to the centre.
    pub fn midi(&self) -> u8 {
        (self.centre / 100.0).round().clamp(0.0, 127.0) as u8
    }
}

/// Unvoiced gaps up to this long are bridged (a consonant inside a note).
const BRIDGE_SECONDS: f64 = 0.040;
/// A pitch step must hold this long, and be this big, to start a note.
const STEP_SECONDS: f64 = 0.030;
const STEP_CENTS: f32 = 70.0;
/// The smoothing the step test reads through: one period of a slow vibrato.
const STEP_SMOOTHING_SECONDS: f64 = 0.18;
/// A dip in level this deep (as a ratio of the level before it), recovered
/// from within 80 ms, is a note struck again.
const RESTRIKE_DIP: f32 = 0.35;
const RESTRIKE_SECONDS: f64 = 0.080;
/// Shorter than this, a piece of a run is not a note.
const MIN_NOTE_SECONDS: f64 = 0.050;
/// Drift is under this; vibrato between it and the next.
const DRIFT_HZ: f64 = 3.0;
const VIBRATO_HZ: f64 = 9.0;

/// The track cut into notes: at unvoiced gaps over 40 ms, at pitch steps of
/// more than 70 cents held over 30 ms, and where the level dips and is
/// struck again. Each note's centre is the median of its middle 60 %; what
/// moves about it is split at 3 Hz into drift and, up to 9 Hz, vibrato.
pub fn segment(track: &F0Track) -> Vec<MonoNote> {
    let n = track.len();
    let frames_of = |seconds: f64| ((seconds / track.hop).round() as usize).max(1);
    let bridge = frames_of(BRIDGE_SECONDS);

    // Voiced runs, short gaps bridged, the gaps' pitch interpolated.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut t = 0;
    while t < n {
        if track.hz[t] <= 0.0 {
            t += 1;
            continue;
        }
        let first = t;
        let mut last = t;
        while t < n {
            if track.hz[t] > 0.0 {
                last = t;
                t += 1;
            } else if t - last <= bridge {
                t += 1;
            } else {
                break;
            }
        }
        runs.push((first, last + 1));
    }

    let mut notes = Vec::new();
    for (first, end) in runs {
        let contour = filled_contour(track, first, end);
        let mut cuts = vec![0usize];
        cuts.extend(pitch_steps(
            &contour,
            frames_of(STEP_SMOOTHING_SECONDS),
            frames_of(STEP_SECONDS),
        ));
        cuts.extend(restrikes(
            &track.rms[first..end],
            frames_of(RESTRIKE_SECONDS),
        ));
        cuts.push(end - first);
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b - a < frames_of(MIN_NOTE_SECONDS) {
                continue;
            }
            notes.push(note(track, first + a, first + b, &contour[a..b]));
        }
    }
    notes
}

/// The run's pitch in cents, every frame, unvoiced frames bridged by a line
/// between their voiced neighbours.
fn filled_contour(track: &F0Track, first: usize, end: usize) -> Vec<f32> {
    let mut out: Vec<Option<f32>> = (first..end).map(|i| track.cents(i)).collect();
    let mut last: Option<(usize, f32)> = None;
    for k in 0..out.len() {
        if let Some(c) = out[k] {
            if let Some((j, prev)) = last
                && k > j + 1
            {
                for (m, slot) in out.iter_mut().enumerate().take(k).skip(j + 1) {
                    let f = (m - j) as f32 / (k - j) as f32;
                    *slot = Some(prev + (c - prev) * f);
                }
            }
            last = Some((k, c));
        }
    }
    out.into_iter().map(|c| c.unwrap_or(0.0)).collect()
}

/// Where the pitch steps to a new note: the smoothed contour leaves the
/// current note's median by more than 70 cents and stays away 30 ms. The cut
/// is placed where the raw contour crosses halfway between the two.
fn pitch_steps(contour: &[f32], smoothing: usize, hold: usize) -> Vec<usize> {
    let n = contour.len();
    let smooth = moving_average(contour, smoothing);
    let mut cuts = Vec::new();
    let mut start = 0;
    let mut away = 0;
    let mut t = 0;
    while t < n {
        let reference = median(
            &contour[start..=t.max(start)]
                .iter()
                .copied()
                .rev()
                .take(smoothing.max(8) * 2)
                .collect::<Vec<_>>(),
        );
        if (smooth[t] - reference).abs() > STEP_CENTS {
            away += 1;
            if away >= hold {
                let left = t + 1 - away;
                // The new note's level: the median of what follows.
                let ahead = &contour[left..(left + 2 * smoothing).min(n)];
                let target = median(ahead);
                let halfway = 0.5 * (reference + target);
                let window = left.saturating_sub(smoothing)..(left + smoothing).min(n);
                let rising = target > reference;
                let cut = window
                    .clone()
                    .find(|&k| {
                        if rising {
                            contour[k] >= halfway
                        } else {
                            contour[k] <= halfway
                        }
                    })
                    .unwrap_or(left);
                if cut > start {
                    cuts.push(cut);
                    start = cut;
                }
                away = 0;
                t = cut.max(t + 1 - hold) + 1;
                continue;
            }
        } else {
            away = 0;
        }
        t += 1;
    }
    cuts
}

/// Where the level dips and comes back: under `RESTRIKE_DIP` of the
/// loudest of the 100 ms before, and back over twice the dip within
/// `recover` frames. The cut is at the bottom of the dip.
fn restrikes(rms: &[f32], recover: usize) -> Vec<usize> {
    let n = rms.len();
    let look = recover + recover / 4;
    let mut cuts = Vec::new();
    let mut t = 1;
    while t + 1 < n {
        let before = rms[t.saturating_sub(look)..t]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        let is_min = rms[t] <= rms[t - 1] && rms[t] <= rms[t + 1];
        if is_min && rms[t] < RESTRIKE_DIP * before {
            let after = rms[t + 1..(t + 1 + recover).min(n)]
                .iter()
                .copied()
                .fold(0.0f32, f32::max);
            if after > 2.0 * rms[t] && after > 0.5 * before {
                cuts.push(t);
                t += recover;
                continue;
            }
        }
        t += 1;
    }
    cuts
}

fn note(track: &F0Track, first: usize, end: usize, contour: &[f32]) -> MonoNote {
    let n = contour.len();
    let middle = &contour[n / 5..(n - n / 5).max(n / 5 + 1)];
    let centre = median(middle);
    let rate = 1.0 / track.hop;
    let slow = zero_phase_lowpass(contour, DRIFT_HZ, rate);
    let residual: Vec<f32> = contour.iter().zip(&slow).map(|(c, s)| c - s).collect();
    let vibrato = zero_phase_lowpass(&residual, VIBRATO_HZ, rate);
    MonoNote {
        first,
        end,
        start_time: track.time(first),
        end_time: track.time(end - 1) + track.hop,
        centre,
        drift: slow.iter().map(|s| s - centre).collect(),
        vibrato,
    }
}

fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let m = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[m]
    } else {
        0.5 * (sorted[m - 1] + sorted[m])
    }
}

/// A centred moving average over `width` frames, shortened at the ends.
fn moving_average(values: &[f32], width: usize) -> Vec<f32> {
    let half = width / 2;
    let mut prefix = vec![0.0f64; values.len() + 1];
    for (k, v) in values.iter().enumerate() {
        prefix[k + 1] = prefix[k] + f64::from(*v);
    }
    (0..values.len())
        .map(|k| {
            let lo = k.saturating_sub(half);
            let hi = (k + half + 1).min(values.len());
            ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32
        })
        .collect()
}

/// A 4th-order Butterworth low-pass run forwards and backwards (no phase),
/// the ends padded by odd reflection so a straight line passes untouched.
fn zero_phase_lowpass(values: &[f32], cutoff: f64, rate: f64) -> Vec<f32> {
    let n = values.len();
    if n < 3 || cutoff >= rate / 2.0 {
        return values.to_vec();
    }
    let pad = ((3.0 * rate / cutoff) as usize).min(n - 1);
    let (first, last) = (f64::from(values[0]), f64::from(values[n - 1]));
    let mut x: Vec<f64> = Vec::with_capacity(n + 2 * pad);
    x.extend((1..=pad).rev().map(|k| 2.0 * first - f64::from(values[k])));
    x.extend(values.iter().map(|v| f64::from(*v)));
    x.extend((1..=pad).map(|k| 2.0 * last - f64::from(values[n - 1 - k])));
    for q in [0.541_196_100_146_197, 1.306_562_964_876_376_6] {
        let c = Biquad::lowpass(cutoff, q, rate);
        c.run(&mut x);
        x.reverse();
        c.run(&mut x);
        x.reverse();
    }
    x[pad..pad + n].iter().map(|v| *v as f32).collect()
}

/// The RBJ cookbook low-pass, direct form I, started at rest on its first
/// input (so a padded edge does not ring).
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    fn lowpass(cutoff: f64, q: f64, rate: f64) -> Self {
        let w = std::f64::consts::TAU * cutoff / rate;
        let alpha = w.sin() / (2.0 * q);
        let cos = w.cos();
        let a0 = 1.0 + alpha;
        Self {
            b: [
                (1.0 - cos) / 2.0 / a0,
                (1.0 - cos) / a0,
                (1.0 - cos) / 2.0 / a0,
            ],
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
        }
    }

    fn run(&self, x: &mut [f64]) {
        let Some(&x0) = x.first() else { return };
        // Settled on the first value: DC gain is one.
        let (mut x1, mut x2, mut y1, mut y2) = (x0, x0, x0, x0);
        for v in x.iter_mut() {
            let input = *v;
            let y = self.b[0] * input + self.b[1] * x1 + self.b[2] * x2
                - self.a[0] * y1
                - self.a[1] * y2;
            x2 = x1;
            x1 = input;
            y2 = y1;
            y1 = y;
            *v = y;
        }
    }
}
