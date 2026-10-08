//! Singing a phrase: lyrics to phonemes, phonemes to time, then the bank's
//! models in turn — duration, pitch, variance, acoustic, vocoder.

use std::sync::Arc;

use fontelle_sing::{Lane, Phrase, Quality, SingError, Sung, SungNote, SungPhoneme, note_pitch};

use crate::bank::{
    Bank, DEFAULT_STEPS, LANE_ENERGY, LANE_EXPRESSIVENESS, LANE_SPEAKER_PREFIX, OPTION_SPEAKER,
    SETTING_NATURAL_PITCH, SETTING_STEPS, SubModel,
};
use crate::config::{DsConfig, Variance};
use crate::curves;
use crate::dict::Dictionary;
use crate::lyrics::words_of;
use crate::models::{Feed, Model, Out, RuntimeOptions};
use crate::phonemize::{NotePhonemes, Reader, phonemize};
use crate::timing::{
    PAD_FRAMES, Placed, Segment, dur_input, flatten, frames_of, pitch_notes, place, segments,
    word_division,
};

/// Sampling steps of a preview, for every model that samples.
pub const PREVIEW_STEPS: u32 = 4;
/// The variance model's steps in a final render.
pub const FINAL_VARIANCE_STEPS: u32 = 20;
/// The pitch model's steps in a final render.
pub const FINAL_PITCH_STEPS: u32 = 10;
/// Noise values a sampling model may draw per frame (mel bins times the
/// features it samples, with room to spare).
pub const NOISE_PER_FRAME: usize = 1024;

/// Noise values a vocoder may draw: per output sample, for a source of up to
/// 15 harmonics (RefineGAN-style vocoders such as Canary DS's tgm_hifigan
/// draw samples x 9), never less than the sampling models' pool.
pub fn vocoder_noise_pool(frames: usize, hop: usize) -> usize {
    (frames * hop.max(1) * 16).max(frames * NOISE_PER_FRAME)
}

/// A bank's models, opened.
pub struct Loaded {
    pub acoustic: Model,
    pub vocoder: Model,
    /// (linguistic encoder, model) of each optional folder.
    pub dur: Option<(Model, Model)>,
    pub pitch: Option<(Model, Model)>,
    pub variance: Option<(Model, Model)>,
}

impl Loaded {
    pub fn load(bank: &Bank, options: &RuntimeOptions) -> Result<Loaded, String> {
        let vocoder = bank.vocoder.as_ref().map_err(Clone::clone)?;
        if let Some(why) = vocoder.config.mismatch(&bank.acoustic.config) {
            return Err(format!("the vocoder does not fit this voicebank: {why}"));
        }
        let acoustic_file = bank
            .acoustic
            .config
            .acoustic
            .as_ref()
            .ok_or("dsconfig.yaml names no acoustic model")?;
        let pair = |sub: &Option<SubModel>,
                    which: fn(&DsConfig) -> Option<&String>|
         -> Result<Option<(Model, Model)>, String> {
            let Some(sub) = sub else {
                return Ok(None);
            };
            let (Some(l), Some(m)) = (sub.config.linguistic.as_ref(), which(&sub.config)) else {
                return Ok(None);
            };
            Ok(Some((
                Model::load(&sub.file(l), options)?,
                Model::load(&sub.file(m), options)?,
            )))
        };
        Ok(Loaded {
            acoustic: Model::load(&bank.acoustic.file(acoustic_file), options)?,
            vocoder: Model::load(&vocoder.model_path(), options)?,
            dur: pair(&bank.dur, |c| c.dur.as_ref())?,
            pitch: pair(&bank.pitch, |c| c.pitch.as_ref())?,
            variance: pair(&bank.variance, |c| c.variance.as_ref())?,
        })
    }
}

/// A phrase read, before any model runs.
pub struct Plan {
    /// The bank's code for the language sung.
    pub language: String,
    pub dictionary: Arc<Dictionary>,
    pub per_note: Vec<NotePhonemes>,
}

