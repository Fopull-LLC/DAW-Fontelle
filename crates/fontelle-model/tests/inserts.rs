//! The insert chain: what a mixer track carries, and every way it changes.
//!
//! `MixerTrack::inserts` has been in the document since the format was written
//! and has held nothing, because `EffectSlot` was a placeholder — an effect id
//! and a bypass flag, with nowhere to put a single parameter. This is the pass
//! that gives it a real shape and the commands to move it around.
//!
//! Every one of those is a `Command` (INVARIANT 9), which for a chain means
//! more than "it can be undone": reordering and removing have to put back the
//! *slot that was there*, at the index it was at, with the settings it had.
//! An undo that restored a fresh EQ where a tuned one used to be would be a
//! command that lost work while claiming not to.

use fontelle_model::{
    AddInsert, Command, History, MixerTrack, MoveInsert, Project, RemoveInsert, SetEqBand,
    SetInsertBypassed, SetInsertMix,
};
use fontelle_types::{BandChannel, BandType, EffectConfig, EffectKind, EqBand, MixerTrackId};

/// A project with a master and one track to hang inserts on.
fn fixture() -> (Project, MixerTrackId) {
    let mut project = Project::new("inserts");
    let master = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(master);
    let track = project.mixer.tracks.insert(MixerTrack::new("Keys"));
    (project, track)
}

fn chain(project: &Project, track: MixerTrackId) -> Vec<EffectKind> {
    project.mixer.tracks[track]
        .inserts
        .iter()
        .map(|slot| slot.config.kind())
        .collect()
}

fn a_band(freq_hz: f32, gain_db: f32) -> EqBand {
    EqBand {
        band_type: BandType::Bell,
        freq_hz,
        gain_db,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

// ------------------------------------------------------------------ adding

#[test]
fn a_track_starts_with_nothing_on_it() {
    let (project, track) = fixture();
    assert!(project.mixer.tracks[track].inserts.is_empty());
}

#[test]
fn adding_an_effect_puts_it_at_the_end_of_the_chain() {
    // The end, because a chain is an order and the order is the sound: an
    // effect that inserted itself in the middle would rearrange a mix that was
    // already balanced.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    assert_eq!(chain(&project, track), vec![EffectKind::Eq, EffectKind::Eq]);
}

#[test]
fn a_freshly_added_effect_changes_nothing_until_it_is_touched() {
    // An insert somebody just dropped on a track must not move the mix. It is
    // the same rule a new channel follows by playing nothing rather than a
    // default instrument.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert!(
        eq.bands.iter().all(|band| !band.is_audible()),
        "every band of a new EQ is switched off"
    );
    assert!(!project.mixer.tracks[track].inserts[0].bypassed);
}

#[test]
fn adding_to_a_track_that_is_not_there_is_refused() {
    let (mut project, track) = fixture();
    project.mixer.tracks.remove(track);
    assert!(
        AddInsert::new(track, EffectKind::Eq)
            .apply(&mut project)
            .is_err()
    );
}

// ---------------------------------------------------------------- removing

#[test]
fn removing_an_insert_takes_the_one_named_and_closes_the_gap() {
    let (mut project, track) = fixture();
    for _ in 0..3 {
        AddInsert::new(track, EffectKind::Eq)
            .apply(&mut project)
            .unwrap();
    }
    // Make the middle one recognisable.
    SetEqBand::new(track, 1, 0, a_band(500.0, 4.0))
        .apply(&mut project)
        .unwrap();

    RemoveInsert::new(track, 0).apply(&mut project).unwrap();
    assert_eq!(project.mixer.tracks[track].inserts.len(), 2);
    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(
        eq.bands[0].freq_hz, 500.0,
        "what was second is now first, settings and all"
    );
}

#[test]
fn undoing_a_removal_puts_back_the_effect_that_was_there() {
    // Not a fresh one of the same kind: the whole point of undo is that the
    // work comes back.
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(
            Box::new(AddInsert::new(track, EffectKind::Eq)),
            &mut project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetEqBand::new(track, 0, 2, a_band(3_000.0, -7.5))),
            &mut project,
        )
        .unwrap();
    history
        .apply(Box::new(RemoveInsert::new(track, 0)), &mut project)
        .unwrap();
    assert!(project.mixer.tracks[track].inserts.is_empty());

    history.undo(&mut project).unwrap().unwrap();
    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(eq.bands[2].freq_hz, 3_000.0);
    assert_eq!(eq.bands[2].gain_db, -7.5);
}

