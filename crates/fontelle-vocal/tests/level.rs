//! Each voicebank sung at a like loudness. Banks come recorded anywhere
//! from near full scale to 25 dB under it: walked end to end, Hitsuboku
//! Kumi's loudest moments were at -12 dBFS and Yoko's at -30, so a song
//! changing singer jumped by 18 dB. The Fontelle Singer's runtimes are
//! levelled per voice from a short reference phrase, sung once, so every
//! take of a voice gets the same gain — deterministic, cached like any
//! other take — and the reference is what warming a voice sings, so the
//! models are loaded before the first note is written.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fontelle_sing::{
    Backend, Capabilities, Phrase, PhraseNote, Quality, Session, SingError, Sung, VoiceInfo,
};
use fontelle_vocal::{PhraseState, VocalRenderer, Wanted, leveled, leveled_kept, reference_phrase};

/// A singer at a fixed level: a 220 Hz tone at `amplitude` over each note.
struct Tone {
    amplitude: f32,
    fails: bool,
    /// How long each phrase takes to sing.
    delay: Duration,
    sung: Arc<AtomicUsize>,
    asked: Arc<Mutex<Vec<Phrase>>>,
}

impl Tone {
    fn new(amplitude: f32) -> Arc<Self> {
        Arc::new(Self {
            amplitude,
            fails: false,
            delay: Duration::ZERO,
            sung: Arc::default(),
            asked: Arc::default(),
        })
    }
}

struct ToneBackend(Arc<Tone>);
struct ToneSession(Arc<Tone>);

/// A tone sung from a folder, at a version: what a built-in runtime says
/// a voice is made of.
struct FiledTone {
    tone: Arc<Tone>,
    folder: std::path::PathBuf,
    version: &'static str,
}

impl Backend for FiledTone {
    fn name(&self) -> &str {
        "Tone"
    }
    fn version(&self) -> String {
        self.version.into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        ToneBackend(Arc::clone(&self.tone)).voices()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(ToneSession(Arc::clone(&self.tone))))
    }
    fn voice_files(&self, voice: &str) -> Vec<std::path::PathBuf> {
        if voice == "tone" {
            vec![self.folder.clone()]
        } else {
            Vec::new()
        }
    }
}

impl Backend for ToneBackend {
    fn name(&self) -> &str {
        "Tone"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        vec![VoiceInfo {
            id: "tone".into(),
            name: "Tone".into(),
            languages: vec!["de".into(), "ja".into()],
            lowest: 55,
            highest: 75,
            research_only: false,
            terms: None,
            image: None,
        }]
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(ToneSession(Arc::clone(&self.0))))
    }
}

impl Session for ToneSession {
    fn sing(
        &mut self,
        phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        self.0.sung.fetch_add(1, Ordering::SeqCst);
        self.0.asked.lock().unwrap().push(phrase.clone());
        std::thread::sleep(self.0.delay);
        if self.0.fails {
            return Err(SingError::Failed("cannot".into()));
        }
        let rate = 24_000;
        let end = phrase.notes.last().map_or(0.0, |n| n.end());
        let samples = (0..(end * f64::from(rate)) as usize)
            .map(|i| {
                let t = i as f32 / rate as f32;
                self.0.amplitude * (std::f32::consts::TAU * 220.0 * t).sin()
            })
            .collect();
        Ok(Sung {
            sample_rate: rate,
            samples,
            ..Sung::default()
        })
    }
}

fn phrase(lyric: &str) -> Phrase {
    Phrase::new(
        "tone".to_string(),
        "ja".to_string(),
        vec![
            PhraseNote::new(0.0, 0.5, 60.0, lyric),
            PhraseNote::new(0.5, 0.5, 62.0, lyric),
        ],
    )
}

fn rms_db(samples: &[f32]) -> f32 {
    let ms = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    10.0 * ms.log10()
}

fn sing(backend: &Arc<dyn Backend>, p: &Phrase) -> Sung {
    backend
        .session()
        .unwrap()
        .sing(p, Quality::Final, 1, &|| false)
        .unwrap()
}

