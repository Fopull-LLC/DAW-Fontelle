//! The render scheduler: phrases sung on worker threads, nearest the
//! playhead first, a quick preview of everything before the final takes,
//! a phrase nobody wants any more cancelled mid-render, and a cache that
//! makes a move or an undo free.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fontelle_sing::{
    Backend, Capabilities, Phrase, PhraseNote, Quality, Session, SingError, Sung, SungNote,
    SungPhoneme, VoiceInfo,
};
use fontelle_vocal::{PhraseState, VocalRenderer, Wanted};

const QUALITY_PREVIEW: Quality = Quality::Preview;
const QUALITY_FINAL: Quality = Quality::Final;

/// A singer that sings silence and writes down what it was asked.
struct Fake {
    log: Arc<Mutex<Vec<(String, Quality)>>>,
    delay: Duration,
    /// How long a final takes, when not `delay`.
    final_delay: Option<Duration>,
    started: Arc<AtomicUsize>,
    cancels_seen: Arc<AtomicUsize>,
}

impl Fake {
    fn new(delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            log: Arc::default(),
            delay,
            final_delay: None,
            started: Arc::default(),
            cancels_seen: Arc::default(),
        })
    }
    fn log(&self) -> Vec<(String, Quality)> {
        self.log.lock().unwrap().clone()
    }
}

struct FakeVoice(Arc<Fake>);

/// The fake's handle to itself, so a session can count what it is asked.
struct FakeBackend(Arc<Fake>);

impl Backend for FakeBackend {
    fn name(&self) -> &str {
        "Fake"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        Vec::new()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(FakeVoice(Arc::clone(&self.0))))
    }
}

impl Session for FakeVoice {
    fn sing(
        &mut self,
        _phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        unreachable!("the scheduler hands its flag over")
    }

