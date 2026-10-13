//! The wavtool's half: units placed in time against each other, and the
//! phrase assembled from their analyses.

use std::path::Path;
use std::sync::Arc;

use fontelle_sing::{CURVE_STEP, Lane, Phrase};
use rayon::prelude::*;

use crate::bank::Voicebank;
use crate::oto::OtoEntry;
use crate::phonemize::{GAP, Plan};
use crate::world::{Analysis, FRAME_PERIOD, Frames};

/// A unit placed in phrase time.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub alias: String,
    pub note: usize,
    pub entry: OtoEntry,
    /// Phrase seconds the sample's offset plays at.
    pub start: f64,
    /// Where its preutterance lands (the note's start for a main unit).
    pub anchor: f64,
    /// Where it stops sounding.
    pub end: f64,
    /// Fade in from `start`, out to `end`, seconds.
    pub fade_in: f64,
    pub fade_out: f64,
    /// Output seconds per sample second over the consonant region:
    /// `1 / velocity`, and any shortening placement needed.
    pub consonant_scale: f64,
    /// Length of the sample in ms (for the cutoff).
    pub file_ms: f64,
    /// Power gain (1 for a sung unit).
    pub gain: f64,
    /// A breath taken before a phrase, not a sung unit: no phoneme of its
    /// note, never smoothed against its neighbours.
    pub breath: bool,
    /// The last unit of a run: its fade out is the voice's release.
    pub release: bool,
}

impl Placed {
    /// The sample time (ms from the file's start) heard at phrase time
    /// `t`: the consonant region at `consonant_scale`, the rest stretched
    /// to fill to `end` when the sample is shorter, or played as recorded
    /// (and cut) when it is longer.
    pub fn source_ms(&self, t: f64) -> f64 {
        let e = &self.entry;
        let end_ms = e.end_ms(self.file_ms);
        let dt = (t - self.start) * 1000.0;
        let consonant = e.consonant.min(end_ms - e.offset).max(0.0);
        let con_out = consonant * self.consonant_scale;
        if dt <= con_out {
            return e.offset + dt / self.consonant_scale.max(1e-6);
        }
        let vowel_src = end_ms - e.offset - consonant;
        let vowel_out = (self.end - self.start) * 1000.0 - con_out;
        let into = dt - con_out;
        let s = if vowel_out > vowel_src && vowel_src > 0.0 {
            into * vowel_src / vowel_out
        } else {
            into
        };
        (e.offset + consonant + s).min(end_ms)
    }

    /// The crossfade weight at `t`, 0..1.
    pub fn weight(&self, t: f64) -> f64 {
        if t < self.start || t > self.end {
            return 0.0;
        }
        let fade_in = if self.fade_in > 0.0 {
            ((t - self.start) / self.fade_in).min(1.0)
        } else {
            1.0
        };
        let fade_out = if self.fade_out > 0.0 {
            ((self.end - t) / self.fade_out).min(1.0)
        } else {
            1.0
        };
        fade_in.min(fade_out).max(0.0)
    }
}

