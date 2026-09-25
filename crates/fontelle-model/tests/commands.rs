//! Every document mutation goes through a `Command` (INVARIANT 9), which is
//! only worth anything if the inverses are exact. These check that they are.
//!
//! The load-bearing property, and the one the plan names: **apply a command,
//! apply its inverse, and the document is byte-for-byte where it started.**
//! Compared as serialised JSON rather than field by field, because that is
//! what a saved project is and it leaves nothing out — including the ids,
//! which are the half a `slotmap` could not have given back.

use fontelle_model::{
    AddChannel, AddClip, AddNotes, Arena, Clip, ClipSource, Command, CommandError, DuplicateClip,
    FlagTarget, History, Lane, MixerTrack, MoveClip, MoveNotes, Note, NoteData, NumberTarget,
    Project, RemoveChannel, RemoveClip, RemoveNotes, ResizeNotes, SetChannelPatch, SetFlag,
    SetLoopRange, SetNumber, TempoMap,
};
use fontelle_types::{ChannelId, ClipId, LaneId, MixerTrackId, NoteId, PPQN};

/// The document as it would be saved. Two projects are the same project when
/// these match.
fn snapshot(project: &Project) -> serde_json::Value {
    serde_json::to_value(project).expect("a project must serialise")
}

fn a_note(start: i64, key: u8) -> Note {
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
        channel: None,
    }
}

/// A project with one lane, one channel on its own mixer track, and one clip
/// holding three notes — enough for every command below to have something real
/// to act on.
struct Fixture {
    project: Project,
    lane: LaneId,
    channel: ChannelId,
    track: MixerTrackId,
    clip: ClipId,
    notes: Vec<NoteId>,
}

fn fixture() -> Fixture {
    let mut project = Project::new("commands");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let master = project.mixer.master.unwrap();
    let track = project.mixer.tracks.insert(MixerTrack::new("Part"));
    project.mixer.tracks[track].output = Some(master);
    let channel = project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "Part".into(),
        color: [0; 4],
        mixer_track: Some(track),
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut notes = Arena::default();
    let ids = vec![
        notes.insert(a_note(0, 60)),
        notes.insert(a_note(PPQN, 64)),
        notes.insert(a_note(PPQN * 2, 67)),
    ];
    let clip = project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    Fixture {
        project,
        lane,
        channel,
        track,
        clip,
        notes: ids,
    }
}

/// Applies `command`, then its inverse, and requires the document to be
/// exactly where it started.
fn round_trips(mut command: Box<dyn Command>, project: &mut Project) {
    let before = snapshot(project);
    command.apply(project).expect("the command must apply");
    let after_apply = snapshot(project);
    assert_ne!(
        before, after_apply,
        "a command that changes nothing proves nothing about its inverse"
    );

    command
        .invert()
        .apply(project)
        .expect("the inverse must apply");
    assert_eq!(
        before,
        snapshot(project),
        "{} did not invert exactly",
        command.label()
    );
}

#[test]
fn adding_and_removing_a_channel_inverts_exactly() {
    let mut f = fixture();
    round_trips(Box::new(AddChannel::new("Strings", None)), &mut f.project);
    round_trips(Box::new(RemoveChannel::new(f.channel)), &mut f.project);
}

#[test]
fn adding_a_channel_is_one_command_and_one_undo_entry() {
    // Choosing an instrument should not need two presses of Ctrl+Z to take
    // back. It used to also mint a mixer track, and this test used to say so;
    // that is deliberately gone — see `tests/routing.rs`, which asserts the
    // opposite, and `Channel::mixer_track` for why.
    let mut f = fixture();
    let tracks_before = f.project.mixer.tracks.len();
    let mut add = AddChannel::new("Strings", None);
    add.apply(&mut f.project).unwrap();

    let channel = add.channel().expect("the id is known once it is applied");
    assert_eq!(f.project.channels[channel].name, "Strings");
    assert_eq!(
        f.project.mixer.tracks.len(),
        tracks_before,
        "a strip is a destination somebody builds, not a side effect of loading a soundfont"
    );

    add.invert().apply(&mut f.project).unwrap();
    assert!(!f.project.channels.contains_key(channel));
}

