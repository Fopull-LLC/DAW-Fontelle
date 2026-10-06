//! Analyze Musically's Record page and the mixer insert through the studio
//! (`docs/analyze-musically-plan.md` §6.1, P5). Ty: *"you should be able to
//! add it to a mixer track as a plugin like you can with edison in fl to
//! record into it like that and then send something into the playlist"*.
//!
//! The engine's half is fed by hand here, block by block, exactly as the
//! insert's node would (`tests/analyze_insert_takes.rs` does the same): a
//! test has no audio device, and what is under test is what the studio does
//! with the takes.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_engine::{AnalyzeCapture, AnalyzeTapPoint, TransportSnapshot, TransportState};
use fontelle_types::{AnalyzeConfig, ArmMode, EffectConfig, EffectKind, StudyCompSpan};
use fontelle_ui::canvas::{AnalyzeRecordOp, AnalyzeSliceLayout, AnalyzeSource, AnalyzeTakeOp};
use fontelle_ui::document::{DocumentHost, JobPoll, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-analyze-record-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A saved song with a mixer track carrying an Analyze Musically insert,
/// and the strip it is on.
fn with_an_insert(name: &str) -> (PathBuf, Session, usize) {
    let dir = scratch(name);
    let mut session = common::a_session_in(
        common::a_project_with_a_clip(16, 120.0, SR),
        Some(dir.join("Song")),
    );
    session.add_mixer_track();
    let strip = session.mixer_strips().len() - 2;
    session.add_insert(strip, EffectKind::Analyze);
    (dir, session, strip)
}

/// The insert's capture: what its node writes into.
fn capture(session: &Session, strip: usize) -> std::sync::Arc<AnalyzeCapture> {
    let track = session.project().mixer.ordered_tracks()[strip];
    session
        .analyze_capture(track, 0)
        .expect("a live graph gives the insert a capture")
}

/// `blocks` blocks of a 220 Hz tone through the capture as the insert
/// hears them, the transport `rolling` from song sample `at` or stopped.
fn play(session: &Session, strip: usize, blocks: usize, rolling: bool, at: i64) {
    let capture = capture(session, strip);
    let track = session.project().mixer.ordered_tracks()[strip];
    let config = match &session.project().mixer.tracks[track].inserts[0].config {
        EffectConfig::Analyze(c) => *c,
        _ => panic!("analyze"),
    };
    capture.configure(&config, false);
    for block in 0..blocks {
        let mut left: Vec<f32> = (0..128)
            .map(|i| {
                let t = (block * 128 + i) as f32 / SR as f32;
                0.4 * (std::f32::consts::TAU * 220.0 * t).sin()
            })
            .collect();
        let mut right = left.clone();
        capture.capture(
            &[&mut left, &mut right],
            AnalyzeTapPoint::PreFader,
            &TransportSnapshot {
                state: if rolling {
                    TransportState::Playing
                } else {
                    TransportState::Stopped
                },
                position_sample: at + (block * 128) as i64,
                ..Default::default()
            },
            SR as f32,
        );
    }
}

/// One take recorded: armed, played, disarmed, and waited for until it is
/// in the study.
fn record_a_take(session: &mut Session, strip: usize, rolling: bool, at: i64) {
    let before = takes(session);
    session
        .analysis_record(AnalyzeRecordOp::Arm(true))
        .expect("arms");
    play(session, strip, 200, rolling, at);
    session
        .analysis_record(AnalyzeRecordOp::Arm(false))
        .expect("disarms");
    // The block after the disarm is where the insert ends the take.
    play(session, strip, 1, rolling, at);
    let started = Instant::now();
    while takes(session) == before {
        let _ = session.poll_analysis();
        assert!(started.elapsed() < Duration::from_secs(10), "no take came");
        std::thread::sleep(Duration::from_millis(5));
    }
    wait_for_analysis(session);
}

fn takes(session: &Session) -> usize {
    session.analyze_view().map_or(0, |v| v.takes.len())
}

fn wait_for_analysis(session: &mut Session) {
    let started = Instant::now();
    while matches!(session.poll_analysis(), JobPoll::Running(_)) {
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn rendered(session: &mut Session) -> Result<String, String> {
    let started = Instant::now();
    loop {
        match session.poll_analysis_render() {
            JobPoll::Finished(result) => return result,
            JobPoll::Running(_) => {}
            JobPoll::Idle => panic!("no render was running"),
        }
        assert!(started.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn take_path(session: &Session, index: usize) -> PathBuf {
    session.analyze_view().unwrap().takes[index]
        .asset
        .path
        .clone()
}

/// Ty: *"add it to a mixer track as a plugin like you can with edison"* —
/// its slot opens this window, bound to a study of its own, the same one
/// every time.
#[test]
fn the_insert_slot_opens_the_window_on_a_study_of_its_own() {
    let (dir, mut session, strip) = with_an_insert("slot");
    assert!(session.is_analyze_insert(strip, 0));
    assert!(!session.is_analyze_insert(strip, 1));
    session.open_insert_analysis(strip, 0).expect("opens");
    let track = session.project().mixer.ordered_tracks()[strip];
    let key = match &session.project().mixer.tracks[track].inserts[0].config {
        EffectConfig::Analyze(c) => c.study.expect("bound on first open"),
        _ => panic!("analyze"),
    };
    let view = session.analyze_view().expect("a view before any take");
    assert!(matches!(view.source, AnalyzeSource::Insert { .. }));
    assert!(view.record.is_some(), "the Record page is live");
    assert!(!view.has_audio, "nothing recorded yet");
    session.close_analysis();
    session.open_insert_analysis(strip, 0).unwrap();
    match &session.project().mixer.tracks[track].inserts[0].config {
        EffectConfig::Analyze(c) => assert_eq!(c.study, Some(key), "the same study"),
        _ => panic!("analyze"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// A take lands in the study and not the arrangement, recorded with the
/// transport stopped (Now), and its file is in the song's `recordings/`.
#[test]
fn a_take_lands_in_the_study_not_the_arrangement_with_the_transport_stopped() {
    let (dir, mut session, strip) = with_an_insert("lands");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::Now))
        .unwrap();
    let clips = session.project().clips.len();
    record_a_take(&mut session, strip, false, 0);
    assert_eq!(
        session.project().clips.len(),
        clips,
        "not on the arrangement"
    );
    let study = session
        .project()
        .studies
        .values()
        .next()
        .expect("a study")
        .clone();
    assert_eq!(study.takes.len(), 1);
    assert_eq!(study.takes[0].frames, 200 * 128);
    assert_eq!(study.takes[0].song_sample, None, "a free take");
    assert_eq!(study.current_take, Some(1), "the first take is in the lane");
    let path = take_path(&session, 0);
    assert!(
        path.starts_with(dir.join("Song").join("recordings")),
        "{}",
        path.display()
    );
    let view = session.analyze_view().unwrap();
    assert!(view.has_audio);
    assert!((view.duration - 200.0 * 128.0 / f64::from(SR)).abs() < 0.01);
    std::fs::remove_dir_all(&dir).ok();
}

/// Discard takes a take out, one undo — and its file goes only once nothing
/// could bring it back.
#[test]
fn discard_deletes_a_file_only_once_nothing_names_it() {
    let (dir, mut session, strip) = with_an_insert("discard");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::Now))
        .unwrap();
    record_a_take(&mut session, strip, false, 0);
    record_a_take(&mut session, strip, false, 0);
    let second = take_path(&session, 1);
    let id = session.analyze_view().unwrap().takes[1].id;
    session.analysis_take(AnalyzeTakeOp::Discard(id)).unwrap();
    assert_eq!(takes(&session), 1);
    assert!(second.is_file(), "an undo could bring it back");
    session.undo();
    assert_eq!(takes(&session), 2, "one undo");
    // A third take, undone, and the redo cut by another edit: nothing names
    // it any more, and the window closing sweeps it.
    record_a_take(&mut session, strip, false, 0);
    let third = take_path(&session, 2);
    session.undo();
    assert_eq!(takes(&session), 2);
    session
        .set_analysis_markers(
            vec![fontelle_types::StudyMarker {
                id: 1,
                at: 100,
                name: String::new(),
            }],
            false,
        )
        .unwrap();
    session.end_gesture();
    session.close_analysis();
    assert!(!third.is_file(), "nothing could bring take 3 back");
    assert!(
        second.is_file() && take_path_exists(&session),
        "the kept ones stay"
    );
    std::fs::remove_dir_all(&dir).ok();
}

fn take_path_exists(session: &Session) -> bool {
    session
        .project()
        .studies
        .values()
        .flat_map(|s| s.takes.iter())
        .all(|t| t.asset.path.is_file())
}

/// Send to arrangement: the take, as a clip at the song position it was
/// recorded from, one undo.
#[test]
fn send_to_arrangement_lands_at_the_recorded_song_position_as_one_undo() {
    let (dir, mut session, strip) = with_an_insert("send");
    session.open_insert_analysis(strip, 0).unwrap();
    // On play (the default): the transport rolling from two seconds in.
    let at = i64::from(SR) * 2;
    record_a_take(&mut session, strip, true, at);
    let study = session.project().studies.values().next().unwrap().clone();
    assert_eq!(study.takes[0].song_sample, Some(at));
    let clips = session.project().clips.len();
    session
        .send_analysis_to_arrangement(0)
        .expect("a send starts");
    rendered(&mut session).expect("it lands");
    assert_eq!(session.project().clips.len(), clips + 1);
    let tempo = session.project().tempo_map.clone();
    let made = session
        .project()
        .clips
        .values()
        .find(|c| matches!(&c.source, fontelle_model::ClipSource::Audio(d) if d.asset.path.to_string_lossy().contains("sent")))
        .expect("the sent clip")
        .clone();
    assert_eq!(made.start, tempo.sample_to_tick(at));
    session.undo();
    assert_eq!(session.project().clips.len(), clips, "one undo");
    std::fs::remove_dir_all(&dir).ok();
}

/// A free take goes to the playhead.
#[test]
fn a_free_take_is_sent_to_the_playhead() {
    let (dir, mut session, strip) = with_an_insert("free");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::Now))
        .unwrap();
    record_a_take(&mut session, strip, false, 0);
    let playhead = i64::from(SR) * 3;
    session.send_analysis_to_arrangement(playhead).unwrap();
    rendered(&mut session).unwrap();
    let tempo = session.project().tempo_map.clone();
    assert!(
        session
            .project()
            .clips
            .values()
            .any(|c| c.start == tempo.sample_to_tick(playhead))
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A take loaded into the lane is what the window studies; one undo puts
/// the last one back. A comp of two takes becomes a take of its own.
#[test]
fn takes_load_into_the_lane_and_a_comp_becomes_a_take() {
    let (dir, mut session, strip) = with_an_insert("comp");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::Now))
        .unwrap();
    record_a_take(&mut session, strip, false, 0);
    record_a_take(&mut session, strip, false, 0);
    let view = session.analyze_view().unwrap();
    let (first, second) = (view.takes[0].id, view.takes[1].id);
    assert_eq!(view.current_take, Some(first));
    session.analysis_take(AnalyzeTakeOp::Load(second)).unwrap();
    wait_for_analysis(&mut session);
    assert_eq!(session.analyze_view().unwrap().current_take, Some(second));
    session.undo();
    assert_eq!(session.analyze_view().unwrap().current_take, Some(first));
    // Star and rename are the takes list's own, each one undo.
    session.analysis_take(AnalyzeTakeOp::Star(second)).unwrap();
    assert!(session.analyze_view().unwrap().takes[1].starred);
    session
        .analysis_take(AnalyzeTakeOp::Rename(first, "Keeper".to_string()))
        .unwrap();
    assert_eq!(session.analyze_view().unwrap().takes[0].name, "Keeper");
    let half = 100 * 128;
    session
        .set_analysis_comp(
            vec![
                StudyCompSpan {
                    take: first,
                    start: 0,
                    end: half,
                },
                StudyCompSpan {
                    take: second,
                    start: half,
                    end: 2 * half,
                },
            ],
            false,
        )
        .unwrap();
    session.analysis_take(AnalyzeTakeOp::UseComp).unwrap();
    let view = session.analyze_view().unwrap();
    assert_eq!(view.takes.len(), 3);
    assert_eq!(view.takes[2].name, "Comp 1");
    assert_eq!(view.current_take, Some(view.takes[2].id));
    std::fs::remove_dir_all(&dir).ok();
}

/// The arm settings are the insert's own saved settings: changed from the
/// Record page, they are its parameters, each one undo.
#[test]
fn the_arm_settings_are_the_inserts() {
    let (dir, mut session, strip) = with_an_insert("arm");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::OnInput))
        .unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Threshold(-30.0))
        .unwrap();
    session
        .analysis_record(AnalyzeRecordOp::PostFader(true))
        .unwrap();
    let track = session.project().mixer.ordered_tracks()[strip];
    let config = match &session.project().mixer.tracks[track].inserts[0].config {
        EffectConfig::Analyze(c) => *c,
        _ => panic!("analyze"),
    };
    assert_eq!(config.arm, ArmMode::OnInput);
    assert_eq!(config.threshold_db, -30.0);
    assert!(config.post_fader);
    let record = session.analyze_view().unwrap().record.unwrap();
    assert_eq!(record.arm, ArmMode::OnInput);
    session.undo();
    let config: AnalyzeConfig = match &session.project().mixer.tracks[track].inserts[0].config {
        EffectConfig::Analyze(c) => *c,
        _ => panic!("analyze"),
    };
    assert!(!config.post_fader, "one undo each");
    std::fs::remove_dir_all(&dir).ok();
}