pub fn plan(bank: &Bank, phrase: &Phrase) -> Result<Plan, SingError> {
    let language = bank.language_for(&phrase.language);
    let dictionary = bank.dictionary(&language).map_err(SingError::Failed)?;
    let reader = Reader {
        dict: &dictionary,
        inventory: &bank.acoustic.inventory,
        language: &language,
    };
    let words = words_of(&phrase.notes);
    let per_note = phonemize(&phrase.notes, &words, &reader);
    Ok(Plan {
        language,
        dictionary,
        per_note,
    })
}

/// Whether the f0 is the notes' own line (with glides, vibrato and the
/// bend lane) rather than the bank's pitch model's: when the writer drew a
/// bend or set a vibrato or glide, when the part turned the model off, or
/// when the bank has none.
pub fn uses_note_pitch(bank: &Bank, phrase: &Phrase) -> bool {
    if bank.pitch.is_none() {
        return true;
    }
    if phrase
        .settings
        .get(SETTING_NATURAL_PITCH)
        .is_some_and(|v| *v < 0.5)
    {
        return true;
    }
    let bent = phrase
        .curves
        .get(&Lane::PitchBend)
        .is_some_and(|c| c.values.iter().any(|v| v.abs() > 1e-3));
    bent || phrase
        .notes
        .iter()
        .any(|n| n.vibrato.is_some() || n.portamento.is_some())
}

/// The steps (acoustic, variance, pitch) a render takes.
pub fn steps(phrase: &Phrase, quality: Quality) -> (u32, u32, u32) {
    let acoustic = phrase
        .settings
        .get(SETTING_STEPS)
        .filter(|v| v.is_finite())
        .map_or(DEFAULT_STEPS, |v| v.round().clamp(1.0, 100.0) as u32);
    match quality {
        Quality::Final => (acoustic, FINAL_VARIANCE_STEPS, FINAL_PITCH_STEPS),
        Quality::Preview => (acoustic.min(PREVIEW_STEPS), PREVIEW_STEPS, PREVIEW_STEPS),
    }
}

/// The sampling inputs of a diffusion or flow model: `steps` and, for a
/// model that renders from a variable depth, `depth` (a fraction, 0..1) —
/// as `steps`/`depth` with continuous acceleration, or a `speedup` (and a
/// depth out of 1000) without.
pub fn sampling_feeds(
    config: &DsConfig,
    model_inputs: &dyn Fn(&str) -> bool,
    steps: u32,
    depth: Option<f64>,
) -> Vec<(&'static str, Feed)> {
    let steps = i64::from(steps.max(1));
    let mut out = Vec::new();
    let depth = depth.filter(|_| model_inputs("depth"));
    if config.use_continuous_acceleration {
        if let Some(d) = depth {
            out.push(("depth", Feed::F32(vec![], vec![d.clamp(0.0, 1.0) as f32])));
        }
        out.push(("steps", Feed::I64(vec![], vec![steps])));
    } else {
        let speedup = match depth {
            Some(d) => {
                let mut depth_steps = (d.clamp(0.0, 1.0) * 1000.0).round() as i64;
                let speedup = (depth_steps / steps).max(1);
                depth_steps = depth_steps / speedup * speedup;
                out.push(("depth", Feed::I64(vec![], vec![depth_steps])));
                speedup
            }
            None => {
                let mut speedup = (1000 / steps).max(1);
                while 1000 % speedup != 0 && speedup > 1 {
                    speedup -= 1;
                }
                speedup
            }
        };
        out.push(("speedup", Feed::I64(vec![], vec![speedup])));
    }
    out
}