#[test]
fn removing_a_channel_takes_its_clips_with_it_and_brings_them_back() {
    // A clip left pointing at a channel that is gone plays nothing and shows
    // nothing — an orphan the user cannot see to fix.
    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut remove = RemoveChannel::new(f.channel);
    remove.apply(&mut f.project).unwrap();
    assert!(f.project.clips.is_empty(), "its clip went with it");

    remove.invert().apply(&mut f.project).unwrap();
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn every_note_command_inverts_exactly() {
    let mut f = fixture();
    round_trips(
        Box::new(AddNotes::new(f.clip, vec![a_note(PPQN * 3, 72)])),
        &mut f.project,
    );
    round_trips(
        Box::new(RemoveNotes::new(f.clip, vec![f.notes[1]])),
        &mut f.project,
    );
    round_trips(
        Box::new(MoveNotes::new(f.clip, f.notes.clone(), PPQN / 2, 3)),
        &mut f.project,
    );
    round_trips(
        Box::new(ResizeNotes::new(f.clip, f.notes.clone(), PPQN / 4)),
        &mut f.project,
    );
}

#[test]
fn every_clip_command_inverts_exactly() {
    let mut f = fixture();
    let new_clip = Clip {
        lane: f.lane,
        start: PPQN * 8,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel: f.channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    };
    round_trips(Box::new(AddClip::new(new_clip)), &mut f.project);
    round_trips(Box::new(RemoveClip::new(f.clip)), &mut f.project);
    round_trips(
        Box::new(MoveClip::new(f.clip, PPQN * 4, None)),
        &mut f.project,
    );
    round_trips(
        Box::new(DuplicateClip::new(f.clip, PPQN * 4)),
        &mut f.project,
    );
}

#[test]
fn every_value_command_inverts_exactly() {
    let mut f = fixture();
    round_trips(
        Box::new(SetNumber::new(NumberTarget::TrackGainDb(f.track), -6.0)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetNumber::new(NumberTarget::TrackPan(f.track), 0.5)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetNumber::new(NumberTarget::ChannelPan(f.channel), -0.5)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetNumber::new(NumberTarget::Tempo, 90.0)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetFlag::new(FlagTarget::TrackMute(f.track), true)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetFlag::new(FlagTarget::LaneMuted(f.lane), true)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetFlag::new(FlagTarget::ClipMuted(f.clip), true)),
        &mut f.project,
    );
    round_trips(
        Box::new(SetLoopRange::new(Some((0, PPQN * 4)))),
        &mut f.project,
    );
    round_trips(
        Box::new(SetChannelPatch::new(
            f.channel,
            Some(fontelle_types::PatchData {
                format_version: 0,
                body: serde_json::json!({ "layers": [] }),
            }),
        )),
        &mut f.project,
    );
}

#[test]
fn setting_the_tempo_keeps_the_changes_that_come_after_it() {
    // An imported file's tempo curve must survive somebody nudging the BPM
    // box. Replacing the map with a constant would silently flatten the piece.
    let mut f = fixture();
    f.project.tempo_map = TempoMap::from_segments(
        vec![
            fontelle_model::TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            },
            fontelle_model::TempoSegment {
                start_tick: PPQN * 4,
                bpm: 60.0,
            },
        ],
        48_000.0,
    );

    let mut set = SetNumber::new(NumberTarget::Tempo, 140.0);
    set.apply(&mut f.project).unwrap();

    assert_eq!(f.project.tempo_map.tempo_at(0), 140.0);
    assert_eq!(
        f.project.tempo_map.tempo_at(PPQN * 4),
        60.0,
        "the later change is still there"
    );
}

#[test]
fn a_note_dragged_off_the_keyboard_is_refused_rather_than_clamped() {
    // Clamping is not invertible: undo would put the note back where the
    // clamp left it, not where it was. The gesture is the caller's to bound.
    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut move_up = MoveNotes::new(f.clip, f.notes.clone(), 0, 100);
    assert!(move_up.apply(&mut f.project).is_err());
    assert_eq!(
        before,
        snapshot(&f.project),
        "a refused command changes nothing"
    );

    let mut back = MoveNotes::new(f.clip, f.notes.clone(), -PPQN * 10, 0);
    assert!(
        back.apply(&mut f.project).is_err(),
        "and not before the clip starts"
    );
    assert_eq!(before, snapshot(&f.project));
}

// --- History ---------------------------------------------------------------

#[test]
fn undo_and_redo_walk_the_stack_and_land_on_the_same_document() {
    let mut f = fixture();
    let mut history = History::new();
    let start = snapshot(&f.project);

    history
        .apply(
            Box::new(AddNotes::new(f.clip, vec![a_note(PPQN * 3, 72)])),
            &mut f.project,
        )
        .unwrap();
    let after_add = snapshot(&f.project);
    history
        .apply(
            Box::new(SetNumber::new(NumberTarget::TrackGainDb(f.track), -6.0)),
            &mut f.project,
        )
        .unwrap();
    let after_gain = snapshot(&f.project);

    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(after_add, snapshot(&f.project));
    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(start, snapshot(&f.project));
    assert!(
        history.undo(&mut f.project).is_none(),
        "nothing left to undo"
    );

    history.redo(&mut f.project).unwrap().unwrap();
    assert_eq!(after_add, snapshot(&f.project));
    history.redo(&mut f.project).unwrap().unwrap();
    assert_eq!(after_gain, snapshot(&f.project));
    assert!(history.redo(&mut f.project).is_none());
}

#[test]
fn a_note_redone_after_being_undone_keeps_the_id_the_next_command_refers_to() {
    // Draw a note, drag it, Ctrl+Z twice, Ctrl+Y twice. If the redone
    // insertion mints a fresh id, the second redo moves a note that no longer
    // exists — the whole reason the document is in an `Arena`.
    let mut f = fixture();
    let mut history = History::new();

    let add = AddNotes::new(f.clip, vec![a_note(PPQN * 3, 72)]);
    history.apply(Box::new(add), &mut f.project).unwrap();
    let drawn = last_note(&f.project, f.clip);
    history
        .apply(
            Box::new(MoveNotes::new(f.clip, vec![drawn], PPQN / 2, 0)),
            &mut f.project,
        )
        .unwrap();
    let after_both = snapshot(&f.project);

    history.undo(&mut f.project).unwrap().unwrap();
    history.undo(&mut f.project).unwrap().unwrap();
    history.redo(&mut f.project).unwrap().unwrap();
    history
        .redo(&mut f.project)
        .expect("there is a second redo")
        .expect("and it must not be looking for a note that no longer exists");

    assert_eq!(after_both, snapshot(&f.project));
}