/// Places a plan's units: each starts its preutterance before its anchor
/// (scaled by the note's velocity), runs until the next one's overlap has
/// faded it out, and is pulled in where its consonant would take more than
/// half of the unit before it (UTAU's rule).
pub fn place(
    bank: &Voicebank,
    plan: &Plan,
    phrase: &Phrase,
    file_ms: &dyn Fn(&Path) -> Option<f64>,
) -> Vec<Placed> {
    let notes = &phrase.notes;
    let rest_between = |a: usize, b: usize| {
        (a + 1..=b.min(notes.len() - 1)).any(|i| notes[i].start - notes[i - 1].end() > GAP)
    };
    // The end of the run of touching notes from `a`.
    let run_end = |a: usize| {
        let mut i = a;
        while i + 1 < notes.len() && !rest_between(i, i + 1) {
            i += 1;
        }
        notes[i].end()
    };
    // Each unit with its entry, before ends are known.
    let mut placed: Vec<Placed> = Vec::new();
    let mut tails: Vec<bool> = Vec::new();
    for u in &plan.units {
        if u.note >= notes.len() {
            continue;
        }
        let note = &notes[u.note];
        let tone = note.pitch.round().clamp(0.0, 127.0) as u8;
        let colour = note
            .options
            .get(crate::NOTE_OPTION_COLOUR)
            .map(String::as_str)
            .unwrap_or("");
        let Some(entry) = bank.find(&u.alias, tone, colour) else {
            continue;
        };
        let Some(ms) = file_ms(&entry.file) else {
            continue;
        };
        let mut scale = 1.0 / f64::from(note.velocity).clamp(0.1, 10.0);
        let mut pre = entry.preutterance * scale / 1000.0;
        let mut ovl = entry.overlap * scale / 1000.0;
        // UTAU's rule: a consonant may take at most half of the unit
        // before it, or the one before would barely sound.
        if u.start.is_none()
            && let Some(prev) = placed.last()
            && !tails.last().copied().unwrap_or(false)
            && !rest_between(prev.note, u.note)
        {
            let half = (u.anchor - prev.anchor) / 2.0;
            if pre - ovl > half && pre - ovl > 0.0 && half > 0.0 {
                let k = half / (pre - ovl);
                pre *= k;
                ovl *= k;
                scale *= k;
            }
        }
        let start = u.start.unwrap_or(u.anchor - pre);
        placed.push(Placed {
            alias: u.alias.clone(),
            note: u.note,
            entry: entry.clone(),
            start,
            anchor: u.anchor,
            end: start,
            fade_in: ovl,
            fade_out: 0.0,
            consonant_scale: scale,
            file_ms: ms,
            gain: 1.0,
            breath: false,
            release: false,
        });
        tails.push(u.tail);
    }
    // Ends: a unit runs until the next one has faded it out, to its notes'
    // end before a rest, or its own length when it is an ending.
    for i in 0..placed.len() {
        let next = placed.get(i + 1).map(|n| (n.start, n.fade_in, n.note));
        let p = &mut placed[i];
        let natural = {
            let e = &p.entry;
            let end_ms = e.end_ms(p.file_ms);
            let consonant = e.consonant.min(end_ms - e.offset).max(0.0);
            (consonant * p.consonant_scale + (end_ms - e.offset - consonant)) / 1000.0
        };
        if tails[i] {
            p.end = p.start + natural;
            p.fade_out = RELEASE.min(natural / 4.0);
            p.release = true;
            continue;
        }
        match next {
            Some((start, ovl, note)) if !rest_between(p.note, note) => {
                if ovl > 0.0 {
                    p.end = start + ovl;
                    p.fade_out = ovl;
                } else {
                    p.end = start + ovl;
                    p.fade_out = MIN_FADE;
                }
            }
            _ => {
                p.end = run_end(p.note);
                p.fade_out = RELEASE.min((p.end - p.start) / 4.0);
                p.release = true;
            }
        }
    }
    for p in &mut placed {
        if p.fade_in <= 0.0 {
            p.fade_in = MIN_FADE;
        }
        p.end = p.end.max(p.start + 2.0 * MIN_FADE);
    }
    placed
}

/// What a bank's breath is called, in the order looked for: the usual
/// English names, then the Japanese 息 (Kasane Teto's extra library).
pub const BREATH_ALIASES: [&str; 4] = ["br", "breath", "息", "息1"];
/// A breath is taken only after a rest at least this long (or at the start
/// of a phrase), lasts at most `BREATH_MAX`, and is heard at `BREATH_GAIN`
/// (power) times the breath setting.
const BREATH_MIN_REST: f64 = 0.3;
const BREATH_MAX: f64 = 0.4;
const BREATH_MIN: f64 = 0.12;
const BREATH_GAIN: f64 = 1.0;

/// The bank's breath for a note at `tone` in `colour`, if it has one.
pub fn breath_entry<'a>(bank: &'a Voicebank, tone: u8, colour: &str) -> Option<&'a OtoEntry> {
    BREATH_ALIASES
        .iter()
        .find_map(|a| bank.find(a, tone, colour))
}

