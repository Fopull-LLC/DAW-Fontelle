//! Cutting notes in two (the piano roll's slice tool).
//!
//! Reported from using the window: *"We need a cut tool in the piano roll (C)
//! that works basically the same as the FL Studio cut tool."*
//!
//! `Tool::Slice` has been in the tool list since the roll was written, was
//! bound to `5`, and did nothing — the same shape as every other control in
//! this project that existed and could not be reached.
//!
//! One command rather than "shorten this note, then add that one", because a
//! slice is **one gesture**: a line drawn across a chord cuts every note it
//! crosses, and taking that back should be one press of Ctrl+Z rather than one
//! per note.

use fontelle_model::{
    AddClip, AddNotes, Arena, Clip, ClipSource, Command, Note, NoteData, Project, SliceNotes,
    TempoMap,
};
use fontelle_types::{ClipId, NoteId, PPQN};

fn a_note(start: i64, length: i64, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 40,
        fine_pitch: -12,
        release: 7,
        mod_x: 3,
        mod_y: 9,
        slide: false,
        channel: None,
    }
}

/// A project with one clip holding `notes`, and their ids in order.
fn fixture(notes: Vec<Note>) -> (Project, ClipId, Vec<NoteId>) {
    let mut project = Project::new("slicing");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel = project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "ch".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
    });
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut add = AddClip::new(Clip {
        lane,
        start: 0,
        length: PPQN * 16,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    add.apply(&mut project).unwrap();
    let clip = add.id().unwrap();

    let mut insert = AddNotes::new(clip, notes);
    insert.apply(&mut project).unwrap();
    let ids = insert.ids().to_vec();
    (project, clip, ids)
}

/// Every note in the clip, as `(start, length, key)`, sorted.
fn notes_of(project: &Project, clip: ClipId) -> Vec<(i64, i64, u8)> {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip")
    };
    let mut out: Vec<(i64, i64, u8)> = data
        .notes
        .values()
        .map(|n| (n.start, n.length, n.key))
        .collect();
    out.sort();
    out
}

#[test]
fn a_note_cut_in_the_middle_becomes_two_that_meet_where_it_was_cut() {
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN * 4, 60)]);

    SliceNotes::new(clip, vec![(ids[0], PPQN)])
        .apply(&mut project)
        .unwrap();

    assert_eq!(
        notes_of(&project, clip),
        vec![(0, PPQN, 60), (PPQN, PPQN * 3, 60)],
        "no gap and no overlap: the second starts exactly where the first ends"
    );
}

#[test]
fn the_second_half_keeps_everything_the_note_was() {
    // A slice is one note becoming two, not one note and a default one. Every
    // per-note property §16.5 names has to come across, or cutting a phrase
    // silently flattens its dynamics.
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN * 4, 60)]);
    SliceNotes::new(clip, vec![(ids[0], PPQN * 2)])
        .apply(&mut project)
        .unwrap();

    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!()
    };
    let tail = data
        .notes
        .values()
        .find(|n| n.start == PPQN * 2)
        .expect("the second half");
    assert_eq!(tail.velocity, 100);
    assert_eq!(tail.pan, 40);
    assert_eq!(tail.fine_pitch, -12);
    assert_eq!(tail.release, 7);
    assert_eq!(tail.mod_x, 3);
    assert_eq!(tail.mod_y, 9);
    assert_eq!(tail.key, 60);
}

#[test]
fn a_slide_note_stays_a_slide_on_both_sides_of_the_cut() {
    let mut note = a_note(0, PPQN * 4, 60);
    note.slide = true;
    let (mut project, clip, ids) = fixture(vec![note]);
    SliceNotes::new(clip, vec![(ids[0], PPQN * 2)])
        .apply(&mut project)
        .unwrap();

    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!()
    };
    assert!(data.notes.values().all(|n| n.slide));
}