#[test]
fn a_drag_coalesces_into_one_history_entry_until_the_gesture_ends() {
    // TDD §10.6: dragging a note is one entry, not four hundred. The boundary
    // is explicit rather than a time window, because only the caller knows
    // when the mouse came up — and a guessed window either splits a slow drag
    // or swallows a deliberate second nudge.
    let mut f = fixture();
    let mut history = History::new();
    let start = snapshot(&f.project);

    for _ in 0..8 {
        history
            .apply(
                Box::new(MoveNotes::new(f.clip, f.notes.clone(), 12, 0)),
                &mut f.project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), 1, "one drag, one entry");

    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(start, snapshot(&f.project), "and it undoes the whole drag");

    history
        .apply(
            Box::new(MoveNotes::new(f.clip, f.notes.clone(), 12, 0)),
            &mut f.project,
        )
        .unwrap();
    history.break_gesture();
    history
        .apply(
            Box::new(MoveNotes::new(f.clip, f.notes.clone(), 12, 0)),
            &mut f.project,
        )
        .unwrap();
    assert_eq!(history.depth(), 2, "two gestures, two entries");
}

#[test]
fn a_merged_fader_gesture_undoes_to_before_the_gesture_started() {
    // A coalesced entry has to remember the value from before the *first*
    // step of the drag, not before its last one — otherwise undoing a fader
    // sweep lands somewhere in the middle of it.
    let mut f = fixture();
    let mut history = History::new();
    f.project.mixer.tracks[f.track].gain_db = 0.0;

    for db in [-3.0, -6.0, -12.0] {
        history
            .apply(
                Box::new(SetNumber::new(NumberTarget::TrackGainDb(f.track), db)),
                &mut f.project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), 1);
    assert_eq!(f.project.mixer.tracks[f.track].gain_db, -12.0);

    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(
        f.project.mixer.tracks[f.track].gain_db, 0.0,
        "the whole sweep has to come back, not its last step"
    );
    history.redo(&mut f.project).unwrap().unwrap();
    assert_eq!(f.project.mixer.tracks[f.track].gain_db, -12.0);
}

#[test]
fn two_different_targets_never_coalesce() {
    let mut f = fixture();
    let mut history = History::new();
    let other = f.project.mixer.master.unwrap();

    history
        .apply(
            Box::new(SetNumber::new(NumberTarget::TrackGainDb(f.track), -3.0)),
            &mut f.project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetNumber::new(NumberTarget::TrackGainDb(other), -3.0)),
            &mut f.project,
        )
        .unwrap();
    assert_eq!(history.depth(), 2);
}

#[test]
fn a_flag_never_coalesces_because_a_toggle_is_not_a_drag() {
    let mut f = fixture();
    let mut history = History::new();
    for value in [true, false, true] {
        history
            .apply(
                Box::new(SetFlag::new(FlagTarget::TrackMute(f.track), value)),
                &mut f.project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), 3, "each press is its own entry");
}

#[test]
fn a_new_edit_after_an_undo_discards_the_redo_stack() {
    let mut f = fixture();
    let mut history = History::new();
    history
        .apply(
            Box::new(SetNumber::new(NumberTarget::TrackGainDb(f.track), -6.0)),
            &mut f.project,
        )
        .unwrap();
    history.undo(&mut f.project).unwrap().unwrap();
    history
        .apply(
            Box::new(SetFlag::new(FlagTarget::TrackMute(f.track), true)),
            &mut f.project,
        )
        .unwrap();
    assert!(history.redo(&mut f.project).is_none());
}

#[test]
fn a_command_that_fails_leaves_the_history_where_it_was() {
    let mut f = fixture();
    let mut history = History::new();
    let missing = ClipId::default();
    let err = history.apply(
        Box::new(AddNotes::new(missing, vec![a_note(0, 60)])),
        &mut f.project,
    );
    assert!(err.is_err());
    assert_eq!(history.depth(), 0, "a failed edit is not an undo entry");
}

#[test]
fn the_history_evicts_the_oldest_entries_once_it_is_over_budget() {
    // Both limits: depth, and the memory ceiling §10.6 gives a default for and
    // which nothing read.
    let mut f = fixture();
    let mut history = History::new();
    history.max_depth = 4;
    for i in 0..10 {
        history
            .apply(
                Box::new(SetNumber::new(
                    NumberTarget::TrackGainDb(f.track),
                    -(i as f64),
                )),
                &mut f.project,
            )
            .unwrap();
        history.break_gesture();
    }
    assert_eq!(history.depth(), 4);

    let mut history = History::new();
    history.memory_ceiling_bytes = 1;
    for i in 0..5 {
        history
            .apply(
                Box::new(AddNotes::new(f.clip, vec![a_note(PPQN * (4 + i), 72)])),
                &mut f.project,
            )
            .unwrap();
        history.break_gesture();
    }
    assert_eq!(
        history.depth(),
        1,
        "a ceiling of one byte keeps only the newest entry"
    );
}

#[test]
fn the_label_says_what_the_entry_did() {
    // §10.6's whole reason for inverse commands over snapshots: the history
    // list reads "Draw 14 notes", not "Snapshot 7".
    let f = fixture();
    let notes: Vec<Note> = (0..14).map(|i| a_note(PPQN * i, 60)).collect();
    assert_eq!(AddNotes::new(f.clip, notes).label(), "Draw 14 notes");
    assert_eq!(
        AddNotes::new(f.clip, vec![a_note(0, 60)]).label(),
        "Draw a note"
    );
    assert_eq!(RemoveClip::new(f.clip).label(), "Delete clip");
}

// --- The property test -----------------------------------------------------

