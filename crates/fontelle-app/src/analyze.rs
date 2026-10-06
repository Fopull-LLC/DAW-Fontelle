//! Analyze Musically's job (`docs/analyze-musically-plan.md` §3.10): one
//! clip's audio analysed on a worker of its own — never the bounce's slot —
//! publishing the notes as they are found, cached under the project (or the
//! XDG cache before there is one), and turned into what the window shows
//! and what it gives back to the song.
//!
//! The analysis itself is `fontelle-analysis`'s and is pure. What is here is
//! the plumbing: a mono mix of the clip's span, the worker, the cache, and
//! the conversions — seconds into song ticks through the tempo map
//! (INVARIANT 5), loudness into velocity, contour bins into cents.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use fontelle_analysis::analysis::{Analysed, Analysis, Mode};
use fontelle_analysis::confidence::Clarity;
use fontelle_analysis::transcribe::NoteEvent;
use fontelle_analysis::transcribe::basic_pitch::BasicPitch;
use fontelle_types::{ClipId, Tick};
use fontelle_ui::canvas::{
    AnalyzeClarity, AnalyzeImage, AnalyzeKey, AnalyzeMode, AnalyzeView, AnalyzedChord, AnalyzedNote,
};

/// Waveform buckets a second, for the lane's background.
const PEAKS_PER_SECOND: f64 = 100.0;
/// Velocity from loudness: the loudest note 120, 30 dB down 30 (plan §3.6).
const VELOCITY_RANGE: (f32, f32) = (30.0, 120.0);
const VELOCITY_DB: f32 = 30.0;
/// A pitch curve point every this many seconds is plenty for a 1.5 px line.
const CURVE_STEP: f64 = 0.012;

/// Where analyses are kept (INVARIANT 10): the project's own
/// `cache/analysis/`, or before the project is saved, the XDG cache —
/// `$XDG_CACHE_HOME/fontelle/analysis`, else `~/.cache/fontelle/analysis`.
/// `None` when there is nowhere, and then nothing is kept.
pub fn analysis_cache_dir(
    bundle: Option<&Path>,
    xdg_cache: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(bundle) = bundle {
        return Some(bundle.join("cache").join("analysis"));
    }
    let base = xdg_cache
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".cache")))?;
    Some(base.join("fontelle").join("analysis"))
}

