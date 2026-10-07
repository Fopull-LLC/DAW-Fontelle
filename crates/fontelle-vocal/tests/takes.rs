//! Takes kept on disk: a song reopened plays at once rather than singing
//! everything again. Final takes are written by the render threads,
//! compressed (16-bit, delta-coded, deflated), keyed by the render key, and
//! the folder is kept under a size limit, least recently used first out.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fontelle_sing::{
    Backend, Capabilities, Phrase, PhraseNote, Quality, Session, SingError, Sung, SungNote,
    SungPhoneme, VoiceInfo,
};
use fontelle_vocal::{PhraseState, Rendered, TakeStore, VocalRenderer, Wanted, render_key};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-takes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn phrase(lyric: &str) -> Phrase {
    Phrase {
        voice: "alto".into(),
        language: "en".into(),
        notes: vec![PhraseNote::new(0.0, 1.0, 60.0, lyric)],
        curves: Default::default(),
        extra_curves: Default::default(),
        settings: Default::default(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

/// A take with a voice-like signal (a sine with a slow swell), so the
/// compression is measured on something like singing.
fn take(seconds: f64, quality: Quality) -> Rendered {
    let rate = 44_100u32;
    let n = (seconds * f64::from(rate)) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            0.6 * (t * 3.0).sin().abs() * (std::f32::consts::TAU * 220.0 * t).sin()
        })
        .collect();
    Rendered {
        quality,
        sample_rate: rate,
        samples: samples.into(),
        start: -0.125,
        pitch: (0..200).map(|i| 220.0 + i as f32 * 0.5).collect(),
        pitch_step: 0.005,
        phonemes: vec![
            SungNote {
                note: 0,
                phonemes: vec![
                    SungPhoneme {
                        symbol: "l".into(),
                        offset: Some(-0.06),
                    },
                    SungPhoneme {
                        symbol: "ああ".into(),
                        offset: None,
                    },
                ],
                guessed: true,
            },
            SungNote {
                note: 1,
                phonemes: Vec::new(),
                guessed: false,
            },
        ],
    }
}

fn key(lyric: &str) -> fontelle_vocal::RenderKey {
    render_key("Fake", "1", &phrase(lyric), Quality::Final)
}

#[test]
fn a_take_saved_loads_back_as_it_was() {
    let dir = scratch("roundtrip");
    let store = TakeStore::new(dir.clone(), 1 << 30);
    let t = take(2.0, Quality::Final);
    store.save(key("la"), &t).unwrap();
    assert!(store.has(key("la")));
    let back = store.load(key("la")).expect("loads");
    assert_eq!(back.quality, Quality::Final);
    assert_eq!(back.sample_rate, t.sample_rate);
    assert_eq!(back.start, t.start);
    assert_eq!(back.pitch_step, t.pitch_step);
    assert_eq!(&back.pitch[..], &t.pitch[..]);
    assert_eq!(back.phonemes, t.phonemes);
    assert_eq!(back.samples.len(), t.samples.len());
    let worst = back
        .samples
        .iter()
        .zip(t.samples.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst <= 1.0 / 32_000.0, "16-bit: off by {worst}");
    // Smaller than the floats, and than plain 16-bit.
    let on_disk = store.size();
    assert!(
        on_disk < (t.samples.len() * 2) as u64,
        "{on_disk} bytes for {} samples",
        t.samples.len()
    );
    // A take louder than full scale keeps its level.
    let mut loud = take(0.5, Quality::Final);
    loud.samples = loud.samples.iter().map(|s| s * 3.0).collect();
    store.save(key("loud"), &loud).unwrap();
    let back = store.load(key("loud")).unwrap();
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak(&back.samples) - peak(&loud.samples)).abs() < 1e-3);
}

#[test]
fn another_keys_file_or_a_damaged_one_is_ignored() {
    let dir = scratch("mismatch");
    let store = TakeStore::new(dir.clone(), 1 << 30);
    store.save(key("la"), &take(0.5, Quality::Final)).unwrap();
    assert!(store.load(key("lo")).is_none(), "nothing saved under it");
    // A file renamed to another key's name is not that key's take.
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1);
    let other = key("lo");
    let name = files[0].file_name().unwrap().to_string_lossy().replace(
        &format!("{:016x}", key("la").hash),
        &format!("{:016x}", other.hash),
    );
    std::fs::copy(&files[0], dir.join(name)).unwrap();
    assert!(store.load(other).is_none(), "the header names another key");
    // A cut-short file is not a take.
    let bytes = std::fs::read(&files[0]).unwrap();
    std::fs::write(&files[0], &bytes[..bytes.len() / 2]).unwrap();
    assert!(store.load(key("la")).is_none());
}

