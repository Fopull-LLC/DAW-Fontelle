//! **Note paths**: one note that holds, slides, holds and slides again,
//! drawn where it goes (`docs/note-paths-plan.md`).
//!
//! Ty: *"when dragging out a note, you can press the s key to place a point
//! there so the note extends to that point, then past that point it becomes a
//! slide note ... id want to be able to make a note slide as many times as i
//! want."*
//!
//! The FL slide note this replaces bent *every* note sounding on the channel,
//! so a chord could not slide apart. A path belongs to one note, so it can.

use fontelle_model::{
    AddClip, AddNotes, Arena, Clip, ClipSource, Command, MoveNotes, Note, NoteData, PathPoint,
    Project, SetNotePath, SliceNotes, TempoMap,
};
use fontelle_types::{ClipId, NoteId, PPQN};

fn a_note(start: i64, length: i64, key: u8) -> Note {
    Note {
        start,
        length,
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

fn point(at: i64, offset: i8) -> PathPoint {
    PathPoint { at, offset }
}

/// Hold a beat at the key, slide up a fifth over the next beat, hold two.
fn hold_slide_hold() -> Note {
    let mut note = a_note(0, PPQN * 4, 60);
    note.path = vec![point(PPQN, 0), point(PPQN * 2, 7)];
    note
}

/// A project with one clip holding `notes`, and their ids in order.
fn fixture(notes: Vec<Note>) -> (Project, ClipId, Vec<NoteId>) {
    let mut project = Project::new("paths");
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
        soloed: false,
        order: 0,
    });
    let mut add = AddClip::new(Clip {
        name: None,
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

fn note(project: &Project, clip: ClipId, id: NoteId) -> Note {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip")
    };
    data.notes[id].clone()
}

fn all_notes(project: &Project, clip: ClipId) -> Vec<Note> {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip")
    };
    let mut out: Vec<Note> = data.notes.values().cloned().collect();
    out.sort_by_key(|n| n.start);
    out
}

#[test]
fn a_plain_note_has_no_path_and_stays_on_its_key() {
    let plain = a_note(0, PPQN * 4, 60);
    assert!(!plain.has_path());
    for at in [0, PPQN, PPQN * 3, PPQN * 4] {
        assert_eq!(plain.pitch_at(at), 0.0);
    }
}

#[test]
fn the_pitch_follows_the_lines_between_the_points() {
    let note = hold_slide_hold();
    assert!(note.has_path());
    assert_eq!(note.pitch_at(0), 0.0, "starts on its key");
    assert_eq!(note.pitch_at(PPQN), 0.0, "flat up to the first point");
    assert_eq!(
        note.pitch_at(PPQN + PPQN / 2),
        3.5,
        "halfway along the diagonal is halfway up"
    );
    assert_eq!(note.pitch_at(PPQN * 2), 7.0, "arrives at the second point");
    assert_eq!(
        note.pitch_at(PPQN * 4),
        7.0,
        "past the last point it holds where it arrived"
    );
}

#[test]
fn a_path_can_slide_as_many_times_as_it_likes() {
    // One note, a whole melody: up two, down one, up five.
    let mut note = a_note(0, PPQN * 8, 60);
    note.path = vec![
        point(PPQN, 2),
        point(PPQN * 2, 2),
        point(PPQN * 3, -1),
        point(PPQN * 5, 4),
    ];
    assert_eq!(note.pitch_at(PPQN), 2.0);
    assert_eq!(note.pitch_at(PPQN * 2), 2.0);
    assert_eq!(note.pitch_at(PPQN * 3), -1.0);
    assert_eq!(note.pitch_at(PPQN * 4), 1.5);
    assert_eq!(note.pitch_at(PPQN * 7), 4.0);
}

#[test]
fn the_segments_name_each_hold_and_each_slide() {
    let note = hold_slide_hold();
    let segments: Vec<_> = note.segments().collect();
    assert_eq!(
        segments,
        vec![
            ((0, 0), (PPQN, 0)),
            ((PPQN, 0), (PPQN * 2, 7)),
            ((PPQN * 2, 7), (PPQN * 4, 7)),
        ],
        "start to each point, then a hold to the note's end"
    );
}

#[test]
fn a_path_ending_exactly_at_the_end_adds_no_empty_hold() {
    // Releasing the drag mid-slide puts the last point on the note's end.
    let mut note = a_note(0, PPQN * 2, 60);
    note.path = vec![point(PPQN, 0), point(PPQN * 2, 5)];
    assert_eq!(
        note.segments().collect::<Vec<_>>(),
        vec![((0, 0), (PPQN, 0)), ((PPQN, 0), (PPQN * 2, 5))]
    );
}

#[test]
fn a_project_written_before_paths_reads_as_plain_notes() {
    let old = r#"{"start":0,"length":960,"key":60,"velocity":100,"pan":0,
        "fine_pitch":0,"release":0,"mod_x":0,"mod_y":0}"#;
    let note: Note = serde_json::from_str(old).expect("reads");
    assert!(note.path.is_empty());
}

