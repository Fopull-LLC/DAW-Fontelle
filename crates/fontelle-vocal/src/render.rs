//! The render scheduler and its cache.
//!
//! Worker threads sing phrases through a `fontelle_sing::Backend`; the app's thread says
//! which phrases it wants ([`VocalRenderer::want`]) and where the playhead is,
//! and collects what was sung ([`VocalRenderer::poll`]). The order is
//! musical: **every phrase's quick preview before any final take**, so an
//! edit is heard soon wherever it is, and within each, the phrase under the
//! playhead, then those ahead of it, then those behind. A phrase that stops
//! being wanted (edited again, deleted) is dropped from the queue, and if it
//! is being sung its cancel flag is raised.
//!
//! Takes are cached by [`RenderKey`]: what is sung, never where, so a phrase
//! moved, a clip duplicated, an undo — all find their take already sung.
//! With a [`TakeStore`] the final takes are also kept on disk (written and
//! read by the render threads), so a reopened song plays at once.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use fontelle_sing::{Backend, Quality, Session, SingError, Sung, SungNote};

use crate::phrases::{RenderKey, render_key};
use crate::takes::TakeStore;

/// A phrase the app wants sung, who sings it, and where it is (for the
/// order).
#[derive(Clone)]
pub struct Wanted {
    pub backend: Arc<dyn Backend>,
    pub phrase: fontelle_sing::Phrase,
    /// Song seconds of its first note and its last note's end.
    pub start: f64,
    pub end: f64,
}

impl Wanted {
    fn key(&self, quality: Quality) -> RenderKey {
        render_key(
            self.backend.name(),
            &self.backend.version(),
            &self.phrase,
            quality,
        )
    }
}

impl std::fmt::Debug for Wanted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wanted")
            .field("backend", &self.backend.name())
            .field("start", &self.start)
            .field("notes", &self.phrase.notes.len())
            .finish()
    }
}

/// A take, cached.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub quality: Quality,
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
    /// Seconds from the phrase's first note to the first sample.
    pub start: f64,
    /// Hz every `pitch_step` seconds from `start`; 0 unvoiced.
    pub pitch: Arc<[f32]>,
    pub pitch_step: f64,
    /// What each note was sung as (index into the phrase's notes).
    pub phonemes: Vec<SungNote>,
}

impl From<(Quality, Sung)> for Rendered {
    fn from((quality, sung): (Quality, Sung)) -> Self {
        Self {
            quality,
            sample_rate: sung.sample_rate,
            samples: sung.samples.into(),
            start: sung.start,
            pitch: sung.pitch.into(),
            pitch_step: sung.pitch_step,
            phonemes: sung.phonemes,
        }
    }
}

/// Where a phrase stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhraseState {
    /// Not asked for.
    Unwanted,
    /// Asked for, nothing sung yet: silence, drawn as rendering.
    Waiting,
    /// The quick take is in; the final one is coming.
    Preview,
    Final,
    /// The engine could not sing it, and said why.
    Failed(String),
}

#[derive(Clone)]
struct Job {
    key: RenderKey,
    quality: Quality,
    backend: Arc<dyn Backend>,
    phrase: fontelle_sing::Phrase,
    start: f64,
    end: f64,
    cancel: Arc<AtomicU32>,
    /// A voice's reference, sung to load it ([`VocalRenderer::warm`]):
    /// after everything else, and its take kept nowhere.
    warm: bool,
}

/// A job a worker is singing, kept whole so a final can step aside for a
/// preview and be queued again.
struct Running {
    key: RenderKey,
    cancel: Arc<AtomicU32>,
    job: Job,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    running: Vec<Running>,
    /// How many worker threads sing.
    workers: usize,
    done: Vec<(RenderKey, Result<Rendered, SingError>)>,
    playhead: f64,
    stop: bool,
    /// Where final takes are kept on disk.
    store: Option<Arc<TakeStore>>,
}

