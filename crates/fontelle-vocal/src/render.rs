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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use fontelle_sing::{Backend, Quality, Session, SingError, Sung, SungNote};

use crate::phrases::{RenderKey, render_key};

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

struct Job {
    key: RenderKey,
    quality: Quality,
    backend: Arc<dyn Backend>,
    phrase: fontelle_sing::Phrase,
    start: f64,
    end: f64,
    cancel: Arc<AtomicU32>,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    running: Vec<(RenderKey, Arc<AtomicU32>)>,
    done: Vec<(RenderKey, Result<Rendered, SingError>)>,
    playhead: f64,
    stop: bool,
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
            (job.quality != Quality::Preview, distance)
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
    cache: HashMap<u64, (Arc<Rendered>, u64)>,
    /// Why a phrase could not be sung, by its content (`RenderKey::seed`):
    /// asked again it is not tried again until something about it changes.
    failed: HashMap<u64, String>,
    wanted: HashSet<u64>,
    stamp: u64,
}

impl VocalRenderer {
    /// A scheduler with `workers` threads, each singing one phrase at a time
    /// through an engine instance of its own.
    pub fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
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
        }
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
        for w in &wanted {
            let preview = w.key(Quality::Preview);
            let fin = w.key(Quality::Final);
            keys.insert(preview.hash);
            keys.insert(fin.hash);
            if self.failed.contains_key(&preview.seed) || self.cache.contains_key(&fin.hash) {
                continue;
            }
            if !self.cache.contains_key(&preview.hash) {
                fresh.push((preview, Quality::Preview, w));
            }
            fresh.push((fin, Quality::Final, w));
        }
        let Ok(mut q) = self.shared.queue.lock() else {
            return;
        };
        q.jobs.retain(|j| keys.contains(&j.key.hash));
        for (key, cancel) in &q.running {
            if !keys.contains(&key.hash) {
                cancel.store(1, Ordering::Relaxed);
            }
        }
        for (key, quality, w) in fresh {
            let queued = q.jobs.iter().any(|j| j.key == key)
                || q.running
                    .iter()
                    .any(|(k, c)| *k == key && c.load(Ordering::Relaxed) == 0);
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
            });
        }
        drop(q);
        self.wanted = keys;
        self.shared.wake.notify_all();
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
                    self.cache.insert(key.hash, (Arc::new(take), self.stamp));
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
            .map(|(k, (r, stamp))| (*k, *stamp, r.samples.len()))
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
            .find_map(|q| self.cache.get(&w.key(q).hash).map(|(r, _)| Arc::clone(r)))
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
            for (_, cancel) in &q.running {
                cancel.store(1, Ordering::Relaxed);
            }
        }
        self.shared.wake.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn work(shared: &Shared) {
    // One session per backend on this thread, made on first use and kept:
    // starting one can be slow (an engine instance, a voicebank's models),
    // and an engine wants one render at a time per instance, which one
    // thread gives.
    let mut sessions: HashMap<String, Box<dyn Session>> = HashMap::new();
    loop {
        let job = {
            let Ok(mut q) = shared.queue.lock() else {
                return;
            };
            loop {
                if q.stop {
                    return;
                }
                if let Some(i) = q.pick() {
                    let job = q.jobs.swap_remove(i);
                    q.running.push((job.key, Arc::clone(&job.cancel)));
                    break job;
                }
                q = match shared.wake.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };
        let id = format!("{}\u{1f}{}", job.backend.name(), job.backend.version());
        let sing = |session: &mut Box<dyn Session>| {
            session.sing_flagged(&job.phrase, job.quality, job.key.seed, &job.cancel)
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
        let Ok(mut q) = shared.queue.lock() else {
            return;
        };
        q.running
            .retain(|(k, c)| !(*k == job.key && Arc::ptr_eq(c, &job.cancel)));
        if let Err(SingError::Failed(_)) = &result {
            // The same words will fail the same way at the other quality.
            q.jobs.retain(|j| j.key.seed != job.key.seed);
        }
        let result = result.map(|sung| Rendered::from((job.quality, sung)));
        q.done.push((job.key, result));
    }
}
