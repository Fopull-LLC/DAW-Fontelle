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
    AddClip, Arena, Clip, ClipSource, Command, Note, NoteData, Project, ResizeClip, SetClipLoop,
    TempoMap,
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
        order: 0,
    });
    let mut notes = Arena::default();
    notes.insert(a_note(0, 60));

    let mut add = AddClip::new(Clip {
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
