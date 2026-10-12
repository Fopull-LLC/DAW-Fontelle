//! The [`fontelle_sing::Backend`] over folders of UTAU voicebanks.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fontelle_sing::{
    Backend, Capabilities, Lane, NoteOptionInfo, Phrase, Quality, Session, SettingInfo, SingError,
    Sung, SungNote, SungPhoneme, VoiceInfo,
};
use rayon::prelude::*;

use crate::bank::{self, Voicebank};
use crate::cache::{self, AnalysisCache};
use crate::phonemize::{self, Phonemizer};
use crate::render::{self, UnitSource, Voicing};
use crate::world::{self, FRAME_PERIOD};

/// The rate a final take is sung at; a preview is sung at half of it.
pub const OUTPUT_RATE: u32 = 44_100;
/// Setting: which phonemizer (0 automatic; see
/// [`crate::phonemize::Phonemizer::setting_value`]).
pub const SETTING_PHONEMIZER: &str = "phonemizer";
/// Setting: how much of the recording's own pitch movement is kept, %.
pub const SETTING_MODULATION: &str = "modulation";
/// Setting: how much of a singer's own expression is added to the written
/// pitch (a scoop into a phrase, overshoot after a leap, a late vibrato on
/// long notes, a small fall at the end), %.
pub const SETTING_EXPRESSION: &str = "expression";
/// Setting: how strongly breaths are taken before phrases and releases
/// aspirated, %.
pub const SETTING_BREATH: &str = "breath";
/// The expression setting's default, %: on, as a singer sings.
pub const DEFAULT_EXPRESSION: f32 = 100.0;
/// The breath setting's default, %.
pub const DEFAULT_BREATH: f32 = 60.0;
/// Note option: the voice colour.
pub const NOTE_OPTION_COLOUR: &str = "colour";

/// Bytes of sample analyses kept in memory.
const MEMORY_BUDGET: usize = 768 << 20;

struct Known {
    dir: PathBuf,
    bank: Option<Arc<Voicebank>>,
    kind: Option<Phonemizer>,
}

pub struct UtauBackend {
    roots: Vec<PathBuf>,
    cache: Arc<AnalysisCache>,
    banks: Arc<Mutex<BTreeMap<String, Known>>>,
}

impl UtauBackend {
    /// Voicebanks in (and under) `roots`; analyses kept in `cache_dir`
    /// (see [`cache::default_cache_dir`]), or in memory only.
    pub fn new(roots: Vec<PathBuf>, cache_dir: Option<PathBuf>) -> Self {
        let b = UtauBackend {
            roots,
            cache: Arc::new(AnalysisCache::new(cache_dir, MEMORY_BUDGET)),
            banks: Arc::new(Mutex::new(BTreeMap::new())),
        };
        b.rescan();
        b
    }

    /// The id a bank in `dir` is sung by: `utau/` and its folder's name,
    /// so a song finds its voice on another machine with the bank
    /// installed under the same name.
    pub fn voice_id(dir: &Path) -> String {
        format!(
            "utau/{}",
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    }

    /// Looks for banks again (after an install). Banks already loaded are
    /// kept.
    pub fn rescan(&self) {
        let mut found: Vec<PathBuf> = self.roots.iter().flat_map(|r| bank::scan(r)).collect();
        found.sort();
        let mut banks = self.banks.lock().unwrap_or_else(|e| e.into_inner());
        banks.retain(|_, k| found.contains(&k.dir));
        for dir in found {
            let mut id = Self::voice_id(&dir);
            // Two banks in folders of the same name: the second by its
            // parent too.
            if banks.get(&id).is_some_and(|k| k.dir != dir) {
                let parent = dir
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                id = format!("{id} ({parent})");
            }
            banks.entry(id).or_insert(Known {
                dir,
                bank: None,
                kind: None,
            });
        }
    }

    pub fn bank(&self, voice: &str) -> Result<Arc<Voicebank>, String> {
        load(&self.banks, voice).map(|(b, _)| b)
    }
}

fn load(
    banks: &Mutex<BTreeMap<String, Known>>,
    voice: &str,
) -> Result<(Arc<Voicebank>, Phonemizer), String> {
    let dir = {
        let banks = banks.lock().unwrap_or_else(|e| e.into_inner());
        let k = banks
            .get(voice)
            .ok_or_else(|| format!("no UTAU voicebank {voice}"))?;
        if let (Some(b), Some(kind)) = (&k.bank, k.kind) {
            return Ok((b.clone(), kind));
        }
        k.dir.clone()
    };
    let b = Arc::new(Voicebank::load(&dir)?);
    let kind = phonemize::detect(&b);
    let mut banks = banks.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(k) = banks.get_mut(voice) {
        k.bank = Some(b.clone());
        k.kind = Some(kind);
    }
    Ok((b, kind))
}

impl Backend for UtauBackend {
    fn name(&self) -> &str {
        "Fontelle Singer (UTAU)"
    }