/// Send to sampler from the window: the take, cut where the window says,
/// on a new Sampler with a clip replaying the slices — one undo — and the
/// keyboard preview says where they land.
#[test]
fn send_to_sampler_from_the_window_is_one_undo() {
    let (dir, mut session, strip) = with_an_insert("sampler");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::Now))
        .unwrap();
    record_a_take(&mut session, strip, false, 0);
    let cuts = [0.1, 0.2, 0.3];
    let keys = session.analysis_slice_keys(&cuts, AnalyzeSliceLayout::Chop);
    assert_eq!(keys.len(), 4, "{keys:?}");
    assert_eq!(keys[0].low, 48);
    assert_eq!(keys[3].low, 51);
    let channels = session.project().channels.len();
    let clips = session.project().clips.len();
    session
        .send_analysis_to_sampler(&cuts, AnalyzeSliceLayout::Chop, true, 0)
        .expect("starts");
    let said = rendered(&mut session).expect("lands");
    assert!(said.contains("4 slices"), "{said}");
    assert_eq!(session.project().channels.len(), channels + 1);
    assert_eq!(session.project().clips.len(), clips + 1, "the replay clip");
    session.undo();
    assert_eq!(session.project().channels.len(), channels, "one undo");
    assert_eq!(session.project().clips.len(), clips);
    std::fs::remove_dir_all(&dir).ok();
}