#[test]
fn undoing_a_removal_puts_it_back_where_it_was() {
    // Order is the sound. An undo that appended would give the chain back with
    // a compressor after the EQ that used to be before it.
    let (mut project, track) = fixture();
    let mut history = History::new();
    for freq in [100.0, 200.0, 300.0] {
        history
            .apply(
                Box::new(AddInsert::new(track, EffectKind::Eq)),
                &mut project,
            )
            .unwrap();
        let index = project.mixer.tracks[track].inserts.len() - 1;
        history
            .apply(
                Box::new(SetEqBand::new(track, index, 0, a_band(freq, 3.0))),
                &mut project,
            )
            .unwrap();
    }
    history
        .apply(Box::new(RemoveInsert::new(track, 1)), &mut project)
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();

    let frequencies: Vec<f32> = project.mixer.tracks[track]
        .inserts
        .iter()
        .map(|slot| {
            let EffectConfig::Eq(eq) = slot.config else {
                unreachable!("this slot holds an EQ")
            };
            eq.bands[0].freq_hz
        })
        .collect();
    assert_eq!(frequencies, vec![100.0, 200.0, 300.0]);
}

#[test]
fn removing_an_index_that_is_not_there_is_refused() {
    let (mut project, track) = fixture();
    assert!(RemoveInsert::new(track, 0).apply(&mut project).is_err());
}

// --------------------------------------------------------------- reordering

#[test]
fn an_insert_can_be_dragged_up_the_chain() {
    let (mut project, track) = fixture();
    for freq in [100.0, 200.0, 300.0] {
        AddInsert::new(track, EffectKind::Eq)
            .apply(&mut project)
            .unwrap();
        let index = project.mixer.tracks[track].inserts.len() - 1;
        SetEqBand::new(track, index, 0, a_band(freq, 3.0))
            .apply(&mut project)
            .unwrap();
    }

    MoveInsert::new(track, 2, 0).apply(&mut project).unwrap();
    let frequencies: Vec<f32> = project.mixer.tracks[track]
        .inserts
        .iter()
        .map(|slot| {
            let EffectConfig::Eq(eq) = slot.config else {
                unreachable!("this slot holds an EQ")
            };
            eq.bands[0].freq_hz
        })
        .collect();
    assert_eq!(frequencies, vec![300.0, 100.0, 200.0]);
}

#[test]
fn undoing_a_move_puts_the_order_back() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    for freq in [100.0, 200.0, 300.0] {
        history
            .apply(
                Box::new(AddInsert::new(track, EffectKind::Eq)),
                &mut project,
            )
            .unwrap();
        let index = project.mixer.tracks[track].inserts.len() - 1;
        history
            .apply(
                Box::new(SetEqBand::new(track, index, 0, a_band(freq, 3.0))),
                &mut project,
            )
            .unwrap();
    }
    history
        .apply(Box::new(MoveInsert::new(track, 0, 2)), &mut project)
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();

    let frequencies: Vec<f32> = project.mixer.tracks[track]
        .inserts
        .iter()
        .map(|slot| {
            let EffectConfig::Eq(eq) = slot.config else {
                unreachable!("this slot holds an EQ")
            };
            eq.bands[0].freq_hz
        })
        .collect();
    assert_eq!(frequencies, vec![100.0, 200.0, 300.0]);
}

#[test]
fn a_move_that_goes_nowhere_is_refused_rather_than_filling_the_history() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    assert!(MoveInsert::new(track, 0, 0).apply(&mut project).is_err());
    assert!(MoveInsert::new(track, 0, 9).apply(&mut project).is_err());
}

// ------------------------------------------------------------------- bypass

