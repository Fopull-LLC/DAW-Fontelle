//! Editing the arrangement's **rows and blocks** — the three things it could
//! not do to itself.
//!
//! Reported from using the studio:
//!
//! - *"theres no way to actually edit arrangement rows right now like i made
//!   one i dont want but i cant right click and delete it."* A lane could be
//!   made (adding a channel makes one) and never removed or renamed, so a
//!   mistake was permanent.
//! - *"stuff like being able to right click and duplicate too, for instruments
//!   in the channel rack for example."*
//! - *"theres no tool for cutting up clips in the arrangement right now (should
//!   be c key) should work like the same tool in fl studio and correctly split
//!   up looped clips and everything taking into account all edge cases
//!   cleanly."*
//!
//! Each of them is a command here, because INVARIANT 9 has no exceptions and
//! because "I did not mean that" is Ctrl+Z or it is nothing.

use fontelle_model::{
    AddClip, AddLane, AddNotes, Arena, Clip, ClipSource, Command, DuplicateChannel, History, Lane,
    MoveLane, Note, NoteData, Project, RemoveLane, RenameLane, SplitClip, TempoMap,
};
use fontelle_types::{ChannelId, ClipId, LaneId, PPQN, Tick};

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
    }
}

fn a_project() -> (Project, ChannelId, LaneId) {
    let mut project = Project::new("arranging");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel = project.channels.insert(fontelle_model::Channel {
        name: "Keys".into(),
        color: [1, 2, 3, 4],
        mixer_track: None,
        patch_data: None,
        pan: 0.25,
        muted: false,
        soloed: false,
        named_keys: true,
        gain_db: -3.0,
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane 1".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    (project, channel, lane)
}

/// A clip on `lane` playing `channel`, holding `notes`.
fn a_clip(
    project: &mut Project,
    lane: LaneId,
    channel: ChannelId,
    start: Tick,
    length: Tick,
    loop_length: Option<Tick>,
    notes: Vec<Note>,
) -> ClipId {
    let mut add = AddClip::new(Clip {
        lane,
        start,
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
    let clip = add.id().unwrap();
    if !notes.is_empty() {
        AddNotes::new(clip, notes).apply(project).unwrap();
    }
    clip
}

/// Every note of a clip as `(start, length, key)`, in time order.
fn notes_of(project: &Project, clip: ClipId) -> Vec<(Tick, Tick, u8)> {
    let ClipSource::Notes(data) = &project.clips.get(clip).expect("the clip is there").source else {
        panic!("not a note clip");
    };
    let mut out: Vec<(Tick, Tick, u8)> = data
        .notes
        .values()
        .map(|n| (n.start, n.length, n.key))
        .collect();
    out.sort();
    out
}

/// The lanes' names **in the order the arrangement stacks them**, which is
/// what `Project::lane_ids` answers and is no longer the arena's own order.
fn lane_names(project: &Project) -> Vec<String> {
    project
        .lane_ids()
        .into_iter()
        .filter_map(|id| project.lanes.get(id).map(|lane| lane.name.clone()))
        .collect()
}

// ------------------------------------------------------------- the rows ---

#[test]
fn a_lane_can_be_added() {
    let (mut project, _channel, _lane) = a_project();
    let mut add = AddLane::new("Drums".to_string());
    add.apply(&mut project).unwrap();
    assert_eq!(lane_names(&project), vec!["Lane 1", "Drums"]);
    assert!(add.id().is_some(), "the new lane's id is on the command");
}

#[test]
fn a_lane_can_be_renamed_and_the_rename_undone() {
    let (mut project, _channel, lane) = a_project();
    let mut history = History::new();
    history
        .apply(
            Box::new(RenameLane::new(lane, "Verse".to_string())),
            &mut project,
        )
        .unwrap();
    assert_eq!(lane_names(&project), vec!["Verse"]);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(lane_names(&project), vec!["Lane 1"]);
}

/// The report. A lane you did not mean to make comes back off — and it takes
/// its clips with it, because a clip on no lane is a clip nothing can draw.
#[test]
fn removing_a_lane_takes_its_clips_and_puts_them_back_on_undo() {
    let (mut project, channel, lane) = a_project();
    let mut add = AddLane::new("Spare".to_string());
    add.apply(&mut project).unwrap();
    let spare = add.id().unwrap();
    let keeper = a_clip(&mut project, lane, channel, 0, PPQN * 4, None, vec![]);
    let doomed = a_clip(
        &mut project,
        spare,
        channel,
        0,
        PPQN * 4,
        None,
        vec![a_note(0, PPQN, 60)],
    );

    let mut history = History::new();
    history
        .apply(Box::new(RemoveLane::new(spare)), &mut project)
        .unwrap();
    assert_eq!(lane_names(&project), vec!["Lane 1"]);
    assert!(project.clips.get(doomed).is_none(), "its clip went with it");
    assert!(project.clips.get(keeper).is_some(), "and no other clip did");

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(lane_names(&project), vec!["Lane 1", "Spare"]);
    assert_eq!(
        notes_of(&project, doomed),
        vec![(0, PPQN, 60)],
        "the clip comes back with its notes, under its own id"
    );
}

/// The last lane stays. An arrangement with no rows has nowhere to draw and no
/// way back — every other "add" in the window puts things *on* a lane.
#[test]
fn the_last_lane_cannot_be_removed() {
    let (mut project, _channel, lane) = a_project();
    assert!(
        RemoveLane::new(lane).apply(&mut project).is_err(),
        "an arrangement has to keep one row"
    );
    assert_eq!(lane_names(&project), vec!["Lane 1"]);
}

// -------------------------------------------------- duplicating a channel ---

#[test]
fn duplicating_a_channel_copies_its_instrument_and_its_clips() {
    let (mut project, channel, lane) = a_project();
    a_clip(
        &mut project,
        lane,
        channel,
        PPQN * 4,
        PPQN * 4,
        None,
        vec![a_note(0, PPQN, 64), a_note(PPQN, PPQN, 67)],
    );

    let mut history = History::new();
    let mut command = DuplicateChannel::new(channel);
    command.apply(&mut project).unwrap();
    let copy = command.channel().expect("a copy was made");

    let original = project.channels.get(channel).unwrap().clone();
    let made = project.channels.get(copy).unwrap();
    assert_eq!(made.pan, original.pan, "the same placement");
    assert_eq!(made.gain_db, original.gain_db, "the same level");
    assert_eq!(made.color, original.color);
    assert_eq!(made.named_keys, original.named_keys);
    assert_ne!(made.name, original.name, "a copy says it is one");

    // Its clips came with it, on a lane of their own — two parts stacked on
    // one row would hide each other.
    let copied: Vec<&Clip> = project
        .clips
        .values()
        .filter(|clip| match &clip.source {
            ClipSource::Notes(data) => data.channel == copy,
            _ => false,
        })
        .collect();
    assert_eq!(copied.len(), 1, "one clip copied");
    assert_eq!(copied[0].start, PPQN * 4, "where the original one was");
    assert_ne!(copied[0].lane, lane, "and on a row of its own");

    // And the whole thing is one history entry.
    let mut project2 = {
        let (p, c, l) = a_project();
        let mut p = p;
        a_clip(&mut p, l, c, 0, PPQN * 4, None, vec![]);
        p
    };
    let channels_before = project2.channels.len();
    let first = project2.channels.keys().next().unwrap();
    history
        .apply(Box::new(DuplicateChannel::new(first)), &mut project2)
        .unwrap();
    assert_eq!(project2.channels.len(), channels_before + 1);
    history.undo(&mut project2).unwrap().unwrap();
    assert_eq!(
        project2.channels.len(),
        channels_before,
        "one Ctrl+Z takes the whole copy back"
    );
}

// ------------------------------------------------------ cutting a clip ---

#[test]
fn cutting_a_clip_in_two_gives_two_clips_that_meet_at_the_cut() {
    let (mut project, channel, lane) = a_project();
    let clip = a_clip(
        &mut project,
        lane,
        channel,
        PPQN * 4,
        PPQN * 8,
        None,
        vec![a_note(0, PPQN, 60), a_note(PPQN * 5, PPQN, 62)],
    );

    let mut command = SplitClip::new(clip, PPQN * 8);
    command.apply(&mut project).unwrap();
    let tail = command.created().expect("a cut makes a second clip");

    let left = project.clips.get(clip).unwrap();
    let right = project.clips.get(tail).unwrap();
    assert_eq!((left.start, left.length), (PPQN * 4, PPQN * 4));
    assert_eq!((right.start, right.length), (PPQN * 8, PPQN * 4));
    assert_eq!(right.lane, left.lane, "the halves stay on the same row");

    // The notes went with the half they fall in, in that half's own ticks.
    assert_eq!(notes_of(&project, clip), vec![(0, PPQN, 60)]);
    assert_eq!(notes_of(&project, tail), vec![(PPQN, PPQN, 62)]);
}

/// A note lying across the cut is cut too, which is what the same tool does in
/// the piano roll and what makes the two halves sound like the one clip did.
#[test]
fn a_note_across_the_cut_is_cut_with_it() {
    let (mut project, channel, lane) = a_project();
    let clip = a_clip(
        &mut project,
        lane,
        channel,
        0,
        PPQN * 8,
        None,
        vec![a_note(0, PPQN * 8, 60)],
    );
    let mut command = SplitClip::new(clip, PPQN * 4);
    command.apply(&mut project).unwrap();
    let tail = command.created().unwrap();
    assert_eq!(notes_of(&project, clip), vec![(0, PPQN * 4, 60)]);
    assert_eq!(notes_of(&project, tail), vec![(0, PPQN * 4, 60)]);
}

/// A cut on either edge is not a cut: it would make a clip of nothing.
#[test]
fn a_cut_at_an_edge_or_outside_does_nothing() {
    let (mut project, channel, lane) = a_project();
    let clip = a_clip(&mut project, lane, channel, PPQN * 4, PPQN * 4, None, vec![]);
    for at in [0, PPQN * 4, PPQN * 8, PPQN * 12] {
        let mut command = SplitClip::new(clip, at);
        assert!(
            command.apply(&mut project).is_ok(),
            "a cut that misses is not an error"
        );
        assert!(command.created().is_none(), "and it makes no clip at {at}");
    }
    assert_eq!(project.clips.len(), 1);
}

/// **The looped case.** Reported from using the window:
///
/// > *"i dont like that right now when i cut something that loops it seems to
/// > change the start and ending of the clip and that is weird. instead we
/// > should do more of what garage band does where you split the cut section
/// > from the rest of the loops so the rest remains a looped clip and the
/// > first part is just a cut clip of what you made."*
///
/// So a cut through a loop is **not** two loops. The right-hand half is the
/// loop, carrying on; the left-hand half is a plain clip holding the notes
/// that were actually sounding over that stretch — the repeats written out.
///
/// The thing this is really protecting is the sentence after it: *"without
/// making any edits the user didnt intend to make themselves"*. A cut must not
/// change a note of what the song plays, and
/// [`cutting_a_loop_changes_nothing_about_what_plays`] is that claim measured.
#[test]
fn cutting_a_loop_leaves_a_plain_clip_and_a_loop_that_carries_on() {
    let (mut project, channel, lane) = a_project();
    // A one-bar pattern, four bars long: four passes.
    let clip = a_clip(
        &mut project,
        lane,
        channel,
        0,
        PPQN * 16,
        Some(PPQN * 4),
        vec![a_note(0, PPQN, 60), a_note(PPQN * 2, PPQN, 64)],
    );
    let mut command = SplitClip::new(clip, PPQN * 8);
    command.apply(&mut project).unwrap();
    let tail = command.created().unwrap();

    let left = project.clips.get(clip).unwrap();
    let right = project.clips.get(tail).unwrap();
    assert_eq!(
        left.loop_length, None,
        "the piece you cut off is a clip of what you made, not a loop"
    );
    assert_eq!(right.loop_length, Some(PPQN * 4), "and the rest is still a loop");
    assert_eq!((left.start, left.length), (0, PPQN * 8));
    assert_eq!((right.start, right.length), (PPQN * 8, PPQN * 8));

    // The left half holds **two passes written out**, because two passes is
    // what sounded in those two bars. Left as a one-pass pattern it would go
    // silent for the second bar, which is an edit nobody asked for.
    assert_eq!(
        notes_of(&project, clip),
        vec![
            (0, PPQN, 60),
            (PPQN * 2, PPQN, 64),
            (PPQN * 4, PPQN, 60),
            (PPQN * 6, PPQN, 64),
        ]
    );
    // The cut fell on a whole number of passes, so the loop that carries on
    // holds the pattern it always had.
    assert_eq!(
        notes_of(&project, tail),
        vec![(0, PPQN, 60), (PPQN * 2, PPQN, 64)]
    );
}

/// A cut through a loop plays back note for note as it did before the cut.
///
/// The sharp version of the rule, and the one worth having: whatever the two
/// halves are made of, the song is the song.
#[test]
fn cutting_a_loop_changes_nothing_about_what_plays() {
    for cut in [PPQN, PPQN * 2, PPQN * 5, PPQN * 8, PPQN * 11] {
        let (mut project, channel, lane) = a_project();
        let clip = a_clip(
            &mut project,
            lane,
            channel,
            PPQN * 4,
            PPQN * 16,
            Some(PPQN * 4),
            vec![a_note(0, PPQN, 60), a_note(PPQN * 2, PPQN, 64)],
        );
        let before = sounding(&project);

        let mut command = SplitClip::new(clip, PPQN * 4 + cut);
        command.apply(&mut project).unwrap();
        assert_eq!(
            sounding(&project),
            before,
            "a cut at {cut} into the clip changed what the song plays"
        );
    }
}

/// Every note the project sounds, in song ticks, repeats written out.
///
/// The one measurement a "the cut changed nothing" claim can be made against —
/// it does not care which clip a note came out of, which is the whole point.
fn sounding(project: &Project) -> Vec<(Tick, Tick, u8)> {
    let mut out = Vec::new();
    for (_, clip) in project.clips.iter() {
        let ClipSource::Notes(data) = &clip.source else {
            continue;
        };
        let period = clip.loop_length.filter(|p| *p > 0);
        for repeat in 0..clip.repeats() {
            let offset = clip.repeat_start(repeat);
            for note in data.notes.values() {
                // The compiler's own rules, and they have to be the same rules
                // or this measures something the song does not do — see
                // `fontelle_sequencer::compile`.
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                let start = note.start + offset;
                if period.is_some() && start >= clip.length {
                    continue;
                }
                let on = clip.start + start;
                let mut off = on + note.length;
                if period.is_some() {
                    off = off.min(clip.start + clip.length);
                }
                if off > on {
                    out.push((on, off - on, note.key));
                }
            }
        }
    }
    out.sort();
    out
}

/// And when it lands **mid-pattern**, the right-hand half's content is rotated
/// to the phase the loop was at — otherwise the second half would restart the
/// pattern and the cut would be audible.
#[test]
fn cutting_a_loop_mid_pattern_rotates_the_second_half() {
    let (mut project, channel, lane) = a_project();
    // Beat 1 and beat 3 of a one-bar pattern.
    let clip = a_clip(
        &mut project,
        lane,
        channel,
        0,
        PPQN * 8,
        Some(PPQN * 4),
        vec![a_note(0, PPQN, 60), a_note(PPQN * 2, PPQN, 64)],
    );
    // Two beats in: the second half starts half way through the pattern, so
    // what it plays first is the note that was on beat 3.
    let mut command = SplitClip::new(clip, PPQN * 2);
    command.apply(&mut project).unwrap();
    let tail = command.created().unwrap();
    assert_eq!(
        notes_of(&project, tail),
        vec![(0, PPQN, 64), (PPQN * 2, PPQN, 60)],
        "the pattern is turned round to the point the cut fell on"
    );
}

/// A cut is one history entry, and undoing it puts the one clip back exactly
/// as it was.
#[test]
fn a_cut_undoes_to_the_clip_it_started_as() {
    let (mut project, channel, lane) = a_project();
    let clip = a_clip(
        &mut project,
        lane,
        channel,
        0,
        PPQN * 8,
        None,
        vec![a_note(0, PPQN * 8, 60), a_note(PPQN * 6, PPQN, 67)],
    );
    let before = notes_of(&project, clip);
    let mut history = History::new();
    history
        .apply(Box::new(SplitClip::new(clip, PPQN * 4)), &mut project)
        .unwrap();
    assert_eq!(project.clips.len(), 2);

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.clips.len(), 1, "the second half is gone");
    let clip_now = project.clips.get(clip).unwrap();
    assert_eq!((clip_now.start, clip_now.length), (0, PPQN * 8));
    assert_eq!(before, notes_of(&project, clip), "and so is the cut note");
}

// ------------------------------------------- cutting a curve rather than notes

fn an_automation_clip(
    project: &mut Project,
    lane: LaneId,
    start: Tick,
    length: Tick,
    points: Vec<(Tick, f64)>,
) -> ClipId {
    let mut arena = Arena::default();
    for (tick, value) in points {
        arena.insert(fontelle_model::AutomationPoint {
            tick,
            value,
            curve: fontelle_model::CurveShape::Linear,
            tension: 0.0,
        });
    }
    let mut add = AddClip::new(Clip {
        lane,
        start,
        length,
        source: ClipSource::Automation(fontelle_model::AutomationData {
            target: fontelle_types::ParamAddress::new("transport/tempo"),
            points: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    add.apply(project).unwrap();
    add.id().unwrap()
}

fn curve_of(project: &Project, clip: ClipId) -> &fontelle_model::AutomationData {
    match &project.clips.get(clip).unwrap().source {
        ClipSource::Automation(data) => data,
        _ => panic!("that clip is not automation"),
    }
}

/// **A ramp cut in the middle still ramps.**
///
/// A note lying across the cut is cut in two — `a_note_across_the_cut_is_cut_
/// with_it` says so — and a *ramp* lying across the cut was not: each point
/// went to the half it fell in and none was put at the seam, so the left half
/// ended at its last point and held, and the right half began at its first
/// point. Between them the value stepped.
///
/// That is audible in exactly the way automation exists to avoid: cutting a
/// filter sweep in half to move one end of it left a click at the join. The
/// two halves have to sound like the one clip did, which is the same rule the
/// note case follows.
#[test]
fn a_ramp_across_the_cut_keeps_its_value_at_the_seam() {
    let (mut project, _channel, lane) = a_project();
    // A straight ramp from 0 to 1 over eight beats, with no point in between.
    let clip = an_automation_clip(
        &mut project,
        lane,
        0,
        PPQN * 8,
        vec![(0, 0.0), (PPQN * 8, 1.0)],
    );

    let mut command = SplitClip::new(clip, PPQN * 2);
    command.apply(&mut project).unwrap();
    let tail = command.created().expect("a cut makes a second clip");

    // A quarter of the way along a straight ramp from 0 to 1.
    let seam = 0.25;
    let head_end = curve_of(&project, clip)
        .value_at(PPQN * 2)
        .expect("the head has a curve");
    let tail_start = curve_of(&project, tail)
        .value_at(0)
        .expect("the tail has a curve");

    assert!(
        (head_end - seam).abs() < 1e-6,
        "the head should end where the ramp was cut, ended at {head_end}"
    );
    assert!(
        (tail_start - seam).abs() < 1e-6,
        "and the tail should start there, started at {tail_start}"
    );
}

/// The whole curve, sampled either side of the cut, is the curve it was.
///
/// The test above checks the seam; this checks that nothing else moved —
/// inserting a point at the cut must not bend the segments around it.
#[test]
fn the_two_halves_read_the_same_as_the_clip_they_came_from() {
    let (mut project, _channel, lane) = a_project();
    let points = vec![(0, 0.2), (PPQN * 4, 0.9), (PPQN * 12, 0.1)];
    let whole = an_automation_clip(&mut project, lane, 0, PPQN * 12, points.clone());
    let before: Vec<f64> = (0..=12)
        .map(|beat| curve_of(&project, whole).value_at(PPQN * beat).unwrap())
        .collect();

    // Cut somewhere that is not on a point, and inside the steepest segment.
    let cut = PPQN * 7;
    let mut command = SplitClip::new(whole, cut);
    command.apply(&mut project).unwrap();
    let tail = command.created().unwrap();

    for beat in 0..=12 {
        let tick = PPQN * beat;
        let after = if tick < cut {
            curve_of(&project, whole).value_at(tick).unwrap()
        } else {
            curve_of(&project, tail).value_at(tick - cut).unwrap()
        };
        assert!(
            (after - before[beat as usize]).abs() < 1e-6,
            "beat {beat} read {} before the cut and {after} after it",
            before[beat as usize]
        );
    }
}

/// A cut landing exactly on a point does not make a second one there.
#[test]
fn cutting_on_a_point_does_not_double_it() {
    let (mut project, _channel, lane) = a_project();
    let clip = an_automation_clip(
        &mut project,
        lane,
        0,
        PPQN * 8,
        vec![(0, 0.0), (PPQN * 4, 0.5), (PPQN * 8, 1.0)],
    );
    let mut command = SplitClip::new(clip, PPQN * 4);
    command.apply(&mut project).unwrap();
    let tail = command.created().unwrap();

    let at_zero = curve_of(&project, tail)
        .points
        .iter()
        .filter(|(_, p)| p.tick == 0)
        .count();
    assert_eq!(at_zero, 1, "the tail starts on one point, not two");
    // And the head still ends where the cut was, which needs a point there:
    // every point strictly before the cut went to the head, and the one *on*
    // it went to the tail.
    assert!(
        (curve_of(&project, clip).value_at(PPQN * 4).unwrap() - 0.5).abs() < 1e-6,
        "the head's last value is the one at the cut"
    );
}

/// Cutting a clip with no points at all is not a crash and not a point.
#[test]
fn cutting_an_empty_curve_leaves_two_empty_curves() {
    let (mut project, _channel, lane) = a_project();
    let clip = an_automation_clip(&mut project, lane, 0, PPQN * 8, vec![]);
    let mut command = SplitClip::new(clip, PPQN * 4);
    command.apply(&mut project).unwrap();
    let tail = command.created().unwrap();
    assert_eq!(curve_of(&project, clip).points.len(), 0);
    assert_eq!(curve_of(&project, tail).points.len(), 0);
}

/// And undo puts the one clip back, seam point and all.
#[test]
fn undoing_a_curve_cut_restores_the_curve_that_was_there() {
    let (mut project, _channel, lane) = a_project();
    let clip = an_automation_clip(
        &mut project,
        lane,
        0,
        PPQN * 8,
        vec![(0, 0.0), (PPQN * 8, 1.0)],
    );
    let before = curve_of(&project, clip).points.len();

    let mut history = History::new();
    history
        .apply(Box::new(SplitClip::new(clip, PPQN * 3)), &mut project)
        .unwrap();
    assert_eq!(project.clips.len(), 2);

    let _ = history.undo(&mut project).unwrap();
    assert_eq!(project.clips.len(), 1, "the second half went away");
    assert_eq!(
        curve_of(&project, clip).points.len(),
        before,
        "and the seam point the cut added went with it"
    );
}

// ------------------------------------------------------- putting rows in order

fn three_lanes(project: &mut Project) -> Vec<LaneId> {
    // The project already has one from `a_project`; name them all so the order
    // is readable in a failure message.
    let existing: Vec<LaneId> = project.lane_ids();
    for (n, id) in existing.iter().enumerate() {
        project.lanes.get_mut(*id).unwrap().name = format!("L{n}");
    }
    let mut ids = existing;
    for n in ids.len()..3 {
        let mut add = AddLane::new(format!("L{n}"));
        add.apply(project).unwrap();
        ids.push(add.id().unwrap());
    }
    ids
}

/// **A row can be moved above another.**
///
/// Rows could be added, renamed, muted and deleted, and the order they stacked
/// in was the order the arena happened to hold them — which is insertion
/// order, and unchangeable. An arrangement whose rows cannot be grouped is one
/// you have to build in the right order the first time.
#[test]
fn a_lane_can_be_moved_up_past_the_one_above_it() {
    let (mut project, _channel, _lane) = a_project();
    three_lanes(&mut project);
    assert_eq!(lane_names(&project), ["L0", "L1", "L2"]);

    let mut command = MoveLane::up(1);
    command.apply(&mut project).unwrap();
    assert_eq!(lane_names(&project), ["L1", "L0", "L2"]);
}

#[test]
fn a_lane_can_be_moved_down_too() {
    let (mut project, _channel, _lane) = a_project();
    three_lanes(&mut project);
    let mut command = MoveLane::down(0);
    command.apply(&mut project).unwrap();
    assert_eq!(lane_names(&project), ["L1", "L0", "L2"]);
}

#[test]
fn moving_the_row_at_either_end_off_the_end_does_nothing() {
    // Not an error — the menu greys these, and a command that failed would
    // make a keyboard shortcut for it an error somebody has to handle.
    let (mut project, _channel, _lane) = a_project();
    three_lanes(&mut project);
    for mut command in [MoveLane::up(0), MoveLane::down(2)] {
        assert!(command.apply(&mut project).is_ok());
    }
    assert_eq!(lane_names(&project), ["L0", "L1", "L2"]);
}

#[test]
fn moving_a_row_takes_its_clips_with_it() {
    // The clips name a `LaneId`, not a position, so this is really a check
    // that the reorder moves the *order* and not the lanes' contents — a swap
    // of the two lanes' fields would leave every clip pointing at the wrong
    // row.
    let (mut project, channel, _lane) = a_project();
    let ids = three_lanes(&mut project);
    let clip = a_clip(
        &mut project,
        ids[2],
        channel,
        0,
        PPQN * 4,
        None,
        vec![a_note(0, PPQN, 60)],
    );

    let mut command = MoveLane::up(2);
    command.apply(&mut project).unwrap();
    assert_eq!(lane_names(&project), ["L0", "L2", "L1"]);
    assert_eq!(
        project.clips.get(clip).unwrap().lane,
        ids[2],
        "the clip is still on the row it was on"
    );
    assert_eq!(project.lanes.get(ids[2]).unwrap().name, "L2");
}

#[test]
fn undoing_a_move_puts_the_row_back() {
    let (mut project, _channel, _lane) = a_project();
    three_lanes(&mut project);
    let mut history = History::new();
    history
        .apply(Box::new(MoveLane::up(2)), &mut project)
        .unwrap();
    assert_eq!(lane_names(&project), ["L0", "L2", "L1"]);
    let _ = history.undo(&mut project).unwrap();
    assert_eq!(lane_names(&project), ["L0", "L1", "L2"]);
}

/// A new row goes to the **bottom**, which is where somebody adding one is
/// looking for it — and not to wherever its order number happens to sort.
#[test]
fn a_row_added_after_a_reorder_goes_to_the_bottom() {
    let (mut project, _channel, _lane) = a_project();
    three_lanes(&mut project);
    MoveLane::up(2).apply(&mut project).unwrap();
    let mut add = AddLane::new("new");
    add.apply(&mut project).unwrap();
    assert_eq!(lane_names(&project), ["L0", "L2", "L1", "new"]);
}

/// **A project written before rows could be ordered keeps the order it had.**
///
/// Every lane in such a file has the field's default, so the sort has to be
/// *stable* and fall back to the arena's own order — the order those rows have
/// always stacked in. A sort that broke ties any other way would silently
/// rearrange every saved arrangement.
#[test]
fn an_old_project_keeps_the_order_its_rows_already_had() {
    let mut project = Project::new("old");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    for n in 0..4 {
        project.lanes.insert(Lane {
            name: format!("L{n}"),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
            // What `#[serde(default)]` gives a file that never had the field.
            order: 0,
        });
    }
    assert_eq!(lane_names(&project), ["L0", "L1", "L2", "L3"]);
}
