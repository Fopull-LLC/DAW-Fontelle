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
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fontelle_engine::VocalPlayer;
use fontelle_host::{SingerEngine, SingerEngines};
use fontelle_model::{Project, TempoMap, VocalVoice};
use fontelle_types::{ChannelId, ClipId, InstrumentKind, Tick};
use fontelle_ui::document::{SingerChoice, VocalPhraseView, VocalRenderState, VocalView};
use fontelle_vocal::{Phrase, PhraseState, Singer, VocalRenderer, Wanted, phrases_of, table_for};

/// The longest an export waits for the last final take before giving up.
const EXPORT_WAIT: Duration = Duration::from_secs(600);

/// One vocal channel's part, as last worked out.
struct Part {
    phrases: Vec<Phrase>,
    /// Parallel to `phrases`: what is asked of the engine, when there is one.
    wanted: Vec<Option<Wanted>>,
    /// Why it cannot sing, when it cannot.
    problem: Option<String>,
}

pub(crate) struct VocalSession {
    folders: Option<Vec<PathBuf>>,
    engines: OnceLock<Arc<SingerEngines>>,
    renderer: Option<Arc<Mutex<VocalRenderer>>>,
    /// One per vocal channel, outliving graphs; what the graph's nodes read.
    pub players: HashMap<ChannelId, Arc<VocalPlayer>>,
    parts: HashMap<ChannelId, Part>,
    revision: u64,
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
            engines: OnceLock::new(),
            renderer: None,
            players: HashMap::new(),
            parts: HashMap::new(),
            revision: 0,
        }
    }

    /// Loads the engines from `folders` rather than Fontelle's own — for
    /// tests, and for a settings page that names another folder.
    pub fn set_folders(&mut self, folders: Vec<PathBuf>) {
        self.folders = Some(folders);
        self.engines = OnceLock::new();
        self.parts.clear();
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

    /// The voice a new vocal track starts with: the first singer that is
    /// not research-only (else the first), in its own first language.
    pub fn default_voice(&self) -> Option<VocalVoice> {
        let engines = self.engines();
        let pick = |research: bool| {
            engines.engines().iter().find_map(|e| {
                e.singers()
                    .iter()
                    .find(|s| s.research_only == research)
                    .map(|s| (e, s))
            })
        };
        let (engine, singer) = pick(false).or_else(|| pick(true))?;
        Some(VocalVoice::new(
            engine.name(),
            singer.id.clone(),
            singer
                .languages
                .first()
                .cloned()
                .unwrap_or_else(|| "en".into()),
        ))
    }

    pub fn singers(&self) -> Vec<SingerChoice> {
        self.engines()
            .engines()
            .iter()
            .flat_map(|e| {
                e.singers().iter().map(move |s| SingerChoice {
                    engine: e.name().to_string(),
                    id: s.id.clone(),
                    name: s.name.clone(),
                    languages: s.languages.clone(),
                    research_only: s.research_only,
                })
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

    fn engine(&self, voice: &VocalVoice) -> Option<Arc<SingerEngine>> {
        self.engines().engine(&voice.engine).cloned()
    }

    fn renderer(&mut self) -> Arc<Mutex<VocalRenderer>> {
        Arc::clone(self.renderer.get_or_insert_with(|| {
            // Leave room for the audio thread and the window.
            let workers = std::thread::available_parallelism()
                .map_or(2, |n| n.get().saturating_sub(2))
                .clamp(1, 4);
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
            let phrases = phrases_of(project, id, tempo);
            let (engine, problem) = match &voice {
                None if self.engines().is_empty() => (None, Some(self.no_engine_message())),
                None => (None, Some("Choose a singer for this track".to_string())),
                Some(v) => match self.engine(v) {
                    Some(engine) => (Some(engine), None),
                    None => (
                        None,
                        Some(if self.engines().is_empty() {
                            self.no_engine_message()
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
                    engine.as_ref().map(|e| Wanted {
                        singer: Arc::clone(e) as Arc<dyn Singer>,
                        spec: p.spec.clone(),
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
            let renderer = self.renderer();
            if let Ok(mut r) = renderer.lock() {
                r.want(all);
            }
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

    pub fn busy(&self) -> bool {
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
        let singer = voice
            .as_ref()
            .and_then(|v| self.engine(v).and_then(|e| e.singer(&v.singer).cloned()));
        view.caption = match (&voice, &singer) {
            (Some(v), Some(s)) => format!("{} \u{b7} {}", s.name, v.language),
            (Some(v), None) => format!("{} \u{b7} {}", v.singer, v.language),
            (None, _) => "No singer".to_string(),
        };
        view.research_only = singer.as_ref().is_some_and(|s| s.research_only);
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
            for (index, phonemes) in &take.phonemes {
                if let Some((_, n)) = mine.iter().find(|(i, _)| i == index) {
                    view.phonemes.push((n.note, phonemes.join(" ")));
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
                self.engine(v)
                    .and_then(|e| e.singer(&v.singer).map(|s| s.name.clone()))
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