/// A deterministic generator, so a failure is reproducible from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn last_note(project: &Project, clip: ClipId) -> NoteId {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        unreachable!("this fixture's clip holds notes")
    };
    data.notes.keys().last().expect("a note")
}

fn random_command(rng: &mut Rng, f: &Fixture, project: &Project) -> Box<dyn Command> {
    let notes: Vec<NoteId> = match &project.clips[f.clip].source {
        ClipSource::Notes(data) => data.notes.keys().collect(),
        _ => Vec::new(),
    };
    let some_notes = || -> Vec<NoteId> {
        if notes.is_empty() {
            Vec::new()
        } else {
            notes.clone()
        }
    };
    match rng.below(8) {
        0 => Box::new(AddNotes::new(
            f.clip,
            vec![a_note(rng.below(PPQN as u64 * 4) as i64, 60)],
        )),
        1 => Box::new(RemoveNotes::new(
            f.clip,
            some_notes().into_iter().take(1).collect(),
        )),
        2 => Box::new(MoveNotes::new(f.clip, some_notes(), 24, 1)),
        3 => Box::new(ResizeNotes::new(f.clip, some_notes(), 24)),
        4 => Box::new(SetNumber::new(
            NumberTarget::TrackGainDb(f.track),
            -(rng.below(24) as f64),
        )),
        5 => Box::new(SetFlag::new(
            FlagTarget::ClipMuted(f.clip),
            rng.below(2) == 0,
        )),
        6 => Box::new(MoveClip::new(f.clip, 24, None)),
        _ => Box::new(DuplicateClip::new(f.clip, PPQN * 16)),
    }
}

#[test]
fn any_sequence_of_edits_undoes_back_to_where_it_started() {
    for seed in 1..40u64 {
        let mut f = fixture();
        let mut history = History::new();
        let start = snapshot(&f.project);
        let mut rng = Rng(seed);

        let mut applied = 0;
        for _ in 0..25 {
            let command = random_command(&mut rng, &f, &f.project);
            if history.apply(command, &mut f.project).is_ok() {
                applied += 1;
            }
            history.break_gesture();
        }
        assert!(applied > 5, "seed {seed} barely edited anything");

        // Not `while ... .is_some()`: a failing undo puts its entry back, so
        // that loop spins forever instead of reporting anything.
        while let Some(result) = history.undo(&mut f.project) {
            result.unwrap_or_else(|e| panic!("seed {seed}: an undo failed: {e:?}"));
        }
        assert_eq!(
            start,
            snapshot(&f.project),
            "seed {seed}: undoing everything must land on the original document"
        );
    }
}

#[test]
fn redoing_everything_lands_where_the_edits_left_it() {
    for seed in 1..40u64 {
        let mut f = fixture();
        let mut history = History::new();
        let mut rng = Rng(seed);

        for _ in 0..25 {
            let command = random_command(&mut rng, &f, &f.project);
            let _ = history.apply(command, &mut f.project);
            history.break_gesture();
        }
        let edited = snapshot(&f.project);

        while let Some(result) = history.undo(&mut f.project) {
            result.unwrap_or_else(|e| panic!("seed {seed}: an undo failed: {e:?}"));
        }
        while let Some(result) = history.redo(&mut f.project) {
            result.unwrap_or_else(|e| panic!("seed {seed}: a redo failed: {e:?}"));
        }
        assert_eq!(
            edited,
            snapshot(&f.project),
            "seed {seed}: redoing everything must land back on the edited document"
        );
    }
}

#[test]
fn command_error_says_what_went_wrong() {
    let mut project = Project::new("empty");
    let mut command = RemoveClip::new(ClipId::default());
    let CommandError(message) = command.apply(&mut project).expect_err("no such clip");
    assert!(!message.is_empty());
}

// --- Note velocity (the piano roll's velocity lane) -------------------------

/// Velocity is the one note property the roll edits by dragging rather than by
/// moving the note itself, and §16.5 lists it first among the property lanes.
/// It needs its own command for the same reason everything else does: so it is
/// undoable, and so a drag across the lane is one entry in the history rather
/// than forty.
#[test]
fn setting_a_notes_velocity_inverts_exactly() {
    let mut f = fixture();
    round_trips(
        Box::new(fontelle_model::SetNoteVelocity::new(
            f.clip,
            vec![f.notes[0], f.notes[2]],
            17,
        )),
        &mut f.project,
    );
}

#[test]
fn a_velocity_command_remembers_each_notes_own_previous_value() {
    use fontelle_model::{ClipSource, SetNoteVelocity};

    let mut f = fixture();
    // Give the three notes three different velocities, so an inverse that
    // restored one value for all of them would be visible.
    let ids = f.notes.clone();
    for (index, id) in ids.iter().enumerate() {
        let ClipSource::Notes(data) = &mut f.project.clips[f.clip].source else {
            unreachable!()
        };
        data.notes.get_mut(*id).unwrap().velocity = 40 + index as u8 * 20;
    }

    let mut set = SetNoteVelocity::new(f.clip, ids.clone(), 100);
    set.apply(&mut f.project).unwrap();
    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert!(ids.iter().all(|id| data.notes[*id].velocity == 100));

    set.invert().apply(&mut f.project).unwrap();
    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(
            data.notes[*id].velocity,
            40 + index as u8 * 20,
            "each note must come back to its own velocity, not to a shared one"
        );
    }
}