/// `placed` with a breath before the start of each run that has room for
/// one: the phrase's first, and any after a rest of at least
/// [`BREATH_MIN_REST`]. It ends just inside the first unit, so the
/// consonant comes out of the breath. `amount` 0..1; none at 0 or when the
/// bank has no breath.
///
/// Ty: "breath/aspiration at phrase ends" — a phrase that starts from
/// silence with no breath is one of the things that makes UTAU sound
/// pasted together.
pub fn add_breaths(
    bank: &Voicebank,
    placed: Vec<Placed>,
    phrase: &Phrase,
    file_ms: &dyn Fn(&Path) -> Option<f64>,
    amount: f64,
) -> Vec<Placed> {
    if amount <= 0.0 {
        return placed;
    }
    let notes = &phrase.notes;
    let mut out: Vec<Placed> = Vec::with_capacity(placed.len() + 2);
    let mut prev_end: Option<f64> = None;
    for (r, p) in placed.iter().enumerate() {
        let after_rest = r == 0
            || (p.note > placed[r - 1].note
                && p.note < notes.len()
                && notes[p.note].start - notes[p.note - 1].end() >= BREATH_MIN_REST);
        if after_rest && !p.breath {
            let room = match prev_end {
                None => BREATH_MAX,
                Some(e) => p.start - e - 0.05,
            };
            let note = &notes[p.note];
            let tone = note.pitch.round().clamp(0.0, 127.0) as u8;
            let colour = note
                .options
                .get(crate::NOTE_OPTION_COLOUR)
                .map(String::as_str)
                .unwrap_or("");
            if let Some(entry) = breath_entry(bank, tone, colour)
                && let Some(ms) = file_ms(&entry.file)
            {
                let natural = (entry.end_ms(ms) - entry.offset) / 1000.0;
                let len = natural.min(BREATH_MAX).min(room);
                if len >= BREATH_MIN {
                    let end = p.start + 0.02;
                    let start = end - len;
                    out.push(Placed {
                        alias: entry.alias.clone(),
                        note: p.note,
                        entry: entry.clone(),
                        start,
                        anchor: start,
                        end,
                        fade_in: (len / 3.0).min(0.05),
                        fade_out: 0.04,
                        consonant_scale: 1.0,
                        file_ms: ms,
                        gain: BREATH_GAIN * amount.min(1.0),
                        breath: true,
                        release: false,
                    });
                }
            }
        }
        prev_end = Some(prev_end.map_or(p.end, |e: f64| e.max(p.end)));
        out.push(p.clone());
    }
    out
}

/// The shortest fade a unit gets at an edge, so nothing clicks.
const MIN_FADE: f64 = 0.005;
/// The fade at the end of a sung run.
const RELEASE: f64 = 0.03;

/// How a phrase is voiced beyond its notes: lanes and settings, read once.
pub struct Voicing<'a> {
    pub phrase: &'a Phrase,
    /// 0..1: how much of each recording's own pitch movement is kept.
    pub modulation: f64,
    /// 0..1: the singer's expression on the written pitch.
    pub expression: f64,
    /// 0..1: breaths and aspirated releases.
    pub breath: f64,
}

/// One unit's analysis, with the figures assembly needs.
pub struct UnitSource {
    pub analysis: Arc<Analysis>,
    /// The median voiced F0, for modulation.
    pub median_f0: f64,
}

