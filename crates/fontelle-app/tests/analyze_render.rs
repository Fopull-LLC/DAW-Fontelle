//! Analyze Musically's P2 through the studio (`docs/analyze-musically-plan.md`
//! §3.7, §3.9, §3.10, §4 P2): a note moved is a study in the song, one undo;
//! the preview plays the moved note's own audio, not a tone; Render to clip
//! swaps the clip's audio for a render of the edits from the original, one
//! undo, and a re-render never compounds.
//!
//! The audio is synthesised (`fontelle_analysis::testsignals`) into a
//! scratch folder; nothing here is committed. What a render or the preview
//! makes is measured with the analysis's own pitch tracker.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fontelle_analysis::mono::{PyinParams, pyin};
use fontelle_analysis::testsignals;
use fontelle_app::Session;
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{AudioNode, PrepareContext, ProcessContext, StudyPlayerNode};
use fontelle_model::ClipSource;
use fontelle_types::{ClipId, StudySource};
use fontelle_ui::canvas::{AnalyzeEdit, AnalyzeEditChange, AnalyzedNote};
use fontelle_ui::document::{ClipKind, DocumentHost, JobPoll, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-analyze-render-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A saved song with the sung melody dropped on it and analysed.
fn analysed(name: &str) -> (PathBuf, Session, ClipId) {
    let dir = scratch(name);
    let mut session = common::a_session_in(
        common::a_project_with_a_clip(16, 120.0, SR),
        Some(dir.join("Song")),
    );
    let path = dir.join("Take.wav");
    let fixture = testsignals::vibrato_melody(SR);
    std::fs::write(&path, build_wav(SR, 1, &fixture.samples)).expect("writable");
    session
        .drop_file_on(&path, i64::from(SR) * 4, Some(0))
        .expect("the drop lands");
    let clip = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip")
        .id;
    session.analyze_musically(clip).unwrap();
    wait_for_analysis(&mut session);
    (dir, session, clip)
}

fn wait_for_analysis(session: &mut Session) {
    let started = Instant::now();
    while matches!(session.poll_analysis(), JobPoll::Running(_)) {
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Polls until the preview has caught up with the edits.
fn settle(session: &mut Session) {
    let started = Instant::now();
    loop {
        let _ = session.poll_analysis();
        if !session.analyze_view().unwrap().preview_pending {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the preview never caught up"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn note(session: &Session, index: usize) -> AnalyzedNote {
    session.analyze_view().unwrap().melody[index].clone()
}

fn moved(n: &AnalyzedNote, cents: f32) -> AnalyzeEditChange {
    AnalyzeEditChange {
        start: n.start,
        end: n.end,
        edit: Some(AnalyzeEdit {
            shift_cents: cents,
            ..AnalyzeEdit::default()
        }),
    }
}

fn the_asset(session: &Session, clip: ClipId) -> fontelle_types::AssetRef {
    match &session.project().clips[clip].source {
        ClipSource::Audio(data) => data.asset.clone(),
        _ => panic!("audio"),
    }
}

/// The median pitch (MIDI cents) of mono `audio` at `rate`, between two
/// times.
fn heard(audio: &[f32], rate: u32, from: f64, to: f64) -> f32 {
    let track = pyin(audio, rate, &PyinParams::default());
    let mut cents: Vec<f32> = (0..track.len())
        .filter(|i| track.time(*i) >= from && track.time(*i) < to)
        .filter_map(|i| track.cents(i))
        .collect();
    assert!(!cents.is_empty(), "nothing voiced between {from} and {to}");
    cents.sort_by(f32::total_cmp);
    cents[cents.len() / 2]
}

fn read_wav(path: &Path) -> (Vec<f32>, u32) {
    let decoded = fontelle_assets::import_audio(path).expect("a WAV");
    let channels = usize::from(decoded.channels.max(1));
    let mono = decoded
        .samples
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect();
    (mono, decoded.sample_rate)
}

#[test]
fn a_moved_note_is_a_study_in_the_song_and_one_undo() {
    let (dir, mut session, clip) = analysed("study");
    let n = note(&session, 2);
    assert!(n.edit.is_none());
    session
        .set_analysis_edits(&[moved(&n, 100.0)], false)
        .unwrap();
    session.end_gesture();
    let studies: Vec<_> = session.project().studies.values().cloned().collect();
    assert_eq!(studies.len(), 1);
    assert_eq!(studies[0].source, StudySource::Clip(clip));
    assert_eq!(studies[0].pitch_edits.len(), 1);
    let edit = &studies[0].pitch_edits[0];
    assert_eq!(edit.shift_cents, 100.0);
    // By span of samples of the file: the note's seconds at its rate.
    assert!(
        (edit.span.0 as f64 / f64::from(SR) - n.start).abs() < 0.002,
        "{edit:?}"
    );
    // The window sees it on the note.
    assert_eq!(note(&session, 2).edit.map(|e| e.shift_cents), Some(100.0));
    assert!(note(&session, 1).edit.is_none());

    session.undo();
    assert!(note(&session, 2).edit.is_none(), "one undo takes it back");
    std::fs::remove_dir_all(&dir).ok();
}

/// A drag: many steps, merged, closed on release — one undo.
#[test]
fn a_drag_is_one_undo() {
    let (dir, mut session, _) = analysed("drag");
    let n = note(&session, 2);
    for cents in [20.0, 60.0, 100.0, 140.0, 200.0] {
        session
            .set_analysis_edits(&[moved(&n, cents)], true)
            .unwrap();
    }
    session.end_gesture();
    assert_eq!(note(&session, 2).edit.map(|e| e.shift_cents), Some(200.0));
    session
        .set_analysis_edits(&[moved(&n, 300.0)], false)
        .unwrap();
    session.end_gesture();
    session.undo();
    assert_eq!(note(&session, 2).edit.map(|e| e.shift_cents), Some(200.0));
    session.undo();
    assert!(note(&session, 2).edit.is_none());
    // Del: reset is an edit taken off.
    session.redo();
    session
        .set_analysis_edits(
            &[AnalyzeEditChange {
                start: n.start,
                end: n.end,
                edit: None,
            }],
            false,
        )
        .unwrap();
    assert!(note(&session, 2).edit.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// Ty: *"when i preview a note its not playing that section repitched to
/// the new note, its just playing like a synth wave"*. Auditioning a moved
/// note plays the note's own span of the audio, moved: the preview player,
/// heard here through its node, sings it 200 cents up.
#[test]
fn auditioning_a_moved_note_plays_its_own_audio_moved() {
    let (dir, mut session, _) = analysed("audition");
    let n = note(&session, 2);
    let before = f32::from(n.midi) * 100.0 + n.cents;
    session
        .set_analysis_edits(&[moved(&n, 200.0)], false)
        .unwrap();
    session.end_gesture();
    assert!(session.analyze_view().unwrap().preview_pending);
    settle(&mut session);

    session.analysis_play(n.start, Some(n.end), false);
    let playhead = session.analysis_playhead().expect("playing");
    assert!((playhead - n.start).abs() < 0.01, "{playhead}");

    // The engine's side: a node on the session's player, as the graph has.
    let mut node = StudyPlayerNode::new(session.study_player());
    node.prepare(&PrepareContext {
        sample_rate: SR as f32,
        max_block_size: 256,
    });
    let mut out = Vec::new();
    let blocks = ((n.end - n.start + 0.1) * f64::from(SR) / 256.0) as usize;
    for _ in 0..blocks {
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        {
            let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut channels,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: Default::default(),
                sample_range: 0..256,
            };
            node.process(&mut ctx);
        }
        out.extend(left);
    }
    assert!(
        !session.study_player().playing(),
        "it stopped at the note's end"
    );
    let length = n.end - n.start;
    let after = heard(&out, SR, length * 0.25, length * 0.75);
    println!(
        "note {}: {before:.1} ct as sung, {after:.1} ct auditioned after +200",
        n.midi
    );
    assert!((after - before - 200.0).abs() < 8.0, "{before} -> {after}");
    // B: the original, in place.
    session.analysis_set_original(true);
    assert!(session.study_player().original());
    session.analysis_stop();
    assert!(session.analysis_playhead().is_none() || !session.study_player().playing());
    std::fs::remove_dir_all(&dir).ok();
}

/// Render to clip: a WAV in the bundle's `renders/`, the clip's whole file
/// long, the clip playing it, the study stamped; one undo puts the original
/// back; Revert does too.
#[test]
fn render_swaps_the_clip_and_undo_restores_it() {
    let (dir, mut session, clip) = analysed("render");
    let original = the_asset(&session, clip);
    let n = note(&session, 2);
    session
        .set_analysis_edits(&[moved(&n, 100.0)], false)
        .unwrap();
    session.end_gesture();
    let said = session.render_analysis(false).expect("renders");
    println!("{said}");
    let rendered = the_asset(&session, clip);
    assert_ne!(rendered, original);
    let path = dir.join("Song").join("renders").join("Take (edited 1).wav");
    assert!(path.is_file(), "{}", path.display());
    let (source, _) = read_wav(&dir.join("Take.wav"));
    let (render, rate) = read_wav(&path);
    assert_eq!(render.len(), source.len(), "whole-file length");
    let sung = heard(&source, rate, n.start + 0.1, n.end - 0.1);
    let moved_to = heard(&render, rate, n.start + 0.1, n.end - 0.1);
    assert!(
        (moved_to - sung - 100.0).abs() < 5.0,
        "{sung} -> {moved_to}"
    );
    let study = session.project().studies.values().next().unwrap().clone();
    assert_eq!(study.original, original);
    assert_eq!(study.rendered.as_ref(), Some(&rendered));
    assert!(session.analyze_view().unwrap().rendered);

    session.undo();
    assert_eq!(the_asset(&session, clip), original, "one undo");
    assert!(
        session
            .project()
            .studies
            .values()
            .next()
            .unwrap()
            .rendered
            .is_none()
    );
    session.redo();
    assert_eq!(the_asset(&session, clip), rendered);

    session.revert_analysis().expect("reverts");
    assert_eq!(the_asset(&session, clip), original);
    assert!(!session.analyze_view().unwrap().rendered);
    std::fs::remove_dir_all(&dir).ok();
}

/// A second render starts from the original: +100, then +200 rendered
/// again, is +200 — never +300 — and is a new file.
#[test]
fn a_rerender_starts_from_the_original() {
    let (dir, mut session, clip) = analysed("rerender");
    let n = note(&session, 2);
    session
        .set_analysis_edits(&[moved(&n, 100.0)], false)
        .unwrap();
    session.end_gesture();
    session.render_analysis(false).unwrap();
    // The window shows the original's notes and the edits, whatever the
    // clip plays now.
    session.close_analysis();
    session.analyze_musically(clip).unwrap();
    wait_for_analysis(&mut session);
    let again = note(&session, 2);
    assert_eq!(again.midi, n.midi, "the original's note, not the render's");
    assert_eq!(again.edit.map(|e| e.shift_cents), Some(100.0));
    session
        .set_analysis_edits(&[moved(&again, 200.0)], false)
        .unwrap();
    session.end_gesture();
    session.render_analysis(false).unwrap();
    let path = dir.join("Song").join("renders").join("Take (edited 2).wav");
    let (render, rate) = read_wav(&path);
    let (source, _) = read_wav(&dir.join("Take.wav"));
    let sung = heard(&source, rate, n.start + 0.1, n.end - 0.1);
    let now = heard(&render, rate, n.start + 0.1, n.end - 0.1);
    assert!((now - sung - 200.0).abs() < 5.0, "{sung} -> {now}");
    let _ = Arc::strong_count(&session.study_player());
    std::fs::remove_dir_all(&dir).ok();
}

/// *Render as a new clip below*: the original clip untouched, the render on
/// a row under it at the same place; one undo.
#[test]
fn render_as_a_new_clip_below() {
    let (dir, mut session, clip) = analysed("below");
    let original = the_asset(&session, clip);
    let n = note(&session, 1);
    session
        .set_analysis_edits(&[moved(&n, -100.0)], false)
        .unwrap();
    session.end_gesture();
    let clips = session.project().clips.len();
    session.render_analysis(true).unwrap();
    assert_eq!(the_asset(&session, clip), original);
    assert_eq!(session.project().clips.len(), clips + 1);
    let start = session.project().clips[clip].start;
    let made = session
        .project()
        .clips
        .iter()
        .find(|(id, _)| *id != clip && matches!(&session.project().clips[*id].source, ClipSource::Audio(d) if d.asset != original))
        .map(|(id, c)| (id, c.start))
        .expect("the render's clip");
    assert_eq!(made.1, start);
    session.undo();
    assert_eq!(session.project().clips.len(), clips);
    std::fs::remove_dir_all(&dir).ok();
}

/// The edits are the song's: saved, reopened, they are on the notes.
#[test]
fn edits_survive_a_save_and_a_reopen() {
    let (dir, mut session, clip) = analysed("persist");
    let n = note(&session, 3);
    session
        .set_analysis_edits(&[moved(&n, -50.0)], false)
        .unwrap();
    session.end_gesture();
    session.render_analysis(false).unwrap();
    DocumentHost::save(&mut session).expect("saves");
    session.close_analysis();
    let bundle = dir.join("Song");
    session.open_project_path(&bundle).expect("reopens");
    session
        .analyze_musically(clip)
        .expect("the clip is still audio");
    wait_for_analysis(&mut session);
    let view = session.analyze_view().unwrap();
    assert!(view.rendered);
    assert_eq!(view.melody[3].edit.map(|e| e.shift_cents), Some(-50.0));
    std::fs::remove_dir_all(&dir).ok();
}