    fn sing_flagged(
        &mut self,
        spec: &Phrase,
        quality: Quality,
        _seed: u64,
        cancel: &AtomicU32,
    ) -> Result<Sung, SingError> {
        self.0.started.fetch_add(1, Ordering::SeqCst);
        let delay = match (quality, self.0.final_delay) {
            (Quality::Final, Some(d)) => d,
            _ => self.0.delay,
        };
        let until = Instant::now() + delay;
        while Instant::now() < until {
            if cancel.load(Ordering::Relaxed) != 0 {
                self.0.cancels_seen.fetch_add(1, Ordering::SeqCst);
                return Err(SingError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let lyric = spec.notes[0].lyric.clone();
        if lyric == "!fail" {
            return Err(SingError::Failed("no".into()));
        }
        self.0.log.lock().unwrap().push((lyric, quality));
        Ok(Sung {
            sample_rate: 24_000,
            samples: vec![quality_number(quality); 2_400],
            start: -0.05,
            pitch: vec![440.0; 10],
            pitch_step: 0.01,
            phonemes: vec![SungNote {
                note: 0,
                phonemes: ["l", "a"]
                    .iter()
                    .map(|s| SungPhoneme {
                        symbol: s.to_string(),
                        offset: None,
                    })
                    .collect(),
                guessed: false,
            }],
        })
    }
}

fn quality_number(q: Quality) -> f32 {
    match q {
        Quality::Preview => 0.0,
        Quality::Final => 1.0,
    }
}

fn phrase(engine: &Arc<Fake>, at: f64, lyric: &str) -> Wanted {
    Wanted {
        backend: Arc::new(FakeBackend(Arc::clone(engine))) as Arc<dyn Backend>,
        phrase: Phrase {
            voice: "alto".into(),
            language: "en".into(),
            notes: vec![PhraseNote::new(0.0, 1.0, 60.0, lyric)],
            curves: Default::default(),
            extra_curves: Default::default(),
            settings: Default::default(),
            state: None,
            start: at,
            note_ids: Vec::new(),
        },
        start: at,
        end: at + 1.0,
    }
}

fn settle(r: &mut VocalRenderer, until: impl Fn(&VocalRenderer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        r.poll();
        if until(r) {
            return;
        }
        assert!(Instant::now() < deadline, "the renderer never settled");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn previews_come_first_nearest_the_playhead_then_the_finals() {
    let fake = Fake::new(Duration::from_millis(3));
    let mut r = VocalRenderer::new(1);
    r.set_playhead(10.2);
    let wanted: Vec<Wanted> = [0.0, 5.0, 10.0, 15.0, 20.0]
        .iter()
        .map(|t| phrase(&fake, *t, &format!("{t}")))
        .collect();
    r.want(wanted.clone());
    settle(&mut r, |r| {
        wanted.iter().all(|w| r.state(w) == PhraseState::Final)
    });
    let log = fake.log();
    let previews: Vec<&str> = log
        .iter()
        .take(5)
        .map(|(l, q)| {
            assert_eq!(*q, QUALITY_PREVIEW, "every preview before any final");
            l.as_str()
        })
        .collect();
    // The phrase under the playhead, then ahead of it, then behind.
    assert_eq!(previews, vec!["10", "15", "20", "5", "0"]);
    assert!(log[5..].iter().all(|(_, q)| *q == QUALITY_FINAL));
    assert_eq!(log.len(), 10);
}

#[test]
fn the_best_take_is_the_final_once_it_lands() {
    let fake = Fake::new(Duration::from_millis(1));
    let mut r = VocalRenderer::new(2);
    let w = phrase(&fake, 0.0, "la");
    r.want(vec![w.clone()]);
    settle(&mut r, |r| r.state(&w) == PhraseState::Final);
    let best = r.best(&w).expect("sung");
    assert_eq!(best.quality, QUALITY_FINAL);
    assert_eq!(best.samples[0], quality_number(QUALITY_FINAL));
    assert_eq!(best.sample_rate, 24_000);
    assert_eq!(best.phonemes[0].phonemes[1].symbol, "a");
}

#[test]
fn a_phrase_nobody_wants_any_more_is_cancelled_mid_render() {
    let fake = Fake::new(Duration::from_secs(5));
    let mut r = VocalRenderer::new(1);
    let w = phrase(&fake, 0.0, "slow");
    r.want(vec![w.clone()]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while fake.started.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let asked = Instant::now();
    r.want(Vec::new());
    while fake.cancels_seen.load(Ordering::SeqCst) == 0 {
        assert!(
            asked.elapsed() < Duration::from_secs(1),
            "the cancel flag was never raised"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    r.poll();
    assert!(r.best(&w).is_none());
    assert_eq!(r.state(&w), PhraseState::Unwanted);
}

#[test]
fn an_edit_sings_only_its_own_phrase_and_a_move_or_undo_sings_nothing() {
    let fake = Fake::new(Duration::from_millis(1));
    let mut r = VocalRenderer::new(2);
    let a = phrase(&fake, 0.0, "a");
    let b = phrase(&fake, 4.0, "b");
    r.want(vec![a.clone(), b.clone()]);
    settle(&mut r, |r| {
        r.state(&a) == PhraseState::Final && r.state(&b) == PhraseState::Final
    });
    assert_eq!(fake.log().len(), 4);

    // b's word retyped.
    let edited = phrase(&fake, 4.0, "bee");
    r.want(vec![a.clone(), edited.clone()]);
    settle(&mut r, |r| r.state(&edited) == PhraseState::Final);
    assert_eq!(
        fake.log().len(),
        6,
        "two renders: the edited phrase's preview and final"
    );

    // Undo, and a move of both: already sung.
    let moved_a = phrase(&fake, 8.0, "a");
    let moved_b = phrase(&fake, 12.0, "b");
    r.want(vec![moved_a.clone(), moved_b.clone()]);
    r.poll();
    assert_eq!(r.state(&moved_a), PhraseState::Final);
    assert_eq!(r.state(&moved_b), PhraseState::Final);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(fake.log().len(), 6);
}

#[test]
fn a_failed_phrase_says_why_and_is_not_tried_again_and_again() {
    let fake = Fake::new(Duration::from_millis(1));
    let mut r = VocalRenderer::new(1);
    let w = phrase(&fake, 0.0, "!fail");
    r.want(vec![w.clone()]);
    settle(&mut r, |r| matches!(r.state(&w), PhraseState::Failed(_)));
    let tries = fake.started.load(Ordering::SeqCst);
    r.want(vec![w.clone()]);
    std::thread::sleep(Duration::from_millis(30));
    r.poll();
    assert_eq!(fake.started.load(Ordering::SeqCst), tries);
    assert_eq!(r.state(&w), PhraseState::Failed("no".into()));
}

#[test]
fn an_export_waits_for_every_final_take_and_says_how_far_it_got() {
    let fake = Fake::new(Duration::from_millis(2));
    let mut r = VocalRenderer::new(2);
    let wanted: Vec<Wanted> = (0..6)
        .map(|i| phrase(&fake, i as f64 * 3.0, &i.to_string()))
        .collect();
    r.want(wanted.clone());
    let mut seen = Vec::new();
    r.wait_for_final(&wanted, Duration::from_secs(10), &mut |done, total| {
        seen.push((done, total))
    })
    .expect("all sung");
    assert!(wanted.iter().all(|w| r.state(w) == PhraseState::Final));
    assert_eq!(seen.last(), Some(&(6, 6)));
}

/// An edit is heard soon even while every worker is singing a slow final:
/// the final steps aside for the preview and is sung again after it.
/// Measured in a 40-phrase song on a DiffSinger bank: an edit waited 2.8 s
/// behind the finals, 1 s when nothing else was being sung.
#[test]
fn an_edits_preview_does_not_wait_behind_slow_finals() {
    let fake = Arc::new(Fake {
        final_delay: Some(Duration::from_secs(3)),
        ..Arc::into_inner(Fake::new(Duration::from_millis(5))).unwrap()
    });
    let mut r = VocalRenderer::new(2);
    let a = phrase(&fake, 0.0, "a");
    let b = phrase(&fake, 5.0, "b");
    r.want(vec![a.clone(), b.clone()]);
    // Both previews in; both workers now on a three-second final.
    settle(&mut r, |r| {
        r.state(&a) == PhraseState::Preview && r.state(&b) == PhraseState::Preview
    });
    std::thread::sleep(Duration::from_millis(50));
    let edited = phrase(&fake, 10.0, "edited");
    let asked = Instant::now();
    r.want(vec![a.clone(), b.clone(), edited.clone()]);
    settle(&mut r, |r| r.state(&edited) != PhraseState::Waiting);
    assert!(
        asked.elapsed() < Duration::from_millis(500),
        "heard after {:?}",
        asked.elapsed()
    );
    // The finals that stepped aside are sung all the same.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        r.poll();
        if [&a, &b, &edited]
            .iter()
            .all(|w| r.state(w) == PhraseState::Final)
        {
            break;
        }
        assert!(Instant::now() < deadline, "a final was dropped");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A voice being warmed (its models loading) is not stopped for the
/// previews that arrive meanwhile: they need what it is loading.
#[test]
fn a_voice_being_warmed_is_not_stopped_for_previews() {
    let fake = Fake::new(Duration::from_millis(300));
    let mut r = VocalRenderer::new(1);
    r.warm(
        Arc::new(WithVoice(Arc::clone(&fake))) as Arc<dyn Backend>,
        "alto",
    );
    std::thread::sleep(Duration::from_millis(50));
    let w = phrase(&fake, 0.0, "la");
    r.want(vec![w.clone()]);
    settle(&mut r, |r| r.state(&w) == PhraseState::Final);
    assert_eq!(fake.cancels_seen.load(Ordering::SeqCst), 0);
}

/// The fake with a voice of its own, as a warmed backend has.
struct WithVoice(Arc<Fake>);

impl Backend for WithVoice {
    fn name(&self) -> &str {
        "Fake"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        vec![VoiceInfo {
            id: "alto".into(),
            name: "Alto".into(),
            languages: vec!["en".into()],
            lowest: 55,
            highest: 72,
            research_only: false,
            terms: None,
            image: None,
        }]
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(FakeVoice(Arc::clone(&self.0))))
    }
}