/// The phrase as vocoder frames: every unit's frames read where its
/// placement says, crossfaded in power, on the pitch line the notes and
/// the bend lane make, with the lanes applied. Returns the frames, the
/// phrase time of the first, and the F0 sung per frame (0 unvoiced).
pub fn assemble(
    placed: &[Placed],
    sources: &[&UnitSource],
    voicing: &Voicing,
    fs: u32,
    fft_size: usize,
) -> (Frames, f64) {
    let phrase = voicing.phrase;
    let bins = fft_size / 2 + 1;
    let (t0, t1) = if placed.is_empty() {
        let end = phrase.notes.last().map(|n| n.end()).unwrap_or(0.0);
        (0.0, end)
    } else {
        (
            placed.iter().map(|p| p.start).fold(f64::INFINITY, f64::min),
            placed
                .iter()
                .map(|p| p.end)
                .fold(f64::NEG_INFINITY, f64::max),
        )
    };
    // Frames on the curve grid, so frame j is the pitch curve's sample j.
    let t0 = (t0 / CURVE_STEP).floor() * CURVE_STEP;
    let nf = (((t1 - t0) / FRAME_PERIOD).ceil() as usize).max(1) + 1;
    let pitch = crate::expression::pitch_line(
        &phrase.notes,
        t0,
        t0 + (nf - 1) as f64 * FRAME_PERIOD,
        voicing.expression,
    );
    let lane = |l: Lane| phrase.curves.get(&l);
    let (bend, dynamics, breath, voicing_lane, tension, gender) = (
        lane(Lane::PitchBend),
        lane(Lane::Dynamics),
        lane(Lane::Breathiness),
        lane(Lane::Voicing),
        lane(Lane::Tension),
        lane(Lane::Gender),
    );
    let df = fs as f64 / fft_size as f64;
    let joins = joins(placed, sources, fs, fft_size);
    let bands = BandMap::new(bins, df);
    let aspirate = 0.6 * voicing.breath.clamp(0.0, 1.0);
    let mut frames = Frames {
        fft_size,
        f0: vec![0.0; nf],
        sp: vec![0.0; nf * bins],
        ap: vec![1.0; nf * bins],
    };
    // Frames are independent: made in parallel, each with its own scratch.
    let scratch = || Scratch {
        total: vec![0.0f64; bins],
        periodic: vec![0.0f64; bins],
        ap_bins: vec![0.0f32; bins],
        ap_bins2: vec![0.0f32; bins],
        warped: vec![0.0f64; bins],
        band_gain: vec![0.0f64; BANDS],
    };
    frames
        .sp
        .par_chunks_mut(bins)
        .zip(frames.ap.par_chunks_mut(bins))
        .zip(frames.f0.par_iter_mut())
        .enumerate()
        .for_each_init(scratch, |scr, (j, ((sp_out, ap_out), f0_out))| {
            let Scratch {
                total,
                periodic,
                ap_bins,
                ap_bins2,
                warped,
                band_gain,
            } = scr;
            let t = t0 + j as f64 * FRAME_PERIOD;
            total.iter_mut().for_each(|v| *v = 0.0);
            periodic.iter_mut().for_each(|v| *v = 0.0);
            let mut voiced_weight = 0.0;
            let mut dominant: Option<(f64, f64, f64)> = None; // (weight, src f0, median)
            // How far into a release the voice is, 0..1.
            let mut release = 0.0f64;
            for (u, (p, src)) in placed.iter().zip(sources).enumerate() {
                let w = p.weight(t) * p.gain;
                if w <= 0.0 {
                    continue;
                }
                if p.release && p.fade_out > 0.0 && t > p.end - p.fade_out {
                    release = release.max(((t - (p.end - p.fade_out)) / p.fade_out).min(1.0));
                }
                let corrected = joins[u].gains(t, band_gain);
                let a = &src.analysis;
                let x = p.source_ms(t) / 1000.0 / FRAME_PERIOD;
                if x < 0.0 || a.frames() == 0 {
                    continue;
                }
                let i = (x.floor() as usize).min(a.frames() - 1);
                let k = (i + 1).min(a.frames() - 1);
                let fr = (x - i as f64).clamp(0.0, 1.0);
                let near = if fr < 0.5 { i } else { k };
                let voiced = a.f0[near] > 0.0;
                let (si, sk) = (a.sp_frame(i), a.sp_frame(k));
                crate::world::expand_ap(a.ap_frame(i), a.fs, a.fft_size, ap_bins);
                crate::world::expand_ap(a.ap_frame(k), a.fs, a.fft_size, ap_bins2);
                let src_bins = a.bins();
                // Sung at a lower rate (a preview), the band kept carries only
                // its share of the variance.
                let level = f64::from(fs) / f64::from(a.fs);
                for b in 0..bins.min(src_bins) {
                    let mut s =
                        level * (f64::from(si[b]) + (f64::from(sk[b]) - f64::from(si[b])) * fr);
                    if corrected {
                        s *= bands.gain(b, band_gain);
                    }
                    total[b] += w * s;
                    if voiced {
                        let ap = f64::from(ap_bins[b])
                            + (f64::from(ap_bins2[b]) - f64::from(ap_bins[b])) * fr;
                        periodic[b] += w * s * (1.0 - ap * ap).max(0.0);
                    }
                }
                if voiced {
                    voiced_weight += w;
                    if dominant.is_none_or(|d| w > d.0) {
                        dominant = Some((w, f64::from(a.f0[near]), src.median_f0));
                    }
                }
            }
            // Gender: the envelope stretched along frequency.
            if let Some(g) = gender {
                let cents = f64::from(g.at(t, 0.0));
                if cents.abs() > 0.5 {
                    let ratio = 2f64.powf(cents / 1200.0);
                    for buf in [&mut *total, &mut *periodic] {
                        for (b, out) in warped.iter_mut().enumerate() {
                            let x = b as f64 / ratio;
                            let i = (x.floor() as usize).min(bins - 1);
                            let k = (i + 1).min(bins - 1);
                            let fr = (x - i as f64).clamp(0.0, 1.0);
                            *out = buf[i] + (buf[k] - buf[i]) * fr;
                        }
                        buf.copy_from_slice(warped);
                    }
                }
            }
            let gain = dynamics
                .map(|c| 10f64.powf(f64::from(c.at(t, 0.0)) / 10.0))
                .unwrap_or(1.0);
            let tens = tension
                .map(|c| f64::from(c.at(t, 0.0)).clamp(-1.0, 1.0))
                .unwrap_or(0.0);
            let breath = breath
                .map(|c| f64::from(c.at(t, 0.0)).clamp(-1.0, 1.0))
                .unwrap_or(0.0);
            let voice = voicing_lane
                .map(|c| f64::from(c.at(t, 0.0)).clamp(-1.0, 1.0))
                .unwrap_or(0.0);
            for b in 0..bins {
                let f = b as f64 * df;
                // Tension: a tilt about 1 kHz, up to 12 dB either way.
                let tilt_db = if tens != 0.0 {
                    (tens * 6.0 * (f.max(250.0) / 1000.0).log2()).clamp(-12.0, 12.0)
                } else {
                    0.0
                };
                let g = gain * 10f64.powf(tilt_db / 10.0);
                let tot = total[b] * g;
                sp_out[b] = tot as f32;
                let mut ap = if tot > 0.0 {
                    (1.0 - periodic[b] * g / tot).clamp(0.0, 1.0).sqrt()
                } else {
                    1.0
                };
                // Breathiness and voicing scale the aperiodicity in dB (towards
                // 0 dB, all noise, or away from it); breath mostly above 1 kHz.
                let mut db = 20.0 * ap.max(1e-4).log10();
                if breath != 0.0 {
                    let reach = (0.4 + 0.6 * (f / 2000.0).min(1.0)) * breath;
                    db *= if reach > 0.0 {
                        1.0 - reach
                    } else {
                        1.0 - reach * 0.8
                    };
                }
                if voice != 0.0 {
                    db *= 1.0 + voice;
                }
                ap = 10f64.powf(db.max(-80.0) / 20.0).min(1.0);
                // The release goes out on breath, not on a cut of the buzz.
                if release > 0.0 && aspirate > 0.0 {
                    ap += (1.0 - ap) * aspirate * release;
                }
                ap_out[b] = ap as f32;
            }
            if voiced_weight > 0.0 && ap_out[0] < 0.999 {
                let mut midi = f64::from(pitch.at(t, 60.0));
                if let Some(c) = bend {
                    midi += f64::from(c.at(t, 0.0)) / 100.0;
                }
                let mut hz = 440.0 * 2f64.powf((midi - 69.0) / 12.0);
                if voicing.modulation > 0.0
                    && let Some((_, f, median)) = dominant
                    && median > 0.0
                {
                    hz *= (f / median).powf(voicing.modulation);
                }
                *f0_out = hz as f32;
            }
        });
    (frames, t0)
}

