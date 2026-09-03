//! Bringing a sound into a document (TDD §15, §10.6).
//!
//! *"i want to also be able to record my voice into the daw or import different
//! sounds and loops and whatnot to make songs with."*
//!
//! An import is **one command**, for the reason `ImportParts` is: it makes a
//! row *and* a clip, and the clip has to name the row the same command is about
//! to mint. A `Compound` cannot do that — it holds commands built before any of
//! them ran — and two entries in the history would mean an undo that leaves an
//! empty row behind, which is worse than either half.

use fontelle_model::{AddAudioClip, ClipSource, Command, Project};
use fontelle_types::{AssetKind, AssetRef, AudioClipData, PPQN};

fn an_asset(name: &str) -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: name.into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

fn a_clip(name: &str) -> AudioClipData {
    AudioClipData::whole(an_asset(name), 48_000, 48_000)
}

fn import(name: &str, start: i64, length: i64) -> AddAudioClip {
    AddAudioClip::new(name, a_clip(name), start, length)
}

#[test]
fn importing_a_sound_makes_a_row_and_puts_the_clip_on_it() {
    let mut project = Project::new("audio");
    let lanes = project.lanes.len();
    let mut command = import("Vocal.wav", PPQN * 4, PPQN * 8);
    command.apply(&mut project).expect("an import applies");

    assert_eq!(project.lanes.len(), lanes + 1);
    assert_eq!(project.clips.len(), 1);
    let (id, clip) = project.clips.iter().next().expect("a clip");
    assert_eq!(clip.start, PPQN * 4);
    assert_eq!(clip.length, PPQN * 8);
    assert!(matches!(clip.source, ClipSource::Audio(_)));
    assert_eq!(command.clip(), Some(id));
    // And the row is named after the file, which is what somebody scanning an
    // arrangement is looking for.
    let lane = project.lanes.get(clip.lane).expect("the row it made");
    assert_eq!(lane.name, "Vocal.wav");
}

#[test]
fn a_new_row_goes_under_what_is_already_there() {
    // A file dropped onto a song must not push the song down the arrangement.
    let mut project = Project::new("audio");
    let existing = project.lanes.insert(fontelle_model::Lane {
        name: "Keys".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 7,
    });
    let mut command = import("Take.wav", 0, PPQN);
    command.apply(&mut project).expect("applies");
    let made = command.lane().expect("a row");
    assert!(project.lanes[made].order > project.lanes[existing].order);
}

#[test]
fn undoing_an_import_leaves_no_row_and_no_clip_behind() {
    // An undo that leaves an empty row is the reason this is one command.
    let mut project = Project::new("audio");
    let lanes = project.lanes.len();
    let mut command = import("Oops.wav", 0, PPQN);
    command.apply(&mut project).expect("applies");
    command.invert().apply(&mut project).expect("inverts");

    assert_eq!(project.lanes.len(), lanes);
    assert!(project.clips.is_empty());
}

#[test]
fn redoing_an_import_puts_everything_back_under_the_ids_it_first_minted() {
    // Anything stacked above this entry names those ids. A redo that minted
    // fresh ones would leave the command above it pointing at nothing.
    let mut project = Project::new("audio");
    let mut command = import("Again.wav", 0, PPQN);
    command.apply(&mut project).expect("applies");
    let (clip, lane) = (command.clip().unwrap(), command.lane().unwrap());

    command.invert().apply(&mut project).expect("inverts");
    command.apply(&mut project).expect("redoes");

    assert_eq!(command.clip(), Some(clip));
    assert_eq!(command.lane(), Some(lane));
    assert!(project.clips.contains_key(clip));
    assert!(project.lanes.contains_key(lane));
}

#[test]
fn an_import_says_what_it_is_in_the_history() {
    let command = import("Vocal.wav", 0, PPQN);
    assert!(
        command.label().contains("Vocal.wav"),
        "the history says {:?}",
        command.label()
    );
}

// ------------------------------------------------------- editing one ---

use fontelle_model::SetAudioClip;
use fontelle_types::{FadeCurve, Fade};

fn a_project_with_a_clip() -> (Project, fontelle_types::ClipId) {
    let mut project = Project::new("audio");
    let mut command = import("Take.wav", 0, PPQN * 4);
    command.apply(&mut project).expect("applies");
    let id = command.clip().expect("a clip");
    (project, id)
}