#[test]
fn an_insert_can_be_switched_out_of_the_chain_and_back() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();

    SetInsertBypassed::new(track, 0, true)
        .apply(&mut project)
        .unwrap();
    assert!(project.mixer.tracks[track].inserts[0].bypassed);
    SetInsertBypassed::new(track, 0, false)
        .apply(&mut project)
        .unwrap();
    assert!(!project.mixer.tracks[track].inserts[0].bypassed);
}

#[test]
fn bypassing_keeps_the_settings_so_switching_back_is_free() {
    // A bypass is not a delete. The whole reason to reach for it is to hear
    // the difference and then put it back.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    SetEqBand::new(track, 0, 0, a_band(800.0, 5.0))
        .apply(&mut project)
        .unwrap();
    SetInsertBypassed::new(track, 0, true)
        .apply(&mut project)
        .unwrap();

    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(eq.bands[0].freq_hz, 800.0);
    assert_eq!(eq.bands[0].gain_db, 5.0);
}

// ------------------------------------------------------------- editing bands

#[test]
fn setting_a_band_writes_only_that_band() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    SetEqBand::new(track, 0, 3, a_band(2_500.0, -4.0))
        .apply(&mut project)
        .unwrap();

    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(eq.bands[3].freq_hz, 2_500.0);
    for (index, band) in eq.bands.iter().enumerate() {
        if index != 3 {
            assert!(!band.enabled, "band {index} was left alone");
        }
    }
}

#[test]
fn dragging_a_band_is_one_undo_entry_rather_than_sixty() {
    // A drag emits a command a frame. Without merging, undo after moving one
    // knob would take a minute of clicking — which is the same rule the
    // mixer's fader and the roll's note drag already follow.
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(
            Box::new(AddInsert::new(track, EffectKind::Eq)),
            &mut project,
        )
        .unwrap();
    let depth = history.depth();

    for gain in [1.0, 2.0, 3.0, 4.0, 5.0] {
        history
            .apply(
                Box::new(SetEqBand::new(track, 0, 0, a_band(1_000.0, gain))),
                &mut project,
            )
            .unwrap();
    }
    assert_eq!(
        history.depth(),
        depth + 1,
        "a whole drag is one entry in the history"
    );

    history.undo(&mut project).unwrap().unwrap();
    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert!(
        !eq.bands[0].enabled,
        "and undoing it goes back to before the drag, not to the middle of it"
    );
}

#[test]
fn a_gesture_break_stops_two_drags_becoming_one() {
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(
            Box::new(AddInsert::new(track, EffectKind::Eq)),
            &mut project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetEqBand::new(track, 0, 0, a_band(1_000.0, 3.0))),
            &mut project,
        )
        .unwrap();
    history.break_gesture();
    history
        .apply(
            Box::new(SetEqBand::new(track, 0, 0, a_band(1_000.0, 6.0))),
            &mut project,
        )
        .unwrap();

    history.undo(&mut project).unwrap().unwrap();
    let EffectConfig::Eq(eq) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(
        eq.bands[0].gain_db, 3.0,
        "back to where the second drag began"
    );
}

#[test]
fn two_different_bands_do_not_merge_into_each_other() {
    // Merging is per control. Two knobs dragged one after the other are two
    // things a person did and two things they can undo.
    let (mut project, track) = fixture();
    let mut history = History::new();
    history
        .apply(
            Box::new(AddInsert::new(track, EffectKind::Eq)),
            &mut project,
        )
        .unwrap();
    let depth = history.depth();
    history
        .apply(
            Box::new(SetEqBand::new(track, 0, 0, a_band(100.0, 3.0))),
            &mut project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetEqBand::new(track, 0, 1, a_band(200.0, 3.0))),
            &mut project,
        )
        .unwrap();
    assert_eq!(history.depth(), depth + 2);
}

// ------------------------------------------------------------ dry and wet ---
//
// Asked for from using the mixer: a wet/dry knob per effect, the way FL and
// every other DAW has one. The parameter lives in the effect's own config
// (see `fontelle-types/tests/effect_mix.rs` for why); what is here is the
// command that moves it, which has to behave like every other dragged
// control: heard while it moves, one undo entry when it stops.