    fn version(&self) -> String {
        format!("{}+analysis{}", env!("CARGO_PKG_VERSION"), RENDER_VERSION)
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        let ids: Vec<String> = self
            .banks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                let (b, kind) = load(&self.banks, &id).ok()?;
                let (lowest, highest) = b.range();
                let lang = kind.language();
                Some(VoiceInfo {
                    id,
                    name: b.name.clone(),
                    languages: vec![lang.to_string()],
                    lowest,
                    highest,
                    research_only: false,
                    terms: b.terms.clone(),
                    image: b.image.as_ref().map(|p| p.to_string_lossy().into_owned()),
                })
            })
            .collect()
    }

    fn capabilities(&self, voice: &str) -> Capabilities {
        let Ok((b, kind)) = load(&self.banks, voice) else {
            return Capabilities::default();
        };
        let mut note_options = Vec::new();
        if !b.colours.is_empty() {
            let mut choices = vec![(String::new(), "Default".to_string())];
            choices.extend(b.colours.iter().map(|c| (c.id.clone(), c.name.clone())));
            note_options.push(NoteOptionInfo {
                id: NOTE_OPTION_COLOUR.into(),
                name: "Voice colour".into(),
                choices,
            });
        }
        Capabilities {
            lanes: vec![
                Lane::PitchBend,
                Lane::Dynamics,
                Lane::Breathiness,
                Lane::Voicing,
                Lane::Tension,
                Lane::Gender,
            ],
            vibrato: true,
            portamento: true,
            velocity: true,
            phoneme_timing: true,
            phonemes: b.base_aliases(),
            extra_lanes: Vec::new(),
            settings: vec![
                SettingInfo {
                    id: SETTING_PHONEMIZER.into(),
                    name: "Phonemizer".into(),
                    min: 0.0,
                    max: Phonemizer::ALL.len() as f32,
                    default: 0.0,
                    unit: String::new(),
                    // 0 is automatic (what the bank's aliases look like),
                    // then each phonemizer at its `setting_value`.
                    choices: std::iter::once(format!("Automatic ({})", kind.name()))
                        .chain(Phonemizer::ALL.iter().map(|p| p.name().to_string()))
                        .collect(),
                },
                SettingInfo {
                    id: SETTING_MODULATION.into(),
                    name: "Modulation".into(),
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    unit: "%".into(),
                    choices: Vec::new(),
                },
                SettingInfo {
                    id: SETTING_EXPRESSION.into(),
                    name: "Expression (scoops, overshoot, a late vibrato on long notes)".into(),
                    min: 0.0,
                    max: 100.0,
                    default: DEFAULT_EXPRESSION,
                    unit: "%".into(),
                },
                SettingInfo {
                    id: SETTING_BREATH.into(),
                    name: "Breath (before phrases, and on the release)".into(),
                    min: 0.0,
                    max: 100.0,
                    default: DEFAULT_BREATH,
                    unit: "%".into(),
                },
            ],
            note_options,
        }
    }

    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(UtauSession {
            banks: self.banks.clone(),
            cache: self.cache.clone(),
        }))
    }
}

/// Bumped when the same phrase would sing differently: part of
/// [`Backend::version`], so cached takes are sung again.
const RENDER_VERSION: u32 = 2;

pub struct UtauSession {
    banks: Arc<Mutex<BTreeMap<String, Known>>>,
    cache: Arc<AnalysisCache>,
}

impl Session for UtauSession {
    fn sing(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        let (b, kind) = load(&self.banks, &phrase.voice).map_err(SingError::Failed)?;
        sing(&b, kind, &self.cache, phrase, quality, seed, cancelled)
    }
}

