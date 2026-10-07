//! The studio's vocal tracks: the engines installed, the phrases each vocal
//! channel sings, the scheduler singing them, and the players the graph's
//! `VocalNode`s read.
//!
//! The session calls [`VocalSession::sync`] after every edit (the phrases
//! and what is wanted follow the song) and [`VocalSession::pump`] every frame
//! (takes that landed are published, the playhead steers the order). Nothing
//! here is on the audio thread; the node only reads the tables published to
//! its [`VocalPlayer`].

use std::collections::HashMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use fontelle_engine::VocalPlayer;
use fontelle_host::{EditorTake, PluginWindow, SingerEditor, SingerEngines};
use fontelle_model::{ClipSource, Project, TempoMap, VocalVoice};
use fontelle_sing::{Lane, Transport};
use fontelle_types::{ChannelId, ClipId, InstrumentKind, NoteId, Tick};
use fontelle_ui::document::{
    CatalogueRow, SingerChoice, SingerKind, VocalPhraseView, VocalRenderState, VocalView,
    VoicebankRow,
};
use fontelle_vocal::catalogue::{Catalogue, DownloadWatch, Get};
use fontelle_vocal::{
    BuiltinBackend, Phrase, PhraseState, SingerRegistry, VocalRenderer, Wanted, phrases_with,
    table_for, voicebanks,
};

/// The longest an export waits for the last final take before giving up.
const EXPORT_WAIT: Duration = Duration::from_secs(600);

/// How long after the last edit the engines are asked: a drag, or typing
/// through a line, is many edits, and a phrase cancelled on every one of
/// them is never sung. Engines take a second or two a phrase, so a fifth of
/// a second of patience costs nothing anybody hears.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// One vocal channel's part, as last worked out.
struct Part {
    phrases: Vec<Phrase>,
    /// Parallel to `phrases`: what is asked of the engine, when there is one.
    wanted: Vec<Option<Wanted>>,
    /// Why it cannot sing, when it cannot.
    problem: Option<String>,
}

/// An engine's window, open on one vocal part.
struct EngineWindow {
    channel: ChannelId,
    clip: ClipId,
    /// Declared before `window`: the engine lets go of the window first.
    editor: SingerEditor,
    window: PluginWindow,
}

/// What an engine's window asked of the session during a frame.
#[derive(Default)]
pub(crate) struct EditorAsks {
    /// One command a batch, each one undo step.
    pub commands: Vec<Box<dyn fontelle_model::Command>>,
    pub transport: Vec<Transport>,
    pub select: Option<Vec<NoteId>>,
    /// Batches that could not be made (the part changed under them).
    pub refused: Vec<String>,
}

pub(crate) struct VocalSession {
    folders: Option<Vec<PathBuf>>,
    /// The voicebanks folder, when not Fontelle's own (tests).
    voicebanks: Option<PathBuf>,
    /// The built-in runtimes, when not this build's own (tests).
    builtins: Option<Vec<BuiltinBackend>>,
    engines: OnceLock<Arc<SingerEngines>>,
    registry: OnceLock<Arc<SingerRegistry>>,
    /// The engine window, while one is open.
    editor: Option<EngineWindow>,
    /// Open engine windows on no screen (tests).
    pub headless_editors: bool,
    /// The roll's selection, for the window.
    pub selection: Vec<NoteId>,
    /// Singers' terms agreed to: `engine \u{1f} singer` → hash of the terms.
    agreed: OnceLock<Mutex<HashMap<String, String>>>,
    /// Where the Find singers index is read from, when not the default.
    catalogue_path: Option<PathBuf>,
    /// Where Get it watches for downloads, when not the desktop's own.
    downloads: Option<PathBuf>,
    /// Pages are not opened in a browser (tests).
    pub quiet_pages: bool,
    /// A Get it waiting for its file.
    watch: Option<(String, DownloadWatch)>,
    /// An Install downloading on a thread.
    fetching: Option<(String, std::thread::JoinHandle<Result<PathBuf, String>>)>,
    /// What happened, for the status line.
    pub messages: Vec<String>,
    renderer: Option<Arc<Mutex<VocalRenderer>>>,
    /// One per vocal channel, outliving graphs; what the graph's nodes read.
    pub players: HashMap<ChannelId, Arc<VocalPlayer>>,
    parts: HashMap<ChannelId, Part>,
    revision: u64,
    /// What to ask of the renderer once the edits stop — see [`DEBOUNCE`].
    pending: Option<(Vec<Wanted>, Instant)>,
}

/// Everything an export needs to wait for the final takes and hand them to
/// its own graph's players, off the session's thread.
pub(crate) struct VocalRenderPrep {
    renderer: Arc<Mutex<VocalRenderer>>,
    jobs: Vec<(Arc<VocalPlayer>, Vec<Wanted>)>,
}