#[test]
fn dragging_across_the_velocity_lane_coalesces_into_one_history_entry() {
    use fontelle_model::{ClipSource, History, SetNoteVelocity};

    let mut f = fixture();
    let mut history = History::new();
    let ids = vec![f.notes[0]];

    for velocity in [90, 80, 70, 60] {
        history
            .apply(
                Box::new(SetNoteVelocity::new(f.clip, ids.clone(), velocity)),
                &mut f.project,
            )
            .unwrap();
    }
    // One drag, one undo — and it goes back to where the drag started, not to
    // the step before last.
    history.undo(&mut f.project).unwrap().unwrap();
    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert_eq!(data.notes[f.notes[0]].velocity, 100);
}

#[test]
fn a_velocity_command_naming_a_note_that_is_gone_is_refused_rather_than_partial() {
    use fontelle_model::SetNoteVelocity;

    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut set = SetNoteVelocity::new(f.clip, vec![f.notes[0], NoteId::default()], 5);
    assert!(set.apply(&mut f.project).is_err());
    assert_eq!(
        before,
        snapshot(&f.project),
        "a refused command must leave the document untouched, not half done"
    );
}

/// The piano roll draws a note and then drags it to length, which means the
/// second half of the gesture needs the id the first half minted. The command
/// keeps it; this is the only way to get at the command after the history has
/// taken ownership of it.
#[test]
fn the_history_can_be_asked_what_it_just_applied() {
    let mut f = fixture();
    let mut history = History::new();
    assert!(history.last_applied().is_none(), "nothing has been applied");

    history
        .apply(
            Box::new(AddNotes::new(f.clip, vec![a_note(PPQN * 3, 72)])),
            &mut f.project,
        )
        .unwrap();
    let ids = history
        .last_applied()
        .and_then(|c| c.as_any().downcast_ref::<AddNotes>())
        .map(|add| add.ids().to_vec())
        .expect("the entry on top is the AddNotes that was just applied");
    assert_eq!(ids.len(), 1);

    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert_eq!(
        data.notes[ids[0]].key, 72,
        "and it names the note that arrived"
    );
}

// --- note properties beyond velocity ---------------------------------------
//
// The piano roll's lane can show pan, tuning, release and the two free
// modulation values as well as velocity (§16.5's property lanes). All five were
// already on `Note` and none of them had a command, so none of them could be
// edited at all — INVARIANT 9 leaves no other way in.

#[test]
fn a_property_command_sets_one_property_and_leaves_the_rest_alone() {
    use fontelle_model::{ClipSource, NoteProperty, SetNoteProperty};

    let mut f = fixture();
    let ids = vec![f.notes[0]];
    let mut set = SetNoteProperty::new(f.clip, ids.clone(), NoteProperty::Pan, -40);
    set.apply(&mut f.project).unwrap();

    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    let note = data.notes[f.notes[0]];
    assert_eq!(note.pan, -40);
    assert_eq!(note.velocity, 100, "velocity is not pan's business");
    assert_eq!(note.fine_pitch, 0);
}

#[test]
fn a_property_command_clamps_to_what_the_field_can_hold() {
    use fontelle_model::{ClipSource, NoteProperty, SetNoteProperty};

    let mut f = fixture();
    let ids = vec![f.notes[0]];
    for (property, asked, expected) in [
        (NoteProperty::Pan, 9_000, 127),
        (NoteProperty::Pan, -9_000, -127),
        (NoteProperty::Velocity, 0, 1),
        (NoteProperty::Velocity, 300, 127),
        // An octave, not a pitch bend's 8191. The range narrowed once fine
        // pitch became audible: read as the cents it is documented in, ±8192
        // is ±81 semitones, which no lane can be aimed inside. See
        // `fontelle-model/tests/note_properties.rs`.
        (NoteProperty::FinePitch, 99_999, 1_200),
        (NoteProperty::FinePitch, -99_999, -1_200),
    ] {
        SetNoteProperty::new(f.clip, ids.clone(), property, asked)
            .apply(&mut f.project)
            .unwrap();
        let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
            unreachable!()
        };
        assert_eq!(
            property.get(&data.notes[f.notes[0]]),
            expected,
            "{property:?} asked for {asked}"
        );
    }
}

#[test]
fn undoing_a_property_edit_restores_each_note_its_own_value() {
    use fontelle_model::{ClipSource, History, NoteProperty, SetNoteProperty};

    let mut f = fixture();
    // Two notes that disagree, which is the case a single restored value gets
    // wrong — the same trap `SetNoteVelocity` documents.
    {
        let ClipSource::Notes(data) = &mut f.project.clips[f.clip].source else {
            unreachable!()
        };
        data.notes[f.notes[0]].pan = -20;
        data.notes[f.notes[1]].pan = 60;
    }
    let ids = vec![f.notes[0], f.notes[1]];

    let mut history = History::new();
    history
        .apply(
            Box::new(SetNoteProperty::new(
                f.clip,
                ids.clone(),
                NoteProperty::Pan,
                0,
            )),
            &mut f.project,
        )
        .unwrap();
    history.undo(&mut f.project).unwrap().unwrap();

    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert_eq!(data.notes[f.notes[0]].pan, -20);
    assert_eq!(data.notes[f.notes[1]].pan, 60);
}