/// How much of each of a model folder's speakers sings note `note` at
/// time `t`: the note's own speaker, with the speaker mix lanes drawn over
/// it (they add up to at most 1; the note's speaker takes the rest).
pub fn speaker_weights(
    bank: &Bank,
    sub: &SubModel,
    phrase: &Phrase,
    note: usize,
    t: f64,
) -> Vec<f32> {
    let count = sub.config.speakers.as_ref().map_or(0, Vec::len);
    let mut w = vec![0.0f32; count];
    if count == 0 {
        return w;
    }
    let own = phrase
        .notes
        .get(note)
        .and_then(|n| n.options.get(OPTION_SPEAKER))
        .cloned()
        .or_else(|| bank.speakers.first().map(|s| s.id.clone()))
        .unwrap_or_default();
    let mut sum = 0.0;
    for speaker in &bank.speakers {
        let Some(curve) = phrase
            .extra_curves
            .get(&format!("{LANE_SPEAKER_PREFIX}{}", speaker.id))
        else {
            continue;
        };
        let v = (curve.at(t, 0.0) / 100.0).clamp(0.0, 1.0);
        w[sub.speaker_index(&speaker.id)] += v;
        sum += v;
    }
    if sum > 1.0 {
        for x in &mut w {
            *x /= sum;
        }
    } else {
        w[sub.speaker_index(&own)] += 1.0 - sum;
    }
    w
}

/// What each note was sung as, for the roll: symbols without the
/// language's prefix, offsets from the note's start.
pub fn sung_phonemes(phrase: &Phrase, plan: &Plan, placed: &[Placed]) -> Vec<SungNote> {
    let prefix = format!("{}/", plan.language);
    phrase
        .notes
        .iter()
        .enumerate()
        .map(|(i, note)| SungNote {
            note: i,
            phonemes: placed
                .iter()
                .filter(|p| p.note == i)
                .map(|p| SungPhoneme {
                    symbol: p
                        .symbol
                        .strip_prefix(&prefix)
                        .unwrap_or(&p.symbol)
                        .to_string(),
                    offset: Some(p.start - note.start),
                })
                .collect(),
            guessed: plan.per_note.get(i).is_some_and(|n| n.guessed),
        })
        .collect()
}