#[test]
fn a_quiet_bank_and_a_loud_one_come_out_alike() {
    // At -30 and -12 dBFS: Yoko and Hitsuboku Kumi, as measured.
    let quiet: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(Tone::new(0.045))));
    let loud: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(Tone::new(0.35))));
    let p = phrase("la");
    let a = rms_db(&sing(&quiet, &p).samples);
    let b = rms_db(&sing(&loud, &p).samples);
    assert!((a - b).abs() < 1.0, "quiet {a:.1} dB, loud {b:.1} dB");
    // Somewhere sensible: well under full scale, well over the floor.
    assert!((-24.0..-12.0).contains(&a), "{a:.1} dB");
}

#[test]
fn the_gain_is_the_voices_own_whatever_is_sung_first() {
    let tone = Tone::new(0.05);
    let backend: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(Arc::clone(&tone))));
    // The version moves, so takes kept from before levelling are sung again.
    assert_ne!(backend.version(), "1");
    let first = sing(&backend, &phrase("la"));
    let again = sing(&backend, &phrase("la"));
    assert_eq!(first.samples, again.samples, "deterministic");
    // A second session (another render thread) agrees, and the reference
    // was sung only once between them.
    let other = sing(&backend, &phrase("la"));
    assert_eq!(first.samples, other.samples);
    let references = tone
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter(|p| **p == reference_phrase(&backend.voices()[0]))
        .count();
    assert_eq!(references, 1);
}

#[test]
fn the_reference_is_sung_in_a_language_the_voice_sings_mid_range() {
    let voice = ToneBackend(Tone::new(0.1)).voices().remove(0);
    let reference = reference_phrase(&voice);
    // Japanese before the voice's first language, German: a vowel every
    // phonemizer reads.
    assert_eq!(reference.language, "ja");
    assert!(reference.notes.iter().all(|n| n.lyric == "あ"));
    assert!(
        reference
            .notes
            .iter()
            .all(|n| (55.0..=75.0).contains(&n.pitch))
    );
}

#[test]
fn a_voice_whose_reference_fails_is_left_as_it_sings() {
    let tone = Arc::new(Tone {
        fails: true,
        ..Arc::into_inner(Tone::new(0.1)).unwrap()
    });
    let backend: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(tone)));
    let err = backend
        .session()
        .unwrap()
        .sing(&phrase("la"), Quality::Final, 1, &|| false)
        .unwrap_err();
    assert!(
        matches!(err, SingError::Failed(_)),
        "the phrase's own error"
    );
}

#[test]
fn a_near_silent_bank_is_not_blown_up() {
    let backend: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(Tone::new(0.0005))));
    let db = rms_db(&sing(&backend, &phrase("la")).samples);
    assert!(db < -40.0, "{db:.1} dB");
}