#[test]
fn one_line_across_a_chord_is_one_command_and_one_undo() {
    // The reason this is a command and not three: a line drawn across a chord
    // cuts every note it crosses, and taking that back is one gesture.
    let (mut project, clip, ids) = fixture(vec![
        a_note(0, PPQN * 4, 60),
        a_note(0, PPQN * 4, 64),
        a_note(0, PPQN * 4, 67),
    ]);
    let before = notes_of(&project, clip);

    let mut command = SliceNotes::new(clip, vec![(ids[0], PPQN), (ids[1], PPQN), (ids[2], PPQN)]);
    command.apply(&mut project).unwrap();
    assert_eq!(notes_of(&project, clip).len(), 6);

    command.invert().apply(&mut project).unwrap();
    assert_eq!(
        notes_of(&project, clip),
        before,
        "one undo, whole chord back"
    );
}

#[test]
fn a_diagonal_slice_cuts_each_note_where_the_line_actually_crossed_it() {
    // Which is the whole point of a *line* rather than a vertical cut: a
    // diagonal through a chord staggers the cuts, and that is a musical
    // gesture rather than an artefact.
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN * 4, 60), a_note(0, PPQN * 4, 64)]);
    SliceNotes::new(clip, vec![(ids[0], PPQN), (ids[1], PPQN * 2)])
        .apply(&mut project)
        .unwrap();

    assert_eq!(
        notes_of(&project, clip),
        vec![
            (0, PPQN, 60),
            (0, PPQN * 2, 64),
            (PPQN, PPQN * 3, 60),
            (PPQN * 2, PPQN * 2, 64),
        ]
    );
}

#[test]
fn a_cut_at_a_notes_own_edge_is_refused_rather_than_making_a_note_of_nothing() {
    // A zero-length note is a note-on and a note-off at the same sample, which
    // is silence you cannot see and cannot select.
    let (mut project, clip, ids) = fixture(vec![a_note(PPQN, PPQN * 2, 60)]);
    let before = notes_of(&project, clip);

    for at in [PPQN, PPQN * 3, 0, PPQN * 8] {
        SliceNotes::new(clip, vec![(ids[0], at)])
            .apply(&mut project)
            .unwrap();
        assert_eq!(
            notes_of(&project, clip),
            before,
            "a cut at {at} is outside the note and must do nothing"
        );
    }
}

#[test]
fn slicing_nothing_is_not_a_history_entry_worth_inverting() {
    let (mut project, clip, _) = fixture(vec![a_note(0, PPQN * 4, 60)]);
    let before = notes_of(&project, clip);

    let mut command = SliceNotes::new(clip, Vec::new());
    command.apply(&mut project).unwrap();
    assert_eq!(notes_of(&project, clip), before, "nothing was cut");

    // And its inverse **refuses**, which is this crate's own convention for
    // "there is nothing here to take back" (`NotApplied`) rather than a
    // silent no-op that would sit in the history looking like an edit.
    assert!(command.invert().apply(&mut project).is_err());
    assert_eq!(notes_of(&project, clip), before);
}

#[test]
fn a_slice_can_be_redone_onto_the_same_ids() {
    // The rule every creating command here follows: a redo re-uses the ids it
    // minted, or the commands stacked above it point at nothing.
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN * 4, 60)]);

    let mut command = SliceNotes::new(clip, vec![(ids[0], PPQN * 2)]);
    command.apply(&mut project).unwrap();
    let after = notes_of(&project, clip);
    let made = command.created().to_vec();
    assert_eq!(made.len(), 1);

    command.invert().apply(&mut project).unwrap();
    command.apply(&mut project).unwrap();
    assert_eq!(notes_of(&project, clip), after);
    assert_eq!(command.created(), made.as_slice(), "the same ids came back");
}

#[test]
fn a_cut_naming_a_note_that_is_gone_is_skipped_rather_than_failing() {
    // A selection can outlive the notes in it, and a slice that refuses
    // outright would lose the cuts that were still good.
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN * 4, 60)]);
    let ghost = NoteId::default();

    SliceNotes::new(clip, vec![(ghost, PPQN), (ids[0], PPQN)])
        .apply(&mut project)
        .unwrap();
    assert_eq!(notes_of(&project, clip).len(), 2, "the good cut still ran");
}
