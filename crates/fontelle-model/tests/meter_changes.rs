//! A song's time signature and its changes (Ty, 2026-10-18 and -19): kept
//! beside the tempo map, set, moved and taken away as one undo each, any
//! signature at all, and a song in plain 4/4 saves exactly as it did.

use fontelle_model::{
    Command, History, MoveMeterChange, NumberTarget, Project, SetMeterChange, SetNumber,
};
use fontelle_types::{MeterChange, PPQN, Signature};

fn sig(top: u32, bottom: u32) -> Option<Signature> {
    Some(Signature::new(top, bottom))
}

#[test]
fn a_song_without_changes_writes_none() {
    let song = Project::new("song");
    let written = serde_json::to_string(&song).unwrap();
    assert!(!written.contains("meter_changes"));
    assert!(!written.contains("beat_unit"));
    assert!(!written.contains("beat_groups"));
    assert_eq!(song.meter().tick_of_bar(2), 8 * PPQN);
    assert_eq!(song.signature(), Signature::new(4, 4));
}

#[test]
fn a_change_is_set_and_taken_away_each_as_one_undo() {
    let mut song = Project::new("song");
    let mut history = History::new();
    history
        .apply(Box::new(SetMeterChange::new(4, sig(7, 8))), &mut song)
        .unwrap();
    assert_eq!(song.meter_changes, vec![MeterChange::new(4, 7, 8)]);
    assert_eq!(song.meter().signature(4), (7, 8));
    // Set again on the same bar: replaced, not added.
    history
        .apply(Box::new(SetMeterChange::new(4, sig(5, 4))), &mut song)
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

/// Bar 1's signature is the song's own, and it may be anything a change
/// may: any top, any bottom, grouped.
#[test]
fn the_songs_own_signature_is_any_signature() {
    let mut song = Project::new("song");
    let mut history = History::new();
    let waltz_in_eighths = Signature::new(7, 8).with_groups(vec![3, 2, 2]);
    history
        .apply(
            Box::new(SetMeterChange::new(0, Some(waltz_in_eighths.clone()))),
            &mut song,
        )
        .unwrap();
    assert_eq!(song.signature(), waltz_in_eighths);
    assert_eq!(song.meter().tick_of_bar(1), 7 * PPQN / 2);
    let read: Project = serde_json::from_str(&serde_json::to_string(&song).unwrap()).unwrap();
    assert_eq!(read.signature(), waltz_in_eighths);
    history.undo(&mut song).unwrap().unwrap();
    assert_eq!(song.signature(), Signature::new(4, 4));
    // Bar 1 always has a signature: it cannot be taken away.
    assert!(
        history
            .apply(Box::new(SetMeterChange::new(0, None)), &mut song)
            .is_err()
    );
}

#[test]
fn a_signature_the_meter_cannot_keep_is_refused() {
    let mut song = Project::new("song");
    for bad in [
        Signature::new(0, 4),
        Signature::new(4, 0),
        Signature::new(4, 65),
        Signature::new(65, 4),
        Signature {
            top: 8,
            bottom: 8,
            groups: vec![3, 3],
        },
    ] {
        assert!(
            SetMeterChange::new(2, Some(bad.clone()))
                .apply(&mut song)
                .is_err(),
            "{bad:?}"
        );
    }
    assert!(song.meter_changes.is_empty());
}

/// Ty: *"they absolutely need to be moveable."* A change moved onto
/// another bar takes that bar's place — one undo puts both back.
#[test]
fn a_change_moves_to_another_bar_as_one_undo() {
    let mut song = Project::new("song");
    let mut history = History::new();
    history
        .apply(Box::new(SetMeterChange::new(4, sig(7, 8))), &mut song)
        .unwrap();
    history
        .apply(Box::new(SetMeterChange::new(8, sig(3, 4))), &mut song)
        .unwrap();
    history
        .apply(Box::new(MoveMeterChange::new(4, 6)), &mut song)
        .unwrap();
    assert_eq!(
        song.meter_changes,
        vec![MeterChange::new(6, 7, 8), MeterChange::new(8, 3, 4)]
    );
    // Onto the other one: it is replaced.
    history
        .apply(Box::new(MoveMeterChange::new(6, 8)), &mut song)
        .unwrap();
    assert_eq!(song.meter_changes, vec![MeterChange::new(8, 7, 8)]);
    history.undo(&mut song).unwrap().unwrap();
    assert_eq!(
        song.meter_changes,
        vec![MeterChange::new(6, 7, 8), MeterChange::new(8, 3, 4)]
    );
    history.undo(&mut song).unwrap().unwrap();
    assert_eq!(
        song.meter_changes,
        vec![MeterChange::new(4, 7, 8), MeterChange::new(8, 3, 4)]
    );
    // Nothing at the bar, or onto bar 1, is refused.
    assert!(MoveMeterChange::new(5, 6).apply(&mut song).is_err());
    assert!(MoveMeterChange::new(4, 0).apply(&mut song).is_err());
    assert!(MoveMeterChange::new(0, 3).apply(&mut song).is_err());
}

#[test]
fn changes_have_a_wire_form() {
    let mut song = Project::new("song");
    let mut set = SetMeterChange::new(2, Some(Signature::new(9, 8).with_groups(vec![2, 2, 2, 3])));
    set.apply(&mut song).unwrap();
    let mut moved = MoveMeterChange::new(2, 5);
    moved.apply(&mut song).unwrap();
    let mut again = Project::new("song");
    for edit in [set.to_edit(), moved.to_edit()] {
        let back = fontelle_model::wire::Edit::from_bytes(&edit.to_bytes()).unwrap();
        assert_eq!(edit.reach(), fontelle_model::wire::Reach::Timeline);
        back.into_command().apply(&mut again).unwrap();
    }
    assert_eq!(again.meter_changes, song.meter_changes);
}

/// The beats-per-bar number reaches as far as the meter does.
#[test]
fn a_long_bar_is_kept() {
    let mut song = Project::new("song");
    SetNumber::new(NumberTarget::BeatsPerBar, 33.0)
        .apply(&mut song)
        .unwrap();
    assert_eq!(song.beats_per_bar, 33);
}