/// Sings a phrase with a bank's opened models.
pub fn sing(
    bank: &Bank,
    models: &Loaded,
    phrase: &Phrase,
    quality: Quality,
    seed: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Sung, SingError> {
    let vocoder = bank
        .vocoder
        .as_ref()
        .map_err(|e| SingError::Failed(e.clone()))?;
    let sample_rate = vocoder.config.sample_rate;
    if phrase.notes.is_empty() {
        return Ok(Sung {
            sample_rate,
            ..Sung::default()
        });
    }
    if cancelled() {
        return Err(SingError::Cancelled);
    }
    let notes = &phrase.notes;
    let plan = plan(bank, phrase)?;
    let dict = plan.dictionary.clone();
    let is_vowel = |s: &str| dict.is_vowel(s);
    let (acoustic_steps, variance_steps, pitch_steps) = steps(phrase, quality);

    // Durations.
    let lengths = match (&models.dur, &bank.dur) {
        (Some((linguistic, dur)), Some(sub)) => {
            predict_lengths(bank, sub, linguistic, dur, phrase, &plan, cancelled)?
        }
        _ => None,
    };
    let placed = place(notes, &plan.per_note, lengths.as_deref(), &is_vowel);
    let frame = bank.acoustic.config.frame_seconds();
    let pad = PAD_FRAMES as f64 * frame;
    let segs = segments(&placed, pad, pad);
    let layout = Layout::new(&segs, &placed, frame);
    let total = layout.total;
    let t0 = layout.start;

    // Pitch, as MIDI at the acoustic frames.
    let mut midi = if uses_note_pitch(bank, phrase) {
        let line = note_pitch(notes, t0, t0 + total as f64 * frame);
        curves::sample(Some(&line), t0, frame, total, 60.0)
    } else {
        let (Some((linguistic, model)), Some(sub)) = (&models.pitch, &bank.pitch) else {
            return Err(SingError::Failed("the pitch model is not loaded".into()));
        };
        let sub_layout = Layout::new(&segs, &placed, sub.config.frame_seconds());
        let predicted = predict_pitch(
            bank,
            sub,
            linguistic,
            model,
            phrase,
            &plan,
            &segs,
            &sub_layout,
            pitch_steps,
            seed,
            cancelled,
        )?;
        curves::resample(&predicted, t0, sub_layout.frame, t0, frame, total)
    };
    let bend = curves::sample(phrase.curves.get(&Lane::PitchBend), t0, frame, total, 0.0);
    for (m, b) in midi.iter_mut().zip(&bend) {
        *m += b / 100.0;
    }
    let f0: Vec<f32> = midi.iter().map(|&m| curves::midi_to_hz(m)).collect();

    // The acoustic model's inputs.
    let ac = &bank.acoustic.config;
    let mut feeds: Vec<(&str, Feed)> = vec![
        (
            "tokens",
            Feed::row_i64(tokens(&bank.acoustic, &segs, "acoustic model")?),
        ),
        (
            "languages",
            Feed::row_i64(language_ids(&bank.acoustic, &segs)),
        ),
        ("durations", Feed::row_i64(layout.frames.clone())),
        ("f0", Feed::row_f32(f0.clone())),
    ];
    let range = ac.key_shift_range().unwrap_or((0.0, 0.0));
    let gender: Vec<f32> = curves::sample(phrase.curves.get(&Lane::Gender), t0, frame, total, 0.0)
        .into_iter()
        .map(|c| curves::gender_input(c, range))
        .collect();
    feeds.push(("gender", Feed::row_f32(gender)));
    let velocity: Vec<f32> = layout
        .frame_notes
        .iter()
        .map(|&n| notes[n].velocity.clamp(0.25, 4.0))
        .collect();
    feeds.push(("velocity", Feed::row_f32(velocity)));
    let wanted = ac.embedded_variances();
    if !wanted.is_empty() {
        let (Some((linguistic, model)), Some(sub)) = (&models.variance, &bank.variance) else {
            return Err(SingError::Failed(
                "this voicebank's acoustic model needs variance curves, and it has no variance model"
                    .into(),
            ));
        };
        let sub_layout = Layout::new(&segs, &placed, sub.config.frame_seconds());
        let sub_midi = curves::resample(&midi, t0, frame, t0, sub_layout.frame, sub_layout.total);
        let predicted = predict_variances(
            bank,
            sub,
            linguistic,
            model,
            phrase,
            &segs,
            &sub_layout,
            sub_midi,
            variance_steps,
            seed,
            &is_vowel,
            cancelled,
        )?;
        for v in wanted {
            let values = predicted.get(&v).ok_or_else(|| {
                SingError::Failed(format!(
                    "the acoustic model needs {}, which the variance model does not predict",
                    v.name()
                ))
            })?;
            let mut values = curves::resample(values, t0, sub_layout.frame, t0, frame, total);
            let (delta, low, high): (Vec<f32>, f32, f32) = match v {
                Variance::Energy => (
                    curves::sample(phrase.extra_curves.get(LANE_ENERGY), t0, frame, total, 0.0),
                    -96.0,
                    0.0,
                ),
                Variance::Breathiness => (
                    lane_delta(
                        phrase,
                        Lane::Breathiness,
                        t0,
                        frame,
                        total,
                        curves::db_delta,
                    ),
                    -96.0,
                    0.0,
                ),
                Variance::Voicing => (
                    lane_delta(phrase, Lane::Voicing, t0, frame, total, curves::db_delta),
                    -96.0,
                    0.0,
                ),
                Variance::Tension => (
                    lane_delta(
                        phrase,
                        Lane::Tension,
                        t0,
                        frame,
                        total,
                        curves::tension_delta,
                    ),
                    -10.0,
                    10.0,
                ),
            };
            for (x, d) in values.iter_mut().zip(&delta) {
                *x = (*x + d).clamp(low, high);
            }
            feeds.push((v.name(), Feed::row_f32(values)));
        }
    }
    if let Some(e) = speaker_feed(bank, &bank.acoustic, phrase, &layout)? {
        feeds.push(("spk_embed", e));
    }
    let depth = ac.max_depth_fraction().map(|d| d.min(1.0));
    let model = &models.acoustic;
    feeds.extend(sampling_feeds(
        ac,
        &|n| model.has_input(n),
        acoustic_steps,
        depth.or(Some(1.0)),
    ));
    let out = model.run(
        feeds,
        Some((seed ^ 0xac0u64, total * NOISE_PER_FRAME)),
        cancelled,
    )?;
    let mel = out
        .get("mel")
        .or_else(|| out.values().next())
        .ok_or_else(|| SingError::Failed("the acoustic model gave no mel".into()))?;
    let (shape, mut mel) = match mel {
        Out::F32(s, v) => (s.clone(), v.clone()),
        _ => {
            return Err(SingError::Failed(
                "the acoustic model's mel is not float".into(),
            ));
        }
    };
    let k = match (vocoder.config.mel_base.as_str(), ac.mel_base.as_str()) {
        ("e", "10") => std::f32::consts::LN_10,
        ("10", "e") => std::f32::consts::LOG10_E,
        _ => 1.0,
    };
    if k != 1.0 {
        for m in &mut mel {
            *m *= k;
        }
    }

    // The vocoder.
    let out = models.vocoder.run(
        vec![
            ("mel", Feed::F32(shape, mel)),
            ("f0", Feed::row_f32(f0.clone())),
        ],
        Some((
            seed ^ 0x70c,
            vocoder_noise_pool(total, vocoder.config.hop_size as usize),
        )),
        cancelled,
    )?;
    let mut samples = out
        .get("waveform")
        .or_else(|| out.values().next())
        .and_then(Out::f32)
        .ok_or_else(|| SingError::Failed("the vocoder gave no waveform".into()))?
        .to_vec();
    if samples.iter().any(|s| !s.is_finite()) {
        return Err(SingError::Failed(
            "the vocoder's output is not finite".into(),
        ));
    }
    if let Some(d) = phrase.curves.get(&Lane::Dynamics) {
        curves::apply_gain(&mut samples, sample_rate, t0, d);
    }
    let pitch = f0
        .iter()
        .zip(&layout.frame_silent)
        .map(|(&hz, &silent)| if silent { 0.0 } else { hz })
        .collect();
    Ok(Sung {
        sample_rate,
        samples,
        start: t0,
        pitch,
        pitch_step: frame,
        phonemes: sung_phonemes(phrase, &plan, &placed),
    })
}

/// A phrase's segments at one model's frame rate.
struct Layout {
    frame: f64,
    start: f64,
    /// Frames per segment.
    frames: Vec<i64>,
    total: usize,
    /// The note each frame belongs to (silence: the nearest phoneme's).
    frame_notes: Vec<usize>,
    /// Frames of silence or breath, where nothing is pitched.
    frame_silent: Vec<bool>,
}

impl Layout {
    fn new(segs: &[Segment], placed: &[Placed], frame: f64) -> Layout {
        let frames = frames_of(segs, frame);
        let total = frames.iter().sum::<i64>().max(0) as usize;
        // Each segment's note: its phoneme's; silence takes the next
        // phoneme's, and the silence after the last, the last one's.
        let mut seg_notes: Vec<Option<usize>> = segs
            .iter()
            .map(|s| s.placed.map(|p| placed[p].note))
            .collect();
        let mut next = None;
        for n in seg_notes.iter_mut().rev() {
            match n {
                Some(v) => next = Some(*v),
                None => *n = next,
            }
        }
        let last = placed.last().map_or(0, |p| p.note);
        let seg_notes: Vec<usize> = seg_notes.into_iter().map(|n| n.unwrap_or(last)).collect();
        let mut frame_notes = Vec::with_capacity(total);
        let mut frame_silent = Vec::with_capacity(total);
        for (i, &n) in frames.iter().enumerate() {
            let silent =
                segs[i].placed.is_none() || segs[i].symbol == "SP" || segs[i].symbol == "AP";
            for _ in 0..n {
                frame_notes.push(seg_notes[i]);
                frame_silent.push(silent);
            }
        }
        Layout {
            frame,
            start: segs.first().map_or(0.0, |s| s.start),
            frames,
            total,
            frame_notes,
            frame_silent,
        }
    }
}

fn lane_delta(
    phrase: &Phrase,
    lane: Lane,
    start: f64,
    frame: f64,
    n: usize,
    to_model: fn(f32) -> f32,
) -> Vec<f32> {
    curves::sample(phrase.curves.get(&lane), start, frame, n, 0.0)
        .into_iter()
        .map(to_model)
        .collect()
}

fn tokens(sub: &SubModel, segs: &[Segment], what: &str) -> Result<Vec<i64>, SingError> {
    segs.iter()
        .map(|s| {
            sub.inventory.token(&s.symbol).ok_or_else(|| {
                SingError::Failed(format!("the {what} has no phoneme \"{}\"", s.symbol))
            })
        })
        .collect()
}

fn language_ids(sub: &SubModel, segs: &[Segment]) -> Vec<i64> {
    segs.iter()
        .map(|s| {
            sub.languages
                .as_ref()
                .map_or(0, |l| l.id_of_symbol(&s.symbol))
        })
        .collect()
}

/// `[1, frames, hidden]` speaker embeddings, when the folder has speakers.
fn speaker_feed(
    bank: &Bank,
    sub: &SubModel,
    phrase: &Phrase,
    layout: &Layout,
) -> Result<Option<Feed>, SingError> {
    if sub.config.speakers.is_none() {
        return Ok(None);
    }
    let embeds = sub.embeddings().map_err(SingError::Failed)?;
    let hidden = sub.config.hidden_size;
    let mut out = Vec::with_capacity(layout.total * hidden);
    for (f, &note) in layout.frame_notes.iter().enumerate() {
        let t = layout.start + f as f64 * layout.frame;
        let w = speaker_weights(bank, sub, phrase, note, t);
        mix(&embeds, &w, hidden, &mut out);
    }
    Ok(Some(Feed::F32(vec![1, layout.total, hidden], out)))
}

fn mix(embeds: &[Vec<f32>], weights: &[f32], hidden: usize, out: &mut Vec<f32>) {
    let at = out.len();
    out.resize(at + hidden, 0.0);
    for (e, &w) in embeds.iter().zip(weights) {
        if w == 0.0 {
            continue;
        }
        for (o, v) in out[at..].iter_mut().zip(e) {
            *o += w * v;
        }
    }
}

/// The duration model's consonant lengths, seconds per flattened phoneme;
/// `None` when it cannot read this phrase (a phoneme it does not know).
fn predict_lengths(
    bank: &Bank,
    sub: &SubModel,
    linguistic: &Model,
    dur: &Model,
    phrase: &Phrase,
    plan: &Plan,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Vec<f64>>, SingError> {
    let frame = sub.config.frame_seconds();
    let input = dur_input(&phrase.notes, &plan.per_note, frame);
    if input.symbols.is_empty() {
        return Ok(None);
    }
    let Some(tokens) = input
        .symbols
        .iter()
        .map(|s| sub.inventory.token(s))
        .collect::<Option<Vec<i64>>>()
    else {
        return Ok(None);
    };
    let languages: Vec<i64> = input
        .symbols
        .iter()
        .map(|s| sub.languages.as_ref().map_or(0, |l| l.id_of_symbol(s)))
        .collect();
    let out = linguistic.run(
        vec![
            ("tokens", Feed::row_i64(tokens)),
            ("languages", Feed::row_i64(languages)),
            ("word_div", Feed::row_i64(input.word_div.clone())),
            ("word_dur", Feed::row_i64(input.word_dur.clone())),
        ],
        None,
        cancelled,
    )?;
    let mut feeds: Vec<(&str, Feed)> = Vec::new();
    for name in ["encoder_out", "x_masks"] {
        if let Some(v) = out.get(name) {
            feeds.push((name, v.clone().into_feed()));
        }
    }
    feeds.push(("ph_midi", Feed::row_i64(input.ph_midi.clone())));
    if sub.config.speakers.is_some() {
        let embeds = sub.embeddings().map_err(SingError::Failed)?;
        let hidden = sub.config.hidden_size;
        let flat = flatten(&plan.per_note);
        let mut e = Vec::with_capacity(input.symbols.len() * hidden);
        for (k, index) in input.index.iter().enumerate() {
            // Silence takes the speaker of the phoneme after it.
            let note = index
                .or_else(|| input.index[k..].iter().flatten().next().copied())
                .or_else(|| input.index.iter().flatten().last().copied())
                .map_or(0, |i| flat[i].0);
            let t = phrase.notes[note].start;
            mix(
                &embeds,
                &speaker_weights(bank, sub, phrase, note, t),
                hidden,
                &mut e,
            );
        }
        feeds.push((
            "spk_embed",
            Feed::F32(vec![1, input.symbols.len(), hidden], e),
        ));
    }
    let out = dur.run(feeds, None, cancelled)?;
    let predicted = out
        .get("ph_dur_pred")
        .or_else(|| out.values().next())
        .and_then(Out::f32)
        .ok_or_else(|| SingError::Failed("the duration model gave no durations".into()))?;
    let mut lengths = vec![0.0; flatten(&plan.per_note).len()];
    for (k, index) in input.index.iter().enumerate() {
        if let (Some(i), Some(&d)) = (index, predicted.get(k)) {
            lengths[*i] = f64::from(d.max(0.0)) * frame;
        }
    }
    Ok(Some(lengths))
}

#[allow(clippy::too_many_arguments)]
fn predict_pitch(
    bank: &Bank,
    sub: &SubModel,
    linguistic: &Model,
    model: &Model,
    phrase: &Phrase,
    plan: &Plan,
    segs: &[Segment],
    layout: &Layout,
    steps: u32,
    seed: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<f32>, SingError> {
    let notes = &phrase.notes;
    let encoded = encode(
        sub,
        linguistic,
        segs,
        layout,
        &|s| plan.dictionary.is_vowel(s),
        cancelled,
    )?;
    // A note sings a pitch when its syllable has a vowel that is not
    // silence or breath; a held note follows the one it holds.
    let mut voiced = Vec::with_capacity(notes.len());
    for (i, np) in plan.per_note.iter().enumerate() {
        let v = if np.is_empty() {
            i > 0 && voiced[i - 1]
        } else {
            np.lead.iter().chain(&np.body).any(|p| {
                plan.dictionary.is_vowel(&p.symbol) && p.symbol != "SP" && p.symbol != "AP"
            })
        };
        voiced.push(v);
    }
    let pn = pitch_notes(
        notes,
        &voiced,
        layout.start,
        layout.frame,
        layout.total as i64,
    );
    let n = layout.total;
    let mut feeds = encoded;
    feeds.push(("ph_dur", Feed::row_i64(layout.frames.clone())));
    feeds.push(("note_midi", Feed::row_f32(pn.midi)));
    feeds.push(("note_rest", Feed::row_bool(pn.rest)));
    feeds.push(("note_dur", Feed::row_i64(pn.dur)));
    feeds.push(("pitch", Feed::row_f32(vec![60.0; n])));
    feeds.push(("retake", Feed::row_bool(vec![true; n])));
    let expr = curves::sample(
        phrase.extra_curves.get(LANE_EXPRESSIVENESS),
        layout.start,
        layout.frame,
        n,
        100.0,
    )
    .into_iter()
    .map(|v| (v / 100.0).clamp(0.0, 1.0))
    .collect();
    feeds.push(("expr", Feed::row_f32(expr)));
    if let Some(e) = speaker_feed(bank, sub, phrase, layout)? {
        feeds.push(("spk_embed", e));
    }
    feeds.extend(sampling_feeds(
        &sub.config,
        &|x| model.has_input(x),
        steps,
        Some(1.0),
    ));
    let out = model.run(feeds, Some((seed ^ 0x917c, n * NOISE_PER_FRAME)), cancelled)?;
    let pitch = out
        .get("pitch_pred")
        .or_else(|| out.values().next())
        .and_then(Out::f32)
        .ok_or_else(|| SingError::Failed("the pitch model gave no pitch".into()))?;
    if pitch.len() != n || pitch.iter().any(|p| !p.is_finite()) {
        return Err(SingError::Failed(
            "the pitch model's output is not usable".into(),
        ));
    }
    Ok(pitch.to_vec())
}

#[allow(clippy::too_many_arguments)]
fn predict_variances(
    bank: &Bank,
    sub: &SubModel,
    linguistic: &Model,
    model: &Model,
    phrase: &Phrase,
    segs: &[Segment],
    layout: &Layout,
    midi: Vec<f32>,
    steps: u32,
    seed: u64,
    is_vowel: &dyn Fn(&str) -> bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<std::collections::HashMap<Variance, Vec<f32>>, SingError> {
    let n = layout.total;
    let mut feeds = encode(sub, linguistic, segs, layout, is_vowel, cancelled)?;
    feeds.push(("ph_dur", Feed::row_i64(layout.frames.clone())));
    feeds.push(("pitch", Feed::row_f32(midi)));
    let predicted = sub.config.predicted_variances();
    for v in &predicted {
        feeds.push((v.name(), Feed::row_f32(vec![0.0; n])));
    }
    feeds.push((
        "retake",
        Feed::Bool(vec![1, n, predicted.len()], vec![true; n * predicted.len()]),
    ));
    if let Some(e) = speaker_feed(bank, sub, phrase, layout)? {
        feeds.push(("spk_embed", e));
    }
    feeds.extend(sampling_feeds(
        &sub.config,
        &|x| model.has_input(x),
        steps,
        Some(1.0),
    ));
    let out = model.run(feeds, Some((seed ^ 0x7a71, n * NOISE_PER_FRAME)), cancelled)?;
    let mut result = std::collections::HashMap::new();
    for v in predicted {
        if let Some(values) = out.get(&format!("{}_pred", v.name())).and_then(Out::f32) {
            if values.iter().any(|x| !x.is_finite()) {
                return Err(SingError::Failed(format!(
                    "the variance model's {} is not finite",
                    v.name()
                )));
            }
            result.insert(v, values.to_vec());
        }
    }
    Ok(result)
}

/// The linguistic encoder over the phrase's segments (phoneme durations
/// given): its outputs, ready to feed the model after it.
fn encode(
    sub: &SubModel,
    linguistic: &Model,
    segs: &[Segment],
    layout: &Layout,
    is_vowel: &dyn Fn(&str) -> bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<(&'static str, Feed)>, SingError> {
    // An encoder in word mode (`predict_dur`) takes words; one in phoneme
    // mode takes phoneme durations. Both are offered; it reads its own.
    let (word_div, word_dur) = word_division(segs, &layout.frames, is_vowel);
    let out = linguistic.run(
        vec![
            (
                "tokens",
                Feed::row_i64(tokens(sub, segs, "pitch or variance model")?),
            ),
            ("languages", Feed::row_i64(language_ids(sub, segs))),
            ("ph_dur", Feed::row_i64(layout.frames.clone())),
            ("word_div", Feed::row_i64(word_div)),
            ("word_dur", Feed::row_i64(word_dur)),
        ],
        None,
        cancelled,
    )?;
    let mut feeds = Vec::new();
    for name in ["encoder_out", "x_masks"] {
        if let Some(v) = out.get(name) {
            feeds.push((name, v.clone().into_feed()));
        }
    }
    Ok(feeds)
}
