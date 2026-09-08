//! Prefabs: one piece of content, drawn in many places (TDD §10.5).
//!
//! > *"this is a good time to start working on the prefab system for having
//! > clips that you can basically draw into your arrangement that making a
//! > change in that prefab clip affects all the clips in the arrangement that
//! > are referencing that prefab. just makes it easy to do multi edits without
//! > being like fl studio and forcing people into working around a workflow
//! > that forces multi edits."* — Ty
//!
//! # The distinction this rests on
//!
//! Fontelle already had two things that look identical on the arrangement and
//! are nothing alike underneath, and this is the third:
//!
//! - **Copying** makes new clips with their own notes. Editing one leaves the
//!   others alone. That is the point of it.
//! - **Looping** is one clip whose content repeats (`Clip::loop_length`). One
//!   set of notes, played again every period.
//! - **A prefab** is one set of notes with many *places*. The places have
//!   their own row, their own start and their own length; the notes are the
//!   prefab's, and there is exactly one copy of them.
//!
//! Which means a prefab instance's `Clip::source` is not where its notes live.
//! Everything that reads a clip's content has to go through
//! [`Project::clip_source`], and the test at the bottom of this file is what
//! says so.

use fontelle_model::{
    AddChannel, AddLane, AddNotes, AddPrefab, AddPrefabInstance, Arena, Clip, ClipSource, Command,
    DetachPrefab, History, Lane, Note, NoteData, NoteHome, Project, RemoveNotes, RemovePrefab,
    RenamePrefab, TempoMap,
};
use fontelle_types::{ChannelId, LaneId, PPQN, PrefabId, Tick};

fn a_note(start: Tick, length: Tick, key: u8) -> Note {
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
        channel: None,
    }
}

fn a_lane() -> Lane {
    Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0, 0, 0, 255],
        muted: false,
        locked: false,
        order: 0,
    }
}

/// A project with one channel and two rows, and nothing on either.
fn a_project() -> (Project, ChannelId, LaneId, LaneId) {
    let mut project = Project::new("Prefabs");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let mut add = AddChannel::new("Channel 1", None);
    add.apply(&mut project).unwrap();
    let channel = add.channel().unwrap();
    let first = project.lanes.insert(a_lane());
    let mut second = a_lane();
    second.order = 1;
    let second = project.lanes.insert(second);
    (project, channel, first, second)
}

/// An empty prefab called `name`, playing `channel`.
fn a_prefab(project: &mut Project, channel: ChannelId, name: &str) -> PrefabId {
    let mut add = AddPrefab::new(
        name,
        ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
    );
    add.apply(project).unwrap();
    add.prefab().expect("just applied")
}

/// The notes an instance actually plays, by key, in order.
fn keys_of(project: &Project, clip: fontelle_types::ClipId) -> Vec<u8> {
    let source = project
        .clip_source(clip)
        .expect("that clip is in the project");
    match source.as_ref() {
        ClipSource::Notes(data) => {
            let mut notes: Vec<&Note> = data.notes.values().collect();
            notes.sort_by_key(|n| (n.start, n.key));
            notes.iter().map(|n| n.key).collect()
        }
        _ => panic!("that clip does not hold notes"),
    }
}

// ------------------------------------------------------- making one ---

#[test]
fn a_prefab_is_named_content_that_is_not_on_the_arrangement() {
    let (mut project, channel, _, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Chorus riff");

    assert_eq!(project.prefabs.len(), 1);
    assert_eq!(project.prefabs.get(prefab).unwrap().name, "Chorus riff");
    assert_eq!(
        project.clips.len(),
        0,
        "making a prefab puts nothing on the arrangement"
    );
}

#[test]
fn making_one_is_one_undo() {
    let (mut project, channel, _, _) = a_project();
    let mut history = History::new();
    history
        .apply(
            Box::new(AddPrefab::new(
                "Riff",
                ClipSource::Notes(NoteData {
                    channel,
                    notes: Arena::default(),
                }),
            )),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.prefabs.len(), 1);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.prefabs.len(), 0);
    history.redo(&mut project).unwrap().unwrap();
    assert_eq!(project.prefabs.len(), 1, "and one press puts it back");
}

#[test]
fn a_prefab_can_be_renamed() {
    let (mut project, channel, _, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    let mut history = History::new();
    history
        .apply(
            Box::new(RenamePrefab::new(prefab, "Verse riff")),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.prefabs.get(prefab).unwrap().name, "Verse riff");
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.prefabs.get(prefab).unwrap().name, "Riff");
}