/// Ty's §6 answer 4: a study is never lost when its window closes — it is
/// listed, and opens again; an insert's study outlives the insert.
#[test]
fn studies_are_listed_and_open_again_even_after_their_insert_goes() {
    let (dir, mut session, strip) = with_an_insert("listed");
    session.open_insert_analysis(strip, 0).unwrap();
    session
        .analysis_record(AnalyzeRecordOp::Mode(ArmMode::Now))
        .unwrap();
    record_a_take(&mut session, strip, false, 0);
    session.close_analysis();
    let rows = session.analysis_studies();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].place.starts_with("insert on"), "{:?}", rows[0]);
    assert!(!rows[0].open);
    session.open_study_analysis(rows[0].id).unwrap();
    assert!(matches!(
        session.analyze_view().unwrap().source,
        AnalyzeSource::Insert { .. }
    ));
    assert!(session.analysis_studies()[0].open);
    session.close_analysis();
    session.remove_insert(strip, 0);
    let rows = session.analysis_studies();
    assert_eq!(rows.len(), 1, "still there");
    assert!(rows[0].place.starts_with("recording"), "{:?}", rows[0]);
    session.open_study_analysis(rows[0].id).unwrap();
    wait_for_analysis(&mut session);
    let view = session.analyze_view().unwrap();
    assert_eq!(view.source, AnalyzeSource::Standalone);
    assert!(view.has_audio);
    assert_eq!(view.takes.len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}
