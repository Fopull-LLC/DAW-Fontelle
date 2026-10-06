//! Looping a clip, as against copying it.
//!
//! Reported from using the window:
//!
//! > *"I want to make it easier to loop things vs just extend the clip. [...]
//! > I want clips that I loop to be genuinely looped so it's just repeating
//! > what was in the first clip length. Copying and pasting is its own
//! > separate thing but looping is its own feature too."*
//!
//! The distinction is real and it is worth writing down, because the two look
//! the same on screen and are nothing alike underneath:
//!
//! - **Copying** makes new clips with their own notes. Editing one does not
//!   touch the others; that is the point of it.
//! - **Looping** is *one* clip whose content repeats. There is one set of
//!   notes, so editing bar 1 changes every repeat — which is what a loop is
//!   for and what "repeat this drum pattern" means.
//!
//! `Clip::loop_length` is the whole of the model half: the period the content
//! repeats at. `None` is a clip that plays once, which is what every clip was
//! before this.

use fontelle_model::{
    AddClip, Arena, Clip, ClipSource, Command, Note, NoteData, Project, RenameClip, ResizeClip,
    ResizeClips, SetClipLoop, TempoMap,
};
use fontelle_types::{ClipId, LaneId, PPQN};

fn a_note(start: i64, key: u8) -> Note {
    Note {
        start,
        length: PPQN / 2,
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

/// A project with one two-bar clip holding one note in its first bar.
fn fixture() -> (Project, ClipId, LaneId) {
    let mut project = Project::new("looping");
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
    let mut notes = Arena::default();
    notes.insert(a_note(0, 60));

    let mut add = AddClip::new(Clip {
        name: None,
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    add.apply(&mut project).unwrap();
    let clip = add.id().expect("the id is known once it is applied");
    (project, clip, lane)
}

#[test]
fn a_clip_plays_once_until_somebody_loops_it() {
    let (project, clip, _) = fixture();
    assert_eq!(
        project.clips[clip].loop_length, None,
        "every clip that ever existed before this was a play-once clip"
    );
}

#[test]
fn looping_a_clip_records_the_period_its_content_repeats_at() {
    let (mut project, clip, _) = fixture();

    SetClipLoop::new(clip, Some(PPQN * 4))
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.clips[clip].loop_length, Some(PPQN * 4));
}

#[test]
fn a_loop_is_undoable_like_every_other_edit() {
    let (mut project, clip, _) = fixture();
    let mut command = SetClipLoop::new(clip, Some(PPQN * 4));
    command.apply(&mut project).unwrap();
    command.invert().apply(&mut project).unwrap();
    assert_eq!(project.clips[clip].loop_length, None);
}

#[test]
fn a_loop_period_of_nothing_is_refused() {
    // A period of zero repeats for ever in no time at all, which is an
    // infinite loop in the compiler rather than a musical statement.
    let (mut project, clip, _) = fixture();
    assert!(SetClipLoop::new(clip, Some(0)).apply(&mut project).is_err());
    assert!(
        SetClipLoop::new(clip, Some(-PPQN))
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(project.clips[clip].loop_length, None);
}

#[test]
fn stretching_a_looped_clip_leaves_its_period_alone() {
    // The whole gesture: the clip gets longer, the *content* does not. That
    // is the difference between looping and stretching, and it is the one
    // thing a resize must not quietly undo.
    let (mut project, clip, _) = fixture();
    SetClipLoop::new(clip, Some(PPQN * 4))
        .apply(&mut project)
        .unwrap();

    ResizeClip::new(clip, PPQN * 12)
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.clips[clip].length, PPQN * 16);
    assert_eq!(
        project.clips[clip].loop_length,
        Some(PPQN * 4),
        "a loop stretched to four bars still repeats every one"
    );
}

#[test]
fn how_many_times_a_clip_repeats_is_arithmetic_anyone_can_check() {
    let (mut project, clip, _) = fixture();
    assert_eq!(project.clips[clip].repeats(), 1, "a plain clip plays once");

    SetClipLoop::new(clip, Some(PPQN))
        .apply(&mut project)
        .unwrap();
    assert_eq!(
        project.clips[clip].repeats(),
        4,
        "a four-beat clip looping every beat plays four times"
    );

    // A partial repeat at the end still counts: it is drawn, and the notes
    // inside it that start before the clip's end still sound.
    ResizeClip::new(clip, PPQN / 2).apply(&mut project).unwrap();
    assert_eq!(project.clips[clip].repeats(), 5);
}

#[test]
fn a_period_longer_than_the_clip_is_one_repeat_and_not_zero() {
    let (mut project, clip, _) = fixture();
    SetClipLoop::new(clip, Some(PPQN * 16))
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.clips[clip].repeats(), 1);
}

// ------------------------------------------- several clips, one edge drag ---
//
// Ty, from using the window: *"if you have multiple clips selected, some
// looping and some not, and then youre holding shift and drag the end of the
// clips out, the expected behavior is it should make any nonlooping clips
// loop from their end point and extend from there looping, any already
// looping clips just extend like normal. it would basically be like if you
// dragged each one out manually. however whenever i do this, it chops
// everything up into the same loop time and then squishes it weirdly."*
//
// `ResizeClips` is the whole selection's edge drag as **one** command: each
// clip's own period (if it is being made to loop) set on the first step, the
// same delta on every end, and every later step folded into it — one undo.

/// Another clip on `lane`, `length` long, looping at `loop_length`.
fn another(project: &mut Project, lane: LaneId, length: i64, loop_length: Option<i64>) -> ClipId {
    let channel = project
        .channels
        .keys()
        .next()
        .expect("the fixture has a channel");
    let mut add = AddClip::new(Clip {
        name: None,
        lane,
        start: PPQN * 16,
        length,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length,
    });
    add.apply(project).unwrap();
    add.id().unwrap()
}

#[test]
fn a_mixed_selection_loops_each_clip_at_its_own_period_and_grows_each_by_the_same() {
    let (mut project, a, lane) = fixture(); // one bar, not looping
    let b = another(&mut project, lane, PPQN * 8, Some(PPQN * 4)); // two bars of a one-bar loop
    let c = another(&mut project, lane, PPQN * 6, None); // a bar and a half, not looping

    let mut drag = ResizeClips::new(vec![a, b, c], PPQN * 8).with_loops(vec![
        Some(PPQN * 4),
        None,
        Some(PPQN * 6),
    ]);
    drag.apply(&mut project).unwrap();

    let got = |id: ClipId| (project.clips[id].length, project.clips[id].loop_length);
    assert_eq!(got(a), (PPQN * 12, Some(PPQN * 4)), "loops at its own end");
    assert_eq!(got(b), (PPQN * 16, Some(PPQN * 4)), "keeps the loop it had");
    assert_eq!(got(c), (PPQN * 14, Some(PPQN * 6)), "loops at its own end");
}

#[test]
fn the_whole_drag_is_one_undo_that_puts_every_clip_back() {
    let (mut project, a, lane) = fixture();
    let b = another(&mut project, lane, PPQN * 8, Some(PPQN * 4));
    let before = (project.clips[a].clone(), project.clips[b].clone());

    let mut history = fontelle_model::History::new();
    history
        .apply(
            Box::new(ResizeClips::new(vec![a, b], PPQN).with_loops(vec![Some(PPQN * 4), None])),
            &mut project,
        )
        .unwrap();
    for _ in 0..5 {
        history
            .apply(Box::new(ResizeClips::new(vec![a, b], PPQN)), &mut project)
            .unwrap();
    }
    history.break_gesture();
    assert_eq!(history.depth(), 1, "one drag, one entry");
    assert_eq!(project.clips[a].length, PPQN * 10);
    assert_eq!(project.clips[b].length, PPQN * 14);

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.clips[a].length, before.0.length);
    assert_eq!(
        project.clips[a].loop_length, None,
        "the loop it was given goes too"
    );
    assert_eq!(project.clips[b].length, before.1.length);
    assert_eq!(project.clips[b].loop_length, Some(PPQN * 4));

    history.redo(&mut project).unwrap().unwrap();
    assert_eq!(
        (project.clips[a].length, project.clips[a].loop_length),
        (PPQN * 10, Some(PPQN * 4))
    );
    assert_eq!(project.clips[b].length, PPQN * 14);
}

#[test]
fn a_selection_shrunk_stops_at_the_shortest_length_a_clip_may_have() {
    let (mut project, a, lane) = fixture();
    let b = another(&mut project, lane, PPQN * 8, None);
    ResizeClips::new(vec![a, b], -PPQN * 6)
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.clips[a].length, fontelle_model::MIN_CLIP_LENGTH);
    assert_eq!(project.clips[b].length, PPQN * 2);
}