// ------------------------------------------------ drawing instances ---

#[test]
fn an_instance_is_a_clip_that_names_a_prefab_and_holds_no_notes_of_its_own() {
    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(0, PPQN, 60)])
        .apply(&mut project)
        .unwrap();

    let mut place = AddPrefabInstance::new(prefab, lane, PPQN * 4, PPQN * 4);
    place.apply(&mut project).unwrap();
    let clip = place.clip().expect("just applied");

    let placed = project.clips.get(clip).expect("the instance is a clip");
    assert_eq!(placed.lane, lane, "on the row it was drawn on");
    assert_eq!(placed.start, PPQN * 4);
    assert_eq!(placed.length, PPQN * 4);
    assert_eq!(
        placed.prefab_link.as_ref().map(|link| link.prefab),
        Some(prefab),
        "and it says which prefab it is"
    );
    // Its *own* source is empty: an instance is a place, not a copy.
    match &placed.source {
        ClipSource::Notes(data) => assert_eq!(
            data.notes.len(),
            0,
            "an instance carries no notes of its own — see `Project::clip_source`"
        ),
        _ => panic!("a note prefab makes a note clip"),
    }
    assert_eq!(
        keys_of(&project, clip),
        vec![60],
        "but it plays the prefab's"
    );
}

// --------------------------------------- the whole point of the thing ---

/// **One edit, every instance.**
///
/// The claim the feature exists for: *"making a change in that prefab clip
/// affects all the clips in the arrangement that are referencing that
/// prefab."*
#[test]
fn editing_a_prefab_changes_every_clip_that_references_it() {
    let (mut project, channel, first, second) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");

    let mut here = AddPrefabInstance::new(prefab, first, 0, PPQN * 4);
    here.apply(&mut project).unwrap();
    let here = here.clip().unwrap();
    let mut there = AddPrefabInstance::new(prefab, second, PPQN * 8, PPQN * 4);
    there.apply(&mut project).unwrap();
    let there = there.clip().unwrap();

    assert_eq!(keys_of(&project, here), Vec::<u8>::new());
    assert_eq!(keys_of(&project, there), Vec::<u8>::new());

    let mut add = AddNotes::new(
        NoteHome::Prefab(prefab),
        vec![a_note(0, PPQN, 60), a_note(PPQN, PPQN, 64)],
    );
    add.apply(&mut project).unwrap();
    let written = add.ids().to_vec();

    assert_eq!(keys_of(&project, here), vec![60, 64]);
    assert_eq!(
        keys_of(&project, there),
        vec![60, 64],
        "both places show the one edit"
    );

    // And taking a note out takes it out of both.
    RemoveNotes::new(NoteHome::Prefab(prefab), vec![written[0]])
        .apply(&mut project)
        .unwrap();
    assert_eq!(keys_of(&project, here), vec![64]);
    assert_eq!(keys_of(&project, there), vec![64]);
}

/// And an edit made *through* an instance is the same edit.
///
/// > *"editing a prefab clip basically works like just editing a normal clip
/// > except you dont have to only be selecting it in the arrangement."*
///
/// The model's half of that is this: a note command addressed at a prefab and
/// one addressed at any of its instances reach the same notes. Which of the
/// two the piano roll sends is the session's business (`fontelle-app`).
#[test]
fn a_clip_that_references_a_prefab_says_where_its_edits_belong() {
    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    let mut place = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    place.apply(&mut project).unwrap();
    let clip = place.clip().unwrap();

    assert_eq!(
        project.note_home(clip),
        Some(NoteHome::Prefab(prefab)),
        "an edit to an instance is an edit to its prefab"
    );

    // An ordinary clip answers itself.
    let plain = project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    assert_eq!(project.note_home(plain), Some(NoteHome::Clip(plain)));
}

// ------------------------------------------------------ letting go ---