/// Sings `phrase` with `bank`, analyses through `cache`.
pub fn sing_with(
    bank: &Voicebank,
    cache: &AnalysisCache,
    phrase: &Phrase,
    quality: Quality,
    seed: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Sung, SingError> {
    let detected = phonemize::detect(bank);
    sing(bank, detected, cache, phrase, quality, seed, cancelled)
}

/// [`sing_with`] with the bank's phonemizer already detected.
fn sing(
    bank: &Voicebank,
    detected: Phonemizer,
    cache: &AnalysisCache,
    phrase: &Phrase,
    quality: Quality,
    seed: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Sung, SingError> {
    if cancelled() {
        return Err(SingError::Cancelled);
    }
    let kind = phrase
        .settings
        .get(SETTING_PHONEMIZER)
        .and_then(|v| Phonemizer::from_setting(*v))
        .unwrap_or(detected);
    let plan = phonemize::plan(bank, phrase, kind);

    // Every sample the plan needs, analysed (in parallel when cold).
    let mut files: Vec<PathBuf> = Vec::new();
    for u in &plan.units {
        let Some(n) = phrase.notes.get(u.note) else {
            continue;
        };
        let tone = n.pitch.round().clamp(0.0, 127.0) as u8;
        let colour = n
            .options
            .get(NOTE_OPTION_COLOUR)
            .map(String::as_str)
            .unwrap_or("");
        if let Some(e) = bank.find(&u.alias, tone, colour)
            && !files.contains(&e.file)
        {
            files.push(e.file.clone());
        }
    }
    let setting = |id: &str, default: f32| {
        f64::from(
            phrase
                .settings
                .get(id)
                .copied()
                .unwrap_or(default)
                .clamp(0.0, 100.0),
        ) / 100.0
    };
    let breath = setting(SETTING_BREATH, DEFAULT_BREATH);
    // The breaths a phrase may take, analysed with the rest.
    if breath > 0.0 {
        for n in &phrase.notes {
            let tone = n.pitch.round().clamp(0.0, 127.0) as u8;
            let colour = n
                .options
                .get(NOTE_OPTION_COLOUR)
                .map(String::as_str)
                .unwrap_or("");
            if let Some(e) = render::breath_entry(bank, tone, colour)
                && !files.contains(&e.file)
            {
                files.push(e.file.clone());
            }
        }
    }
    // The cancel flag cannot cross threads, so it is read between batches.
    let mut analysed: Vec<(PathBuf, Option<Arc<UnitSource>>)> = Vec::new();
    let batch = rayon::current_num_threads().max(1);
    for chunk in files.chunks(batch) {
        if cancelled() {
            return Err(SingError::Cancelled);
        }
        analysed.extend(
            chunk
                .par_iter()
                .map(|f| (f.clone(), source(cache, f)))
                .collect::<Vec<_>>(),
        );
    }
    if cancelled() {
        return Err(SingError::Cancelled);
    }
    let sources: HashMap<PathBuf, Arc<UnitSource>> = analysed
        .into_iter()
        .filter_map(|(f, s)| s.map(|s| (f, s)))
        .collect();
    let file_ms = |p: &Path| {
        sources
            .get(p)
            .map(|s| s.analysis.frames().saturating_sub(1) as f64 * FRAME_PERIOD * 1000.0)
    };
    let placed = render::place(bank, &plan, phrase, &file_ms);
    let placed = render::add_breaths(bank, placed, phrase, &file_ms, breath);
    let unit_sources: Vec<&UnitSource> = placed
        .iter()
        .filter_map(|p| sources.get(&p.entry.file).map(|s| s.as_ref()))
        .collect();
    if unit_sources.len() != placed.len() {
        return Err(SingError::Failed("a sample could not be read".into()));
    }

    let (fs, fft_size) = match quality {
        // Half the rate: the envelope's lower half on half the FFT, the
        // same bins in Hz, half the work.
        Quality::Preview => (OUTPUT_RATE / 2, 1024),
        Quality::Final => (OUTPUT_RATE, 2048),
    };
    let voicing = Voicing {
        phrase,
        modulation: setting(SETTING_MODULATION, 0.0),
        expression: setting(SETTING_EXPRESSION, DEFAULT_EXPRESSION),
        breath,
    };
    let (frames, t0) = render::assemble(&placed, &unit_sources, &voicing, fs, fft_size);
    let mut samples =
        world::synthesize(&frames, fs, seed, cancelled).ok_or(SingError::Cancelled)?;
    // Never past full scale: a phrase that would clip is turned down as a
    // whole.
    let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.98 {
        let g = 0.98 / peak;
        samples.iter_mut().for_each(|v| *v *= g);
    }

    let mut phonemes: Vec<SungNote> = (0..phrase.notes.len())
        .map(|i| SungNote {
            note: i,
            phonemes: Vec::new(),
            guessed: plan.guessed.get(i).copied().unwrap_or(false),
        })
        .collect();
    for p in placed.iter().filter(|p| !p.breath) {
        if let Some(n) = phonemes.get_mut(p.note) {
            n.phonemes.push(SungPhoneme {
                symbol: p.alias.clone(),
                offset: Some(p.start - phrase.notes[p.note].start),
            });
        }
    }
    Ok(Sung {
        sample_rate: fs,
        samples,
        start: t0,
        pitch: frames.f0.clone(),
        pitch_step: FRAME_PERIOD,
        phonemes,
    })
}

fn source(cache: &AnalysisCache, f: &Path) -> Option<Arc<UnitSource>> {
    let a = cache.get(f, &cache::options_for(f)).ok()?;
    let mut v: Vec<f32> = a.f0.iter().cloned().filter(|&x| x > 0.0).collect();
    v.sort_by(f32::total_cmp);
    let median = f64::from(v.get(v.len() / 2).copied().unwrap_or(0.0));
    Some(Arc::new(UnitSource {
        analysis: a,
        median_f0: median,
    }))
}