#[test]
fn the_folder_is_kept_under_its_limit_least_recently_used_out_first() {
    let dir = scratch("limit");
    // Room for about three of these takes.
    let one = {
        let probe = TakeStore::new(scratch("limit-probe"), 1 << 30);
        probe
            .save(key("probe"), &take(1.0, Quality::Final))
            .unwrap();
        probe.size()
    };
    let store = TakeStore::new(dir.clone(), one * 3 + one / 2);
    for name in ["a", "b", "c"] {
        store.save(key(name), &take(1.0, Quality::Final)).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    // "a" is played again: it is the most recently used now.
    assert!(store.load(key("a")).is_some());
    std::thread::sleep(Duration::from_millis(20));
    store.save(key("d"), &take(1.0, Quality::Final)).unwrap();
    assert!(
        store.size() <= one * 3 + one / 2,
        "{} > limit",
        store.size()
    );
    assert!(store.has(key("a")), "recently played");
    assert!(!store.has(key("b")), "the least recently used went");
    assert!(store.has(key("c")));
    assert!(store.has(key("d")));
}

// ----------------------------------------------------------- the renderer

struct Counting {
    sung: Arc<AtomicUsize>,
}

struct CountingSession(Arc<AtomicUsize>);

impl Backend for Counting {
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
        Ok(Box::new(CountingSession(Arc::clone(&self.sung))))
    }
}

impl Session for CountingSession {
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
        _spec: &Phrase,
        quality: Quality,
        _seed: u64,
        _cancel: &AtomicU32,
    ) -> Result<Sung, SingError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let t = take(0.5, quality);
        Ok(Sung {
            sample_rate: t.sample_rate,
            samples: t.samples.to_vec(),
            start: t.start,
            pitch: t.pitch.to_vec(),
            pitch_step: t.pitch_step,
            phonemes: t.phonemes,
        })
    }
}

fn wanted(sung: &Arc<AtomicUsize>, lyric: &str) -> Wanted {
    Wanted {
        backend: Arc::new(Counting {
            sung: Arc::clone(sung),
        }),
        phrase: phrase(lyric),
        start: 0.0,
        end: 1.0,
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
fn a_reopened_song_plays_its_saved_takes_without_singing_again() {
    let dir = scratch("reopen");
    let sung = Arc::new(AtomicUsize::new(0));
    let w = wanted(&sung, "la");
    {
        let mut r = VocalRenderer::new(1);
        r.set_store(Some(TakeStore::new(dir.clone(), 1 << 30)));
        r.want(vec![w.clone()]);
        settle(&mut r, |r| r.state(&w) == PhraseState::Final);
        // The final is written by the render thread once it is sung.
        let store = TakeStore::new(dir.clone(), 1 << 30);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !store.has(render_key("Fake", "1", &w.phrase, Quality::Final)) {
            assert!(Instant::now() < deadline, "the final was never saved");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    assert_eq!(sung.load(Ordering::SeqCst), 2, "a preview and a final");
    // Reopened: a new renderer, the same folder.
    let mut r = VocalRenderer::new(1);
    r.set_store(Some(TakeStore::new(dir.clone(), 1 << 30)));
    r.want(vec![w.clone()]);
    settle(&mut r, |r| r.state(&w) == PhraseState::Final);
    assert_eq!(
        sung.load(Ordering::SeqCst),
        2,
        "neither a preview nor a final was sung again"
    );
    assert_eq!(r.best(&w).unwrap().phonemes[0].phonemes[1].symbol, "ああ");
}

#[test]
fn a_store_given_later_receives_the_finals_already_sung() {
    // An unsaved song saved for the first time: its takes move with it.
    let dir = scratch("later");
    let sung = Arc::new(AtomicUsize::new(0));
    let w = wanted(&sung, "la");
    let mut r = VocalRenderer::new(1);
    r.want(vec![w.clone()]);
    settle(&mut r, |r| r.state(&w) == PhraseState::Final);
    r.set_store(Some(TakeStore::new(dir.clone(), 1 << 30)));
    let store = TakeStore::new(dir, 1 << 30);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !store.has(render_key("Fake", "1", &w.phrase, Quality::Final)) {
        assert!(Instant::now() < deadline, "the final was never written");
        std::thread::sleep(Duration::from_millis(2));
    }
}