/// Detaching bakes the content in, so the clip goes on sounding the same.
///
/// The alternative — an instance that empties when it stops being one — would
/// make "I want to change just this one" a destructive operation, which is the
/// opposite of what a prefab is for.
#[test]
fn detaching_an_instance_keeps_what_it_was_playing_and_stops_following() {
    let (mut project, channel, lane, other) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(0, PPQN, 60)])
        .apply(&mut project)
        .unwrap();

    let mut kept = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    kept.apply(&mut project).unwrap();
    let kept = kept.clip().unwrap();
    let mut freed = AddPrefabInstance::new(prefab, other, 0, PPQN * 4);
    freed.apply(&mut project).unwrap();
    let freed = freed.clip().unwrap();

    let mut history = History::new();
    history
        .apply(Box::new(DetachPrefab::new(freed)), &mut project)
        .unwrap();

    assert!(
        project.clips.get(freed).unwrap().prefab_link.is_none(),
        "it is its own clip now"
    );
    assert_eq!(
        keys_of(&project, freed),
        vec![60],
        "still playing what it was"
    );

    // And it no longer follows the prefab.
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(PPQN, PPQN, 67)])
        .apply(&mut project)
        .unwrap();
    assert_eq!(
        keys_of(&project, freed),
        vec![60],
        "the detached one is left"
    );
    assert_eq!(
        keys_of(&project, kept),
        vec![60, 67],
        "the other still follows"
    );

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(
        project
            .clips
            .get(freed)
            .unwrap()
            .prefab_link
            .as_ref()
            .map(|l| l.prefab),
        Some(prefab),
        "one press puts it back on the prefab"
    );
}

/// Deleting a prefab **bakes** it into every instance rather than emptying
/// them.
///
/// A delete that silently blanked eight bars of somebody's arrangement is the
/// single worst thing this feature could do, and it is one press away from
/// being the default.
#[test]
fn removing_a_prefab_leaves_the_arrangement_sounding_the_same() {
    let (mut project, channel, lane, other) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(0, PPQN, 60)])
        .apply(&mut project)
        .unwrap();

    let mut here = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    here.apply(&mut project).unwrap();
    let here = here.clip().unwrap();
    let mut there = AddPrefabInstance::new(prefab, other, PPQN * 8, PPQN * 4);
    there.apply(&mut project).unwrap();
    let there = there.clip().unwrap();

    let mut history = History::new();
    history
        .apply(Box::new(RemovePrefab::new(prefab)), &mut project)
        .unwrap();

    assert_eq!(project.prefabs.len(), 0, "the prefab is gone");
    assert_eq!(project.clips.len(), 2, "and both places are still there");
    assert_eq!(keys_of(&project, here), vec![60], "still playing it");
    assert_eq!(keys_of(&project, there), vec![60]);
    assert!(project.clips.get(here).unwrap().prefab_link.is_none());

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.prefabs.len(), 1, "and one press brings it back");
    assert_eq!(
        project
            .clips
            .get(here)
            .unwrap()
            .prefab_link
            .as_ref()
            .map(|l| l.prefab),
        Some(prefab),
        "with its instances following it again"
    );
}

// ----------------------------------------------------- reading them ---

/// An instance's own `source` is not what it plays, and anything that reads it
/// directly is wrong.
///
/// This is the trap the whole design has: `Clip::source` is still there, still
/// holds notes for every ordinary clip, and reads correctly for all of them.
/// It is only wrong for the clips this feature makes — which is exactly the
/// shape of bug that ships.
#[test]
fn a_prefabs_notes_are_not_in_the_instances_own_source() {
    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(0, PPQN, 60)])
        .apply(&mut project)
        .unwrap();
    let mut place = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    place.apply(&mut project).unwrap();
    let clip = place.clip().unwrap();

    match &project.clips.get(clip).unwrap().source {
        ClipSource::Notes(data) => assert_eq!(data.notes.len(), 0),
        _ => panic!("notes"),
    }
    assert_eq!(keys_of(&project, clip), vec![60]);
}

/// A link naming a prefab that is not there reads as **empty, not missing**.
///
/// A malformed or half-migrated file is a project that still opens.
#[test]
fn an_instance_whose_prefab_has_gone_reads_as_its_own_empty_source() {
    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    let mut place = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    place.apply(&mut project).unwrap();
    let clip = place.clip().unwrap();
    project.prefabs.remove(prefab);

    assert!(
        project.clip_source(clip).is_some(),
        "a clip pointing at nothing is still a clip"
    );
    assert_eq!(keys_of(&project, clip), Vec::<u8>::new());
}

#[test]
fn a_prefab_survives_a_save_and_reopen_with_its_instances_still_pointing_at_it() {
    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Chorus riff");
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(0, PPQN, 60)])
        .apply(&mut project)
        .unwrap();
    let mut place = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    place.apply(&mut project).unwrap();
    let clip = place.clip().unwrap();

    let text = serde_json::to_string(&project).expect("a project serialises");
    let back: Project = serde_json::from_str(&text).expect("and reads back");

    assert_eq!(back.prefabs.len(), 1);
    assert_eq!(back.prefabs.get(prefab).unwrap().name, "Chorus riff");
    assert_eq!(keys_of(&back, clip), vec![60], "and the link came with it");
}

