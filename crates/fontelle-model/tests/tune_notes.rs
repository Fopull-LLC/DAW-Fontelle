//! Pointing an insert at a channel's notes (`docs/tune-plan.md` §5.1, §9.6).
//!
//! A **routing edge** on the slot rather than a parameter in the config, and
//! that is forced rather than chosen: a `ParamSpec` is a float with a fixed
//! range and a permanent id (INVARIANT 7), and a channel is neither. It is the
//! same argument `EffectSlot::key` makes about a mixer track, one level over.

use fontelle_model::{
    AddChannel, AddInsert, Command, History, MixerTrack, Project, RemoveChannel, SetInsertNotes,
};
use fontelle_types::{ChannelId, EffectKind, MixerTrackId};

/// A project with a master, a track to hang inserts on, and two channels.
fn fixture() -> (Project, MixerTrackId, ChannelId, ChannelId) {
    let mut project = Project::new("tune");
    let master = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(master);
    let track = project.mixer.tracks.insert(MixerTrack::new("Vocal"));
    let mut add = AddChannel::new("Melody", None);
    add.apply(&mut project).unwrap();
    let melody = add.channel().unwrap();
    let mut add = AddChannel::new("Pad", None);
    add.apply(&mut project).unwrap();
    let pad = add.channel().unwrap();
    (project, track, melody, pad)
}

fn with_insert(kind: EffectKind) -> (Project, MixerTrackId, ChannelId, ChannelId) {
    let (mut project, track, melody, pad) = fixture();
    AddInsert::new(track, kind).apply(&mut project).unwrap();
    (project, track, melody, pad)
}

#[test]
fn set_insert_notes_points_a_tuner_at_a_channel() {
    let (mut project, track, melody, _) = with_insert(EffectKind::Tune);
    SetInsertNotes::new(track, 0, Some(melody))
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.mixer.tracks[track].inserts[0].notes, Some(melody));
    assert_eq!(
        project.mixer.tracks[track].inserts[0].effective_notes(),
        Some(melody)
    );
}

#[test]
fn set_insert_notes_refuses_an_effect_that_takes_none() {
    let (mut project, track, melody, _) = with_insert(EffectKind::Eq);
    let error = SetInsertNotes::new(track, 0, Some(melody))
        .apply(&mut project)
        .expect_err("an EQ has no notes to take");
    assert!(
        error.0.contains("notes"),
        "the message should say what is wrong: {}",
        error.0
    );
    assert_eq!(project.mixer.tracks[track].inserts[0].notes, None);
}

#[test]
fn set_insert_notes_refuses_a_channel_that_is_not_there() {
    let (mut project, track, _, pad) = with_insert(EffectKind::Tune);
    RemoveChannel::new(pad).apply(&mut project).unwrap();
    SetInsertNotes::new(track, 0, Some(pad))
        .apply(&mut project)
        .expect_err("a channel that is gone is not a source");
}

#[test]
fn set_insert_notes_refuses_a_slot_that_is_not_there() {
    let (mut project, track, melody, _) = fixture();
    SetInsertNotes::new(track, 3, Some(melody))
        .apply(&mut project)
        .expect_err("there is no insert 3");
}

#[test]
fn set_insert_notes_is_undoable() {
    let (mut project, track, melody, pad) = with_insert(EffectKind::Tune);
    let mut history = History::new();
    history
        .apply(
            Box::new(SetInsertNotes::new(track, 0, Some(melody))),
            &mut project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetInsertNotes::new(track, 0, Some(pad))),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.mixer.tracks[track].inserts[0].notes, Some(pad));
    history
        .undo(&mut project)
        .expect("something to undo")
        .unwrap();
    assert_eq!(
        project.mixer.tracks[track].inserts[0].notes,
        Some(melody),
        "undo went back to the channel before it, not to nothing"
    );
    history
        .undo(&mut project)
        .expect("something to undo")
        .unwrap();
    assert_eq!(project.mixer.tracks[track].inserts[0].notes, None);
    history
        .redo(&mut project)
        .expect("something to redo")
        .unwrap();
    assert_eq!(project.mixer.tracks[track].inserts[0].notes, Some(melody));
}

/// The same rule removing a mixer track follows for the sends that fed it: an
/// edge to something that is gone is either silent or a panic, and both are
/// worse than the edge going with what it named.
#[test]
fn removing_a_channel_clears_the_inserts_that_listened_to_it() {
    let (mut project, track, melody, pad) = with_insert(EffectKind::Tune);
    AddInsert::new(track, EffectKind::Tune)
        .apply(&mut project)
        .unwrap();
    SetInsertNotes::new(track, 0, Some(melody))
        .apply(&mut project)
        .unwrap();
    SetInsertNotes::new(track, 1, Some(pad))
        .apply(&mut project)
        .unwrap();

    let mut history = History::new();
    history
        .apply(Box::new(RemoveChannel::new(melody)), &mut project)
        .unwrap();
    assert_eq!(project.mixer.tracks[track].inserts[0].notes, None);
    assert_eq!(
        project.mixer.tracks[track].inserts[1].notes,
        Some(pad),
        "the other listener was left alone"
    );

    history
        .undo(&mut project)
        .expect("something to undo")
        .unwrap();
    assert_eq!(
        project.mixer.tracks[track].inserts[0].notes,
        Some(melody),
        "undoing the delete put the edge back, not just the channel"
    );
}

#[test]
fn a_project_with_notes_on_a_slot_round_trips_and_one_without_writes_no_field() {
    let (mut project, track, melody, _) = with_insert(EffectKind::Tune);
    // An EQ slot, because the corrector's *own* twelve-key mask is called
    // `notes` too — one is a channel and the other is which keys are in, and
    // they sit at different levels of the same document.
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    let bare = serde_json::to_string(&project.mixer.tracks[track].inserts[1]).unwrap();
    assert!(
        !bare.contains("\"notes\""),
        "a slot with no notes writes no field, so every project written before \
         this opens and is written back unchanged: {bare}"
    );

    SetInsertNotes::new(track, 0, Some(melody))
        .apply(&mut project)
        .unwrap();
    let slot = serde_json::to_string(&project.mixer.tracks[track].inserts[0]).unwrap();
    assert!(slot.contains("\"notes\""));
    let text = serde_json::to_string(&project).unwrap();
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back.mixer.tracks[track].inserts[0].notes, Some(melody));

    // And the two never see each other: writing the channel leaves the key
    // mask alone.
    let fontelle_types::EffectConfig::Tune(tune) = project.mixer.tracks[track].inserts[0].config
    else {
        unreachable!()
    };
    assert_eq!(tune.notes, 0x0FFF);
}

#[test]
fn only_two_effects_take_notes() {
    // The corrector, which is told *which note* to force, and Lapse, whose
    // twelve scenes are picked by pitch class so a kit is playable from an
    // octave of a keyboard (`docs/lapse-plan.md` §4.7). Two is the whole
    // list, and an effect that answers yes without wanting a channel is a
    // routing edge feeding nothing.
    for kind in EffectKind::ALL {
        assert_eq!(
            kind.takes_notes(),
            matches!(kind, EffectKind::Tune | EffectKind::Lapse),
            "{kind:?} answers the wrong way about notes"
        );
    }
}
