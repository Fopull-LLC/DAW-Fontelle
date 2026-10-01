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

    session.set_song_key(Some(key.clone()), fitted, Vec::new());
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
    session.set_song_key(Some(KeyScale::new(0, "major")), Vec::new(), Vec::new());
    session.set_song_key(None, Vec::new(), Vec::new());
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
        paths: Vec::new(),
    });
    session.end_gesture();
    assert_eq!(keys(&session, &ids), vec![60, 62, 67]);
    session.undo();
    assert_eq!(keys(&session, &ids), vec![60, 61, 66]);
}

/// A sliding note is fitted at each point it lands on, and the key, the
/// keys and the paths are one undo (`docs/note-paths-plan.md` §3).
#[test]
fn choosing_a_key_fits_where_the_slides_land_in_the_same_undo() {
    use fontelle_model::PathPoint;
    let mut session = common::a_session_for(common::a_clip_project(1));
    let mut sliding = note(0, 60);
    sliding.path = vec![PathPoint {
        at: PPQN / 2,
        offset: 6,
    }]; // to F#
    let id = session.edit(RollEdit::Add { note: sliding })[0];
    session.end_gesture();

    let key = KeyScale::new(0, "major");
    let mask = RollScale::of(&key).unwrap().mask;
    let fitted = scale_fit(session.notes(), &[], mask);
    let paths = fontelle_ui::canvas::scale_fit_paths(session.notes(), &[], mask);
    session.set_song_key(Some(key), fitted, paths);
    assert_eq!(
        session.notes()[id].path,
        vec![PathPoint {
            at: PPQN / 2,
            offset: 5
        }],
        "lands on F"
    );

    session.undo();
    assert_eq!(session.song_key(), None);
    assert_eq!(
        session.notes()[id].path,
        vec![PathPoint {
            at: PPQN / 2,
            offset: 6
        }]
    );

    // And fitting again from the scale chip is one edit too.
    session.edit(RollEdit::SetKeys {
        ids: Vec::new(),
        keys: Vec::new(),
        paths: vec![(
            id,
            vec![PathPoint {
                at: PPQN / 2,
                offset: 7,
            }],
        )],
    });
    session.end_gesture();
    assert_eq!(session.notes()[id].path[0].offset, 7);
    session.undo();
    assert_eq!(session.notes()[id].path[0].offset, 6);
}
