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
use fontelle_types::{Fade, FadeCurve};

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
    wanted.fade_in = Fade {
        frames: 4096,
        curve: FadeCurve::SCurve,
        tension: 0.0,
    };

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

    let mut clips: Vec<_> = project
        .clips
        .iter()
        .map(|(id, c)| (id, c.clone()))
        .collect();
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
    assert!(
        left.source_frames() > 0 && right.source_frames() > 0,
        "a half is empty"
    );
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
    let mut clips: Vec<_> = project
        .clips
        .iter()
        .map(|(id, c)| (id, c.clone()))
        .collect();
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

// ---------------------------- cutting a take that follows something else ---
//
// *"resolve the issue of it trying to stretch while looping and whatnot so it
// all works together cleanly."* The seam is *where the player is* at the tick
// the blade fell, and the player is not always reading the file at the file's
// own rate: a clip in `ClipStretch::Resample` fits the file to its block, and
// a clip the arrangement repeats comes round again at every period. A seam
// found at the file's rate, counted from the block's start, is right only for
// a clip that does neither — and lands past the end of the file for both,
// where it clamps and hands one half the whole take and the other half
// nothing.

#[test]
fn cutting_a_stretched_take_cuts_where_the_player_is_and_not_where_its_own_rate_would_be() {
    // One second of audio spread over two seconds of block. Halfway along the
    // block the player is halfway through the file, so that is the seam —
    // whereas the file's own rate would have run out at the block's middle.
    let mut project = Project::new("audio");
    let mut data = a_clip("Take.wav");
    data.stretch = fontelle_types::ClipStretch::Resample;
    let mut import = AddAudioClip::new("Take.wav", data, 0, TAKE_LENGTH * 2);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    let whole = data_of(&project, id);

    let mut cut = SplitClip::new(id, TAKE_LENGTH);
    cut.apply(&mut project).expect("cuts");
    let mut clips: Vec<_> = project
        .clips
        .iter()
        .map(|(id, c)| (id, c.clone()))
        .collect();
    clips.sort_by_key(|(_, c)| c.start);
    assert_eq!(clips.len(), 2);
    let (left, right) = (data_of(&project, clips[0].0), data_of(&project, clips[1].0));

    let middle = whole.source_start + whole.source_frames() / 2;
    assert!(
        (left.source_end - middle).abs() <= 2,
        "the seam is the file's middle ({middle}), not {}",
        left.source_end
    );
    assert_eq!(left.source_end, right.source_start, "the halves must meet");
    assert!(
        right.source_frames() > 0,
        "the second half got none of the take"
    );
    // Both halves still follow their blocks, or the cut has changed the sound.
    assert_eq!(left.stretch, fontelle_types::ClipStretch::Resample);
    assert_eq!(right.stretch, fontelle_types::ClipStretch::Resample);
}

#[test]
fn cutting_a_looped_take_divides_the_arrangement_and_not_the_file() {
    // A loop is cut in the **arrangement**: both halves keep the whole take
    // and go on repeating it. Trimming them to the blade looks tempting —
    // the cut would be seamless — but a half whose range is trimmed still
    // repeats every period, so every pass after the first would play the
    // shortened range and then sit silent for the rest of the period. That
    // trades one wrong frame at the blade for a hole in every bar.
    //
    // Exact when the cut lands on a seam, which is what the snapped grid
    // gives you. A cut mid-pass restarts the loop at the blade, because a
    // clip stores where in the file it begins and not where in the *pass* —
    // the phase a mid-pass half would need has nowhere to live.
    let mut project = Project::new("audio");
    let mut import = AddAudioClip::new("Take.wav", a_clip("Take.wav"), 0, TAKE_LENGTH * 4);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    project.clips[id].loop_length = Some(TAKE_LENGTH);
    let whole = data_of(&project, id);

    let mut cut = SplitClip::new(id, TAKE_LENGTH * 2 + TAKE_LENGTH / 2);
    cut.apply(&mut project).expect("cuts");
    let mut clips: Vec<_> = project
        .clips
        .iter()
        .map(|(id, c)| (id, c.clone()))
        .collect();
    clips.sort_by_key(|(_, c)| c.start);
    assert_eq!(clips.len(), 2);

    for (id, clip) in &clips {
        let data = data_of(&project, *id);
        assert_eq!(
            data.source_frames(),
            whole.source_frames(),
            "a half of a loop lost part of the take, so its passes go quiet"
        );
        assert_eq!(
            clip.loop_length,
            Some(TAKE_LENGTH),
            "a half stopped repeating"
        );
    }
    // The blocks, though, are divided where the blade fell.
    assert_eq!(clips[0].1.length, TAKE_LENGTH * 2 + TAKE_LENGTH / 2);
    assert_eq!(
        clips[1].1.length,
        TAKE_LENGTH * 4 - (TAKE_LENGTH * 2 + TAKE_LENGTH / 2)
    );
}