struct Scratch {
    total: Vec<f64>,
    periodic: Vec<f64>,
    ap_bins: Vec<f32>,
    ap_bins2: Vec<f32>,
    warped: Vec<f64>,
    band_gain: Vec<f64>,
}

/// Bands the join smoothing measures and corrects in, log-spaced.
const BANDS: usize = 16;
const BAND_LOW: f64 = 80.0;
/// How long a join's correction takes to relax back to the recording's
/// own level and colour, either side of the crossfade.
const JOIN_RELAX: f64 = 0.15;
/// The most a join corrects: overall, and in colour on top.
const JOIN_LEVEL_MAX: f64 = 10.0;
const JOIN_SHAPE_MAX: f64 = 6.0;
/// A colour difference past this (RMS dB over the bands) is two different
/// sounds meeting, a vowel change: only the level is matched then.
const JOIN_SAME_SOUND: f64 = 3.0;

/// Output bins to log-spaced bands and back.
struct BandMap {
    /// Each band's bins.
    ranges: Vec<(usize, usize)>,
    /// Each bin's place between band centres: (lower band, share of the
    /// upper).
    place: Vec<(usize, f64)>,
}

impl BandMap {
    fn new(bins: usize, df: f64) -> Self {
        let top = df * (bins - 1) as f64 * 0.95;
        let edge = |k: usize| BAND_LOW * (top / BAND_LOW).powf(k as f64 / BANDS as f64);
        let ranges: Vec<(usize, usize)> = (0..BANDS)
            .map(|k| {
                let a = (edge(k) / df).floor() as usize;
                let b = ((edge(k + 1) / df).ceil() as usize).max(a + 1).min(bins);
                (a.min(bins - 1), b)
            })
            .collect();
        let centre = |k: usize| (edge(k) * edge(k + 1)).sqrt().ln();
        let place = (0..bins)
            .map(|b| {
                let f = (b as f64 * df).max(1.0).ln();
                if f <= centre(0) {
                    return (0, 0.0);
                }
                for k in 0..BANDS - 1 {
                    if f <= centre(k + 1) {
                        return (k, (f - centre(k)) / (centre(k + 1) - centre(k)));
                    }
                }
                (BANDS - 2, 1.0)
            })
            .collect();
        BandMap { ranges, place }
    }

