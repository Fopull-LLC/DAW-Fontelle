//! A bounce runs beside the window, not in it.
//!
//! > *"right now the program freezes during actions instead of showing
//! > progress bars for example when exporting / rendering things."*
//!
//! The window reaches an export and a row render through [`StudioHost`], and
//! through that door they **start** a job and return at once; the window then
//! asks [`StudioHost::poll_job`] each pass for how far it has got, and draws
//! that. The inherent `Session::export_wav` and `Session::render_lane` stay
//! synchronous — the command line and every other test want the file when the
//! call returns — and both are the same code, which is what
//! `the_background_export_writes_the_same_file_the_foreground_one_does` holds
//! them to.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_ui::document::{
    DocumentHost, ExportOptions, ExportRange, ExportTail, JobPoll, StudioHost,
};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-jobs-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_saved_session(dir: &Path) -> Session {
    common::a_session_in(
        common::a_project_with_a_clip(4, 120.0, SR),
        Some(dir.join("Song.fontelle")),
    )
}

fn whole_song() -> ExportOptions {
    ExportOptions {
        range: ExportRange::WholeSong,
        tail: ExportTail::Keep,
    }
}

/// Polls until the job says it is done, keeping every progress report on the
/// way. A job that never finishes fails the test rather than hanging it.
fn finish(session: &mut Session) -> (Result<String, String>, Vec<Option<f32>>) {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut seen = Vec::new();
    loop {
        match StudioHost::poll_job(session) {
            JobPoll::Idle => panic!("the job vanished without finishing"),
            JobPoll::Running(progress) => {
                assert!(!progress.label.is_empty(), "a bar with no words");
                seen.push(progress.fraction);
            }
            JobPoll::Finished(result) => return (result, seen),
        }
        assert!(Instant::now() < deadline, "the job never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wavs(bundle: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(bundle.join("renders"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

#[test]
fn nothing_is_running_until_something_is_started() {
    let dir = scratch("idle");
    let mut session = a_saved_session(&dir);
    assert_eq!(StudioHost::poll_job(&mut session), JobPoll::Idle);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_export_through_the_window_returns_at_once_and_finishes_in_the_background() {
    let dir = scratch("export");
    let mut session = a_saved_session(&dir);

    StudioHost::export_wav_with(&mut session, whole_song()).expect("it starts");
    let (result, seen) = finish(&mut session);
    let said = result.expect("the export succeeds");
    assert!(said.contains("Song.wav"), "it says where it went: {said}");
    assert_eq!(wavs(&dir.join("Song.fontelle")).len(), 1);

    // What progress there was only ever went forwards, inside 0..=1.
    let known: Vec<f32> = seen.into_iter().flatten().collect();
    assert!(known.iter().all(|f| (0.0..=1.0).contains(f)), "{known:?}");
    assert!(
        known.windows(2).all(|w| w[1] >= w[0]),
        "went back: {known:?}"
    );

    // Told once: the finish is taken, and the host is idle again.
    assert_eq!(StudioHost::poll_job(&mut session), JobPoll::Idle);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_background_export_writes_the_same_file_the_foreground_one_does() {
    let dir = scratch("same");
    let mut session = a_saved_session(&dir);

    session.export_wav().expect("the foreground export");
    StudioHost::export_wav_with(&mut session, whole_song()).expect("it starts");
    finish(&mut session).0.expect("the background export");

    let files = wavs(&dir.join("Song.fontelle"));
    assert_eq!(files.len(), 2, "{files:?}");
    let a = std::fs::read(&files[0]).unwrap();
    let b = std::fs::read(&files[1]).unwrap();
    assert_eq!(a.len(), b.len(), "the two exports are different lengths");
    assert!(a == b, "the two exports differ");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_export_that_cannot_start_says_so_at_once_and_leaves_nothing_running() {
    let dir = scratch("refused");
    let mut session = common::a_session_in(common::a_project_with_a_clip(2, 120.0, SR), None);
    let refused = StudioHost::export_wav_with(&mut session, whole_song());
    assert!(refused.is_err(), "a project with no folder cannot export");
    assert_eq!(StudioHost::poll_job(&mut session), JobPoll::Idle);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_row_rendered_through_the_window_lands_when_the_job_finishes() {
    let dir = scratch("row");
    let mut session = a_saved_session(&dir);
    let rows = session.lanes().len();

    StudioHost::render_lane(&mut session, 0, None).expect("it starts");
    let said = finish(&mut session).0.expect("the render succeeds");
    assert!(said.contains("Rendered"), "{said}");

    let lanes = session.lanes();
    assert_eq!(lanes.len(), rows + 1, "no row was made for the render");
    assert_eq!(lanes[1].name, "Lane 1 (rendered)");
    assert!(session.is_dirty(), "a new row is an unsaved change");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_job_waits_its_turn_rather_than_racing_the_first() {
    let dir = scratch("second");
    let mut session = a_saved_session(&dir);
    StudioHost::export_wav_with(&mut session, whole_song()).expect("it starts");
    // Either it is refused because the first is still running, or the first
    // was already done — never two at once writing into one folder.
    if let JobPoll::Running(_) = StudioHost::poll_job(&mut session) {
        let second = StudioHost::export_wav_with(&mut session, whole_song());
        assert!(second.is_err(), "a second export started over the first");
    }
    finish_or_idle(&mut session);
    std::fs::remove_dir_all(&dir).ok();
}

fn finish_or_idle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !matches!(
        StudioHost::poll_job(session),
        JobPoll::Idle | JobPoll::Finished(_)
    ) {
        assert!(Instant::now() < deadline, "the job never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
}
