//! Finding the note in a monophonic signal (`docs/tune-plan.md` §3.2).
//!
//! YIN with the cumulative-mean-normalised difference (de Cheveigné &
//! Kawahara, 2002), in **two passes**. The plain difference function is
//! `O(W·τ)`, and at a 64-sample hop over an alto's range that is a tenth of a
//! core; a coarse pass at a quarter of the sample rate costs a sixteenth of
//! that, and a thirteen-lag refine at full rate buys the accuracy back.
//!
//! # What it is not
//!
//! Not polyphonic, and it cannot be: a chord has no single period, and asking
//! this for one gets an answer about whichever note happens to win. Not a
//! look-ahead: it only ever reads *backwards*, so it costs no latency at all —
//! only reaction time, which is one hop plus the median's one.
//!
//! # RT
//!
//! Everything is sized in [`prepare`](PitchTracker::prepare). `push` allocates
//! nothing (INVARIANT 1).

// The difference function walks a lag and a window index over the same rings
// and the same scratch. Clippy's `needless_range_loop` wants each of those as
// an iterator chain, which for two indices over a wrapped ring is less
// readable than the loop it replaces.
#![allow(clippy::needless_range_loop)]

use crate::{SvfCoeffs, SvfFilter, SvfMode};

/// What the tracker knows about one hop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchFrame {
    /// The fundamental, in hertz.
    pub hz: f32,
    /// The same, in MIDI cents: `6900 + 1200·log₂(hz/440)`. Carried rather
    /// than recomputed, because everything above this works in cents and a
    /// second `log2` per hop is a second place to get the reference wrong.
    pub cents: f32,
    /// How periodic the window was, 0..=1. One is a perfect period.
    pub confidence: f32,
    /// The window's level, 0..=1.
    pub rms: f32,
}

/// The default confidence threshold, on the **normalised difference**: the
/// middle of the `tracking` knob (§4.1).
pub const DEFAULT_TRACKING_THRESHOLD: f32 = 0.19;

/// The ends the `tracking` knob maps onto. Relaxed follows a breathy or
/// distorted source and mis-tracks noise into notes now and then; strict
/// corrects only what it is sure of.
pub const RELAXED_TRACKING_THRESHOLD: f32 = 0.30;
pub const STRICT_TRACKING_THRESHOLD: f32 = 0.08;

/// How far a candidate may be from double or half the last voiced pitch and
/// still be treated as the same note misheard, in cents.
const OCTAVE_GUARD_CENTS: f32 = 30.0;

/// How many hops of disagreement it takes to overrule the guard. Two, because
/// one is a consonant and two is a new note.
const GUARD_PATIENCE: u32 = 2;

/// How far either side of the coarse answer the refine looks, beyond the
/// decimation factor itself. A coarse lag is worth `D` full-rate ones and the
/// decimation's own rounding is up to one of those, so `D + 2` covers it
/// whatever `D` turns out to be.
const REFINE_MARGIN: i32 = 2;

/// The ends of the decimation factor. Two, because there is no point
/// decimating less; sixteen, because past it the coarse pass is looking at a
/// signal with one harmonic in it.
const MIN_DECIMATION: usize = 2;
const MAX_DECIMATION: usize = 16;

/// How many decimated samples the shortest period must still span for the
/// coarse search to place it within one lag. Eight, not the four the geometry
/// alone would allow: at four the low-pass in front of the decimator has taken
/// so many harmonics off a vowel that the search starts answering with its
/// subharmonic, which is an octave error the refine cannot undo.
const SAMPLES_PER_SHORTEST_PERIOD: f32 = 8.0;

/// The shortest integration window the coarse pass will use, in decimated
/// samples. Two periods of a very high note is a handful of samples, and a
/// correlation over a handful of samples is noise.
const MIN_COARSE_WINDOW: usize = 24;

/// A YIN pitch tracker over a fixed frequency range.
pub struct PitchTracker {
    f_min: f32,
    f_max: f32,
    hop: u32,
    sample_rate: f32,

    /// The full-rate search bounds, in samples.
    p_min: usize,
    p_max: usize,
    /// How many samples are thrown away for each one the coarse pass keeps.
    ///
    /// **Chosen from the range**, not fixed at the four §3.2 names. The coarse
    /// search costs the square of its longest lag, and the low range's lags
    /// are six times the alto's — at a fixed four that one setting costs a
    /// quarter of a core, which is fifteen times its whole budget (§10). What
    /// the search really needs is four samples across the *shortest* period it
    /// is looking for, and everything under that is free.
    decimation: usize,
    /// The search bounds at the decimated rate, which is where it happens.
    p_min4: usize,
    p_max4: usize,
    /// The widest coarse window, `2·P_max/D`.
    w4: usize,