    /// Mean power per band, dB.
    fn levels(&self, power: &[f64]) -> Vec<f64> {
        self.ranges
            .iter()
            .map(|&(a, b)| {
                let n = (b - a).max(1) as f64;
                10.0 * (power[a..b].iter().sum::<f64>() / n).max(1e-20).log10()
            })
            .collect()
    }

    /// The power gain at bin `b` from per-band dB.
    fn gain(&self, b: usize, band_db: &[f64]) -> f64 {
        let (k, x) = self.place[b];
        let db = band_db[k] + (band_db[k + 1] - band_db[k]) * x;
        10f64.powf(db / 10.0)
    }
}

/// A unit's corrections where it meets the units either side of it.
#[derive(Default)]
struct Join {
    /// Joined from the unit before: full until `.0`, relaxing after.
    incoming: Option<(f64, Vec<f64>)>,
    /// Joined into the unit after: full from `.0`, relaxing before.
    outgoing: Option<(f64, Vec<f64>)>,
}

impl Join {
    /// Fills `out` with the band gains (dB) at `t`; false when there are
    /// none.
    fn gains(&self, t: f64, out: &mut [f64]) -> bool {
        let ramp_in = self.incoming.as_ref().map(|(at, _)| {
            if t <= *at {
                1.0
            } else {
                (1.0 - (t - at) / JOIN_RELAX).max(0.0)
            }
        });
        let ramp_out = self.outgoing.as_ref().map(|(at, _)| {
            if t >= *at {
                1.0
            } else {
                (1.0 - (at - t) / JOIN_RELAX).max(0.0)
            }
        });
        let ri = ramp_in.unwrap_or(0.0);
        let ro = ramp_out.unwrap_or(0.0);
        if ri <= 0.0 && ro <= 0.0 {
            return false;
        }
        for (k, o) in out.iter_mut().enumerate() {
            *o = 0.0;
            if let Some((_, c)) = &self.incoming {
                *o += ri * c[k];
            }
            if let Some((_, c)) = &self.outgoing {
                *o += ro * c[k];
            }
        }
        true
    }
}