#[test]
fn dragging_a_property_lane_coalesces_into_one_history_entry() {
    use fontelle_model::{ClipSource, History, NoteProperty, SetNoteProperty};

    let mut f = fixture();
    let mut history = History::new();
    let ids = vec![f.notes[0]];

    for pan in [10, 20, 30, 40] {
        history
            .apply(
                Box::new(SetNoteProperty::new(
                    f.clip,
                    ids.clone(),
                    NoteProperty::Pan,
                    pan,
                )),
                &mut f.project,
            )
            .unwrap();
    }
    history.undo(&mut f.project).unwrap().unwrap();
    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert_eq!(data.notes[f.notes[0]].pan, 0, "back to before the drag");

    // But a drag on a *different* property is a different entry: cycling the
    // lane mid-gesture must not fold a pan edit into a velocity one.
    let mut history = History::new();
    history
        .apply(
            Box::new(SetNoteProperty::new(
                f.clip,
                ids.clone(),
                NoteProperty::Pan,
                50,
            )),
            &mut f.project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetNoteProperty::new(
                f.clip,
                ids.clone(),
                NoteProperty::Velocity,
                50,
            )),
            &mut f.project,
        )
        .unwrap();
    history.undo(&mut f.project).unwrap().unwrap();
    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert_eq!(data.notes[f.notes[0]].velocity, 100, "the velocity is back");
    assert_eq!(data.notes[f.notes[0]].pan, 50, "and the pan edit survived");
}

#[test]
fn a_property_command_naming_a_note_that_is_gone_is_refused_rather_than_partial() {
    use fontelle_model::{NoteProperty, SetNoteProperty};

    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut set = SetNoteProperty::new(
        f.clip,
        vec![f.notes[0], NoteId::default()],
        NoteProperty::Pan,
        5,
    );
    assert!(set.apply(&mut f.project).is_err());
    assert_eq!(
        before,
        snapshot(&f.project),
        "a refused command must leave the document untouched, not half done"
    );
}

// --- naming a channel ------------------------------------------------------
//
// A channel's name is what the rack shows, and until this existed there was no
// way to change it — so a channel whose soundfont had been swapped went on
// saying what it used to be, which is the loudest "nothing happened" a rack can
// give somebody who just changed an instrument.