fn mix_of(project: &Project, track: MixerTrackId, slot: usize) -> f32 {
    project.mixer.tracks[track].inserts[slot].config.mix()
}

#[test]
fn an_insert_can_be_mixed_back_towards_the_dry_signal() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    assert_eq!(
        mix_of(&project, track, 0),
        1.0,
        "a new effect is the effect"
    );

    SetInsertMix::new(track, 0, 0.35)
        .apply(&mut project)
        .unwrap();
    assert!((mix_of(&project, track, 0) - 0.35).abs() < 1e-6);
}

#[test]
fn mixing_one_insert_leaves_the_others_alone() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    AddInsert::new(track, EffectKind::Compressor)
        .apply(&mut project)
        .unwrap();

    SetInsertMix::new(track, 1, 0.2)
        .apply(&mut project)
        .unwrap();
    assert_eq!(mix_of(&project, track, 0), 1.0);
    assert!((mix_of(&project, track, 1) - 0.2).abs() < 1e-6);
}

#[test]
fn a_mix_is_undoable_back_to_where_the_drag_started() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    let mut history = History::new();
    history
        .apply(Box::new(SetInsertMix::new(track, 0, 0.5)), &mut project)
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(mix_of(&project, track, 0), 1.0);
}

#[test]
fn a_whole_mix_drag_is_one_undo_entry() {
    // The rule every dragged control in this document follows: sixty commands
    // land, one entry is left behind, and undo goes back to before the drag
    // rather than to the middle of it.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    let mut history = History::new();
    let depth = history.depth();
    for step in 0..20 {
        history
            .apply(
                Box::new(SetInsertMix::new(track, 0, 1.0 - step as f32 / 40.0)),
                &mut project,
            )
            .unwrap();
    }
    assert_eq!(
        history.depth(),
        depth + 1,
        "a drag is one thing somebody did"
    );
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(mix_of(&project, track, 0), 1.0);
}

#[test]
fn two_inserts_mixed_one_after_the_other_are_two_entries() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    AddInsert::new(track, EffectKind::Compressor)
        .apply(&mut project)
        .unwrap();
    let mut history = History::new();
    let depth = history.depth();
    history
        .apply(Box::new(SetInsertMix::new(track, 0, 0.5)), &mut project)
        .unwrap();
    history
        .apply(Box::new(SetInsertMix::new(track, 1, 0.5)), &mut project)
        .unwrap();
    assert_eq!(history.depth(), depth + 2);
}

#[test]
fn mixing_an_insert_that_is_not_there_is_refused() {
    let (mut project, track) = fixture();
    assert!(
        SetInsertMix::new(track, 0, 0.5)
            .apply(&mut project)
            .is_err()
    );
}

// ------------------------------------------------------------ round tripping

#[test]
fn a_chain_survives_being_written_out_and_read_back() {
    // The document is JSON on disk (§17.2). An effect chain that could not be
    // saved would be a mix you lose when you close the window.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    let mut band = a_band(4_200.0, -3.5);
    band.band_type = BandType::HighShelf;
    band.channel = BandChannel::Side;
    SetEqBand::new(track, 0, 5, band)
        .apply(&mut project)
        .unwrap();
    SetInsertBypassed::new(track, 0, true)
        .apply(&mut project)
        .unwrap();
    SetInsertMix::new(track, 0, 0.25)
        .apply(&mut project)
        .unwrap();

    let json = serde_json::to_string(&project).expect("a project serialises");
    let back: Project = serde_json::from_str(&json).expect("and comes back");

    let slot = &back.mixer.tracks[track].inserts[0];
    assert!(slot.bypassed);
    let EffectConfig::Eq(eq) = slot.config else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(eq.bands[5].freq_hz, 4_200.0);
    assert_eq!(eq.bands[5].gain_db, -3.5);
    assert_eq!(eq.bands[5].band_type, BandType::HighShelf);
    assert_eq!(eq.bands[5].channel, BandChannel::Side);
    assert!(
        (eq.mix - 0.25).abs() < 1e-6,
        "a wet/dry setting has to survive the file"
    );
}