/// A lane added by hand is what an instance is drawn on; nothing here makes
/// one. Guarding the same rule the rest of the arrangement follows.
#[test]
fn drawing_an_instance_makes_no_lane_and_no_channel() {
    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    let lanes = project.lanes.len();
    let channels = project.channels.len();
    AddPrefabInstance::new(prefab, lane, 0, PPQN * 4)
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.lanes.len(), lanes);
    assert_eq!(project.channels.len(), channels);

    // And the same for `AddLane`, so this file's fixture is honest about it.
    let mut add = AddLane::new("Another");
    add.apply(&mut project).unwrap();
    assert_eq!(project.lanes.len(), lanes + 1);
}

// -------------------------------------- turning a clip into a prefab ---

/// A clip somebody has written becomes a prefab **in place**.
///
/// The clip keeps its id, its row, its start and its length, and stops holding
/// its own notes — those become the prefab's. Keeping the id is not a detail:
/// it is what makes "turn this into a prefab" a change to the block you are
/// looking at rather than a block that vanishes and a different one appearing
/// where it was.
#[test]
fn a_written_clip_becomes_a_prefab_in_place() {
    use fontelle_model::MakePrefabFromClip;

    let (mut project, channel, lane, other) = a_project();
    let clip = project.clips.insert(Clip {
        lane,
        start: PPQN * 4,
        length: PPQN * 8,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    AddNotes::new(clip, vec![a_note(0, PPQN, 60)])
        .apply(&mut project)
        .unwrap();

    let mut history = History::new();
    history
        .apply(
            Box::new(MakePrefabFromClip::new(clip, "Riff")),
            &mut project,
        )
        .unwrap();
    let prefab = *project.prefab_ids().first().expect("a prefab was made");

    let placed = project.clips.get(clip).expect("the same clip");
    assert_eq!(placed.start, PPQN * 4, "where it was");
    assert_eq!(placed.length, PPQN * 8, "and as long as it was");
    assert_eq!(
        placed.prefab_link.as_ref().map(|l| l.prefab),
        Some(prefab),
        "and it follows the prefab now"
    );
    assert_eq!(keys_of(&project, clip), vec![60], "playing what it played");

    // A second place shows the same content, and an edit reaches both.
    let mut second = AddPrefabInstance::new(prefab, other, 0, PPQN * 8);
    second.apply(&mut project).unwrap();
    let second = second.clip().unwrap();
    assert_eq!(keys_of(&project, second), vec![60]);
    AddNotes::new(NoteHome::Prefab(prefab), vec![a_note(PPQN, PPQN, 64)])
        .apply(&mut project)
        .unwrap();
    assert_eq!(keys_of(&project, clip), vec![60, 64]);
    assert_eq!(keys_of(&project, second), vec![60, 64]);

    // And one press takes the whole thing back: the notes return to the clip
    // and the prefab is gone.
    //
    // **With the note added since**, which is worth being explicit about. Undo
    // takes back *making the prefab*; it does not take back the editing done
    // afterwards, and the content the clip gets handed is the content that
    // exists now. The alternative — restoring the notes as they were when the
    // prefab was made — would silently throw away work that was never undone.
    project.clips.remove(second);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.prefabs.len(), 0, "the prefab is gone");
    assert!(project.clips.get(clip).unwrap().prefab_link.is_none());
    assert_eq!(
        keys_of(&project, clip),
        vec![60, 64],
        "with the notes it holds now baked back into it"
    );
}

/// A clip that already follows one is refused rather than nested.
#[test]
fn a_clip_that_is_already_a_place_cannot_become_a_prefab() {
    use fontelle_model::MakePrefabFromClip;

    let (mut project, channel, lane, _) = a_project();
    let prefab = a_prefab(&mut project, channel, "Riff");
    let mut place = AddPrefabInstance::new(prefab, lane, 0, PPQN * 4);
    place.apply(&mut project).unwrap();
    let clip = place.clip().unwrap();

    assert!(
        MakePrefabFromClip::new(clip, "Again")
            .apply(&mut project)
            .is_err(),
        "a place is not content to make a prefab out of"
    );
    assert_eq!(project.prefabs.len(), 1, "and nothing was made");
}
