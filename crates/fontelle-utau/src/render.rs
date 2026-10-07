//! The wavtool's half: units placed in time against each other, and the
//! phrase assembled from their analyses.

use std::path::Path;
use std::sync::Arc;

use fontelle_sing::{CURVE_STEP, Lane, Phrase, note_pitch};
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

/// The shortest fade a unit gets at an edge, so nothing clicks.
const MIN_FADE: f64 = 0.005;
/// The fade at the end of a sung run.
const RELEASE: f64 = 0.03;

/// How a phrase is voiced beyond its notes: lanes and settings, read once.
pub struct Voicing<'a> {
    pub phrase: &'a Phrase,
    /// 0..1: how much of each recording's own pitch movement is kept.
    pub modulation: f64,
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
    let pitch = note_pitch(&phrase.notes, t0, t0 + (nf - 1) as f64 * FRAME_PERIOD);
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
            } = scr;
            let t = t0 + j as f64 * FRAME_PERIOD;
            total.iter_mut().for_each(|v| *v = 0.0);
            periodic.iter_mut().for_each(|v| *v = 0.0);
            let mut voiced_weight = 0.0;
            let mut dominant: Option<(f64, f64, f64)> = None; // (weight, src f0, median)
            for (p, src) in placed.iter().zip(sources) {
                let w = p.weight(t);
                if w <= 0.0 {
                    continue;
                }
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
                    let s = level * (f64::from(si[b]) + (f64::from(sk[b]) - f64::from(si[b])) * fr);
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
}
