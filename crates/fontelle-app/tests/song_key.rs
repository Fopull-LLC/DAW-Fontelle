//! The song's key, through the session the window talks to.
//!
//! > *"a scale tool so you can chose between any note and the mode or
//! > whatever and it will snap all of your notes to that scale"*
//!
//! Choosing a scale is one thing somebody did, so it is one undo: the key
//! and the notes it moved come back together.

mod common;

use fontelle_model::Note;
use fontelle_types::{KeyScale, NoteId, PPQN};
use fontelle_ui::canvas::{RollEdit, RollScale, scale_fit};
use fontelle_ui::document::{DocumentHost, StudioHost};

fn note(start: i64, key: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
    }
}

fn keys(session: &fontelle_app::Session, ids: &[NoteId]) -> Vec<u8> {
    ids.iter().map(|id| session.notes()[*id].key).collect()
}

fn a_session_with_notes() -> (fontelle_app::Session, Vec<NoteId>) {
    let mut session = common::a_session_for(common::a_clip_project(1));
    let ids = [60, 61, 66]
        .iter()
        .enumerate()
        .flat_map(|(i, key)| {
            session.edit(RollEdit::Add {
                note: note(PPQN * i as i64, *key),
            })
        })
        .collect::<Vec<_>>();
    session.end_gesture();
    assert_eq!(ids.len(), 3);
    (session, ids)
}

#[test]
fn a_new_song_has_no_key() {
    let (session, _) = a_session_with_notes();
    assert_eq!(session.song_key(), None);
}

#[test]
fn choosing_a_key_fits_the_notes_and_one_undo_takes_both_back() {
    let (mut session, ids) = a_session_with_notes();
    let key = KeyScale::new(0, "major");
    let mask = RollScale::of(&key).unwrap().mask;
    let fitted = scale_fit(session.notes(), &[], mask);
    let before = session.revision();

    session.set_song_key(Some(key.clone()), fitted);
    assert_eq!(session.song_key(), Some(key));
    assert_eq!(keys(&session, &ids), vec![60, 60, 65]);
    assert!(session.revision() > before, "the window hears about it");

    session.undo();
    assert_eq!(session.song_key(), None);
    assert_eq!(keys(&session, &ids), vec![60, 61, 66]);
}

#[test]
fn clearing_the_key_leaves_the_notes_where_they_are() {
    let (mut session, ids) = a_session_with_notes();
    session.set_song_key(Some(KeyScale::new(0, "major")), Vec::new());
    session.set_song_key(None, Vec::new());
    assert_eq!(session.song_key(), None);
    assert_eq!(keys(&session, &ids), vec![60, 61, 66]);
    session.undo();
    assert_eq!(session.song_key(), Some(KeyScale::new(0, "major")));
}

#[test]
fn fitting_again_is_one_edit_of_keys_each_its_own() {
    let (mut session, ids) = a_session_with_notes();
    session.edit(RollEdit::SetKeys {
        ids: vec![ids[1], ids[2]],
        keys: vec![62, 67],
    });
    session.end_gesture();
    assert_eq!(keys(&session, &ids), vec![60, 62, 67]);
    session.undo();
    assert_eq!(keys(&session, &ids), vec![60, 61, 66]);
}