#[test]
fn renaming_a_channel_inverts_exactly() {
    use fontelle_model::RenameChannel;

    let mut f = fixture();
    let before = snapshot(&f.project);
    let was = f.project.channels[f.channel].name.clone();

    let mut rename = RenameChannel::new(f.channel, "tri baja");
    rename.apply(&mut f.project).unwrap();
    assert_eq!(f.project.channels[f.channel].name, "tri baja");
    assert_ne!(was, "tri baja", "the fixture already had the new name");

    rename.invert().apply(&mut f.project).unwrap();
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn renaming_a_channel_that_is_gone_is_refused() {
    use fontelle_model::RenameChannel;
    use fontelle_types::ChannelId;

    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut rename = RenameChannel::new(ChannelId::default(), "nowhere");
    assert!(rename.apply(&mut f.project).is_err());
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn renaming_the_same_channel_twice_is_one_history_entry() {
    use fontelle_model::{History, RenameChannel};

    let mut f = fixture();
    let was = f.project.channels[f.channel].name.clone();
    let mut history = History::new();
    for name in ["a", "ab", "abc"] {
        history
            .apply(
                Box::new(RenameChannel::new(f.channel, name)),
                &mut f.project,
            )
            .unwrap();
    }
    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(
        f.project.channels[f.channel].name, was,
        "one undo goes back to the name it started with, not to \"ab\""
    );
}

// --- one gesture, one entry ------------------------------------------------
//
// Choosing an instrument is two document changes — the patch, and the channel's
// name — and it is one thing a person did. `Compound` is what makes those the
// same number of Ctrl+Z presses as the sentence "I chose an instrument" has
// verbs.

#[test]
fn a_compound_applies_its_parts_in_order_and_undoes_them_all_at_once() {
    use fontelle_model::{Compound, RenameChannel, SetNumber};

    let mut f = fixture();
    let before = snapshot(&f.project);

    let mut compound = Compound::new(
        "Choose instrument",
        vec![
            Box::new(RenameChannel::new(f.channel, "tri baja")),
            Box::new(SetNumber::new(NumberTarget::Tempo, 90.0)),
        ],
    );
    compound.apply(&mut f.project).unwrap();
    assert_eq!(f.project.channels[f.channel].name, "tri baja");
    assert_eq!(f.project.tempo_map.tempo_at(0), 90.0);
    assert_eq!(compound.label(), "Choose instrument");

    compound.invert().apply(&mut f.project).unwrap();
    assert_eq!(
        before,
        snapshot(&f.project),
        "a compound's inverse is its parts' inverses, in reverse"
    );
}

#[test]
fn a_compound_whose_second_part_fails_leaves_the_document_alone() {
    use fontelle_model::{Compound, RenameChannel};
    use fontelle_types::ChannelId;

    let mut f = fixture();
    let before = snapshot(&f.project);

    let mut compound = Compound::new(
        "Two renames",
        vec![
            Box::new(RenameChannel::new(f.channel, "first")),
            Box::new(RenameChannel::new(ChannelId::default(), "nowhere")),
        ],
    );
    assert!(compound.apply(&mut f.project).is_err());
    assert_eq!(
        before,
        snapshot(&f.project),
        "half a compound is exactly the state its inverse cannot describe"
    );
}

#[test]
fn a_compound_goes_through_the_history_as_one_entry() {
    use fontelle_model::{ClipSource, Compound, History, RemoveNotes, RenameChannel};

    let mut f = fixture();
    let mut history = History::new();
    history
        .apply(
            Box::new(Compound::new(
                "Tidy up",
                vec![
                    Box::new(RenameChannel::new(f.channel, "lead")),
                    Box::new(RemoveNotes::new(f.clip, vec![f.notes[0]])),
                ],
            )),
            &mut f.project,
        )
        .unwrap();

    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(f.project.channels[f.channel].name, "Part");
    let ClipSource::Notes(data) = &f.project.clips[f.clip].source else {
        unreachable!()
    };
    assert!(
        data.notes.get(f.notes[0]).is_some(),
        "one undo put both halves back"
    );
}

// --- resizing a clip -------------------------------------------------------
//
// The arrangement canvas drags a clip's right-hand edge, which is the one clip
// operation the command set was missing — `MoveClip` and `DuplicateClip` were
// both there.

#[test]
fn resizing_a_clip_inverts_exactly() {
    use fontelle_model::ResizeClip;

    let mut f = fixture();
    let before = snapshot(&f.project);
    let was = f.project.clips[f.clip].length;

    let mut resize = ResizeClip::new(f.clip, PPQN * 4);
    resize.apply(&mut f.project).unwrap();
    assert_eq!(f.project.clips[f.clip].length, was + PPQN * 4);

    resize.invert().apply(&mut f.project).unwrap();
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn a_clip_can_never_be_resized_to_nothing() {
    use fontelle_model::ResizeClip;

    let mut f = fixture();
    let mut resize = ResizeClip::new(f.clip, -PPQN * 10_000);
    resize.apply(&mut f.project).unwrap();
    assert!(
        f.project.clips[f.clip].length > 0,
        "a clip of zero length is one nobody can grab again"
    );
}

#[test]
fn dragging_a_clips_edge_coalesces_into_one_history_entry() {
    use fontelle_model::{History, ResizeClip};

    let mut f = fixture();
    let was = f.project.clips[f.clip].length;
    let mut history = History::new();
    for _ in 0..4 {
        history
            .apply(Box::new(ResizeClip::new(f.clip, PPQN)), &mut f.project)
            .unwrap();
    }
    assert_eq!(f.project.clips[f.clip].length, was + PPQN * 4);
    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(f.project.clips[f.clip].length, was, "one drag, one undo");
}

#[test]
fn turning_a_knob_on_a_patch_coalesces_into_one_history_entry() {
    use fontelle_model::{History, SetChannelPatch};
    use fontelle_types::PatchData;

    let mut f = fixture();
    let was = f.project.channels[f.channel].patch_data.clone();
    let mut history = History::new();

    // The instrument editor writes the whole patch on every step of a knob
    // drag; without a merge, one drag would leave forty entries and one Ctrl+Z
    // would go back one pixel.
    for cutoff in [200u32, 400, 800, 1600] {
        history
            .apply(
                Box::new(SetChannelPatch::new(
                    f.channel,
                    Some(PatchData {
                        format_version: 0,
                        body: serde_json::json!({ "cutoff": cutoff }),
                    }),
                )),
                &mut f.project,
            )
            .unwrap();
    }
    assert_eq!(
        f.project.channels[f.channel]
            .patch_data
            .as_ref()
            .map(|d| d.body.clone()),
        Some(serde_json::json!({ "cutoff": 1600 }))
    );

    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(
        f.project.channels[f.channel]
            .patch_data
            .as_ref()
            .map(|d| d.body.clone()),
        was.as_ref().map(|d| d.body.clone()),
        "one undo went back one step of the drag instead of all of it"
    );
}

#[test]
fn a_patch_written_to_a_different_channel_is_a_different_entry() {
    use fontelle_model::{AddChannel, History, SetChannelPatch};
    use fontelle_types::PatchData;

    let mut f = fixture();
    let mut history = History::new();
    let mut add = AddChannel::new("Second", None);
    add.apply(&mut f.project).unwrap();
    let second = add.channel().expect("the channel was created");

    let data = |n: u32| {
        Some(PatchData {
            format_version: 0,
            body: serde_json::json!({ "n": n }),
        })
    };
    history
        .apply(
            Box::new(SetChannelPatch::new(f.channel, data(1))),
            &mut f.project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetChannelPatch::new(second, data(2))),
            &mut f.project,
        )
        .unwrap();

    history.undo(&mut f.project).unwrap().unwrap();
    assert!(
        f.project.channels[second].patch_data.is_none(),
        "the second channel's patch came off"
    );
    assert_eq!(
        f.project.channels[f.channel]
            .patch_data
            .as_ref()
            .map(|d| d.body.clone()),
        data(1).map(|d| d.body),
        "and the first channel's survived — two channels are two gestures"
    );
}

// ---- docs/collab-plan.md §5.2: the edits that used to skip a command ----

/// F7. The project's name was written straight into the document from four
/// places in the session. It is the bundle's label rather than the song, so
/// it never crosses a wire (§5.6), but it goes through a command like
/// everything else that changes the document (INVARIANT 9).
#[test]
fn renaming_the_project_inverts_exactly() {
    let mut f = fixture();
    round_trips(
        Box::new(fontelle_model::RenameProject::new("Second Song")),
        &mut f.project,
    );
}

/// F5. An automation clip used to get its row from a bare insert — off the
/// undo stack, so taking the clip back left an empty lavender row behind, and
/// invisible to anyone sharing the song. The row and the clip are one
/// command now, and one undo.
#[test]
fn a_clip_on_a_new_row_is_one_command_and_one_undo() {
    let mut f = fixture();
    let lanes_before = f.project.lanes.len();
    let clip = Clip {
        lane: f.lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel: f.channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    };
    let mut add =
        fontelle_model::AddClip::on_new_row(clip.clone(), "Part \u{2014} pan", [1, 2, 3, 255]);
    add.apply(&mut f.project).unwrap();
    let made = add.id().expect("the clip is known once it is applied");
    let row = f.project.clips[made].lane;
    assert_ne!(row, f.lane, "on a row of its own");
    assert_eq!(f.project.lanes[row].name, "Part \u{2014} pan");
    assert_eq!(f.project.lanes[row].color, [1, 2, 3, 255]);
    assert_eq!(f.project.lanes.len(), lanes_before + 1);

    add.invert().apply(&mut f.project).unwrap();
    assert_eq!(f.project.lanes.len(), lanes_before, "the row went with it");
    round_trips(
        Box::new(fontelle_model::AddClip::on_new_row(clip, "again", [0; 4])),
        &mut f.project,
    );
}

/// F9. Markers are an arena now, so an edit can name one.
#[test]
fn adding_and_removing_a_marker_inverts_exactly() {
    let mut f = fixture();
    let mut add = fontelle_model::AddMarker::new("Chorus", PPQN * 16);
    add.apply(&mut f.project).unwrap();
    let marker = add.id().expect("minted on apply");
    assert_eq!(f.project.markers[marker].name, "Chorus");
    round_trips(
        Box::new(fontelle_model::RemoveMarker::new(marker)),
        &mut f.project,
    );
    round_trips(
        Box::new(fontelle_model::AddMarker::new("Bridge", PPQN * 32)),
        &mut f.project,
    );
}

/// F6. A render lands on a row named after the row it came from — "Lane 1
/// (rendered)" — and that name used to be written into the lane by hand after
/// the import's command, where no undo and no wire could see it. The import
/// names the row it makes now.
#[test]
fn an_imported_clip_can_name_the_row_it_makes() {
    let mut f = fixture();
    let asset = fontelle_types::AssetRef {
        id: fontelle_types::AssetId::default(),
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: fontelle_types::AssetKind::Sample,
    };
    let data = fontelle_types::AudioClipData::whole(asset, 48_000, 48_000);
    let mut add = fontelle_model::AddAudioClip::new("take.wav", data.clone(), 0, PPQN)
        .row_named("Lane 1 (rendered)");
    add.apply(&mut f.project).unwrap();
    let clip = add.clip().unwrap();
    let row = f.project.clips[clip].lane;
    assert_eq!(f.project.lanes[row].name, "Lane 1 (rendered)");
    round_trips(
        Box::new(fontelle_model::AddAudioClip::new("take.wav", data, 0, PPQN).row_named("again")),
        &mut f.project,
    );
}

/// F23. Collecting a song's files moves where they are kept, and every
/// reference to a moved file has to follow it — an audio clip's, a prefab's,
/// and the ones inside a patch's own body — or a clip plays nothing and a
/// sampler goes silent. One command, so it can be taken back like any other.
#[test]
fn relocating_a_file_rewrites_every_reference_to_it() {
    use fontelle_types::{AssetId, AssetKind, AssetRef, AudioClipData, PatchData};
    let mut f = fixture();
    let asset = |path: &str, hash: u64| AssetRef {
        id: AssetId::default(),
        path: path.into(),
        content_hash: hash,
        size: 10,
        kind: AssetKind::Sample,
    };
    let old = asset("/home/alice/loop.wav", 0);
    AddClip::new(Clip {
        lane: f.lane,
        start: PPQN * 8,
        length: PPQN,
        source: ClipSource::Audio(AudioClipData::whole(old.clone(), 4_800, 48_000)),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(&mut f.project)
    .unwrap();
    fontelle_model::AddPrefab::new(
        "Loop",
        ClipSource::Audio(AudioClipData::whole(old.clone(), 4_800, 48_000)),
    )
    .apply(&mut f.project)
    .unwrap();
    // A patch body names its file inside JSON the model cannot read as types.
    let body = serde_json::json!({ "layers": [{ "source": { "Sample": {
        "file": { "file": serde_json::to_value(&old).unwrap(), "sample": 0 }
    } } }] });
    SetChannelPatch::new(
        f.channel,
        Some(PatchData {
            format_version: 1,
            body,
        }),
    )
    .apply(&mut f.project)
    .unwrap();
    let other = asset("/home/alice/other.wav", 5);
    AddClip::new(Clip {
        lane: f.lane,
        start: PPQN * 12,
        length: PPQN,
        source: ClipSource::Audio(AudioClipData::whole(other.clone(), 4_800, 48_000)),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(&mut f.project)
    .unwrap();
    assert_eq!(f.project.files().len(), 4);

    let moved = asset("assets/9f86d08.wav", 0x9f86);
    round_trips(
        Box::new(fontelle_model::RelocateAssets::new(vec![(
            old.path.clone(),
            moved.path.clone(),
            moved.content_hash,
            moved.size,
        )])),
        &mut f.project,
    );
    fontelle_model::RelocateAssets::new(vec![(
        old.path.clone(),
        moved.path.clone(),
        moved.content_hash,
        moved.size,
    )])
    .apply(&mut f.project)
    .unwrap();
    let files = f.project.files();
    assert_eq!(files.iter().filter(|a| a.path == moved.path).count(), 3);
    assert!(files.iter().all(|a| a.path != old.path), "{files:?}");
    assert!(files.contains(&other), "a file not moved stays");
    assert!(
        files
            .iter()
            .filter(|a| a.path == moved.path)
            .all(|a| a.content_hash == 0x9f86),
        "the hash moves with it"
    );
}