impl VocalRenderPrep {
    /// **Blocks** until every phrase has its final take (or failed), then
    /// publishes the takes to the export's players. `progress` hears
    /// `(done, total)` as phrases land.
    pub fn wait_and_publish(&self, progress: &mut dyn FnMut(usize, usize)) -> Result<(), String> {
        let all: Vec<&Wanted> = self.jobs.iter().flat_map(|(_, w)| w.iter()).collect();
        let total = all.len();
        let deadline = Instant::now() + EXPORT_WAIT;
        let mut last = usize::MAX;
        loop {
            // Locked briefly, so the window's own polling is never held up.
            let done = {
                let Ok(mut r) = self.renderer.lock() else {
                    return Err("the vocal renderer stopped".to_string());
                };
                r.poll();
                all.iter()
                    .filter(|w| matches!(r.state(w), PhraseState::Final | PhraseState::Failed(_)))
                    .count()
            };
            if done != last {
                progress(done, total);
                last = done;
            }
            if done == total {
                break;
            }
            if Instant::now() > deadline {
                return Err(format!(
                    "{} vocal phrases were still being sung after {} minutes",
                    total - done,
                    EXPORT_WAIT.as_secs() / 60
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let r = self
            .renderer
            .lock()
            .map_err(|_| "the vocal renderer stopped".to_string())?;
        for (player, wanted) in &self.jobs {
            player.publish(table_for(&r, wanted));
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.iter().all(|(_, w)| w.is_empty())
    }
}

fn is_vocal(project: &Project, id: ChannelId) -> bool {
    project
        .channels
        .get(id)
        .is_some_and(|c| c.instrument == Some(InstrumentKind::Vocal))
}

impl VocalSession {
    pub fn new() -> Self {
        Self {
            folders: None,
            voicebanks: None,
            builtins: None,
            engines: OnceLock::new(),
            registry: OnceLock::new(),
            editor: None,
            headless_editors: false,
            selection: Vec::new(),
            agreed: OnceLock::new(),
            catalogue_path: None,
            downloads: None,
            quiet_pages: false,
            watch: None,
            fetching: None,
            messages: Vec::new(),
            renderer: None,
            players: HashMap::new(),
            parts: HashMap::new(),
            revision: 0,
            pending: None,
        }
    }

    /// Loads the engines from `folders` rather than Fontelle's own — for
    /// tests, and for a settings page that names another folder.
    pub fn set_folders(&mut self, folders: Vec<PathBuf>) {
        self.folders = Some(folders);
        self.engines = OnceLock::new();
        self.registry = OnceLock::new();
        self.editor = None;
        self.parts.clear();
    }

    /// The voicebanks folder and the built-in runtimes over it, rather than
    /// this build's own — for tests.
    pub fn set_voicebanks(&mut self, folder: PathBuf, builtins: Vec<BuiltinBackend>) {
        self.voicebanks = Some(folder);
        self.builtins = Some(builtins);
        self.registry = OnceLock::new();
        self.agreed = OnceLock::new();
        self.parts.clear();
    }

    /// Where Find singers reads its index and Get it watches for files.
    pub fn set_catalogue(&mut self, index: Option<PathBuf>, downloads: Option<PathBuf>) {
        self.catalogue_path = index;
        self.downloads = downloads;
    }

    pub fn voicebanks_dir(&self) -> PathBuf {
        self.voicebanks
            .clone()
            .or_else(voicebanks::voicebanks_dir)
            .unwrap_or_else(|| PathBuf::from("voicebanks"))
    }

    /// Everyone who can sing, made the first time anything asks.
    pub fn registry(&self) -> &Arc<SingerRegistry> {
        self.registry.get_or_init(|| {
            let builtins = self
                .builtins
                .clone()
                .unwrap_or_else(fontelle_vocal::builtin_backends);
            Arc::new(SingerRegistry::new(
                &self.voicebanks_dir(),
                builtins,
                Arc::clone(self.engines()),
            ))
        })
    }

    /// Reads the voicebanks again (after an install or an uninstall).
    fn rescan(&mut self) {
        self.registry = OnceLock::new();
        self.revision += 1;
    }

    /// The engines, loaded the first time anything asks — loading one runs
    /// its code, which nothing should pay for until a vocal track is near.
    pub fn engines(&self) -> &Arc<SingerEngines> {
        self.engines.get_or_init(|| {
            let folders = self
                .folders
                .clone()
                .unwrap_or_else(fontelle_host::singer_search_paths);
            let engines = SingerEngines::load(&folders);
            for failure in &engines.failures {
                eprintln!(
                    "Fontelle: singer engine {} did not load: {}",
                    failure.path.display(),
                    failure.why
                );
            }
            Arc::new(engines)
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The voice a new vocal track starts with: the first free voicebank,
    /// else the first engine singer that is not research-only, else any.
    pub fn default_voice(&self) -> Option<VocalVoice> {
        self.registry().default_voice()
    }

    fn agreed(&self) -> &Mutex<HashMap<String, String>> {
        self.agreed.get_or_init(|| {
            let map = std::fs::read_to_string(self.voicebanks_dir().join(AGREED_FILE))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            Mutex::new(map)
        })
    }

    /// Whether `terms` for `engine`'s `singer` have been agreed to here.
    pub fn terms_agreed(&self, engine: &str, singer: &str, terms: Option<&str>) -> bool {
        let Some(terms) = terms else {
            return true;
        };
        self.agreed()
            .lock()
            .is_ok_and(|m| m.get(&agree_key(engine, singer)) == Some(&terms_hash(terms)))
    }

    /// Records that the singer's current terms were agreed to.
    pub fn agree(&mut self, engine: &str, singer: &str) {
        let terms = self
            .registry()
            .voices()
            .into_iter()
            .find(|v| v.backend == engine && v.info.id == singer)
            .and_then(|v| v.info.terms);
        let Some(terms) = terms else {
            return;
        };
        let dir = self.voicebanks_dir();
        if let Ok(mut map) = self.agreed().lock() {
            map.insert(agree_key(engine, singer), terms_hash(&terms));
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(text) = serde_json::to_string_pretty(&*map) {
                let _ = std::fs::write(dir.join(AGREED_FILE), text);
            }
        }
        self.revision += 1;
    }

    pub fn singers(&self) -> Vec<SingerChoice> {
        self.registry()
            .voices()
            .into_iter()
            .map(|v| SingerChoice {
                terms_agreed: self.terms_agreed(&v.backend, &v.info.id, v.info.terms.as_deref()),
                engine: v.backend,
                engine_name: v.backend_name,
                id: v.info.id,
                name: v.info.name,
                languages: v.info.languages,
                research_only: v.info.research_only,
                kind: match v.kind {
                    fontelle_vocal::SingerKind::Utau => SingerKind::Utau,
                    fontelle_vocal::SingerKind::DiffSinger => SingerKind::DiffSinger,
                    fontelle_vocal::SingerKind::Engine => SingerKind::Engine,
                },
                has_editor: v.has_editor,
                image: v.info.image,
                terms: v.info.terms,
            })
            .collect()
    }

    pub fn problems(&self) -> Vec<String> {
        self.engines()
            .failures
            .iter()
            .map(|f| format!("{}: {}", f.path.display(), f.why))
            .collect()
    }

    /// The display name of who sings `voice`, if they are installed.
    fn singer_name(&self, voice: &VocalVoice) -> Option<(String, bool)> {
        let backend = self.registry().backend(&voice.engine)?;
        backend
            .voices()
            .into_iter()
            .find(|v| v.id == voice.singer)
            .map(|v| (v.name, v.research_only))
    }

    fn renderer(&mut self) -> Arc<Mutex<VocalRenderer>> {
        Arc::clone(self.renderer.get_or_insert_with(|| {
            // Leave room for the audio thread and the window.
            // Two at most: each worker holds an engine instance of its own,
            // and an engine's voice can be large.
            let workers = std::thread::available_parallelism()
                .map_or(1, |n| n.get().saturating_sub(2))
                .clamp(1, 2);
            Arc::new(Mutex::new(VocalRenderer::new(workers)))
        }))
    }

    /// The vocal channels that have no player yet: the graph needs a
    /// rebuild before they can be heard.
    pub fn needs_players(&self, project: &Project) -> bool {
        project.channels.iter().any(|(id, c)| {
            c.instrument == Some(InstrumentKind::Vocal) && !self.players.contains_key(&id)
        })
    }

    /// After an edit: every vocal channel's phrases again, what is wanted of
    /// the engines, and the tables republished.
    pub fn sync(&mut self, project: &Project, tempo: &TempoMap) {
        let vocal: Vec<ChannelId> = project
            .channels
            .keys()
            .filter(|id| is_vocal(project, *id))
            .collect();
        self.players.retain(|id, _| is_vocal(project, *id));
        if vocal.is_empty() && self.parts.is_empty() {
            return;
        }
        let mut parts = HashMap::new();
        for id in vocal {
            let voice = project.channels[id].vocal.clone();
            let registry = Arc::clone(self.registry());
            let caps = voice
                .as_ref()
                .map(|v| registry.capabilities(v))
                .unwrap_or_default();
            let phrases = phrases_with(project, id, tempo, &caps);
            let nobody = registry.voices().is_empty();
            let (backend, problem) = match &voice {
                None if nobody => (None, Some(self.no_engine_message())),
                None => (None, Some("Choose a singer for this track".to_string())),
                Some(v) => match registry.backend(&v.engine) {
                    Some(backend) => (Some(backend), None),
                    None => (
                        None,
                        Some(if nobody {
                            self.no_engine_message()
                        } else if SingerRegistry::is_builtin_key(&v.engine) {
                            format!(
                                "{} is not installed here, so this track is silent \u{2014} \
                                 install the voicebank from Voicebanks",
                                v.singer
                            )
                        } else {
                            format!(
                                "{} is not installed here, so this track is silent",
                                v.engine
                            )
                        }),
                    ),
                },
            };
            let wanted = phrases
                .iter()
                .map(|p| {
                    backend.as_ref().map(|b| Wanted {
                        backend: Arc::clone(b),
                        phrase: p.sing.clone(),
                        start: p.start,
                        end: p.end,
                    })
                })
                .collect();
            parts.insert(
                id,
                Part {
                    phrases,
                    wanted,
                    problem,
                },
            );
        }
        self.parts = parts;
        let all: Vec<Wanted> = self
            .parts
            .values()
            .flat_map(|p| p.wanted.iter().flatten().cloned())
            .collect();
        if !all.is_empty() || self.renderer.is_some() {
            // Asked once the edits settle (`pump`); what is already sung
            // plays straight away from the tables published below.
            self.renderer();
            self.pending = Some((all, Instant::now()));
        }
        self.publish();
        self.revision += 1;
    }

    fn publish(&mut self) {
        let Some(renderer) = &self.renderer else {
            for player in self.players.values() {
                player.publish(fontelle_engine::VocalTable::empty());
            }
            return;
        };
        let Ok(r) = renderer.lock() else {
            return;
        };
        for (id, player) in &self.players {
            let wanted: Vec<Wanted> = self
                .parts
                .get(id)
                .map(|p| p.wanted.iter().flatten().cloned().collect())
                .unwrap_or_default();
            player.publish(table_for(&r, &wanted));
        }
    }

    /// Every frame: the playhead steers the order; takes that landed are
    /// published; replaced tables are freed. True when the view changed.
    pub fn pump(&mut self, playhead_seconds: Option<f64>) -> bool {
        for player in self.players.values() {
            player.reclaim();
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= DEBOUNCE)
        {
            self.flush();
        }
        let Some(renderer) = &self.renderer else {
            return false;
        };
        let landed = match renderer.lock() {
            Ok(mut r) => {
                if let Some(at) = playhead_seconds {
                    r.set_playhead(at);
                }
                r.poll()
            }
            Err(_) => 0,
        };
        if landed > 0 {
            self.publish();
            self.revision += 1;
            return true;
        }
        false
    }

    /// Asks the renderer for what the last edit wants, now.
    fn flush(&mut self) {
        let Some((all, _)) = self.pending.take() else {
            return;
        };
        if let Some(renderer) = &self.renderer
            && let Ok(mut r) = renderer.lock()
        {
            r.want(all);
        }
    }

    pub fn busy(&self) -> bool {
        if self.pending.is_some() {
            return true;
        }
        self.renderer
            .as_ref()
            .is_some_and(|r| r.lock().is_ok_and(|r| r.busy()))
    }

    fn state_of(&self, renderer: &VocalRenderer, wanted: &Option<Wanted>) -> VocalRenderState {
        match wanted {
            None => VocalRenderState::Failed,
            Some(w) => match renderer.state(w) {
                PhraseState::Final => VocalRenderState::Final,
                PhraseState::Preview => VocalRenderState::Preview,
                PhraseState::Failed(_) => VocalRenderState::Failed,
                PhraseState::Waiting | PhraseState::Unwanted => VocalRenderState::Waiting,
            },
        }
    }

    /// What the roll draws over `channel`'s notes in the open clip.
    pub fn view(
        &self,
        project: &Project,
        channel: ChannelId,
        clip: ClipId,
        tempo: &TempoMap,
    ) -> Option<VocalView> {
        if !is_vocal(project, channel) {
            return None;
        }
        let voice = project.channels[channel].vocal.clone();
        let language = voice.as_ref().map_or("en", |v| v.language.as_str());
        let mut view = VocalView {
            default_lyric: fontelle_model::lyrics::default_syllable(language).to_string(),
            ..VocalView::default()
        };
        let singer = voice.as_ref().and_then(|v| self.singer_name(v));
        view.caption = match (&voice, &singer) {
            (Some(v), Some((name, _))) => format!("{name} \u{b7} {}", v.language),
            (Some(v), None) => format!("{} \u{b7} {}", v.singer, v.language),
            (None, _) => "No singer".to_string(),
        };
        view.research_only = singer.as_ref().is_some_and(|(_, r)| *r);
        if let Some(v) = &voice {
            let registry = self.registry();
            view.backend = v.engine.clone();
            view.capabilities = registry.capabilities(v);
            view.editor = registry
                .engine(&v.engine)
                .filter(|e| e.has_editor())
                .map(|e| e.name().to_string());
            view.editor_open = self.editor.as_ref().is_some_and(|e| e.channel == channel);
        }
        if let Some(ClipSource::Notes(data)) = project.clip_source(clip).as_deref() {
            view.part = data.vocal.as_deref().cloned().unwrap_or_default();
        }
        if let Some(c) = project.clips.get(clip) {
            let at = tempo.tick_to_seconds(c.start);
            let sr = tempo.sample_rate_hz();
            let ticks = tempo.sample_to_tick(((at + 1.0) * sr).round() as i64) - c.start;
            view.ticks_per_second = ticks.max(1) as f64;
        }
        let part = self.parts.get(&channel);
        view.message = part.and_then(|p| p.problem.clone());
        let Some(part) = part else {
            return Some(view);
        };
        let Some(clip_data) = project.clips.get(clip) else {
            return Some(view);
        };
        let origin = clip_data.start;
        let period = clip_data
            .loop_length
            .filter(|p| *p > 0)
            .unwrap_or(Tick::MAX);
        let renderer = self.renderer.as_ref().and_then(|r| r.lock().ok());
        let sr = tempo.sample_rate_hz();
        for (phrase, wanted) in part.phrases.iter().zip(&part.wanted) {
            // Only the notes of the open clip's first pass: that is what the
            // roll shows.
            let mine: Vec<_> = phrase
                .notes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.clip == clip && n.start - origin < period)
                .collect();
            if mine.is_empty() {
                continue;
            }
            let start = mine.iter().map(|(_, n)| n.start).min().unwrap_or(0) - origin;
            let end = mine.iter().map(|(_, n)| n.end).max().unwrap_or(0) - origin;
            let state = match &renderer {
                Some(r) => self.state_of(r, wanted),
                None if part.problem.is_some() => VocalRenderState::Failed,
                None => VocalRenderState::Waiting,
            };
            view.phrases.push(VocalPhraseView { start, end, state });
            // The pitch the notes, glides and vibrato make, and the bend
            // lane over it: the line the bend is drawn against.
            {
                let sung = &phrase.sing;
                let from = -fontelle_vocal::CURVE_LEAD;
                let to = sung.notes.last().map_or(0.0, |n| n.end());
                let line = fontelle_sing::note_pitch(&sung.notes, from, to);
                let bend = sung.curves.get(&Lane::PitchBend);
                let mut points = Vec::with_capacity(line.values.len());
                let mut plain = Vec::with_capacity(line.values.len());
                for (i, key) in line.values.iter().enumerate() {
                    let t = from + i as f64 * line.step;
                    let cents = bend.map_or(0.0, |c| c.at(t, 0.0));
                    let tick =
                        tempo.sample_to_tick(((phrase.start + t) * sr).round() as i64) - origin;
                    if tick < start || tick > end {
                        continue;
                    }
                    if points
                        .last()
                        .is_some_and(|(last, _): &(Tick, f32)| *last == tick)
                    {
                        continue;
                    }
                    points.push((tick, key + cents / 100.0));
                    plain.push((tick, *key));
                }
                if points.len() > 1 {
                    view.pitch_line.push(points);
                    view.note_line.push(plain);
                }
            }
            let take = match (&renderer, wanted) {
                (Some(r), Some(w)) => {
                    if let PhraseState::Failed(why) = r.state(w)
                        && view.message.is_none()
                    {
                        view.message = Some(format!("A phrase could not be sung: {why}"));
                    }
                    r.best(w)
                }
                _ => None,
            };
            let Some(take) = take else {
                continue;
            };
            for sung in &take.phonemes {
                if let Some((_, n)) = mine.iter().find(|(i, _)| *i == sung.note) {
                    let symbols: Vec<&str> =
                        sung.phonemes.iter().map(|p| p.symbol.as_str()).collect();
                    view.phonemes.push((n.note, symbols.join(" ")));
                    view.timed.push((
                        n.note,
                        sung.phonemes
                            .iter()
                            .map(|p| (p.symbol.clone(), p.offset))
                            .collect(),
                    ));
                    if sung.guessed {
                        view.guessed.push(n.note);
                    }
                }
            }
            let mut line: Vec<(Tick, f32)> = Vec::new();
            for (i, hz) in take.pitch.iter().enumerate() {
                let seconds = phrase.start + take.start + i as f64 * take.pitch_step;
                let tick = tempo.sample_to_tick((seconds * sr).round() as i64) - origin;
                if *hz <= 0.0 || !hz.is_finite() || tick < start || tick > end {
                    if line.len() > 1 {
                        view.pitch.push(std::mem::take(&mut line));
                    }
                    line.clear();
                    continue;
                }
                let key = 69.0 + 12.0 * (hz / 440.0).log2();
                line.push((tick, key));
            }
            if line.len() > 1 {
                view.pitch.push(line);
            }
        }
        Some(view)
    }

    /// Each vocal clip's stretches still waiting for a first take, in the
    /// clip's own ticks.
    pub fn rendering_spans(&self, project: &Project) -> Vec<(ClipId, Vec<(Tick, Tick)>)> {
        let Some(renderer) = self.renderer.as_ref().and_then(|r| r.lock().ok()) else {
            return Vec::new();
        };
        let mut out: HashMap<ClipId, Vec<(Tick, Tick)>> = HashMap::new();
        for part in self.parts.values() {
            for (phrase, wanted) in part.phrases.iter().zip(&part.wanted) {
                if self.state_of(&renderer, wanted) != VocalRenderState::Waiting {
                    continue;
                }
                for n in &phrase.notes {
                    let Some(clip) = project.clips.get(n.clip) else {
                        continue;
                    };
                    let span = (n.start - clip.start, n.end - clip.start);
                    let spans = out.entry(n.clip).or_default();
                    match spans.last_mut() {
                        Some(last) if last.1 >= span.0 => last.1 = last.1.max(span.1),
                        _ => spans.push(span),
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// The first words a vocal clip sings, for its caption on the
    /// arrangement: "Test Alto · hel-lo world".
    pub fn caption(&self, project: &Project, clip: ClipId) -> Option<String> {
        let found = project.clips.get(clip)?;
        let source = project.clip_source(clip)?;
        let fontelle_model::ClipSource::Notes(data) = source.as_ref() else {
            return None;
        };
        // The clip's own channel if it sings; else the vocal track the clip
        // holds notes for (a clip can hold several instruments).
        let channel = if is_vocal(project, data.channel) {
            data.channel
        } else {
            data.notes
                .values()
                .map(|n| n.channel_or(data.channel))
                .find(|c| is_vocal(project, *c))?
        };
        let _ = found;
        let mut notes: Vec<&fontelle_model::Note> = data
            .notes
            .values()
            .filter(|n| n.channel_or(channel) == channel)
            .collect();
        notes.sort_by_key(|n| n.start);
        let mut words = String::new();
        for note in notes {
            let lyric = note.lyric.trim();
            if lyric.is_empty() || lyric == "-" {
                continue;
            }
            if words.chars().count() > 40 {
                break;
            }
            // A syllable ending in a hyphen joins the next one into a word.
            if let Some(stem) = lyric.strip_suffix('-') {
                words.push_str(stem);
            } else {
                words.push_str(lyric);
                words.push(' ');
            }
        }
        let voice = project.channels[channel].vocal.as_ref();
        let who = voice
            .and_then(|v| {
                self.singer_name(v)
                    .map(|(name, _)| name)
                    .or_else(|| Some(v.singer.clone()))
            })
            .unwrap_or_else(|| project.channels[channel].name.clone());
        let words = words.trim();
        Some(if words.is_empty() {
            who
        } else {
            format!("{who} \u{b7} {words}")
        })
    }

    /// Fresh players for an export's graph, and what fills them.
    pub fn render_prep(
        &mut self,
        project: &Project,
    ) -> (
        HashMap<ChannelId, Arc<VocalPlayer>>,
        Option<VocalRenderPrep>,
    ) {
        // An export does not wait out the debounce.
        self.flush();
        let mut players = HashMap::new();
        let mut jobs = Vec::new();
        for (id, part) in &self.parts {
            if !is_vocal(project, *id) {
                continue;
            }
            let player = VocalPlayer::new();
            players.insert(*id, Arc::clone(&player));
            jobs.push((player, part.wanted.iter().flatten().cloned().collect()));
        }
        let prep = self
            .renderer
            .clone()
            .map(|renderer| VocalRenderPrep { renderer, jobs });
        (players, prep)
    }
}

impl VocalSession {
    /// Where an engine goes: Fontelle's own singers folder (the last of the
    /// search paths, after `FONTELLE_SINGERS`).
    pub fn folder_hint(&self) -> String {
        self.folders
            .clone()
            .unwrap_or_else(fontelle_host::singer_search_paths)
            .last()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    fn no_engine_message(&self) -> String {
        no_engine_message(self.folders.clone())
    }
}

fn no_engine_message(folders: Option<Vec<PathBuf>>) -> String {
    let folder = folders
        .unwrap_or_else(fontelle_host::singer_search_paths)
        .last()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Fontelle's singers folder".to_string());
    format!("No singer engine is installed \u{2014} put one in {folder}")
}

/// Where the agreed singers' terms are recorded, in the voicebanks folder.
const AGREED_FILE: &str = ".agreed-terms.json";

fn agree_key(engine: &str, singer: &str) -> String {
    format!("{engine}\u{1f}{singer}")
}

fn terms_hash(terms: &str) -> String {
    let mut h = twox_hash::XxHash64::with_seed(7);
    h.write(terms.as_bytes());
    format!("{:016x}", h.finish())
}

/// The Find singers index shipped with this build.
const SHIPPED_INDEX: &str = include_str!("../../../docs/singers-index.json");

impl VocalSession {
    // ------------------------------------------------- an engine's window

    /// Opens the window of the engine singing `channel`, on `clip`.
    pub fn open_editor(
        &mut self,
        project: &Project,
        channel: ChannelId,
        clip: ClipId,
    ) -> Result<String, String> {
        let voice = project
            .channels
            .get(channel)
            .and_then(|c| c.vocal.clone())
            .ok_or("this track has no singer")?;
        let engine = self
            .registry()
            .engine(&voice.engine)
            .filter(|e| e.has_editor())
            .cloned()
            .ok_or_else(|| format!("{} has no window of its own", voice.engine))?;
        if let Some(open) = &mut self.editor {
            if open.channel == channel && open.clip == clip {
                open.window.raise();
                return Ok(format!("{} is open", engine.name()));
            }
            self.editor = None;
        }
        let size = fontelle_host::GuiSize {
            width: 900,
            height: 560,
        };
        let mut window = if self.headless_editors {
            PluginWindow::headless(size.width, size.height)
        } else {
            PluginWindow::open(engine.name(), size).map_err(|e| e.to_string())?
        };
        let editor = SingerEditor::open(&engine, window.id())?;
        let (w, h) = editor.size();
        window.resize(fontelle_host::GuiSize {
            width: w,
            height: h,
        });
        self.editor = Some(EngineWindow {
            channel,
            clip,
            editor,
            window,
        });
        self.revision += 1;
        Ok(format!("{} opened", engine.name()))
    }

    pub fn close_editor(&mut self) {
        if self.editor.take().is_some() {
            self.revision += 1;
        }
    }

    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    /// One frame of the engine's window: it is shown the part as it is now
    /// (told when that changed), drawn and handled, and what it asked for
    /// comes back as commands — each batch one undo step.
    pub fn tick_editor(
        &mut self,
        project: &Project,
        tempo: &TempoMap,
        playhead: f64,
        playing: bool,
    ) -> EditorAsks {
        let mut asks = EditorAsks::default();
        let Some(open) = self.editor.as_ref() else {
            return asks;
        };
        let (channel, clip) = (open.channel, open.clip);
        let still_there = project.clips.get(clip).is_some()
            && project
                .channels
                .get(channel)
                .is_some_and(|c| c.vocal.is_some());
        if !still_there {
            self.close_editor();
            return asks;
        }
        // What each note was last sung as, and the takes, for the window.
        let mut sung: HashMap<NoteId, Vec<String>> = HashMap::new();
        let mut takes = Vec::new();
        if let (Some(part), Some(renderer)) = (
            self.parts.get(&channel),
            self.renderer.as_ref().and_then(|r| r.lock().ok()),
        ) {
            for (phrase, wanted) in part.phrases.iter().zip(&part.wanted) {
                let Some(take) = wanted.as_ref().and_then(|w| renderer.best(w)) else {
                    continue;
                };
                for s in &take.phonemes {
                    if let Some(n) = phrase.notes.get(s.note) {
                        sung.insert(
                            n.note,
                            s.phonemes.iter().map(|p| p.symbol.clone()).collect(),
                        );
                    }
                }
                takes.push(EditorTake {
                    start: phrase.start,
                    end: phrase.end,
                    sample_rate: take.sample_rate,
                    take_start: take.start,
                    samples: Arc::clone(&take.samples),
                    pitch: Arc::clone(&take.pitch),
                    pitch_step: take.pitch_step,
                    phonemes: take.phonemes.clone(),
                });
            }
        }
        let selection = self.selection.clone();
        let view = crate::vocal_part::part_view(
            project,
            tempo,
            channel,
            clip,
            &selection,
            playhead,
            playing,
            &|id| sung.get(&id).cloned().unwrap_or_default(),
        );
        let caps = project
            .channels
            .get(channel)
            .and_then(|c| c.vocal.as_ref())
            .map(|v| self.registry().capabilities(v))
            .unwrap_or_default();
        let Some(open) = self.editor.as_mut() else {
            return asks;
        };
        open.editor.set_takes(takes);
        if let Some(view) = &view {
            open.editor.set_part(view);
        }
        let poll = open.window.poll();
        if let Some(size) = poll.resized {
            open.editor.resized(size.width, size.height);
        }
        let asked = open.editor.tick();
        if poll.closed {
            self.close_editor();
        }
        asks.transport = asked.transport;
        for batch in &asked.edits {
            match crate::vocal_part::edits_to_command(project, tempo, channel, clip, batch, &caps) {
                Ok(applied) => {
                    if let Some(command) = applied.command {
                        asks.commands.push(command);
                    }
                    if applied.select.is_some() {
                        asks.select = applied.select;
                    }
                }
                Err(why) => asks.refused.push(why),
            }
        }
        asks
    }

    // ------------------------------------------------------ voicebanks

    pub fn voicebank_rows(&self) -> Vec<VoicebankRow> {
        let dir = self.voicebanks_dir();
        voicebanks::list(&dir)
            .into_iter()
            .chain(voicebanks::dependencies(&dir))
            .map(|b| VoicebankRow {
                id: if b.kind == voicebanks::PackageKind::Dependency {
                    format!("{}/{}", voicebanks::DEPENDENCIES, b.id)
                } else {
                    b.id.clone()
                },
                name: b.name.clone(),
                kind: b.kind.label().to_string(),
                author: b.info.author.clone(),
                portrait: b.info.portrait.as_ref().map(|p| p.display().to_string()),
                terms_agreed: voicebanks::terms_acknowledged(&b.path),
                terms: b.terms.clone(),
                path: b.path.display().to_string(),
            })
            .collect()
    }

    pub fn install(&mut self, path: &Path) -> Result<String, String> {
        let dir = self.voicebanks_dir();
        let bank = voicebanks::install(path, &dir).map_err(|e| e.to_string())?;
        self.rescan();
        Ok(match (&bank.kind, &bank.terms) {
            (voicebanks::PackageKind::Dependency, _) => {
                format!("Installed {} (a dependency)", bank.name)
            }
            (kind, Some(_)) => format!(
                "Installed {} ({}) \u{2014} read its terms in Voicebanks before it sings",
                bank.name,
                kind.label()
            ),
            (kind, None) => format!("Installed {} ({})", bank.name, kind.label()),
        })
    }

    pub fn uninstall(&mut self, id: &str) -> Result<String, String> {
        voicebanks::uninstall(&self.voicebanks_dir(), id)?;
        self.rescan();
        Ok(format!("Removed {id}"))
    }

    pub fn agree_bank(&mut self, id: &str) {
        let path = self.voicebanks_dir().join(id);
        if voicebanks::acknowledge_terms(&path).is_ok() {
            self.revision += 1;
        }
    }

    // ---------------------------------------------------- find singers

    fn catalogue(&self) -> Catalogue {
        let local = self
            .catalogue_path
            .clone()
            .or_else(|| fontelle_host::fontelle_data_dir().map(|d| d.join("singers-index.json")));
        local
            .filter(|p| p.is_file())
            .and_then(|p| Catalogue::load(&p).ok())
            .or_else(|| Catalogue::parse(SHIPPED_INDEX).ok())
            .unwrap_or_default()
    }

    pub fn catalogue_rows(&self) -> Vec<CatalogueRow> {
        let perm = |p: fontelle_vocal::catalogue::Permission| {
            use fontelle_vocal::catalogue::Permission as P;
            match p {
                P::Yes => "yes",
                P::No => "no",
                P::Ask => "ask",
                P::Unknown => "?",
            }
            .to_string()
        };
        self.catalogue()
            .singers
            .into_iter()
            .map(|s| CatalogueRow {
                direct: matches!(s.get(), Get::Download(_)),
                waiting: self.watch.as_ref().is_some_and(|(id, _)| *id == s.id)
                    || self.fetching.as_ref().is_some_and(|(id, _)| *id == s.id),
                id: s.id.clone(),
                name: s.name.clone(),
                kind: s.kind.label().to_string(),
                languages: s.languages.clone(),
                author: s.author.clone(),
                portrait: s.portrait.clone(),
                sample: s.sample.clone(),
                terms_summary: s.terms.summary.clone(),
                terms_url: s.terms.url.clone(),
                commercial: perm(s.terms.commercial),
                ai_training: perm(s.terms.ai_training),
                credit: perm(s.terms.credit),
                page: s.page.clone(),
            })
            .collect()
    }

    /// Install (fetch on a thread, check, install) or Get it (open the page,
    /// watch Downloads).
    pub fn catalogue_get(&mut self, id: &str) -> Result<String, String> {
        let singer = self
            .catalogue()
            .singers
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("{id} is not in the catalogue"))?;
        match singer.get() {
            Get::Download(download) => {
                let into = self.voicebanks_dir().join(".downloads");
                std::fs::create_dir_all(&into).map_err(|e| e.to_string())?;
                let file = into.join(format!("{}.zip", id.replace(['/', '\\'], "_")));
                let worker = std::thread::spawn(move || {
                    let body = crate::updates::fetch_with_progress(&download.url, &mut |_, _| {})?;
                    std::fs::write(&file, body).map_err(|e| e.to_string())?;
                    fontelle_vocal::catalogue::verify(&file, &download.sha256, download.size)?;
                    Ok(file)
                });
                self.fetching = Some((id.to_string(), worker));
                Ok(format!("Downloading {}\u{2026}", singer.name))
            }
            Get::Page(page) => {
                let downloads = self
                    .downloads
                    .clone()
                    .or_else(fontelle_vocal::catalogue::downloads_dir)
                    .ok_or("there is no Downloads folder to watch")?;
                if !self.quiet_pages {
                    crate::desktop::open_url(&page)?;
                }
                let sha = singer.download.as_ref().map(|d| d.sha256.clone());
                self.watch = Some((
                    id.to_string(),
                    DownloadWatch::new(&downloads, SystemTime::now(), sha),
                ));
                Ok(format!(
                    "Download {} from its page \u{2014} it installs when it lands in {}",
                    singer.name,
                    downloads.display()
                ))
            }
        }
    }

    /// Every frame: a Get it's file landing, an Install's download done.
    pub fn pump_installs(&mut self) {
        if let Some((_, watch)) = &mut self.watch
            && let Some(file) = watch.poll()
        {
            self.watch = None;
            let said = self.install(&file).unwrap_or_else(|e| e);
            self.messages.push(said);
        }
        if self.fetching.as_ref().is_some_and(|(_, h)| h.is_finished())
            && let Some((_, worker)) = self.fetching.take()
        {
            let said = match worker.join() {
                Ok(Ok(file)) => {
                    let said = self.install(&file).unwrap_or_else(|e| e);
                    let _ = std::fs::remove_file(&file);
                    said
                }
                Ok(Err(why)) => format!("The download failed: {why}"),
                Err(_) => "The download stopped".to_string(),
            };
            self.messages.push(said);
        }
    }
}