#[test]
fn a_plain_note_writes_no_path_and_a_path_round_trips() {
    let plain = serde_json::to_string(&a_note(0, PPQN, 60)).unwrap();
    assert!(!plain.contains("path"), "{plain}");

    let note = hold_slide_hold();
    let back: Note = serde_json::from_str(&serde_json::to_string(&note).unwrap()).unwrap();
    assert_eq!(back, note);
}

#[test]
fn setting_a_path_is_one_undoable_edit_and_puts_the_points_in_time_order() {
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN * 4, 60)]);
    let mut set = SetNotePath::new(clip, ids[0], vec![point(PPQN * 2, 7), point(PPQN, 0)]);
    set.apply(&mut project).unwrap();
    assert_eq!(
        note(&project, clip, ids[0]).path,
        vec![point(PPQN, 0), point(PPQN * 2, 7)]
    );

    set.invert().apply(&mut project).unwrap();
    assert!(
        note(&project, clip, ids[0]).path.is_empty(),
        "undo puts it back"
    );
}

#[test]
fn moving_a_note_carries_its_whole_shape() {
    // The points are relative to the note, so a move up a third moves the
    // slide's landing up a third with it.
    let (mut project, clip, ids) = fixture(vec![hold_slide_hold()]);
    MoveNotes::new(clip, vec![ids[0]], PPQN, 4)
        .apply(&mut project)
        .unwrap();
    let moved = note(&project, clip, ids[0]);
    assert_eq!((moved.start, moved.key), (PPQN, 64));
    assert_eq!(moved.path, hold_slide_hold().path);
}

#[test]
fn a_cut_mid_slide_starts_the_second_half_where_the_pitch_had_got_to() {
    let (mut project, clip, ids) = fixture(vec![hold_slide_hold()]);
    // Cut at 1.5 beats: 3.5 semitones up, which rounds to 64 (60 + 4).
    SliceNotes::new(clip, vec![(ids[0], PPQN + PPQN / 2)])
        .apply(&mut project)
        .unwrap();
    let notes = all_notes(&project, clip);
    assert_eq!(notes.len(), 2);
    let (head, tail) = (&notes[0], &notes[1]);
    assert_eq!(head.length, PPQN + PPQN / 2);
    assert_eq!(
        head.pitch_at(head.length),
        3.5,
        "the first half still slides up to the cut"
    );
    assert_eq!(tail.key, 64);
    assert_eq!(
        tail.path,
        vec![point(PPQN / 2, 3)],
        "the rest of the slide, measured from the new key: it lands on 67 again"
    );
    assert_eq!(tail.pitch_at(tail.length), 3.0);
}