impl Queue {
    /// The next job: previews before finals; then under or ahead of the
    /// playhead, nearest first; then behind it, nearest first.
    fn pick(&self) -> Option<usize> {
        let playhead = self.playhead;
        let rank = |job: &Job| {
            let distance = if job.end > playhead {
                (job.start - playhead).max(0.0)
            } else {
                1e9 + (playhead - job.end)
            };
            (
                job.warm as u8 * 2 + (job.quality != Quality::Preview) as u8,
                distance,
            )
        };
        (0..self.jobs.len()).min_by(|a, b| {
            let (qa, da) = rank(&self.jobs[*a]);
            let (qb, db) = rank(&self.jobs[*b]);
            qa.cmp(&qb).then(da.total_cmp(&db))
        })
    }
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// Takes not wanted any more are kept up to this many samples (about 256 MB
/// of floats), so an undo a while later is still free.
const SPARE_SAMPLES: usize = 64 * 1024 * 1024;

/// The scheduler and the cache. See the module doc.
pub struct VocalRenderer {
    shared: Arc<Shared>,
    workers: Vec<std::thread::JoinHandle<()>>,
    /// Takes by key hash: the take, when it was last stored or used, and
    /// its whole key (for the disk).
    cache: HashMap<u64, (Arc<Rendered>, u64, RenderKey)>,
    /// Why a phrase could not be sung, by its content (`RenderKey::seed`):
    /// asked again it is not tried again until something about it changes.
    failed: HashMap<u64, String>,
    wanted: HashSet<u64>,
    stamp: u64,
    /// Voices warmed already, by backend, version and voice.
    warmed: HashSet<String>,
}

impl VocalRenderer {
    /// A scheduler with `workers` threads, each singing one phrase at a time
    /// through an engine instance of its own.
    pub fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                workers: workers.max(1),
                ..Queue::default()
            }),
            wake: Condvar::new(),
        });
        let workers = (0..workers.max(1))
            .map(|i| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("fontelle-vocal-{i}"))
                    .spawn(move || work(&shared))
                    .expect("a render thread starts")
            })
            .collect();
        Self {
            shared,
            workers,
            cache: HashMap::new(),
            failed: HashMap::new(),
            wanted: HashSet::new(),
            stamp: 0,
            warmed: HashSet::new(),
        }
    }

    /// Where final takes are kept on disk, or nowhere.
    ///
    /// Final takes already sung are written to a new store on a thread of
    /// their own: an unsaved song saved for the first time takes its takes
    /// with it.
    pub fn set_store(&mut self, store: Option<TakeStore>) {
        let store = store.map(Arc::new);
        if let Ok(mut q) = self.shared.queue.lock() {
            let same = match (&q.store, &store) {
                (Some(a), Some(b)) => a.dir() == b.dir(),
                (None, None) => true,
                _ => false,
            };
            if same {
                return;
            }
            q.store = store.clone();
        }
        let Some(store) = store else {
            return;
        };
        let finals: Vec<(RenderKey, Arc<Rendered>)> = self
            .cache
            .values()
            .filter(|(r, _, _)| r.quality == Quality::Final)
            .map(|(r, _, k)| (*k, Arc::clone(r)))
            .collect();
        if finals.is_empty() {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("fontelle-vocal-takes".into())
            .spawn(move || {
                for (key, take) in finals {
                    if !store.has(key) {
                        let _ = store.save(key, &take);
                    }
                }
            });
    }

    /// Where the playhead is, in song seconds: what the order is measured
    /// from.
    pub fn set_playhead(&mut self, seconds: f64) {
        if let Ok(mut q) = self.shared.queue.lock() {
            q.playhead = seconds;
        }
    }

    /// The phrases wanted now — all of them, every time: anything queued or
    /// singing that is not among them is dropped or cancelled.
    pub fn want(&mut self, wanted: Vec<Wanted>) {
        self.poll();
        let mut keys = HashSet::new();
        let mut fresh = Vec::new();
        let store = self.shared.queue.lock().ok().and_then(|q| q.store.clone());
        for w in &wanted {
            let preview = w.key(Quality::Preview);
            let fin = w.key(Quality::Final);
            keys.insert(preview.hash);
            keys.insert(fin.hash);
            if self.failed.contains_key(&preview.seed) || self.cache.contains_key(&fin.hash) {
                continue;
            }
            // A final on disk is read back by a render thread: no preview.
            let on_disk = store.as_ref().is_some_and(|s| s.has(fin));
            if !self.cache.contains_key(&preview.hash) && !on_disk {
                fresh.push((preview, Quality::Preview, w));
            }
            fresh.push((fin, Quality::Final, w));
        }
        let Ok(mut q) = self.shared.queue.lock() else {
            return;
        };
        q.jobs.retain(|j| j.warm || keys.contains(&j.key.hash));
        for r in &q.running {
            if !r.job.warm && !keys.contains(&r.key.hash) {
                r.cancel.store(1, Ordering::Relaxed);
            }
        }
        for (key, quality, w) in fresh {
            let queued = q.jobs.iter().any(|j| j.key == key)
                || q.running
                    .iter()
                    .any(|r| r.key == key && r.cancel.load(Ordering::Relaxed) == 0);
            if queued {
                continue;
            }
            q.jobs.push(Job {
                key,
                quality,
                backend: Arc::clone(&w.backend),
                phrase: w.phrase.clone(),
                start: w.start,
                end: w.end,
                cancel: Arc::new(AtomicU32::new(0)),
                warm: false,
            });
        }
        step_aside(&mut q);
        drop(q);
        self.wanted = keys;
        self.shared.wake.notify_all();
    }

    /// Sings `voice`'s reference phrase (`crate::reference_phrase`) when
    /// nothing else is waiting, once per voice, keeping nothing: its models
    /// are loaded and its level found before its first phrase is asked
    /// for. A voice `backend` does not have is passed over.
    pub fn warm(&mut self, backend: Arc<dyn Backend>, voice: &str) {
        let id = format!("{}\u{1f}{}\u{1f}{voice}", backend.name(), backend.version());
        if self.warmed.contains(&id) {
            return;
        }
        let Some(info) = backend.voices().into_iter().find(|v| v.id == voice) else {
            return;
        };
        self.warmed.insert(id);
        let phrase = crate::reference_phrase(&info);
        let key = render_key(
            backend.name(),
            &backend.version(),
            &phrase,
            Quality::Preview,
        );
        let end = phrase.notes.last().map_or(0.0, |n| n.end());
        if let Ok(mut q) = self.shared.queue.lock() {
            q.jobs.push(Job {
                key,
                quality: Quality::Preview,
                backend,
                phrase,
                start: 0.0,
                end,
                cancel: Arc::new(AtomicU32::new(0)),
                warm: true,
            });
        }
        self.shared.wake.notify_all();
    }

    /// Forgets which phrases could not be sung: something they needed may
    /// have arrived (a voicebank or a vocoder installed), so the next
    /// [`want`](Self::want) tries them again.
    pub fn forget_failures(&mut self) {
        self.failed.clear();
    }

    /// Collects what the workers sang. Returns how many takes landed — the
    /// app republishes the tables when it is not zero.
    pub fn poll(&mut self) -> usize {
        let done = match self.shared.queue.lock() {
            Ok(mut q) => std::mem::take(&mut q.done),
            Err(_) => return 0,
        };
        let mut landed = 0;
        for (key, result) in done {
            match result {
                Ok(take) => {
                    self.stamp += 1;
                    self.cache
                        .insert(key.hash, (Arc::new(take), self.stamp, key));
                    landed += 1;
                }
                Err(SingError::Failed(why)) => {
                    self.failed.insert(key.seed, why);
                    landed += 1;
                }
                Err(SingError::Cancelled) => {}
            }
        }
        if landed > 0 {
            self.evict();
        }
        landed
    }

    fn evict(&mut self) {
        let mut spare: Vec<(u64, u64, usize)> = self
            .cache
            .iter()
            .filter(|(k, _)| !self.wanted.contains(k))
            .map(|(k, (r, stamp, _))| (*k, *stamp, r.samples.len()))
            .collect();
        let mut total: usize = spare.iter().map(|s| s.2).sum();
        if total <= SPARE_SAMPLES {
            return;
        }
        spare.sort_by_key(|s| s.1);
        for (key, _, size) in spare {
            if total <= SPARE_SAMPLES {
                break;
            }
            self.cache.remove(&key);
            total -= size;
        }
    }

    /// The best take of `w` so far: the final, else the preview.
    pub fn best(&self, w: &Wanted) -> Option<Arc<Rendered>> {
        [Quality::Final, Quality::Preview]
            .into_iter()
            .find_map(|q| {
                self.cache
                    .get(&w.key(q).hash)
                    .map(|(r, _, _)| Arc::clone(r))
            })
    }

    pub fn state(&self, w: &Wanted) -> PhraseState {
        let fin = w.key(Quality::Final);
        if self.cache.contains_key(&fin.hash) {
            return PhraseState::Final;
        }
        let preview = w.key(Quality::Preview);
        if let Some(why) = self.failed.get(&preview.seed) {
            return PhraseState::Failed(why.clone());
        }
        if self.cache.contains_key(&preview.hash) {
            return PhraseState::Preview;
        }
        if self.wanted.contains(&fin.hash) {
            PhraseState::Waiting
        } else {
            PhraseState::Unwanted
        }
    }

    /// Whether anything is queued or being sung.
    pub fn busy(&self) -> bool {
        self.shared
            .queue
            .lock()
            .is_ok_and(|q| !q.jobs.is_empty() || !q.running.is_empty())
    }

    /// **Blocks** until every phrase in `wanted` has its final take (or has
    /// failed), telling `progress` how many of how many are done as they
    /// land — what an export waits on. They must already be wanted. Returns
    /// how many failed; `Err` if `timeout` passed first.
    pub fn wait_for_final(
        &mut self,
        wanted: &[Wanted],
        timeout: Duration,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<usize, String> {
        let deadline = Instant::now() + timeout;
        let total = wanted.len();
        let mut last = usize::MAX;
        loop {
            self.poll();
            let mut done = 0;
            let mut failed = 0;
            for w in wanted {
                match self.state(w) {
                    PhraseState::Final => done += 1,
                    PhraseState::Failed(_) => {
                        done += 1;
                        failed += 1;
                    }
                    _ => {}
                }
            }
            if done != last {
                progress(done, total);
                last = done;
            }
            if done == total {
                return Ok(failed);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "{} of {total} phrases were still being sung",
                    total - done
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for VocalRenderer {
    fn drop(&mut self) {
        if let Ok(mut q) = self.shared.queue.lock() {
            q.stop = true;
            q.jobs.clear();
            for r in &q.running {
                r.cancel.store(1, Ordering::Relaxed);
            }
        }
        self.shared.wake.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Previews waiting with every worker busy on a final: as many finals as
/// there are previews waiting step aside — cancelled, and queued again
/// behind them — farthest from the playhead first. An edit is then heard
/// in a preview's time, not after somebody else's final.
fn step_aside(q: &mut Queue) {
    let waiting = q
        .jobs
        .iter()
        .filter(|j| j.quality == Quality::Preview && !j.warm)
        .count();
    let busy: Vec<usize> = (0..q.running.len())
        .filter(|i| q.running[*i].cancel.load(Ordering::Relaxed) == 0)
        .collect();
    let free = q.workers.saturating_sub(busy.len());
    if waiting <= free {
        return;
    }
    let playhead = q.playhead;
    let mut finals: Vec<usize> = busy
        .into_iter()
        // A warming job is loading what the previews need: it stays.
        .filter(|i| q.running[*i].job.quality == Quality::Final && !q.running[*i].job.warm)
        .collect();
    let distance = |job: &Job| {
        if job.end > playhead {
            (job.start - playhead).max(0.0)
        } else {
            1e9 + (playhead - job.end)
        }
    };
    finals.sort_by(|a, b| distance(&q.running[*b].job).total_cmp(&distance(&q.running[*a].job)));
    for i in finals.into_iter().take(waiting - free) {
        let r = &q.running[i];
        r.cancel.store(1, Ordering::Relaxed);
        let mut again = r.job.clone();
        again.cancel = Arc::new(AtomicU32::new(0));
        q.jobs.push(again);
    }
}

fn work(shared: &Shared) {
    // One session per backend on this thread, made on first use and kept:
    // starting one can be slow (an engine instance, a voicebank's models),
    // and an engine wants one render at a time per instance, which one
    // thread gives.
    let mut sessions: HashMap<String, Box<dyn Session>> = HashMap::new();
    loop {
        let (job, store) = {
            let Ok(mut q) = shared.queue.lock() else {
                return;
            };
            loop {
                if q.stop {
                    return;
                }
                if let Some(i) = q.pick() {
                    let job = q.jobs.swap_remove(i);
                    q.running.push(Running {
                        key: job.key,
                        cancel: Arc::clone(&job.cancel),
                        job: job.clone(),
                    });
                    break (job, q.store.clone());
                }
                q = match shared.wake.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };
        // Sung before, and kept: read it back rather than sing it.
        let kept = match (&store, job.quality) {
            (Some(store), Quality::Final) => store.load(job.key),
            _ => None,
        };
        if let Some(take) = kept {
            let Ok(mut q) = shared.queue.lock() else {
                return;
            };
            q.running
                .retain(|r| !(r.key == job.key && Arc::ptr_eq(&r.cancel, &job.cancel)));
            q.done.push((job.key, Ok(take)));
            continue;
        }
        let id = format!("{}\u{1f}{}", job.backend.name(), job.backend.version());
        let seed = if job.warm {
            crate::REFERENCE_SEED
        } else {
            job.key.seed
        };
        let sing = |session: &mut Box<dyn Session>| {
            session.sing_flagged(&job.phrase, job.quality, seed, &job.cancel)
        };
        let result = match sessions.get_mut(&id) {
            Some(session) => sing(session),
            None => match job.backend.session() {
                Ok(mut session) => {
                    let r = sing(&mut session);
                    sessions.insert(id, session);
                    r
                }
                Err(why) => Err(SingError::Failed(why)),
            },
        };
        let result = result.map(|sung| Rendered::from((job.quality, sung)));
        // A final is kept on disk once it is handed over (its samples are
        // shared, not copied), so the window hears it without waiting.
        let keep = match (&store, &result) {
            (Some(store), Ok(take)) if job.quality == Quality::Final => {
                Some((Arc::clone(store), take.clone()))
            }
            _ => None,
        };
        {
            let Ok(mut q) = shared.queue.lock() else {
                return;
            };
            q.running
                .retain(|r| !(r.key == job.key && Arc::ptr_eq(&r.cancel, &job.cancel)));
            if job.warm {
                // Sung to load the voice; nobody asked for the take.
                continue;
            }
            if let Err(SingError::Failed(_)) = &result {
                // The same words will fail the same way at the other quality.
                q.jobs.retain(|j| j.key.seed != job.key.seed);
            }
            q.done.push((job.key, result));
        }
        if let Some((store, take)) = keep {
            let _ = store.save(job.key, &take);
        }
    }
}