fn data_of(project: &Project, id: fontelle_types::ClipId) -> AudioClipData {
    match &project.clips[id].source {
        ClipSource::Audio(data) => data.clone(),
        _ => panic!("not an audio clip"),
    }
}

#[test]
fn setting_a_clips_properties_changes_only_that_clip() {
    // *"double clicking on an audio clip should open a menu that lets me make
    // changes to that audio."* Non-destructive, per §15.1: the file is
    // untouched and the numbers live on the clip.
    let (mut project, id) = a_project_with_a_clip();
    let mut wanted = data_of(&project, id);
    wanted.gain_db = -6.0;
    wanted.filter.cutoff_hz = 900.0;
    wanted.fade_in = Fade { frames: 4096, curve: FadeCurve::SCurve };

    let mut command = SetAudioClip::new(id, wanted.clone());
    command.apply(&mut project).expect("applies");
    assert_eq!(data_of(&project, id), wanted);
    // And the asset it points at is the one it always pointed at: an editor
    // that could repoint a clip at another file by accident would be a very
    // confusing undo.
    assert_eq!(data_of(&project, id).asset, wanted.asset);
}

#[test]
fn undoing_an_edit_puts_every_property_back() {
    let (mut project, id) = a_project_with_a_clip();
    let before = data_of(&project, id);
    let mut wanted = before.clone();
    wanted.reverse = true;
    wanted.speed = 0.5;

    let mut command = SetAudioClip::new(id, wanted);
    command.apply(&mut project).expect("applies");
    command.invert().apply(&mut project).expect("inverts");
    assert_eq!(data_of(&project, id), before);
}