    /// The input as it arrived, for the refine.
    ///
    /// **Mirrored**: every sample is stored twice, at `w` and at `w + N`, so
    /// the most recent `M ≤ N` samples are always one contiguous slice and the
    /// difference function's inner loop is two array reads rather than two
    /// divisions. That is not a micro-optimisation — a modulo per access made
    /// the corrector cost a tenth of a core at its default settings, which is
    /// six times its whole budget (§10).
    full: Vec<f32>,
    full_write: usize,
    full_count: u64,

    /// The same low-passed and decimated by four, for the coarse pass. Also
    /// mirrored — this is the buffer the cost is in.
    deci: Vec<f32>,
    deci_write: usize,
    deci_count: u64,
    /// Where in the four-sample decimation cycle the next sample falls.
    deci_phase: usize,

    /// The anti-alias filter: two cascaded SVF low-passes at the Butterworth
    /// Q pair, which together are a 4th-order response at `fs/10`.
    lp: [SvfFilter; 2],
    lp_coeffs: [SvfCoeffs; 2],

    /// How many samples each mirrored ring really holds.
    full_len: usize,
    deci_len: usize,

    /// Scratch for the coarse difference function, `p_max4 + 1` long.
    d4: Vec<f32>,

    since_hop: u32,
    threshold: f32,
    gate_db: f32,

    /// The last three voiced periods, for the median.
    history: [f32; 3],
    history_len: usize,
    /// The last voiced period and how sure the tracker was of it — what the
    /// octave guard argues from.
    previous: Option<(f32, f32)>,
    /// How many hops in a row have disagreed with the guard.
    disagreements: u32,
}

impl PitchTracker {
    /// A tracker for notes between `f_min` and `f_max`, reporting every `hop`
    /// samples.
    pub fn new(f_min: f32, f_max: f32, hop: u32) -> Self {
        Self {
            f_min: f_min.max(1.0),
            f_max: f_max.max(f_min * 2.0),
            hop: hop.max(1),
            sample_rate: 48_000.0,
            p_min: 2,
            p_max: 2,
            decimation: 4,
            p_min4: 2,
            p_max4: 2,
            w4: 4,
            full: Vec::new(),
            full_write: 0,
            full_count: 0,
            deci: Vec::new(),
            full_len: 0,
            deci_len: 0,
            deci_write: 0,
            deci_count: 0,
            deci_phase: 0,
            lp: Default::default(),
            lp_coeffs: [SvfFilter::coeffs(SvfMode::Lowpass, 4_800.0, 0.5412, 0.0, 48_000.0); 2],
            d4: Vec::new(),
            since_hop: 0,
            threshold: DEFAULT_TRACKING_THRESHOLD,
            gate_db: -50.0,
            history: [0.0; 3],
            history_len: 0,
            previous: None,
            disagreements: 0,
        }
    }

    /// Off-RT: sizes every ring from the range, in samples at this rate.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.p_max = (self.sample_rate / self.f_min).ceil() as usize;
        self.p_min = ((self.sample_rate / self.f_max).floor() as usize).max(2);
        self.decimation = ((self.sample_rate / (SAMPLES_PER_SHORTEST_PERIOD * self.f_max)).floor()
            as usize)
            .clamp(MIN_DECIMATION, MAX_DECIMATION);
        self.p_max4 = self.p_max.div_ceil(self.decimation).max(4);
        self.p_min4 = (self.p_min / self.decimation).max(2);
        self.w4 = 2 * self.p_max4;

        // The refine reads a window of `2·P_max` and a lag of up to `P_max`
        // past it, and the hop may land anywhere; one more block of slack.
        self.full_len = 3 * self.p_max + self.hop as usize + 64;
        self.full = vec![0.0; self.full_len * 2];
        // The coarse pass reads `2τ` and a lag of `τ` past it, for `τ` up to
        // `P_max4` — three of those, and room to spare.
        self.deci_len = 3 * self.p_max4 + 16;
        self.deci = vec![0.0; self.deci_len * 2];
        self.d4 = vec![0.0; self.p_max4 + 2];

