//! Bouncing a project to a WAV, and keeping a copy while you work
//! (items 10 and 11 of `docs/first-usable-plan.md`).
//!
//! Two halves of the same worry — *what happens to what I made* — and both
//! write into folders every project bundle has already been reserving since
//! §17.1 was implemented and which nothing has ever written to: `renders/`
//! and `backups/`.
//!
//! # Export closes the gate sentence
//!
//! §3's one-sentence gate ends *"…and export a WAV"*. `render_offline` has
//! existed since M0 and renders at `RENDER_QUALITY`; what was missing was
//! somewhere to put the result and a way to ask for it.
//!
//! # Autosave is about the hour you have not saved yet
//!
//! A `save_project` writes one JSON file — assets are referenced, not copied
//! (§17.4) — so a backup costs a few kilobytes and can be taken often.

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_engine::timeline_channel;
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::DocumentHost;

const SR: u32 = 48_000;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-export-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A session over a project saved at `<dir>/Song.fontelle`.
fn saved_session(dir: &Path) -> Session {
    let bundle = dir.join("Song.fontelle");
    let project = blank_project(2, 120.0, SR);
    fontelle_app::save_project(&project, &bundle).expect("a blank project must save");

    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    Session::new(
        project,
        SampleLibrary::new(),
        channel_nodes,
        publisher,
        RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        },
        clip,
        Some(bundle),
    )
    .with_settings_path(dir.join("settings.json"))
}

/// The same, never written to disk — `--blank` with no `--save`.
fn scratch_session(dir: &Path) -> Session {
    let project = blank_project(2, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    Session::new(
        project,
        SampleLibrary::new(),
        channel_nodes,
        publisher,
        RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        },
        clip,
        None,
    )
    .with_settings_path(dir.join("settings.json"))
}

fn a_note(session: &mut Session) {
    session.edit(RollEdit::Add {
        note: fontelle_model::Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
}

fn wavs(bundle: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(bundle.join("renders"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

// ---------------------------------------------------------------- export ---

#[test]
fn exporting_writes_a_wav_into_the_projects_own_renders_folder() {
    // The folder has been in every bundle since §17.1 and nothing had ever
    // written to it.
    let dir = scratch("writes");
    let mut session = saved_session(&dir);
    a_note(&mut session);

    let said = session.export_wav().expect("a saved project must export");
    assert_eq!(wavs(&dir.join("Song.fontelle")), vec!["Song.wav"]);
    assert!(said.contains("Song.wav"), "it says where it put it: {said}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_wav_is_a_real_stereo_file_with_audio_in_it() {
    let dir = scratch("real");
    let mut session = saved_session(&dir);
    a_note(&mut session);
    session.export_wav().unwrap();

    let path = dir.join("Song.fontelle").join("renders").join("Song.wav");
    let bytes = std::fs::read(&path).expect("the file is there");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    // Sixteen-bit stereo at the session's own rate.
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "channels");
    assert_eq!(
        u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
        SR,
        "sample rate"
    );
    assert!(
        bytes.len() > 44,
        "a header and no audio is not an export: {} bytes",
        bytes.len()
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_export_does_not_overwrite_the_first() {
    // Bouncing twice to compare them is the ordinary thing to do, and a render
    // that silently replaced the one you were comparing against would be the
    // worst possible time to find that out.
    let dir = scratch("twice");
    let mut session = saved_session(&dir);
    a_note(&mut session);

    session.export_wav().unwrap();
    session.export_wav().unwrap();
    assert_eq!(
        wavs(&dir.join("Song.fontelle")),
        vec!["Song 2.wav", "Song.wav"]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_with_no_file_of_its_own_says_so_rather_than_guessing() {
    // A render lives inside the bundle, so there has to be a bundle. Guessing
    // at somewhere else to put it would be a write outside anywhere the user
    // named (INVARIANT 10).
    let dir = scratch("unsaved");
    let mut session = scratch_session(&dir);
    a_note(&mut session);

    let error = session
        .export_wav()
        .expect_err("there is nowhere to put it");
    assert!(
        error.to_lowercase().contains("save"),
        "the message has to say what to do about it: {error}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_empty_project_still_exports_rather_than_refusing() {
    // A silent bounce is a legitimate thing to have asked for, and "there are
    // no notes" is not an error the user needs to be stopped by.
    let dir = scratch("silent");
    let mut session = saved_session(&dir);

    session.export_wav().expect("an empty project exports");
    assert_eq!(wavs(&dir.join("Song.fontelle")), vec!["Song.wav"]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exporting_leaves_the_document_alone() {
    // A bounce is a read. It must not mark the project dirty, move the
    // playhead, or change what is open.
    let dir = scratch("readonly");
    let mut session = saved_session(&dir);
    let clip = session.notes().len();
    session.export_wav().unwrap();

    assert!(!session.is_dirty(), "a bounce is not an edit");
    assert_eq!(session.notes().len(), clip);

    std::fs::remove_dir_all(&dir).ok();
}

// -------------------------------------------------------------- autosave ---

fn backups(bundle: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(bundle.join("backups"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn autosaving_writes_a_copy_into_the_projects_own_backups_folder() {
    let dir = scratch("autosave");
    let mut session = saved_session(&dir);
    a_note(&mut session);

    assert!(session.autosave(), "there were unsaved changes");
    let bundle = dir.join("Song.fontelle");
    assert_eq!(backups(&bundle), vec!["autosave.fontelle"]);
    // A whole bundle, so it opens with the ordinary code path rather than
    // needing somebody to rename a file by hand.
    let recovered = fontelle_app::open_project(&bundle.join("backups").join("autosave.fontelle"))
        .expect("the backup opens like any other project");
    assert_eq!(
        recovered
            .project
            .clips
            .values()
            .filter_map(|c| match &c.source {
                fontelle_model::ClipSource::Notes(d) => Some(d.notes.len()),
                _ => None,
            })
            .sum::<usize>(),
        1,
        "and it holds the note that had not been saved"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_autosave_does_not_count_as_saving_the_project() {
    // The document is still unsaved: the title bar's dot has to stay, or
    // somebody quits believing their work is on disk where they put it.
    let dir = scratch("still-dirty");
    let mut session = saved_session(&dir);
    a_note(&mut session);
    assert!(session.is_dirty());

    session.autosave();
    assert!(
        session.is_dirty(),
        "a backup is not the file you were editing"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn nothing_is_written_when_there_is_nothing_new_to_write() {
    // The window is asleep at idle (§16.3) and an autosave that ran anyway
    // would be a timer waking it to write a file identical to the last one.
    let dir = scratch("clean");
    let mut session = saved_session(&dir);

    assert!(!session.autosave(), "nothing has changed since the save");
    assert!(backups(&dir.join("Song.fontelle")).is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_with_no_file_of_its_own_is_not_autosaved() {
    // Nowhere to put it that the user named. Every project *made in the
    // window* is written to disk the moment it is made, so this is the
    // scratch session `--blank` opens and nothing else.
    let dir = scratch("no-bundle");
    let mut session = scratch_session(&dir);
    a_note(&mut session);
    assert!(!session.autosave());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_properly_clears_what_the_backup_was_protecting() {
    let dir = scratch("saved");
    let mut session = saved_session(&dir);
    a_note(&mut session);
    session.autosave();

    session.save().expect("it has a file");
    assert!(!session.is_dirty());
    assert!(
        !session.autosave(),
        "there is nothing left for a backup to protect"
    );

    std::fs::remove_dir_all(&dir).ok();
}