// ------------------------------------------------------- a clip's own name ---
//
// Ty: *"there should be a name on each clip ... and make it so you can click
// that name to open a little menu ... renaming it"*. A clip is captioned
// with what it plays until somebody names it; the name is the clip's, so a
// copy, a cut half or a loop of it carries it.

#[test]
fn a_clip_has_no_name_of_its_own_until_it_is_given_one() {
    let (mut project, clip, _) = fixture();
    assert_eq!(project.clips[clip].name, None);
    let mut rename = RenameClip::new(clip, Some("Verse riff".to_string()));
    rename.apply(&mut project).unwrap();
    assert_eq!(project.clips[clip].name.as_deref(), Some("Verse riff"));
    rename.invert().apply(&mut project).unwrap();
    assert_eq!(
        project.clips[clip].name, None,
        "the undo takes the name back off"
    );
}

#[test]
fn typing_a_name_is_one_undo_and_a_blank_one_goes_back_to_the_caption() {
    let (mut project, clip, _) = fixture();
    let mut history = fontelle_model::History::new();
    for typed in ["V", "Ve", "Verse"] {
        history
            .apply(
                Box::new(RenameClip::new(clip, Some(typed.to_string()))),
                &mut project,
            )
            .unwrap();
    }
    history.break_gesture();
    assert_eq!(history.depth(), 1);
    assert_eq!(project.clips[clip].name.as_deref(), Some("Verse"));
    // Blank is no name: the clip is captioned with what it plays again.
    history
        .apply(
            Box::new(RenameClip::new(clip, Some("  ".to_string()))),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.clips[clip].name, None);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.clips[clip].name.as_deref(), Some("Verse"));
}

#[test]
fn a_name_survives_saving_and_a_clip_without_one_saves_as_it_always_did() {
    let (mut project, clip, _) = fixture();
    let plain = serde_json::to_string(&project.clips[clip]).unwrap();
    assert!(!plain.contains("\"name\""), "{plain}");
    RenameClip::new(clip, Some("Hook".to_string()))
        .apply(&mut project)
        .unwrap();
    let named = serde_json::to_string(&project.clips[clip]).unwrap();
    let back: Clip = serde_json::from_str(&named).unwrap();
    assert_eq!(back.name.as_deref(), Some("Hook"));
    let old: Clip = serde_json::from_str(&plain).unwrap();
    assert_eq!(old.name, None, "a file from before names opens unnamed");
}