#[test]
fn a_cut_after_the_slide_starts_the_second_half_on_the_landed_key() {
    let (mut project, clip, ids) = fixture(vec![hold_slide_hold()]);
    SliceNotes::new(clip, vec![(ids[0], PPQN * 3)])
        .apply(&mut project)
        .unwrap();
    let notes = all_notes(&project, clip);
    let tail = &notes[1];
    assert_eq!(tail.key, 67);
    assert!(tail.path.is_empty(), "nothing left to slide");
}

#[test]
fn undoing_a_cut_through_a_path_gives_back_the_whole_note() {
    let (mut project, clip, ids) = fixture(vec![hold_slide_hold()]);
    let mut slice = SliceNotes::new(clip, vec![(ids[0], PPQN + PPQN / 2)]);
    slice.apply(&mut project).unwrap();
    slice.invert().apply(&mut project).unwrap();
    assert_eq!(all_notes(&project, clip), vec![hold_slide_hold()]);
}

#[test]
fn a_shape_sets_the_length_with_the_path_and_undo_puts_both_back() {
    // Drawing a path with S moves the note's end with the pointer; one edit,
    // so one undo takes both back.
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN, 60)]);
    let mut set = SetNotePath::new(clip, ids[0], vec![point(PPQN, 0), point(PPQN * 2, 7)])
        .with_length(PPQN * 2);
    set.apply(&mut project).unwrap();
    let shaped = note(&project, clip, ids[0]);
    assert_eq!(shaped.length, PPQN * 2);
    assert_eq!(shaped.path.len(), 2);

    set.invert().apply(&mut project).unwrap();
    let back = note(&project, clip, ids[0]);
    assert_eq!(back.length, PPQN);
    assert!(back.path.is_empty());
}

#[test]
fn a_drag_of_shapes_is_one_undo() {
    // Every pointer step of the S-gesture is a shape; they fold into the
    // first, which keeps what the note was before any of them.
    let (mut project, clip, ids) = fixture(vec![a_note(0, PPQN, 60)]);
    let mut first = SetNotePath::new(clip, ids[0], vec![point(PPQN, 0), point(PPQN * 2, 3)])
        .with_length(PPQN * 2);
    first.apply(&mut project).unwrap();
    let mut second = SetNotePath::new(clip, ids[0], vec![point(PPQN, 0), point(PPQN * 3, 5)])
        .with_length(PPQN * 3);
    second.apply(&mut project).unwrap();
    assert!(first.merge_with(&second), "the same note's shape folds");

    first.invert().apply(&mut project).unwrap();
    let back = note(&project, clip, ids[0]);
    assert_eq!((back.length, back.path.len()), (PPQN, 0));

    // Another note's is its own entry.
    let (_, _, other) = fixture(vec![a_note(0, PPQN, 60), a_note(PPQN, PPQN, 62)]);
    let third = SetNotePath::new(clip, other[1], Vec::new());
    assert!(!first.merge_with(&third));
}

#[test]
fn a_path_previews_as_holds_and_a_staircase_up_each_slide() {
    // The arrangement draws a clip's notes as small bars, a row each; a
    // slide is a run of short steps from row to row, which at that size
    // reads as the slant it is.
    let note = hold_slide_hold();
    let pieces = note.preview_pieces();
    assert_eq!(pieces.first(), Some(&(0, PPQN, 60)), "the hold");
    assert_eq!(
        pieces.last(),
        Some(&(PPQN * 2, PPQN * 2, 67)),
        "held where it landed"
    );
    // Contiguous, end to end, covering the whole note.
    let mut at = 0;
    for &(start, length, _) in &pieces {
        assert_eq!(start, at, "{pieces:?}");
        assert!(length > 0);
        at = start + length;
    }
    assert_eq!(at, note.length);
    // The steps climb a key at a time.
    let keys: Vec<u8> = pieces.iter().map(|p| p.2).collect();
    assert!(
        keys.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] <= 1),
        "{keys:?}"
    );
    assert!(keys.contains(&63), "it passes through the keys between");

    // A plain note is itself.
    assert_eq!(a_note(PPQN, PPQN, 62).preview_pieces(), vec![(0, PPQN, 62)]);
}