        // Two cascaded SVF low-passes at the Butterworth pair: together a
        // 4th-order response at a fifth of the decimated rate, which is what
        // makes taking every fourth sample honest.
        // A fifth under the decimated rate's own Nyquist, so taking every
        // `D`th sample is honest rather than an alias.
        let corner =
            (self.sample_rate / (2.5 * self.decimation as f32)).min(self.sample_rate * 0.4);
        for (filter, q) in self.lp_coeffs.iter_mut().zip([0.5412, 1.3066]) {
            *filter = SvfFilter::coeffs(SvfMode::Lowpass, corner, q, 0.0, self.sample_rate);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        self.full.fill(0.0);
        self.deci.fill(0.0);
        self.full_write = 0;
        self.deci_write = 0;
        self.full_count = 0;
        self.deci_count = 0;
        self.deci_phase = 0;
        self.since_hop = 0;
        self.history_len = 0;
        self.previous = None;
        self.disagreements = 0;
        for filter in &mut self.lp {
            filter.reset();
        }
    }

    /// The confidence threshold, on the normalised difference: low is strict.
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.clamp(0.01, 0.6);
    }

    /// Where the `tracking` knob puts the threshold: 0 relaxed, 1 strict.
    pub fn set_tracking(&mut self, tracking: f32) {
        let t = tracking.clamp(0.0, 1.0);
        self.set_threshold(
            RELAXED_TRACKING_THRESHOLD
                + (STRICT_TRACKING_THRESHOLD - RELAXED_TRACKING_THRESHOLD) * t,
        );
    }

    /// Under this level the tracker reports unvoiced whatever it hears. Room
    /// noise does not get tuned.
    pub fn set_gate_db(&mut self, gate_db: f32) {
        self.gate_db = gate_db;
    }

    /// How many samples until the next hop lands. What lets a caller split a
    /// block on hop boundaries so its control rate and this one agree.
    pub fn samples_to_hop(&self) -> u32 {
        self.hop - self.since_hop.min(self.hop)
    }

    pub fn hop(&self) -> u32 {
        self.hop
    }

    /// Pushes a block in, calling `report` once for every hop that completed
    /// inside it. `None` is unvoiced.
    ///
    /// RT-safe: every buffer was sized in `prepare`.
    pub fn push(&mut self, block: &[f32], report: &mut dyn FnMut(Option<PitchFrame>)) {
        if self.full.is_empty() {
            return;
        }
        for sample in block {
            self.full[self.full_write] = *sample;
            self.full[self.full_write + self.full_len] = *sample;
            self.full_write += 1;
            if self.full_write == self.full_len {
                self.full_write = 0;
            }
            self.full_count += 1;

            let mut filtered = *sample;
            for (filter, coeffs) in self.lp.iter_mut().zip(&self.lp_coeffs) {
                filtered = filter.process(filtered, coeffs);
            }
            if self.deci_phase == 0 {
                self.deci[self.deci_write] = filtered;
                self.deci[self.deci_write + self.deci_len] = filtered;
                self.deci_write += 1;
                if self.deci_write == self.deci_len {
                    self.deci_write = 0;
                }
                self.deci_count += 1;
            }
            self.deci_phase += 1;
            if self.deci_phase == self.decimation {
                self.deci_phase = 0;
            }

            self.since_hop += 1;
            if self.since_hop >= self.hop {
                self.since_hop = 0;
                let frame = self.analyse();
                report(frame);
            }
        }
    }

    /// The most recent `want` full-rate samples, oldest first — one slice,
    /// because the ring is mirrored.
    fn full_window(&self, want: usize) -> &[f32] {
        let end = self.full_write + self.full_len;
        let want = want.min(self.full_len);
        &self.full[end - want..end]
    }

    /// The same, decimated.
    fn deci_window(&self, want: usize) -> &[f32] {
        let end = self.deci_write + self.deci_len;
        let want = want.min(self.deci_len);
        &self.deci[end - want..end]
    }

    fn analyse(&mut self) -> Option<PitchFrame> {
        let window = 2 * self.p_max;
        if (self.full_count as usize) < window + self.p_max {
            return None;
        }
        if (self.deci_count as usize) < self.w4 + self.p_max4 {
            return None;
        }

        // The gate, first: nothing under it is a note, whatever it correlates
        // with.
        let recent = self.full_window(window);
        let energy: f32 = recent.iter().map(|s| s * s).sum();
        let rms = (energy / recent.len().max(1) as f32).sqrt();
        if 20.0 * rms.max(1e-12).log10() < self.gate_db {
            self.previous = None;
            self.history_len = 0;
            return None;
        }

        // The scratch is lent to the coarse pass and put straight back: its
        // reader borrows the rings on `&self`, and a buffer written through
        // `&mut self` while another is read through `&self` is not a thing the
        // borrow checker will take. Moving a `Vec` header allocates nothing
        // (INVARIANT 1).
        let mut d4 = std::mem::take(&mut self.d4);
        let coarse = self.coarse_lag(&mut d4);
        self.d4 = d4;
        let coarse = coarse?;
        let (period, confidence) = self.refine(coarse * self.decimation as f32)?;
        if 1.0 - confidence >= self.threshold {
            self.previous = None;
            self.history_len = 0;
            return None;
        }

        // The octave guard. A candidate a whole octave from the last voiced
        // pitch, on a hop the *previous* period also explains, is a tracker
        // that has slipped rather than a singer who has jumped — and one hop
        // of it is what a breathy consonant sounds like. Two hops of the same
        // disagreement is a new note, and the guard lets go.
        let mut period = period;
        if let Some((previous, previous_confidence)) = self.previous {
            let cents = 1200.0 * (period / previous).log2();
            let octave_ish = (cents.abs() - 1200.0).abs() < OCTAVE_GUARD_CENTS;
            if octave_ish && previous_confidence > 0.8 {
                let lag = previous.round() as usize;
                let explained = self
                    .confidence_at(lag)
                    .is_some_and(|c| 1.0 - c < self.threshold);
                if explained && self.disagreements < GUARD_PATIENCE {
                    self.disagreements += 1;
                    period = previous;
                } else {
                    self.disagreements = 0;
                }
            } else {
                self.disagreements = 0;
            }
        }

        // The median of three, on the period: one hop of nonsense between two
        // good ones never reaches the corrector.
        if self.history_len < 3 {
            self.history[self.history_len] = period;
            self.history_len += 1;
        } else {
            self.history[0] = self.history[1];
            self.history[1] = self.history[2];
            self.history[2] = period;
        }
        let reported = if self.history_len == 3 {
            let mut sorted = self.history;
            sorted.sort_by(f32::total_cmp);
            sorted[1]
        } else {
            period
        };

        self.previous = Some((period, confidence));
        let hz = self.sample_rate / reported.max(1.0);
        Some(PitchFrame {
            hz,
            cents: hz_to_cents(hz),
            confidence,
            rms,
        })
    }

    /// The coarse search, at a quarter of the rate.
    ///
    /// YIN's difference function and its "first minimum under the threshold"
    /// rule — which is the step that stops octave-up errors — over an
    /// integration window of **`2τ` rather than a fixed `2·P_max`**, anchored
    /// at the newest sample.
    ///
    /// A departure from the plan's §3.2, and the reason is reaction time: a
    /// fixed window two periods of the *lowest* note long is 20 ms at
    /// alto/tenor, so a step up to a 330 Hz note takes sixteen hops to clear
    /// it however short that note's own period is. With the window measured in
    /// periods of the *candidate*, a note is found in two of its own periods,
    /// which is the fastest anything can be. The price is that the plain
    /// cumulative mean cannot normalise windows of different lengths, so the
    /// normalisation is against the two half-windows' energy instead: zero is
    /// a perfect period and one is unrelated, which is the same scale the
    /// voiced test and the octave guard already read.
    fn coarse_lag(&self, d4: &mut [f32]) -> Option<f32> {
        let low = self.p_min4.max(1);
        let high = self.p_max4.saturating_sub(1);
        if low >= high || d4.len() <= high {
            return None;
        }
        // From lag one, not from the bottom of the range: the cumulative mean
        // below needs every shorter lag to divide by, and it is that mean
        // which makes a short lag have to be *better than average* rather than
        // merely good — which is what stops the octave-up error a
        // lag-proportional window would otherwise invite.
        let mut running = 0.0f32;
        for tau in 1..=high {
            // Two periods of the candidate, floored so the shortest lags are
            // still measured over enough samples to mean anything.
            let w = (2 * tau).clamp(MIN_COARSE_WINDOW, 2 * self.p_max4);
            // One slice for both halves, oldest first: `later` is the newest
            // `w` samples and `earlier` is the `w` a lag before them.
            let window = self.deci_window(w + tau);
            if window.len() < w + tau {
                continue;
            }
            let earlier = &window[..w];
            let later = &window[tau..tau + w];
            let mut difference = 0.0f32;
            let mut here = 0.0f32;
            let mut there = 0.0f32;
            for n in 0..w {
                let a = later[n];
                let b = earlier[n];
                let diff = a - b;
                difference += diff * diff;
                here += a * a;
                there += b * b;
            }
            let energy = here + there;
            let normalised = if energy > 1e-12 {
                difference / energy
            } else {
                1.0
            };
            running += normalised;
            d4[tau] = if running > 1e-12 {
                normalised * tau as f32 / running
            } else {
                1.0
            };
        }

        let mut best = low;
        for tau in low..=high {
            if d4[tau] < d4[best] {
                best = tau;
            }
        }
        // The first lag under the threshold, walked down to its own local
        // minimum. Falls back to the global minimum when nothing is under it,
        // which is YIN's step 4.
        for tau in low..=high {
            if d4[tau] < self.threshold {
                let mut at = tau;
                while at < high && d4[at + 1] < d4[at] {
                    at += 1;
                }
                best = at;
                break;
            }
        }
        Some(best as f32)
    }

    /// The full-rate refine: thirteen lags round the coarse answer, then a
    /// parabola through the three either side of the minimum.
    fn refine(&mut self, around: f32) -> Option<(f32, f32)> {
        let centre = around.round() as i32;
        let span = self.decimation as i32 + REFINE_MARGIN;
        let low = (centre - span).max(self.p_min as i32);
        let high = (centre + span).min(self.p_max as i32);
        if low >= high {
            return None;
        }
        let w = (2 * (centre.max(2) as usize)).min(2 * self.p_max);
        let need = w + high as usize;
        if need > self.full_len {
            return None;
        }
        let window = self.full_window(need);
        let difference = |tau: usize| {
            let earlier = &window[high as usize - tau..high as usize - tau + w];
            let later = &window[high as usize..high as usize + w];
            let mut sum = 0.0f32;
            for n in 0..w {
                let diff = later[n] - earlier[n];
                sum += diff * diff;
            }
            sum
        };

        let mut best = low as usize;
        let mut best_d = f32::MAX;
        for tau in low..=high {
            let d = difference(tau as usize);
            if d < best_d {
                best_d = d;
                best = tau as usize;
            }
        }

        // A parabola through the neighbours: a period is a real number, and a
        // tuner that rounded it to a sample would be four cents out at 400 Hz.
        let period = if best > low as usize && best < high as usize {
            let dm = difference(best - 1);
            let dp = difference(best + 1);
            let denominator = dm - 2.0 * best_d + dp;
            if denominator.abs() > 1e-12 {
                best as f32 + 0.5 * (dm - dp) / denominator
            } else {
                best as f32
            }
        } else {
            best as f32
        };

        let confidence = periodicity(window, high as usize, w, best, best_d);
        Some((
            period.clamp(self.p_min as f32, self.p_max as f32),
            confidence,
        ))
    }

    /// How periodic the window is at a lag the caller names — what the octave
    /// guard asks about the period it is defending.
    fn confidence_at(&self, lag: usize) -> Option<f32> {
        if lag < self.p_min || lag > self.p_max {
            return None;
        }
        let w = (2 * lag).min(2 * self.p_max);
        let need = w + lag;
        if need > self.full_len {
            return None;
        }
        let window = self.full_window(need);
        let earlier = &window[..w];
        let later = &window[lag..lag + w];
        let mut difference = 0.0f32;
        for n in 0..w {
            let diff = later[n] - earlier[n];
            difference += diff * diff;
        }
        Some(periodicity(window, lag, w, lag, difference))
    }
}

/// How periodic a window is at one lag, 0..=1.
///
/// The normalised difference against the energy of the two half-windows rather
/// than against the cumulative mean: a ratio the two ends of which mean
/// something — one is a perfect period and zero is unrelated — and the same
/// number the voiced test and the octave guard both read.
///
/// `window` is oldest-first and `at` is where the later half starts in it.
fn periodicity(window: &[f32], at: usize, w: usize, lag: usize, difference: f32) -> f32 {
    let earlier = &window[at - lag..at - lag + w];
    let later = &window[at..at + w];
    let mut here = 0.0f32;
    let mut there = 0.0f32;
    for n in 0..w {
        here += later[n] * later[n];
        there += earlier[n] * earlier[n];
    }
    let total = here + there;
    if total < 1e-12 {
        return 0.0;
    }
    (1.0 - difference / total).clamp(0.0, 1.0)
}

/// MIDI cents from hertz: A440 is 6900.
pub fn hz_to_cents(hz: f32) -> f32 {
    6900.0 + 1200.0 * (hz.max(1e-6) / 440.0).log2()
}

/// And back.
pub fn cents_to_hz(cents: f32) -> f32 {
    440.0 * ((cents - 6900.0) / 1200.0).exp2()
}