/// A unit's power per output bin at phrase time `t`, and whether it is
/// voiced there.
fn unit_power(
    p: &Placed,
    src: &UnitSource,
    t: f64,
    bins: usize,
    fs: u32,
) -> Option<(Vec<f64>, bool)> {
    let a = &src.analysis;
    let x = p.source_ms(t) / 1000.0 / FRAME_PERIOD;
    if x < 0.0 || a.frames() == 0 {
        return None;
    }
    let i = (x.floor() as usize).min(a.frames() - 1);
    let k = (i + 1).min(a.frames() - 1);
    let fr = (x - i as f64).clamp(0.0, 1.0);
    let (si, sk) = (a.sp_frame(i), a.sp_frame(k));
    let level = f64::from(fs) / f64::from(a.fs);
    let mut out = vec![1e-20; bins];
    for (b, o) in out.iter_mut().enumerate().take(bins.min(a.bins())) {
        *o = level * (f64::from(si[b]) + (f64::from(sk[b]) - f64::from(si[b])) * fr);
    }
    let near = if fr < 0.5 { i } else { k };
    Some((out, a.f0[near] > 0.0))
}

/// Where two sung units crossfade, both voiced, the difference between
/// them shared out: each meets the other halfway at the crossfade and
/// relaxes back to its own sound over [`JOIN_RELAX`]. Two recordings of
/// the same vowel sung at different levels, or with a different colour,
/// no longer jump at the join.
fn joins(placed: &[Placed], sources: &[&UnitSource], fs: u32, fft_size: usize) -> Vec<Join> {
    let bins = fft_size / 2 + 1;
    let map = BandMap::new(bins, fs as f64 / fft_size as f64);
    let mut out: Vec<Join> = placed.iter().map(|_| Join::default()).collect();
    let mut prev: Option<usize> = None;
    for i in 0..placed.len() {
        if placed[i].breath {
            continue;
        }
        let Some(h) = prev.replace(i) else {
            continue;
        };
        let (a, b) = (&placed[h], &placed[i]);
        let (ws, we) = (b.start, a.end.min(b.start + b.fade_in.max(0.005)));
        if we <= ws - 1e-9 {
            continue;
        }
        let mut sum_a = vec![0.0; bins];
        let mut sum_b = vec![0.0; bins];
        // Where both are voiced (a recording's first frame may not read as
        // voiced yet); at least two points of the five.
        let mut voiced = 0;
        for s in 0..5 {
            let t = ws + (we - ws) * s as f64 / 4.0;
            if let (Some((pa, true)), Some((pb, true))) = (
                unit_power(a, sources[h], t, bins, fs),
                unit_power(b, sources[i], t, bins, fs),
            ) {
                sum_a.iter_mut().zip(&pa).for_each(|(x, y)| *x += y);
                sum_b.iter_mut().zip(&pb).for_each(|(x, y)| *x += y);
                voiced += 1;
            }
        }
        if voiced < 2 {
            continue;
        }
        let (la, lb) = (map.levels(&sum_a), map.levels(&sum_b));
        let diff: Vec<f64> = la.iter().zip(&lb).map(|(x, y)| x - y).collect();
        let level = diff.iter().sum::<f64>() / BANDS as f64;
        let shape: Vec<f64> = diff.iter().map(|d| d - level).collect();
        let spread = (shape.iter().map(|d| d * d).sum::<f64>() / BANDS as f64).sqrt();
        let level = level.clamp(-JOIN_LEVEL_MAX, JOIN_LEVEL_MAX);
        let corr: Vec<f64> = shape
            .iter()
            .map(|d| {
                let s = if spread <= JOIN_SAME_SOUND {
                    d.clamp(-JOIN_SHAPE_MAX, JOIN_SHAPE_MAX)
                } else {
                    0.0
                };
                (level + s) / 2.0
            })
            .collect();
        out[i].incoming = Some((we, corr.clone()));
        out[h].outgoing = Some((ws, corr.iter().map(|c| -c).collect()));
    }
    out
}
