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
        name: "Part".into(),
        color: [0; 4],
        mixer_track: track,
        patch_data: None,
        pan: 0.0,
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
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
fn adding_a_channel_gives_it_a_mixer_track_of_its_own() {
    // One command, one undo entry: choosing an instrument should not need two
    // presses of Ctrl+Z to take back.
    let mut f = fixture();
    let tracks_before = f.project.mixer.tracks.len();
    let mut add = AddChannel::new("Strings", None);
    add.apply(&mut f.project).unwrap();

    let channel = add.channel().expect("the id is known once it is applied");
    assert_eq!(f.project.mixer.tracks.len(), tracks_before + 1);
    let track = f.project.channels[channel].mixer_track;
    assert!(f.project.mixer.tracks.contains_key(track));
    assert_eq!(f.project.mixer.tracks[track].output, f.project.mixer.master);
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
