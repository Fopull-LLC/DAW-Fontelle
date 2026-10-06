//! What a clip's name menu does to the song: rename it, render just it, and
//! — for audio — the Analyze Musically hook.
//!
//! Ty: *"make it so you can click that name to open a little menu thats like
//! a right click menu and in it gives you a bunch of options, like rendering
//! just that clip into audio, renaming it, and a new option for audio,
//! analyze musically."*

mod common;

use std::path::PathBuf;

use fontelle_app::Session;
use fontelle_types::PPQN;
use fontelle_ui::canvas::ArrangeEdit;
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-clip-menu-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_saved_session(dir: &std::path::Path) -> Session {
    common::a_session_in(
        common::a_project_with_a_clip(4, 120.0, SR),
        Some(dir.join("Song")),
    )
}

fn name_of(session: &Session, id: fontelle_types::ClipId) -> String {
    session
        .clips()
        .into_iter()
        .find(|c| c.id == id)
        .expect("the clip")
        .name
}

// ------------------------------------------------------------- rename ---

#[test]
fn a_renamed_clip_is_captioned_with_its_name_and_one_undo_takes_it_back() {
    let dir = scratch("rename");
    let mut session = a_saved_session(&dir);
    let clip = session.clips()[0].clone();
    let caption = clip.name.clone();

    session.rename_clip(clip.id, "Verse riff");
    session.end_gesture();
    assert_eq!(name_of(&session, clip.id), "Verse riff");
    assert!(session.is_dirty(), "a rename is an edit to the song");

    session.undo();
    assert_eq!(name_of(&session, clip.id), caption, "back to what it plays");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_blank_name_puts_the_caption_back() {
    let dir = scratch("blank");
    let mut session = a_saved_session(&dir);
    let clip = session.clips()[0].clone();
    session.rename_clip(clip.id, "Hook");
    session.end_gesture();
    session.rename_clip(clip.id, "   ");
    session.end_gesture();
    assert_eq!(name_of(&session, clip.id), clip.name);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_copy_of_a_named_clip_carries_its_name() {
    let dir = scratch("copy");
    let mut session = a_saved_session(&dir);
    let clip = session.clips()[0].clone();
    session.rename_clip(clip.id, "Hook");
    session.end_gesture();
    let made = session.arrange(ArrangeEdit::Stamp {
        source: clip.id,
        lane: 0,
        start: PPQN * 32,
    });
    assert_eq!(name_of(&session, made.clips[0]), "Hook");
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- render ---

#[test]
fn rendering_one_clip_bounces_just_it_onto_a_row_named_after_it() {
    let dir = scratch("render");
    let mut session = a_saved_session(&dir);
    let first = session.clips()[0].clone();
    // A second clip on the same row, well after the first: a render of the
    // row would run on to it; a render of the clip must not.
    let made = session.arrange(ArrangeEdit::Stamp {
        source: first.id,
        lane: 0,
        start: first.start + first.length + PPQN * 32,
    });
    assert_eq!(made.clips.len(), 1);
    session.end_gesture();
    session.rename_clip(first.id, "Hook");
    session.end_gesture();
    let rows = session.lanes().len();

    session.render_clip(first.id).expect("renders");

    let lanes = session.lanes();
    assert_eq!(lanes.len(), rows + 1, "a row was made for it");
    assert_eq!(lanes[1].name, "Hook (rendered)");
    let take = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a take");
    assert_eq!(take.lane, 1, "under the clip's own row");
    assert_eq!(take.start, first.start, "where the clip is");
    assert!(
        take.length >= first.length,
        "the whole clip: {}",
        take.length
    );
    assert!(
        take.length < first.length + PPQN * 16,
        "and not on to the next clip on its row: {} ticks",
        take.length
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_clip_that_is_not_there_is_not_rendered() {
    let dir = scratch("gone");
    let mut session = a_saved_session(&dir);
    let clip = session.clips()[0].id;
    session.arrange(ArrangeEdit::Remove(vec![clip]));
    assert!(session.render_clip(clip).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------- analyze musically ---

#[test]
fn analyzing_a_note_clip_is_refused_and_an_audio_clip_reaches_the_hook() {
    let dir = scratch("analyze");
    let mut session = a_saved_session(&dir);
    let notes = session.clips()[0].id;
    assert!(
        StudioHost::analyze_musically(&mut session, notes).is_err(),
        "only audio is analysed"
    );
    // An audio clip, made the way a render makes one.
    session.render_clip(notes).expect("renders");
    let audio = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a take")
        .id;
    let said = StudioHost::analyze_musically(&mut session, audio).expect("the hook answers");
    assert!(said.contains("Analyze Musically"), "{said}");
    std::fs::remove_dir_all(&dir).ok();
}