#[test]
fn the_half_you_cut_off_a_loop_is_still_a_loop_so_it_still_sounds_like_it_did() {
    // A note clip's front half stops looping because `split_notes` writes its
    // repeats out and it plays them anyway. There is nothing to write out for
    // audio: a front half that stopped looping would play the take once and
    // then sit silent for the passes it used to play, which is the cut
    // changing the sound.
    let mut project = Project::new("audio");
    let mut import = AddAudioClip::new("Take.wav", a_clip("Take.wav"), 0, TAKE_LENGTH * 4);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    project.clips[id].loop_length = Some(TAKE_LENGTH);

    let mut cut = SplitClip::new(id, TAKE_LENGTH * 2);
    cut.apply(&mut project).expect("cuts");
    let mut clips: Vec<_> = project
        .clips
        .iter()
        .map(|(id, c)| (id, c.clone()))
        .collect();
    clips.sort_by_key(|(_, c)| c.start);
    assert_eq!(
        clips[0].1.loop_length,
        Some(TAKE_LENGTH),
        "the front half stopped repeating and now plays silence"
    );
    assert_eq!(clips[1].1.loop_length, Some(TAKE_LENGTH));
    // Cut on a seam, so neither half starts mid-pass: both play the take from
    // its front, which is what a loop cut on its own period should give.
    let (left, right) = (data_of(&project, clips[0].0), data_of(&project, clips[1].0));
    assert_eq!(left.source_start, right.source_start);
    assert_eq!(left.source_end, right.source_end);
}

#[test]
fn cutting_a_repitched_take_cuts_at_the_files_own_time() {
    // With stretch off, pitch does not move through the file — it only moves
    // what is heard — so the blade at the block's middle is the file's
    // middle whatever the pitch says. Before the shifter this clip would
    // have been read an octave up, twice as fast, and the seam would have
    // been the whole file.
    let mut project = Project::new("audio");
    let mut data = a_clip("Take.wav");
    data.pitch_semitones = 12.0;
    assert_eq!(data.stretch, fontelle_types::ClipStretch::Off);
    let mut import = AddAudioClip::new("Take.wav", data, 0, TAKE_LENGTH);
    import.apply(&mut project).expect("applies");
    let id = import.clip().expect("a clip");
    let whole = data_of(&project, id);

    let mut cut = SplitClip::new(id, TAKE_LENGTH / 2);
    cut.apply(&mut project).expect("cuts");
    let mut clips: Vec<_> = project
        .clips
        .iter()
        .map(|(id, c)| (id, c.clone()))
        .collect();
    clips.sort_by_key(|(_, c)| c.start);
    let left = data_of(&project, clips[0].0);
    let middle = whole.source_start + whole.source_frames() / 2;
    assert!(
        (left.source_end - middle).abs() <= 2,
        "the seam is the file's middle ({middle}), not {}",
        left.source_end
    );
}

// --- Dropping onto an existing row, not always a new one -------------------
//
// > *"instead of just putting it where im actually dragging it to snapping to
// > the lane nearest to my mouse ... it automatically places it on a new lane
// > in the arrangement at the bottom."*
//
// A drop over a row that is already there puts the clip on that row; the
// new-row behaviour above is what a drop into the empty space past the last
// row still does.