/// [`analysis_cache_dir`] from this process's environment.
pub(crate) fn cache_dir_here(bundle: Option<&Path>) -> Option<PathBuf> {
    analysis_cache_dir(
        bundle,
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// The model, loaded once for the process (it is `Send + Sync`).
fn model() -> Result<&'static BasicPitch, String> {
    static MODEL: OnceLock<Result<BasicPitch, String>> = OnceLock::new();
    MODEL
        .get_or_init(|| BasicPitch::load().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// A finished analysis and what the window and the copies need beside it.
#[derive(Debug)]
pub(crate) struct Finished {
    pub analysed: Analysed,
    /// RMS loudness of each of `analysis.notes`, then each of `melody`,
    /// relative to the loudest of its list (0..1).
    pub note_loudness: Vec<f32>,
    pub melody_loudness: Vec<f32>,
    /// The contour image's cells, shared with every view rather than copied.
    pub image_data: Arc<[u8]>,
}

/// What the worker and the window share.
#[derive(Debug, Default)]
struct Shared {
    fraction: f32,
    duration: f64,
    peaks: Arc<[(f32, f32)]>,
    /// The notes found so far, while it runs.
    partial: Vec<NoteEvent>,
    done: Option<Result<Arc<Finished>, String>>,
    /// The cache key the worker worked out, for a later reopen.
    key: Option<String>,
}

/// One clip's analysis, open in the window.
pub(crate) struct OpenAnalysis {
    pub clip: ClipId,
    pub name: String,
    /// What was analysed: the asset and the span of it, for the key memo.
    pub span: (fontelle_types::AssetId, i64, i64),
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
    revision: Arc<AtomicU64>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for OpenAnalysis {
    /// A window closed, or another clip opened: the job stops at its next
    /// chance. It is not waited for — it holds only its own copies.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The span of the clip's audio, mixed to mono.
pub(crate) fn mono_span(buffer: &fontelle_core::AudioBuffer, from: usize, to: usize) -> Vec<f32> {
    let channels = usize::from(buffer.channels.max(1));
    let to = to.min(buffer.frames());
    let from = from.min(to);
    let scale = 1.0 / channels as f32;
    (from..to)
        .map(|frame| {
            let at = frame * channels;
            buffer.data[at..at + channels].iter().sum::<f32>() * scale
        })
        .collect()
}

fn peaks_of(mono: &[f32], rate: u32) -> Arc<[(f32, f32)]> {
    let per = (f64::from(rate) / PEAKS_PER_SECOND).max(1.0) as usize;
    mono.chunks(per)
        .map(|chunk| {
            chunk
                .iter()
                .fold((0.0f32, 0.0f32), |(lo, hi), s| (lo.min(*s), hi.max(*s)))
        })
        .collect::<Vec<_>>()
        .into()
}

/// Each span's RMS, relative to the loudest.
fn loudness(mono: &[f32], rate: u32, spans: impl Iterator<Item = (f64, f64)>) -> Vec<f32> {
    let rms: Vec<f32> = spans
        .map(|(start, end)| {
            let a = ((start * f64::from(rate)) as usize).min(mono.len());
            let b = ((end * f64::from(rate)) as usize).clamp(a, mono.len());
            if b == a {
                return 0.0;
            }
            (mono[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt()
        })
        .collect();
    let loudest = rms.iter().copied().fold(0.0f32, f32::max).max(1e-9);
    rms.into_iter().map(|r| r / loudest).collect()
}

fn finish(analysed: Analysed, mono: &[f32], rate: u32) -> Finished {
    let a = &analysed.analysis;
    let note_loudness = loudness(mono, rate, a.notes.iter().map(|n| (n.start, n.end)));
    let melody_loudness = loudness(
        mono,
        rate,
        a.melody.iter().map(|n| (n.start_time, n.end_time)),
    );
    let image_data = analysed.image.data.clone().into();
    Finished {
        analysed,
        note_loudness,
        melody_loudness,
        image_data,
    }
}

impl OpenAnalysis {
    /// Opens on `mono` (the clip's span at `rate`): from the cache when
    /// `key` is known and kept there — finished at once — and otherwise on a
    /// worker of its own.
    pub fn start(
        clip: ClipId,
        name: String,
        span: (fontelle_types::AssetId, i64, i64),
        mono: Vec<f32>,
        rate: u32,
        cache: Option<PathBuf>,
        known_key: Option<String>,
    ) -> Self {
        let duration = mono.len() as f64 / f64::from(rate.max(1));
        let shared = Arc::new(Mutex::new(Shared {
            duration,
            peaks: peaks_of(&mono, rate),
            ..Default::default()
        }));
        let open = |worker| Self {
            clip,
            name: name.clone(),
            span,
            shared: Arc::clone(&shared),
            cancel: Arc::new(AtomicBool::new(false)),
            revision: Arc::new(AtomicU64::new(1)),
            worker,
        };
        // A reopen: the key is known, so the cache is read here and the
        // window shows the analysis at once.
        if let (Some(dir), Some(key)) = (&cache, &known_key)
            && let Some(analysis) = fontelle_analysis::cache::load(dir, key)
            && let Some(image) = fontelle_analysis::cache::load_image(dir, key)
        {
            let finished = finish(Analysed { analysis, image }, &mono, rate);
            if let Ok(mut shared) = shared.lock() {
                shared.fraction = 1.0;
                shared.done = Some(Ok(Arc::new(finished)));
                shared.key = Some(key.clone());
            }
            return open(None);
        }
        let mut this = open(None);
        let worker_shared = Arc::clone(&shared);
        let cancel = Arc::clone(&this.cancel);
        let revision = Arc::clone(&this.revision);
        let spawned = std::thread::Builder::new()
            .name("fontelle-analyze".to_string())
            .spawn(move || {
                let result = run(
                    &mono,
                    rate,
                    cache.as_deref(),
                    &worker_shared,
                    &cancel,
                    &revision,
                );
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok(mut shared) = worker_shared.lock() {
                    shared.fraction = 1.0;
                    shared.partial.clear();
                    shared.done = Some(result.map(Arc::new));
                }
                revision.fetch_add(1, Ordering::Relaxed);
            });
        match spawned {
            Ok(handle) => this.worker = Some(handle),
            Err(e) => {
                if let Ok(mut shared) = shared.lock() {
                    shared.done = Some(Err(format!("could not start the analysis: {e}")));
                }
            }
        }
        this
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    pub fn running(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| !w.is_finished())
    }

    /// The worker has ended and nobody has been told: it is joined, and
    /// the answer is how it went.
    pub fn take_ending(&mut self) -> Option<Result<String, String>> {
        let worker = self.worker.as_ref()?;
        if !worker.is_finished() {
            return None;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let done = self.shared.lock().ok()?.done.clone();
        Some(match done {
            Some(Ok(_)) => Ok(format!("Analysed \u{201c}{}\u{201d}", self.name)),
            Some(Err(e)) => Err(e),
            None => Err("the analysis stopped".to_string()),
        })
    }

    pub fn fraction(&self) -> f32 {
        self.shared.lock().map_or(0.0, |s| s.fraction)
    }

    /// The cache key, once the worker has worked it out.
    pub fn key(&self) -> Option<String> {
        self.shared.lock().ok()?.key.clone()
    }

    pub fn finished(&self) -> Option<Arc<Finished>> {
        match &self.shared.lock().ok()?.done {
            Some(Ok(finished)) => Some(Arc::clone(finished)),
            _ => None,
        }
    }

    /// What the window shows.
    pub fn view(&self) -> AnalyzeView {
        let Ok(shared) = self.shared.lock() else {
            return AnalyzeView::default();
        };
        let mut view = AnalyzeView {
            name: self.name.clone(),
            duration: shared.duration,
            peaks: Arc::clone(&shared.peaks),
            peaks_per_second: PEAKS_PER_SECOND,
            ..Default::default()
        };
        match &shared.done {
            None => {
                view.analysing = Some(shared.fraction);
                let loud = vec![0.7; shared.partial.len()];
                view.notes = shared
                    .partial
                    .iter()
                    .zip(&loud)
                    .map(|(n, l)| poly_view(n, *l))
                    .collect();
            }
            Some(Err(e)) => view.error = Some(e.clone()),
            Some(Ok(finished)) => fill_view(&mut view, finished),
        }
        view
    }
}

/// The worker's whole job: the cache key, the cache, or the analysis.
fn run(
    mono: &[f32],
    rate: u32,
    cache: Option<&Path>,
    shared: &Mutex<Shared>,
    cancel: &AtomicBool,
    revision: &AtomicU64,
) -> Result<Finished, String> {
    let key = fontelle_analysis::cache::cache_key(mono, rate);
    if let Ok(mut s) = shared.lock() {
        s.key = Some(key.clone());
    }
    if let Some(dir) = cache
        && let Some(analysis) = fontelle_analysis::cache::load(dir, &key)
        && let Some(image) = fontelle_analysis::cache::load_image(dir, &key)
    {
        return Ok(finish(Analysed { analysis, image }, mono, rate));
    }
    let model = model()?;
    let analysed = fontelle_analysis::analysis::analyse_progressive(model, mono, rate, &mut |p| {
        if let Ok(mut s) = shared.lock() {
            s.fraction = p.fraction;
            s.partial = p.notes.to_vec();
        }
        revision.fetch_add(1, Ordering::Relaxed);
        !cancel.load(Ordering::Relaxed)
    })
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "the analysis was stopped".to_string())?;
    // A cache that cannot be written is a slower reopen, not a failure.
    if let Some(dir) = cache {
        let _ = fontelle_analysis::cache::store(dir, &key, &analysed.analysis)
            .and_then(|()| fontelle_analysis::cache::store_image(dir, &key, &analysed.image));
    }
    Ok(finish(analysed, mono, rate))
}

// ------------------------------------------------------- the view ---

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    Some(values[values.len() / 2])
}

/// A basic-pitch note as the lane draws it: its centre is the median of its
/// bends, its curve the bends at their frames' times.
fn poly_view(n: &NoteEvent, loudness: f32) -> AnalyzedNote {
    use fontelle_analysis::transcribe::Posteriorgrams;
    let bends = n.bend_cents();
    let cents = median(bends.clone()).unwrap_or(0.0);
    // The contour moves in thirds of a semitone; drawn as it is, a held
    // note is a staircase. Five frames' average (58 ms) is the line a
    // listener hears.
    let smooth: Vec<f32> = (0..bends.len())
        .map(|i| {
            let from = i.saturating_sub(2);
            let to = (i + 3).min(bends.len());
            bends[from..to].iter().sum::<f32>() / (to - from) as f32
        })
        .collect();
    let curve = smooth
        .iter()
        .enumerate()
        .step_by(2)
        .map(|(i, c)| (Posteriorgrams::frame_time(n.start_frame + i), *c))
        .collect();
    AnalyzedNote {
        start: n.start,
        end: n.end,
        midi: n.midi,
        cents,
        amplitude: loudness,
        // The model's own activation is how sure it is: 0.3 is its floor.
        confidence: ((n.amplitude - 0.25) / 0.45).clamp(0.05, 1.0),
        poly: true,
        curve,
    }
}

fn melody_view(
    n: &fontelle_analysis::mono::MonoNote,
    loudness: f32,
    notes: &[NoteEvent],
) -> AnalyzedNote {
    let midi = n.midi();
    let offset = n.centre - f32::from(midi) * 100.0;
    let frames = n.end.saturating_sub(n.first).max(1);
    let hop = (n.end_time - n.start_time) / frames as f64;
    let every = ((CURVE_STEP / hop.max(1e-6)).round() as usize).max(1);
    let curve = n
        .drift
        .iter()
        .zip(&n.vibrato)
        .enumerate()
        .step_by(every)
        .map(|(i, (d, v))| (n.start_time + i as f64 * hop, offset + d + v))
        .collect();
    // How sure: the model's activation for the same key over the same time.
    let confidence = notes
        .iter()
        .filter(|m| m.midi == midi && m.start < n.end_time && m.end > n.start_time)
        .map(|m| m.amplitude)
        .fold(None, |best: Option<f32>, a| {
            Some(best.map_or(a, |b| b.max(a)))
        })
        .map_or(0.5, |a| ((a - 0.25) / 0.45).clamp(0.05, 1.0));
    AnalyzedNote {
        start: n.start_time,
        end: n.end_time,
        midi,
        cents: offset,
        amplitude: loudness,
        confidence,
        poly: false,
        curve,
    }
}

fn clarity_reason(a: &Analysis) -> String {
    let e = &a.extraction.evidence;
    if a.guessed {
        return "nothing clearly pitched was heard: these notes are a guess".to_string();
    }
    match a.extraction.clarity {
        Clarity::Clear => "clean, pitched audio: these notes are a reliable reading".to_string(),
        Clarity::Usable if e.snr_db < 20.0 => {
            "some noise under it: check the notes by ear".to_string()
        }
        Clarity::Usable => "some sound no note explains: check the notes by ear".to_string(),
        Clarity::RoughGuess if e.flatness > 0.3 => {
            "lots of noise or drums: treat these notes as a starting point".to_string()
        }
        Clarity::RoughGuess => "hard to hear notes in: treat these as a starting point".to_string(),
    }
}

/// How far the recording sits from A = 440: the duration-weighted median of
/// the notes' offsets from their keys.
fn tuning(a: &Analysis, melody: &[AnalyzedNote], notes: &[AnalyzedNote]) -> Option<f32> {
    let list = if a.mode == Mode::Melody && !melody.is_empty() {
        melody
    } else {
        notes
    };
    let mut weighted: Vec<(f32, f64)> = list
        .iter()
        .map(|n| (n.cents, (n.end - n.start).max(0.0)))
        .collect();
    let total: f64 = weighted.iter().map(|w| w.1).sum();
    if weighted.is_empty() || total <= 0.0 {
        return None;
    }
    weighted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut run = 0.0;
    for (cents, weight) in &weighted {
        run += weight;
        if run >= total / 2.0 {
            return Some(*cents);
        }
    }
    weighted.last().map(|w| w.0)
}

fn fill_view(view: &mut AnalyzeView, finished: &Finished) {
    let a = &finished.analysed.analysis;
    view.duration = a.duration;
    view.detected = Some(match a.mode {
        Mode::Melody => AnalyzeMode::Melody,
        Mode::Chords => AnalyzeMode::Chords,
    });
    view.notes = a
        .notes
        .iter()
        .zip(&finished.note_loudness)
        .map(|(n, l)| poly_view(n, *l))
        .collect();
    view.melody = a
        .melody
        .iter()
        .zip(&finished.melody_loudness)
        .map(|(n, l)| melody_view(n, *l, &a.notes))
        .collect();
    view.chords = a
        .chords
        .iter()
        .map(|c| AnalyzedChord {
            start: c.start,
            end: c.end,
            label: c.chord.map(|c| c.label()).unwrap_or_default(),
        })
        .collect();
    view.key = a.key.as_ref().map(|k| AnalyzeKey {
        key: k.key.clone(),
        confidence: k.confidence,
        relative: k.relative.clone(),
        tonic_confidence: k.tonic_confidence,
        alternatives: k.alternatives.clone(),
    });
    view.clarity = Some((
        match a.extraction.clarity {
            Clarity::Clear => AnalyzeClarity::Clear,
            Clarity::Usable => AnalyzeClarity::Usable,
            Clarity::RoughGuess => AnalyzeClarity::Rough,
        },
        a.extraction.confidence,
    ));
    view.clarity_reason = clarity_reason(a);
    view.tuning_cents = tuning(a, &view.melody, &view.notes);
    let image = &finished.analysed.image;
    view.spectrogram = (image.columns > 0).then(|| AnalyzeImage {
        columns: image.columns,
        rows: image.rows,
        columns_per_second: image.columns_per_second,
        lowest_midi: image.midi_of_row(0),
        rows_per_semitone: 3.0,
        data: Arc::clone(&finished.image_data),
    });
}

// ------------------------------------------- notes for a piano roll ---

/// Where the analysed audio sits in the song: what turns its seconds into
/// song ticks.
pub(crate) struct Placement<'a> {
    pub tempo: &'a fontelle_model::TempoMap,
    /// The clip's start, as a song sample.
    pub start_sample: i64,
    /// The project's rate: a second of the clip is this many song samples.
    pub sample_rate: u32,
    /// The clip's playback speed (a second of audio lasts 1/speed).
    pub speed: f64,
    /// Its transposition, in semitones (heard notes move with it).
    pub semitones: f32,
}

impl Placement<'_> {
    fn tick(&self, seconds: f64) -> Tick {
        let samples = seconds / self.speed.max(1e-6) * f64::from(self.sample_rate);
        self.tempo
            .sample_to_tick(self.start_sample + samples.round() as i64)
    }
}

/// The analysed notes `selection` names (all with none) as piano-roll notes
/// in **song ticks**, and the earliest one's tick. Clean semitones, timing
/// as played, velocity from loudness; with `keep_bends`, the centre's cents
/// as `fine_pitch` and bends of a semitone or more as path points (Ty, plan
/// §6 answer 5).
pub(crate) fn notes_for_roll(
    view: &AnalyzeView,
    mode: AnalyzeMode,
    selection: &[usize],
    keep_bends: bool,
    placement: &Placement<'_>,
) -> Option<(Vec<fontelle_model::Note>, Tick)> {
    let list = match mode {
        AnalyzeMode::Melody if view.detected.is_some() => &view.melody,
        _ => &view.notes,
    };
    let picked: Vec<&AnalyzedNote> = if selection.is_empty() {
        list.iter().collect()
    } else {
        selection.iter().filter_map(|i| list.get(*i)).collect()
    };
    if picked.is_empty() {
        return None;
    }
    let shift = placement.semitones.round() as i32;
    let notes: Vec<fontelle_model::Note> = picked
        .iter()
        .map(|n| {
            let start = placement.tick(n.start);
            let end = placement.tick(n.end).max(start + 1);
            let level_db = 20.0 * n.amplitude.max(1e-6).log10();
            let t = ((level_db + VELOCITY_DB) / VELOCITY_DB).clamp(0.0, 1.0);
            let velocity = VELOCITY_RANGE.0 + t * (VELOCITY_RANGE.1 - VELOCITY_RANGE.0);
            let key = (i32::from(n.midi) + shift).clamp(0, 127) as u8;
            let mut note = fontelle_model::Note {
                start,
                length: end - start,
                key,
                velocity: velocity.round() as u8,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                slide: false,
                path: Vec::new(),
                channel: None,
            };
            if keep_bends {
                note.fine_pitch = n.cents.round().clamp(-1200.0, 1200.0) as i16;
                note.path = bend_path(n, start, placement);
            }
            note
        })
        .collect();
    let origin = notes.iter().map(|n| n.start).min()?;
    Some((notes, origin))
}

/// A note's slides, as path points: where its pitch, rounded to semitones
/// from its key, changes. Finer bends are its `fine_pitch` and no more.
fn bend_path(
    n: &AnalyzedNote,
    start: Tick,
    placement: &Placement<'_>,
) -> Vec<fontelle_model::PathPoint> {
    let mut path = Vec::new();
    let mut last = 0i32;
    for (seconds, cents) in &n.curve {
        let semis = (cents / 100.0).round() as i32;
        if semis != last {
            let at = placement.tick(*seconds) - start;
            if at > 0 {
                path.push(fontelle_model::PathPoint {
                    at,
                    offset: semis.clamp(-48, 48) as i8,
                });
            }
            last = semis;
        }
    }
    path
}
