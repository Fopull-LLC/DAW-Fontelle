//! Analyze Musically through the studio (`docs/analyze-musically-plan.md`
//! §3.6, §3.10, §4 P1): a clip's analysis as a job beside the window, its
//! cache, and the three things it gives back to the song — notes for a piano
//! roll, the key, and a note clip under the audio.
//!
//! The audio is synthesised by `fontelle_analysis::testsignals` and written
//! to a scratch folder; nothing here is committed.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontelle_analysis::testsignals;
use fontelle_app::Session;
use fontelle_assets::fixtures::build_wav;
use fontelle_types::{ClipId, PPQN};
use fontelle_ui::canvas::{AnalyzeMode, AnalyzeView};
use fontelle_ui::document::{ClipKind, DocumentHost, JobPoll, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-analyze-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_saved_session(dir: &Path) -> Session {
    common::a_session_in(
        common::a_project_with_a_clip(16, 120.0, SR),
        Some(dir.join("Song")),
    )
}

/// `samples` as a WAV in `dir`, dropped on the first row at bar 3 (two bars
/// of 120 BPM in: 4 s). The audio clip it made.
fn drop_audio(session: &mut Session, dir: &Path, name: &str, samples: &[f32]) -> ClipId {
    let path = dir.join(name);
    std::fs::write(&path, build_wav(SR, 1, samples)).expect("writable");
    session
        .drop_file_on(&path, i64::from(SR) * 4, Some(0))
        .expect("the drop lands");
    session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip")
        .id
}

/// Polls until the job ends, collecting every view seen on the way.
fn run_to_end(session: &mut Session) -> (Result<String, String>, Vec<AnalyzeView>) {
    let started = Instant::now();
    let mut seen = Vec::new();
    loop {
        match session.poll_analysis() {
            JobPoll::Finished(result) => return (result, seen),
            JobPoll::Running(progress) => {
                assert!(progress.fraction.is_some_and(|f| (0.0..=1.0).contains(&f)));
                if let Some(view) = session.analyze_view() {
                    seen.push(view);
                }
            }
            JobPoll::Idle => return (Ok("idle".to_string()), seen),
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the analysis never ended"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn opening_on_a_clip_starts_a_job_that_finishes_with_the_notes_and_the_key() {
    let dir = scratch("finish");
    let mut session = a_saved_session(&dir);
    let fixture = testsignals::vibrato_melody(SR);
    let clip = drop_audio(&mut session, &dir, "Vox.wav", &fixture.samples);

    let said = session.analyze_musically(clip).expect("an audio clip");
    assert!(said.contains("analysing"), "{said}");
    let view = session
        .analyze_view()
        .expect("the window has a view at once");
    assert_eq!(view.name, "Vox");
    let (result, _) = run_to_end(&mut session);
    assert!(result.is_ok(), "{result:?}");

    let view = session.analyze_view().unwrap();
    assert!(view.analysing.is_none());
    assert_eq!(view.detected, Some(AnalyzeMode::Melody));
    assert_eq!(view.melody.len(), fixture.notes.len(), "{:?}", view.melody);
    for (heard, played) in view.melody.iter().zip(&fixture.notes) {
        assert_eq!(heard.midi, played.midi);
        assert!(
            (heard.start - played.start).abs() < 0.05,
            "{heard:?} vs {played:?}"
        );
        assert!(!heard.curve.is_empty(), "a sung note has its pitch curve");
    }
    assert!(view.key.is_some());
    assert!(view.clarity.is_some());
    assert!(!view.clarity_reason.is_empty());
    assert!(view.tuning_cents.is_some_and(|c| c.abs() < 15.0));
    assert!(!view.peaks.is_empty());
    assert!(view.spectrogram.is_some());
    let seconds = fixture.samples.len() as f64 / f64::from(SR);
    assert!((view.duration - seconds).abs() < 0.01);
    // Once said, the job is gone.
    assert_eq!(session.poll_analysis(), JobPoll::Idle);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn notes_appear_before_the_analysis_is_done() {
    let dir = scratch("progressive");
    let mut session = a_saved_session(&dir);
    // The sine melody four times over: 18 s, in parts.
    let one = testsignals::sine_melody(SR).samples;
    let long: Vec<f32> = (0..4).flat_map(|_| one.iter().copied()).collect();
    let clip = drop_audio(&mut session, &dir, "Long.wav", &long);
    session.analyze_musically(clip).unwrap();
    let (result, seen) = run_to_end(&mut session);
    assert!(result.is_ok());
    assert!(
        seen.iter()
            .any(|v| v.analysing.is_some() && !v.notes.is_empty()),
        "some notes were on the lane while it was still listening"
    );
    let done = session.analyze_view().unwrap();
    assert_eq!(done.notes.len(), 36);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn reopening_reads_the_cache_inside_the_project() {
    let dir = scratch("cache");
    let mut session = a_saved_session(&dir);
    let fixture = testsignals::c_major_triad(SR);
    let clip = drop_audio(&mut session, &dir, "Triad.wav", &fixture.samples);
    session.analyze_musically(clip).unwrap();
    let (result, _) = run_to_end(&mut session);
    assert!(result.is_ok());
    let first = session.analyze_view().unwrap();

    // INVARIANT 10: the cache is the project's own.
    let cache = dir.join("Song").join("cache").join("analysis");
    let files: Vec<String> = std::fs::read_dir(&cache)
        .expect("the cache folder")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(files.iter().any(|f| f.ends_with(".json")), "{files:?}");
    assert!(files.iter().any(|f| f.ends_with(".bin")), "{files:?}");

    session.close_analysis();
    assert!(session.analyze_view().is_none());
    assert_eq!(session.poll_analysis(), JobPoll::Idle);

    // Again: there at once, without a job.
    session.analyze_musically(clip).unwrap();
    let again = session.analyze_view().unwrap();
    assert!(again.analysing.is_none(), "read from the cache");
    assert_eq!(again.notes, first.notes);
    assert_eq!(again.key, first.key);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_not_saved_yet_caches_under_the_xdg_cache() {
    let xdg = PathBuf::from("/home/someone/.cache");
    assert_eq!(
        fontelle_app::analysis_cache_dir(None, Some(xdg.clone()), None),
        Some(xdg.join("fontelle").join("analysis"))
    );
    assert_eq!(
        fontelle_app::analysis_cache_dir(None, None, Some(PathBuf::from("/home/someone"))),
        Some(PathBuf::from("/home/someone/.cache/fontelle/analysis"))
    );
    let bundle = PathBuf::from("/songs/Song");
    assert_eq!(
        fontelle_app::analysis_cache_dir(Some(&bundle), Some(xdg), None),
        Some(bundle.join("cache").join("analysis"))
    );
    assert_eq!(fontelle_app::analysis_cache_dir(None, None, None), None);
}

#[test]
fn closing_the_window_stops_the_job() {
    let dir = scratch("close");
    let mut session = a_saved_session(&dir);
    let one = testsignals::sine_melody(SR).samples;
    let long: Vec<f32> = (0..8).flat_map(|_| one.iter().copied()).collect();
    let clip = drop_audio(&mut session, &dir, "Long.wav", &long);
    session.analyze_musically(clip).unwrap();
    session.close_analysis();
    assert_eq!(session.poll_analysis(), JobPoll::Idle);
    assert!(session.analyze_view().is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn only_an_audio_clip_is_analysed() {
    let dir = scratch("notes");
    let mut session = a_saved_session(&dir);
    let notes = session.clips()[0].id;
    assert!(session.analyze_musically(notes).is_err());
    assert!(session.analyze_view().is_none());
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------- what it gives back ---

fn analysed(name: &str, samples: &[f32]) -> (PathBuf, Session, ClipId) {
    let dir = scratch(name);
    let mut session = a_saved_session(&dir);
    let clip = drop_audio(&mut session, &dir, "Take.wav", samples);
    session.analyze_musically(clip).unwrap();
    let (result, _) = run_to_end(&mut session);
    assert!(result.is_ok());
    (dir, session, clip)
}

#[test]
fn set_as_song_key_is_one_undo() {
    let fixture = testsignals::vibrato_melody(SR);
    let (dir, mut session, _) = analysed("key", &fixture.samples);
    let key = session.analyze_view().unwrap().key.unwrap().key;
    assert_eq!(session.song_key(), None);
    session.set_song_key(Some(key.clone()), Vec::new(), Vec::new());
    session.end_gesture();
    assert_eq!(session.song_key(), Some(key));
    session.undo();
    assert_eq!(session.song_key(), None, "one undo takes it back");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn copied_notes_are_clean_semitones_as_played_under_the_clip() {
    let fixture = testsignals::vibrato_melody(SR);
    let (dir, session, clip) = analysed("copy", &fixture.samples);
    let start = session
        .clips()
        .into_iter()
        .find(|c| c.id == clip)
        .unwrap()
        .start;
    assert_eq!(start, PPQN * 8, "bar 3 at 120 BPM");
    let (notes, origin) = session
        .analysis_notes(AnalyzeMode::Melody, &[], false)
        .expect("notes to copy");
    assert_eq!(notes.len(), fixture.notes.len());
    // Song ticks, through the tempo map: 120 BPM is 1920 ticks a second.
    let ticks_per_second = PPQN as f64 * 2.0;
    for (note, played) in notes.iter().zip(&fixture.notes) {
        let expected = start as f64 + played.start * ticks_per_second;
        assert!(
            (note.start as f64 - expected).abs() < 0.05 * ticks_per_second,
            "{} vs {expected}",
            note.start
        );
        assert_eq!(note.key, played.midi);
        assert_eq!(note.fine_pitch, 0, "clean semitones");
        assert!(note.path.is_empty(), "no bends unless asked for");
        assert!((30..=120).contains(&note.velocity));
    }
    assert_eq!(origin, notes.iter().map(|n| n.start).min().unwrap());

    // Two of them, by index.
    let (two, _) = session
        .analysis_notes(AnalyzeMode::Melody, &[1, 3], false)
        .unwrap();
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].key, fixture.notes[1].midi);
    assert_eq!(two[1].key, fixture.notes[3].midi);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn keep_slides_and_bends_keeps_the_cents() {
    // A4, 40 cents sharp, held: clean, it is an A; with bends, an A 40
    // cents up.
    let audio = testsignals::sung_curve(SR, 2.6, 0.3, 2.3, |_| 6940.0);
    let (dir, session, _) = analysed("bends", &audio);
    for mode in [AnalyzeMode::Melody, AnalyzeMode::Chords] {
        let (clean, _) = session.analysis_notes(mode, &[], false).unwrap();
        let a = clean.iter().find(|n| n.key == 69).expect("an A4");
        assert_eq!(a.fine_pitch, 0);
        let (bent, _) = session.analysis_notes(mode, &[], true).unwrap();
        let a = bent.iter().find(|n| n.key == 69).expect("an A4");
        assert!(
            (20..=60).contains(&a.fine_pitch),
            "{mode:?}: {} cents",
            a.fine_pitch
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn notes_under_the_audio_is_one_undo() {
    let fixture = testsignals::vibrato_melody(SR);
    let (dir, mut session, clip) = analysed("under", &fixture.samples);
    let rows = session.lanes().len();
    let clips = session.clips().len();
    let audio_row = session
        .clips()
        .into_iter()
        .find(|c| c.id == clip)
        .unwrap()
        .lane;

    let said = session
        .make_analysis_clip(AnalyzeMode::Melody, &[], false)
        .expect("made");
    session.end_gesture();
    assert!(!said.is_empty());
    assert_eq!(session.lanes().len(), rows + 1, "a row of its own");
    let made = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Notes && c.lane == audio_row + 1)
        .expect("a note clip directly under the audio");
    let audio = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert!(made.start <= audio.start + PPQN, "lined up under the audio");

    session.undo();
    assert_eq!(session.lanes().len(), rows, "one undo takes the row");
    assert_eq!(session.clips().len(), clips, "and the clip");
    std::fs::remove_dir_all(&dir).ok();
}
