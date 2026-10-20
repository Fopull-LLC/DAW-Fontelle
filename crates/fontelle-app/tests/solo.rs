//! Solo is each studio's own, and not undoable.
//!
//! Ty, 2026-10-18: *"soloing should just be a local thing not a synced
//! state"*, *"just make it so soloing cant be undoable then"* — and an export
//! still honours it: *"if you have solo on it actually should export with that
//! solo taking effect ... i do this all the time to export a specific track"*.

mod common;

use fontelle_types::PPQN;
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{DocumentHost, StudioHost};

fn a_note() -> fontelle_model::Note {
    fontelle_model::Note {
        start: PPQN,
        length: PPQN,
        key: 64,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

#[test]
fn an_undo_after_a_solo_takes_back_the_edit_before_it_and_leaves_the_solo() {
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.add_mixer_track();
    session.edit(RollEdit::Add { note: a_note() });
    session.end_gesture();
    let notes = session.notes().len();
    session.toggle_track_solo(1);
    session.toggle_solo(0);
    session.toggle_lane_solo(0);
    session.undo();
    let song = session.project();
    assert_eq!(
        session.notes().len(),
        notes - 1,
        "the note was what the undo took back"
    );
    assert!(
        song.mixer.tracks.values().any(|t| t.solo),
        "the track is still soloed"
    );
    assert!(
        song.channels.values().any(|c| c.soloed),
        "the channel is still soloed"
    );
    assert!(
        song.lanes.values().any(|l| l.soloed),
        "the lane is still soloed"
    );
}

#[test]
fn a_solo_does_not_make_the_song_unsaved() {
    let mut session = common::a_session_for(common::a_clip_project(4));
    assert!(!session.is_dirty());
    session.toggle_solo(0);
    session.toggle_lane_solo(0);
    session.toggle_track_solo(0);
    assert!(!session.is_dirty(), "nothing in the song changed");
}