#[test]
fn editing_a_clip_that_is_not_audio_is_refused_rather_than_replacing_it() {
    // A stale id naming a note clip: turning somebody's part into a take
    // silently is the worst possible outcome.
    let mut project = Project::new("audio");
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "Keys".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let notes = project.clips.insert(fontelle_model::Clip {
        lane,
        start: 0,
        length: PPQN,
        source: ClipSource::Notes(fontelle_model::NoteData {
            channel: project.channels.insert(fontelle_model::Channel {
                name: "ch".into(),
                color: [0; 4],
                mixer_track: None,
                patch_data: None,
                pan: 0.0,
                muted: false,
                soloed: false,
                named_keys: false,
                gain_db: 0.0,
            }),
            notes: Default::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    let mut command = SetAudioClip::new(notes, a_clip("Elsewhere.wav"));
    assert!(command.apply(&mut project).is_err());
    assert!(matches!(project.clips[notes].source, ClipSource::Notes(_)));
}

#[test]
fn a_run_of_edits_to_one_clip_is_one_history_entry() {
    // Stepping a cutoff ten times is one thing you did, and ten undos to get
    // back is not an undo anybody wants. The gesture is broken on mouse-up,
    // which is what stops the *next* thing merging into it.
    let (mut project, id) = a_project_with_a_clip();
    let before = data_of(&project, id);
    let mut first = SetAudioClip::new(id, {
        let mut d = before.clone();
        d.gain_db = -1.0;
        d
    });
    first.apply(&mut project).expect("applies");
    let mut second = SetAudioClip::new(id, {
        let mut d = before.clone();
        d.gain_db = -2.0;
        d
    });
    assert!(first.merge_with(&second), "two steps did not coalesce");
    second.apply(&mut project).expect("applies");

    first.invert().apply(&mut project).expect("inverts");
    assert_eq!(
        data_of(&project, id),
        before,
        "one undo did not reach the start of the run"
    );
}

#[test]
fn edits_to_two_different_clips_are_two_history_entries() {
    let (mut project, first_id) = a_project_with_a_clip();
    let mut second = import("Other.wav", PPQN * 8, PPQN * 4);
    second.apply(&mut project).expect("applies");
    let second_id = second.clip().expect("a clip");

    let mut a = SetAudioClip::new(first_id, data_of(&project, first_id));
    let b = SetAudioClip::new(second_id, data_of(&project, second_id));
    assert!(!a.merge_with(&b), "two clips coalesced into one entry");
}

// ------------------------------------------------------------ cutting one ---
//
// *"should work cleanly with all the tools like cutting and whatnot."*
//
// A note clip is cut by dealing its notes out either side of the seam; an
// automation clip by putting a point on the seam so neither half steps. An
// audio clip is neither: what has to move is **where in the file each half
// starts**, and a split that left both halves pointing at the front of the file
// would give you the same audio twice, quietly, with the picture agreeing with
// it.

use fontelle_model::SplitClip;

/// A take on the arrangement whose **block is exactly as long as its audio**:
/// 48 000 frames at 48 kHz is one second, and one second at 120 bpm is two
/// beats. That matters here and nowhere else — the seam is found the way the
/// player finds it, so a block longer than its audio has cut points that fall
/// past the end of the take, which is correct and makes for a confusing test.
const TAKE_LENGTH: i64 = PPQN * 2;

#[test]
fn cutting_a_take_in_half_gives_two_halves_of_the_take() {
    let mut project = Project::new("audio");
    let mut import = AddAudioClip::new("Take.wav", a_clip("Take.wav"), 0, TAKE_LENGTH);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    let whole = data_of(&project, id);

    let mut cut = SplitClip::new(id, TAKE_LENGTH / 2);
    cut.apply(&mut project).expect("cuts");

    let mut clips: Vec<_> = project.clips.iter().map(|(id, c)| (id, c.clone())).collect();
    clips.sort_by_key(|(_, c)| c.start);
    assert_eq!(clips.len(), 2);

    let (left, right) = (data_of(&project, clips[0].0), data_of(&project, clips[1].0));
    assert_eq!(left.source_start, whole.source_start, "the front moved");
    assert_eq!(
        left.source_end, right.source_start,
        "the two halves do not meet: {} then {}",
        left.source_end, right.source_start
    );
    assert_eq!(right.source_end, whole.source_end, "the back moved");
    // And together they are the take: no frames lost, none played twice.
    assert_eq!(
        left.source_frames() + right.source_frames(),
        whole.source_frames()
    );
    assert!(left.source_frames() > 0 && right.source_frames() > 0, "a half is empty");
}

#[test]
fn a_cut_lands_where_the_blade_did_rather_than_halfway() {
    // A quarter of the way along a four-bar clip is a quarter of the way into
    // the audio, not a half. A split that always divided evenly would be right
    // exactly once.
    let mut project = Project::new("audio");
    let mut import = AddAudioClip::new("Take.wav", a_clip("Take.wav"), 0, TAKE_LENGTH);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    let whole = data_of(&project, id);

    let mut cut = SplitClip::new(id, TAKE_LENGTH / 4);
    cut.apply(&mut project).expect("cuts");
    let mut clips: Vec<_> = project.clips.iter().map(|(id, c)| (id, c.clone())).collect();
    clips.sort_by_key(|(_, c)| c.start);
    let left = data_of(&project, clips[0].0);
    let quarter = whole.source_frames() / 4;
    assert!(
        (left.source_frames() - quarter).abs() <= 2,
        "a quarter of {} frames came out as {}",
        whole.source_frames(),
        left.source_frames()
    );
}

#[test]
fn cutting_a_reversed_take_keeps_both_halves_reversed_and_in_order() {
    // The pieces are still the pieces: what you hear from the left half is the
    // first half of what the whole clip played, backwards audio and all.
    let mut project = Project::new("audio");
    let mut data = a_clip("Take.wav");
    data.reverse = true;
    let mut import = AddAudioClip::new("Take.wav", data, 0, TAKE_LENGTH);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");

    let mut cut = SplitClip::new(id, TAKE_LENGTH / 2);
    cut.apply(&mut project).expect("cuts");
    for (_, clip) in project.clips.iter() {
        let ClipSource::Audio(data) = &clip.source else {
            panic!("not audio")
        };
        assert!(data.reverse, "a half forgot it was reversed");
        assert!(data.source_frames() > 0, "a half has no audio in it");
    }
}

#[test]
fn cutting_a_take_undoes_back_to_one_take() {
    let mut project = Project::new("audio");
    let mut import = AddAudioClip::new("Take.wav", a_clip("Take.wav"), 0, TAKE_LENGTH);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    let whole = data_of(&project, id);

    let mut cut = SplitClip::new(id, TAKE_LENGTH / 2);
    cut.apply(&mut project).expect("cuts");
    cut.invert().apply(&mut project).expect("uncuts");

    assert_eq!(project.clips.len(), 1);
    assert_eq!(data_of(&project, id), whole);
}