#[test]
fn a_drop_onto_an_existing_row_puts_the_clip_on_it_and_makes_no_new_row() {
    let mut project = Project::new("audio");
    let target = project.lanes.insert(fontelle_model::Lane {
        name: "Drums".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let lanes = project.lanes.len();

    let mut command = import("Loop.wav", PPQN * 2, PPQN * 4).on_lane(target);
    command.apply(&mut project).expect("an import applies");

    // No new row — the clip landed on the one that was there.
    assert_eq!(
        project.lanes.len(),
        lanes,
        "no new row for an onto-lane drop"
    );
    let (_, clip) = project.clips.iter().next().expect("a clip");
    assert_eq!(
        clip.lane, target,
        "the clip is on the row it was dropped on"
    );
    assert_eq!(clip.start, PPQN * 2);
}

#[test]
fn undoing_a_drop_onto_an_existing_row_leaves_that_row_alone() {
    let mut project = Project::new("audio");
    let target = project.lanes.insert(fontelle_model::Lane {
        name: "Drums".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });

    let mut command = import("Loop.wav", 0, PPQN * 4).on_lane(target);
    command.apply(&mut project).expect("applies");
    let inverse = command.invert();
    let mut inverse = inverse;
    inverse.apply(&mut project).expect("undo applies");

    // The clip is gone; the row it was dropped on is not — undoing a drop must
    // not delete a row that was there before the drop.
    assert_eq!(project.clips.len(), 0, "the clip is undone");
    assert!(
        project.lanes.get(target).is_some(),
        "the existing row survives the undo"
    );
}

// --- A row where you are looking, not always at the foot ------------------
//
// > *"i dont like how when recording something, importing something,
// > dragging an audio file in, etc anything it always goes on a new lane at
// > the very bottom its very annoying ... if i wasnt dragging however and
// > imported some other way it should go on a new lane added in between the
// > lane in the middlemost of your arrangement screen that way its cleanly
// > visible for you."*
//
// `at_row` is the command's half of that: a new row put *at* an index in the
// stack, pushing what was there down, the way `AddLane::at` already does for
// the right-click menu. Which index is the middle of the screen is the
// window's business (`fontelle_ui::canvas::arrival_row`).

fn a_row(name: &str, order: u32) -> fontelle_model::Lane {
    fontelle_model::Lane {
        name: name.into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order,
    }
}

fn stack(project: &Project) -> Vec<String> {
    project
        .lane_ids()
        .into_iter()
        .map(|id| project.lanes[id].name.clone())
        .collect()
}

#[test]
fn an_import_at_a_row_index_goes_there_and_pushes_the_rest_down() {
    let mut project = Project::new("audio");
    for (i, name) in ["Drums", "Bass", "Keys", "Vox"].iter().enumerate() {
        project.lanes.insert(a_row(name, i as u32));
    }
    let mut command = import("Take.wav", 0, PPQN).at_row(2);
    command.apply(&mut project).expect("applies");

    assert_eq!(
        stack(&project),
        ["Drums", "Bass", "Take.wav", "Keys", "Vox"],
        "the new row is at index 2 and the rows under it moved down one"
    );
    let made = command.lane().expect("a row was made");
    let (_, clip) = project.clips.iter().next().expect("a clip");
    assert_eq!(clip.lane, made, "and the clip is on it");
}

#[test]
fn an_import_at_a_row_index_past_the_stack_is_the_foot() {
    let mut project = Project::new("audio");
    project.lanes.insert(a_row("Drums", 0));
    project.lanes.insert(a_row("Bass", 1));
    let mut command = import("Take.wav", 0, PPQN).at_row(99);
    command.apply(&mut project).expect("applies");
    assert_eq!(stack(&project), ["Drums", "Bass", "Take.wav"]);
}

#[test]
fn undoing_an_import_at_a_row_index_closes_the_gap_it_opened() {
    let mut project = Project::new("audio");
    for (i, name) in ["Drums", "Bass", "Keys"].iter().enumerate() {
        project.lanes.insert(a_row(name, i as u32));
    }
    let mut command = import("Take.wav", 0, PPQN).at_row(1);
    command.apply(&mut project).expect("applies");
    let mut inverse = command.invert();
    inverse.apply(&mut project).expect("undo applies");

    assert_eq!(project.clips.len(), 0);
    assert_eq!(
        stack(&project),
        ["Drums", "Bass", "Keys"],
        "the stack reads as it did before the import"
    );
}

#[test]
fn redoing_an_import_at_a_row_index_puts_it_back_at_that_index() {
    let mut project = Project::new("audio");
    for (i, name) in ["Drums", "Bass", "Keys"].iter().enumerate() {
        project.lanes.insert(a_row(name, i as u32));
    }
    let mut command = import("Take.wav", 0, PPQN).at_row(1);
    command.apply(&mut project).expect("applies");
    let made = command.lane().expect("a row");
    let mut inverse = command.invert();
    inverse.apply(&mut project).expect("undo applies");
    command.apply(&mut project).expect("redo applies");

    assert_eq!(stack(&project), ["Drums", "Take.wav", "Bass", "Keys"]);
    assert_eq!(command.lane(), Some(made), "under the id it minted first");
}
