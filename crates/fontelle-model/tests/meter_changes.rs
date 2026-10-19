//! A song's time signature changes (Ty, 2026-10-18): kept beside the tempo
//! map, set and taken away as one undo each, and a song with none saves
//! exactly as it did.

use fontelle_model::{Command, History, Project, SetMeterChange};
use fontelle_types::{MeterChange, PPQN};

#[test]
fn a_song_without_changes_writes_none() {
    let song = Project::new("song");
    let written = serde_json::to_string(&song).unwrap();
    assert!(!written.contains("meter_changes"));
    assert_eq!(song.meter().tick_of_bar(2), 8 * PPQN);
}

#[test]
fn a_change_is_set_moved_and_taken_away_each_as_one_undo() {
    let mut song = Project::new("song");
    let mut history = History::new();
    history
        .apply(Box::new(SetMeterChange::new(4, Some((7, 8)))), &mut song)
        .unwrap();
    assert_eq!(
        song.meter_changes,
        vec![MeterChange {
            bar: 4,
            top: 7,
            bottom: 8
        }]
    );
    assert_eq!(song.meter().signature(4), (7, 8));
    // Set again on the same bar: replaced, not added.
    history
        .apply(Box::new(SetMeterChange::new(4, Some((5, 4)))), &mut song)
        .unwrap();
    assert_eq!(song.meter_changes.len(), 1);
    assert_eq!(song.meter().signature(4), (5, 4));
    history
        .apply(Box::new(SetMeterChange::new(4, None)), &mut song)
        .unwrap();
    assert!(song.meter_changes.is_empty());
    history.undo(&mut song).unwrap().unwrap();
    assert_eq!(song.meter().signature(4), (5, 4));
    history.undo(&mut song).unwrap().unwrap();
    assert_eq!(song.meter().signature(4), (7, 8));
    history.undo(&mut song).unwrap().unwrap();
    assert!(song.meter_changes.is_empty());
    // And it travels: a written song reads back with its change.
    history.redo(&mut song).unwrap().unwrap();
    let read: Project = serde_json::from_str(&serde_json::to_string(&song).unwrap()).unwrap();
    assert_eq!(read.meter_changes, song.meter_changes);
}

#[test]
fn a_change_has_a_wire_form() {
    let mut song = Project::new("song");
    let mut command = SetMeterChange::new(2, Some((3, 4)));
    command.apply(&mut song).unwrap();
    let edit = command.to_edit();
    let back = fontelle_model::wire::Edit::from_bytes(&edit.to_bytes()).unwrap();
    let mut again = Project::new("song");
    back.into_command().apply(&mut again).unwrap();
    assert_eq!(again.meter_changes, song.meter_changes);
    assert_eq!(edit.reach(), fontelle_model::wire::Reach::Timeline);
}