/// Warming a voice sings its reference in the background (loading its
/// models, finding its level) and keeps nothing; the first real phrase
/// then sings without a reference before it.
#[test]
fn warming_a_voice_sings_its_reference_once_and_keeps_nothing() {
    let tone = Tone::new(0.1);
    let backend: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(Arc::clone(&tone))));
    let mut r = VocalRenderer::new(1);
    r.warm(Arc::clone(&backend), "tone");
    let deadline = Instant::now() + Duration::from_secs(10);
    while tone.sung.load(Ordering::SeqCst) < 1 || r.busy() {
        assert!(Instant::now() < deadline, "never warmed");
        r.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(tone.sung.load(Ordering::SeqCst), 1, "the reference, once");
    // Warmed again: nothing more.
    r.warm(Arc::clone(&backend), "tone");
    std::thread::sleep(Duration::from_millis(50));
    r.poll();
    assert_eq!(tone.sung.load(Ordering::SeqCst), 1);
    let w = Wanted {
        backend,
        phrase: phrase("la"),
        start: 0.0,
        end: 1.0,
    };
    r.want(vec![w.clone()]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while r.state(&w) != PhraseState::Final {
        assert!(Instant::now() < deadline);
        r.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        tone.sung.load(Ordering::SeqCst),
        3,
        "preview and final only"
    );
}

/// Two render threads starting on one voice at once measure it once: a
/// DiffSinger bank runs one phrase on a model at a time, so a second
/// reference was a second's wait for both.
#[test]
fn two_threads_starting_at_once_measure_the_voice_once() {
    let tone = Arc::new(Tone {
        delay: Duration::from_millis(150),
        ..Arc::into_inner(Tone::new(0.1)).unwrap()
    });
    let backend: Arc<dyn Backend> = leveled(Arc::new(ToneBackend(Arc::clone(&tone))));
    let threads: Vec<_> = ["la", "li"]
        .iter()
        .map(|lyric| {
            let backend = Arc::clone(&backend);
            let p = phrase(lyric);
            std::thread::spawn(move || sing(&backend, &p))
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let reference = reference_phrase(&backend.voices()[0]);
    let references = tone
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter(|p| **p == reference)
        .count();
    assert_eq!(references, 1);
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-level-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn references(tone: &Tone, voice: &VoiceInfo) -> usize {
    let reference = reference_phrase(voice);
    tone.asked
        .lock()
        .unwrap()
        .iter()
        .filter(|p| **p == reference)
        .count()
}

/// A voice's level is kept on disk by what the voice is made of: the next
/// session (a new run of the studio, a rescan after an install) sings its
/// first phrase without singing the reference first — about half a second
/// of a DiffSinger bank's first phrase, every session.
#[test]
fn a_voices_level_is_kept_on_disk_and_not_measured_again() {
    let dir = scratch("kept");
    let bank = dir.join("bank");
    std::fs::create_dir_all(bank.join("sub")).unwrap();
    std::fs::write(bank.join("oto.ini"), "a.wav=a,0,0,0,0,0").unwrap();
    std::fs::write(bank.join("sub").join("model.onnx"), vec![7u8; 300_000]).unwrap();
    let levels = dir.join("levels");
    let filed = |tone: &Arc<Tone>, version| -> Arc<dyn Backend> {
        leveled_kept(
            Arc::new(FiledTone {
                tone: Arc::clone(tone),
                folder: bank.clone(),
                version,
            }),
            Some(levels.clone()),
        )
    };
    let first = Tone::new(0.05);
    let a = sing(&filed(&first, "1"), &phrase("la"));
    let voice = ToneBackend(Tone::new(0.1)).voices().remove(0);
    assert_eq!(references(&first, &voice), 1);
    assert!(
        std::fs::read_dir(&levels).is_ok_and(|d| d.count() > 0),
        "the level is written"
    );
    // The next session: no reference, the same take.
    let second = Tone::new(0.05);
    let b = sing(&filed(&second, "1"), &phrase("la"));
    assert_eq!(references(&second, &voice), 0, "measured again");
    assert_eq!(second.sung.load(Ordering::SeqCst), 1, "the phrase only");
    assert_eq!(a.samples, b.samples);

    // A new runtime version measures again (it may sing louder).
    let third = Tone::new(0.05);
    sing(&filed(&third, "2"), &phrase("la"));
    assert_eq!(references(&third, &voice), 1);

    // So does a bank whose files changed (a new release in the same
    // folder), even at the same size.
    let mut model = vec![7u8; 300_000];
    model[150_000] = 8;
    std::fs::write(bank.join("sub").join("model.onnx"), model).unwrap();
    let fourth = Tone::new(0.05);
    sing(&filed(&fourth, "1"), &phrase("la"));
    assert_eq!(references(&fourth, &voice), 1);
    let fifth = Tone::new(0.05);
    sing(&filed(&fifth, "1"), &phrase("la"));
    assert_eq!(references(&fifth, &voice), 0);
}

/// A voice that says nothing of its files (an external engine's) is
/// measured each session, and a level folder that cannot be written only
/// costs the reference again.
#[test]
fn a_voice_without_files_or_a_folder_that_cannot_be_written_still_sings() {
    let dir = scratch("unkept");
    let levels = dir.join("levels");
    for _ in 0..2 {
        let tone = Tone::new(0.05);
        let backend = leveled_kept(
            Arc::new(ToneBackend(Arc::clone(&tone))),
            Some(levels.clone()),
        );
        sing(&backend, &phrase("la"));
        assert_eq!(references(&tone, &backend.voices()[0]), 1);
    }
    // A file where the folder would be.
    let blocked = dir.join("blocked");
    std::fs::write(&blocked, "not a folder").unwrap();
    let bank = dir.join("bank");
    std::fs::create_dir_all(&bank).unwrap();
    std::fs::write(bank.join("oto.ini"), "x").unwrap();
    let tone = Tone::new(0.05);
    let backend = leveled_kept(
        Arc::new(FiledTone {
            tone: Arc::clone(&tone),
            folder: bank,
            version: "1",
        }),
        Some(blocked),
    );
    let quiet = rms_db(&sing(&backend, &phrase("la")).samples);
    assert!((-24.0..-12.0).contains(&quiet), "{quiet:.1} dB: levelled");
}
