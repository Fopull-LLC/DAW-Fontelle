//! One clip, several instruments, and the rack decides which one you edit.
//!
//! Reported from using the window:
//!
//! > *"whenever i make a new clip in a lane, for some reason its guessing
//! > what instrument i want based on the lane which is super weird. it
//! > should instead just be based on whatever instrument you have selected in
//! > the channel rack. as a consequence of how its currently implemented, if
//! > i have a clip that has notes on multiple instruments, theres currently
//! > no way to actually swap between editing the different instruments notes
//! > youre just locked to editing the piano roll for the instrument the clips
//! > track is on. please fix this to make it much more how im envisioning
//! > where clips can have multiple instruments, we just base our
//! > interactions on what your currently selected instrument in the channel
//! > rack is."*
//!
//! FL's rule, and one rule: **the rack's selection is the instrument every
//! interaction means.** A drawn clip is on the selected channel. A note drawn
//! into a clip is on the selected channel, whatever the clip's own channel
//! is. The roll shows the open clip's notes *on the selected channel*, and
//! selecting another channel shows that channel's notes in the same clip.
//! Opening a clip does not move the rack, and selecting a channel does not
//! move the roll to another clip.

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{ClipSource, Note};
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::{ArrangeEdit, RollEdit};
use fontelle_ui::document::{ClipKind, DocumentHost, GhostFilter, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-multi-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn studio(dir: &std::path::Path) -> Session {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised =
        fontelle_app::realise(&project, &library, options).expect("an empty project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

/// A studio with two instruments, the second selected, and the first
/// channel's clip open.
fn two_instruments(dir: &std::path::Path) -> Session {
    let mut session = studio(dir);
    session.add_channel().expect("a second channel");
    assert_eq!(session.channels().len(), 2);
    let first = session.clips()[0].id;
    session.open_clip(first);
    session.select_channel(1);
    session
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
        path: Vec::new(),
        channel: None,
    }
}

// ------------------------------------------------------- a drawn clip ---

#[test]
fn a_drawn_clip_plays_the_selected_channel_not_whatever_the_lane_holds() {
    // *"its guessing what instrument i want based on the lane which is
    // super weird."*
    let dir = scratch("draw");
    let mut session = two_instruments(&dir);
    let lane_of_first = session.clips()[0].lane;
    let second = session.channels()[1].name.clone();

    let made = session.arrange(ArrangeEdit::Add {
        lane: lane_of_first,
        start: PPQN * 16,
    });
    let clips = session.clips();
    let drawn = clips
        .iter()
        .find(|c| c.id == made.clips[0])
        .expect("the new clip");
    assert_eq!(drawn.name, second, "the block plays the rack's selection");
    assert_eq!(
        session.selected_channel(),
        1,
        "and the rack stayed where it was"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------ drawing notes ---

#[test]
fn a_note_drawn_into_another_channels_clip_is_on_the_selected_channel() {
    let dir = scratch("note");
    let mut session = two_instruments(&dir);
    let clip = session.clips()[0].id;
    let ids = session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    assert_eq!(ids.len(), 1);

    let project = session.project();
    let channels: Vec<_> = project.channels.keys().collect();
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip")
    };
    assert_eq!(
        data.channel, channels[0],
        "the clip is still the first channel's"
    );
    assert_eq!(
        data.notes[ids[0]].channel,
        Some(channels[1]),
        "the note is the second channel's"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_note_drawn_on_the_clips_own_channel_carries_no_channel_of_its_own() {
    // So a project that never mixes instruments in a clip is saved exactly
    // as it always was.
    let dir = scratch("home");
    let mut session = two_instruments(&dir);
    session.select_channel(0);
    let clip = session.clips()[0].id;
    let ids = session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    let project = session.project();
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip")
    };
    assert_eq!(data.notes[ids[0]].channel, None);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pasted_notes_land_on_the_selected_channel_wherever_they_were_copied_from() {
    let dir = scratch("paste");
    let mut session = two_instruments(&dir);
    let clip = session.clips()[0].id;
    // A note copied off the first channel's view carries that channel...
    let channels: Vec<_> = session.project().channels.keys().collect();
    let mut copied = a_note(0, 60);
    copied.channel = Some(channels[0]);
    // ...and pasted while the second is selected, it plays the second.
    let ids = session.edit(RollEdit::Insert(vec![copied]));
    let project = session.project();
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip")
    };
    assert_eq!(data.notes[ids[0]].channel, Some(channels[1]));
    std::fs::remove_dir_all(&dir).ok();
}

// -------------------------------------------------- what the roll shows ---

#[test]
fn the_roll_shows_the_open_clips_notes_on_the_selected_channel_only() {
    let dir = scratch("show");
    let mut session = two_instruments(&dir);
    // Two notes on the second channel, one on the first, all in one clip.
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 62),
    });
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 36),
    });

    assert_eq!(session.notes().len(), 1, "the first channel's view");
    assert_eq!(session.notes().values().next().unwrap().key, 36);
    session.select_channel(1);
    assert_eq!(session.notes().len(), 2, "the second channel's view");
    let mut keys: Vec<u8> = session.notes().values().map(|n| n.key).collect();
    keys.sort();
    assert_eq!(keys, vec![60, 62]);
    // And the clip holds all three.
    let clip = session.clips()[0].clone();
    assert_eq!(clip.notes.len(), 3, "the block previews every note in it");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_other_channels_notes_in_the_same_clip_are_ghosts() {
    // Where the drums are while you write the bass over them.
    let dir = scratch("ghost");
    let mut session = two_instruments(&dir);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 36),
    });

    let ghosts = session.ghost_notes(GhostFilter::All);
    assert_eq!(
        ghosts.len(),
        1,
        "the second channel's note, seen from the first"
    );
    assert_eq!(ghosts[0].key, 60);
    assert_eq!(ghosts[0].start, 0);
    // Filtering to the channel being edited shows nothing: its notes are
    // drawn solid already.
    assert!(session.ghost_notes(GhostFilter::Channel(0)).is_empty());
    assert_eq!(session.ghost_notes(GhostFilter::Channel(1)).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn editing_a_note_on_the_selected_channel_leaves_the_others_alone() {
    let dir = scratch("edit");
    let mut session = two_instruments(&dir);
    let on_second = session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 36),
    });
    let mine: Vec<_> = session.notes().keys().collect();
    session.edit(RollEdit::Remove(mine));
    assert!(session.notes().is_empty());
    session.select_channel(1);
    assert_eq!(session.notes().len(), 1);
    assert!(session.notes().contains_key(on_second[0]));
    std::fs::remove_dir_all(&dir).ok();
}

// ----------------------------------------------- the rack and the roll ---

#[test]
fn opening_a_clip_holding_one_instrument_selects_it() {
    // *"if that clip only uses one instrument and not multiple instruments
    // in it, it should switch my instrument selection automatically to that
    // instrument that the clip uses ... you could click your clips to
    // already have the instrument selected to start editing."*
    let dir = scratch("open-one");
    let mut session = two_instruments(&dir);
    // A clip drawn with the second channel selected is the second channel's
    // — empty, and captioned with it.
    let made = session.arrange(ArrangeEdit::Add {
        lane: 0,
        start: PPQN * 16,
    });
    assert_eq!(session.selected_channel(), 1);
    session.select_channel(0);
    session.open_clip(made.clips[0]);
    assert_eq!(
        session.selected_channel(),
        1,
        "the rack follows a one-instrument clip"
    );
    assert!(
        session
            .clips()
            .iter()
            .any(|c| c.id == made.clips[0] && c.open)
    );
    // And back: the first clip is the first channel's, with notes on it.
    let first = session.clips()[0].id;
    session.select_channel(0);
    session.open_clip(first);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.select_channel(1);
    session.open_clip(first);
    assert_eq!(session.selected_channel(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opening_a_clip_holding_several_instruments_keeps_one_that_plays_there() {
    // The rack is the instrument you chose; a clip is the place you are
    // writing. A clip that holds both leaves the question open, so a
    // selection that already plays in it stays — picking the clip up to
    // write more bass must not throw you onto the drums.
    let dir = scratch("open-several");
    let mut session = two_instruments(&dir);
    // A note from each channel makes it hold both. (It used to take only the
    // second's: the first was counted as playing there because the clip was
    // drawn with it, which is the very thing Ty reported.)
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 62),
    });
    session.select_channel(1);
    let first = session.clips()[0].id;
    assert_eq!(session.selected_channel(), 1);
    session.open_clip(first);
    assert_eq!(session.selected_channel(), 1, "stayed on the second");
    session.select_channel(0);
    session.open_clip(first);
    assert_eq!(session.selected_channel(), 0, "stayed on the first");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn selecting_a_channel_keeps_the_clip_that_is_open() {
    let dir = scratch("select");
    let mut session = two_instruments(&dir);
    let made = session.arrange(ArrangeEdit::Add {
        lane: 0,
        start: PPQN * 16,
    });
    let opened = made.clips[0];
    assert!(session.clips().iter().any(|c| c.id == opened && c.open));
    session.select_channel(0);
    assert!(
        session.clips().iter().any(|c| c.id == opened && c.open),
        "selecting a channel moved the roll to another clip"
    );
    session.select_channel(1);
    assert!(session.clips().iter().any(|c| c.id == opened && c.open));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_new_channel_does_not_come_with_a_clip_of_its_own() {
    // It used to: a lane and a clip per channel, because a clip could only
    // play one instrument. A channel is an instrument now, and a clip is a
    // place — you draw one where you want it.
    let dir = scratch("add");
    let mut session = studio(&dir);
    let before = session.clips().len();
    session.add_channel().expect("a second channel");
    assert_eq!(session.clips().len(), before);
    assert_eq!(
        session.selected_channel(),
        1,
        "the new instrument is selected"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------------- the caption ---

#[test]
fn a_block_holding_several_instruments_says_so() {
    let dir = scratch("caption");
    let mut session = two_instruments(&dir);
    let first = session.channels()[0].name.clone();
    assert_eq!(session.clips()[0].name, first);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    // Only the second plays in it so far — so it is the second's block.
    // (It said "first +1": the first was counted because the clip was drawn
    // with it, though not one of its notes was in there.)
    let second = session.channels()[1].name.clone();
    assert_eq!(session.clips()[0].name, second);
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 62),
    });
    // Both now, one note each: the tie goes to the higher in the rack.
    assert_eq!(session.clips()[0].name, format!("{first} +1"));
    assert_eq!(session.clips()[0].kind, ClipKind::Notes);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------- the audition ---

#[test]
fn clicking_a_note_in_the_roll_sounds_the_selected_channel() {
    // The roll only ever shows the selected channel's notes, so the
    // instrument a click sounds is that channel's — the same node live MIDI
    // plays.
    let dir = scratch("audition");
    let mut session = two_instruments(&dir);
    let nodes = session.project().channels.keys().collect::<Vec<_>>();
    let _ = nodes;
    let second = session.audition_target();
    session.select_channel(0);
    let first = session.audition_target();
    assert_ne!(first, second);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ clip colours ---
//
// > *"clips made in old versions projects now on newer versions do not color
// > the clips"* — every block on the arrangement the same blue.
//
// A clip with no colour of its own was its lane's, and every lane is made the
// same blue; nothing ever gave a lane another. So the colour that says which
// instrument a block plays comes from the instrument: its mixer track's when
// it has one, else one of the palette's chosen by its place in the rack. A
// lane somebody did colour (the tour song, a MIDI import) still wins.

const DEFAULT_BLUE: [u8; 4] = [0x4f, 0x8f, 0xd0, 0xff];

fn studio_from(project: fontelle_model::Project, dir: &std::path::Path) -> Session {
    let clip = Session::first_clip(&project).expect("a clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("realises");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

#[test]
fn a_clip_on_an_uncoloured_lane_wears_its_instruments_track_colour() {
    let dir = scratch("colour-track");
    let mut session = studio(&dir);
    session.add_mixer_track();
    session.set_channel_route(0, Some(0));
    let track = session.mixer_strips()[0].color;

    let clip = session.clips()[0].clone();
    assert_eq!(
        clip.color, track,
        "the block is the colour of where it plays"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_clip_whose_instrument_goes_to_the_master_still_has_a_colour() {
    let dir = scratch("colour-master");
    let session = studio(&dir);
    let clip = session.clips()[0].clone();
    assert_ne!(clip.color, DEFAULT_BLUE, "not every block the one blue");
    assert!(
        fontelle_model::TRACK_PALETTE.contains(&clip.color),
        "{:?} is one of the palette's",
        clip.color
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_instruments_on_the_master_are_two_colours() {
    let dir = scratch("colour-two");
    let mut session = two_instruments(&dir);
    let lane = session.clips()[0].lane;
    let made = session.arrange(ArrangeEdit::Add {
        lane,
        start: PPQN * 40,
    });
    let clips = session.clips();
    let first = clips.iter().find(|c| c.id != made.clips[0]).unwrap();
    let second = clips.iter().find(|c| c.id == made.clips[0]).unwrap();
    assert_ne!(first.color, second.color, "{first:?} / {second:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lane_somebody_coloured_keeps_its_colour() {
    let dir = scratch("colour-lane");
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    for lane in project.lanes.values_mut() {
        lane.color = [0x12, 0x34, 0x56, 0xff];
    }
    let session = studio_from(project, &dir);
    assert_eq!(session.clips()[0].color, [0x12, 0x34, 0x56, 0xff]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_ghost_is_the_colour_of_the_block_it_came_from() {
    let dir = scratch("colour-ghost");
    let mut session = two_instruments(&dir);
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(0, 36),
    });
    session.select_channel(1);
    let ghosts = session.ghost_notes(GhostFilter::All);
    assert_eq!(ghosts.len(), 1);
    // The clip's colour is its home channel's; the ghost is the colour of
    // the channel the note plays, which here is the same one.
    assert_eq!(ghosts[0].color, session.clips()[0].color);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------- onion skins and loops ---
//
// > *"piano roll onion skinning does not show looped notes it only shows the
// > start of clips."*

#[test]
fn a_looped_clips_ghosts_repeat_with_every_pass() {
    let dir = scratch("ghost-loop");
    let mut session = two_instruments(&dir);
    let clip = session.clips()[0].clone();
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 36),
    });
    session.arrange(ArrangeEdit::SetLoop {
        ids: vec![clip.id],
        loop_length: Some(PPQN * 4),
    });
    session.end_gesture();
    session.select_channel(1);

    let mut starts: Vec<i64> = session
        .ghost_notes(GhostFilter::All)
        .iter()
        .map(|g| g.start)
        .collect();
    starts.sort();
    let passes = (clip.length / (PPQN * 4)) as usize;
    let expected: Vec<i64> = (0..passes as i64).map(|n| PPQN + n * PPQN * 4).collect();
    assert_eq!(starts, expected, "one ghost per pass, where it plays");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_ghost_stops_where_its_clip_does() {
    // What is heard: a note past the clip's end is not played, and one
    // running over it is cut there (`fontelle_sequencer::compile`).
    let dir = scratch("ghost-end");
    let mut session = two_instruments(&dir);
    let clip = session.clips()[0].clone();
    session.select_channel(0);
    let mut long = a_note(clip.length - PPQN, 36);
    long.length = PPQN * 8;
    session.edit(RollEdit::Add { note: long });
    session.edit(RollEdit::Add {
        note: a_note(clip.length + PPQN, 38),
    });
    session.select_channel(1);

    let ghosts = session.ghost_notes(GhostFilter::All);
    assert_eq!(ghosts.len(), 1, "{ghosts:?}");
    assert_eq!(ghosts[0].start + ghosts[0].length, clip.length);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------ moving several clips ---

#[test]
fn dragging_several_clips_is_one_undo() {
    // Ty: *"when moving multiple clips at the same time ... when i try to
    // undo its not one action its a tonnn of tiny actions."*
    let dir = scratch("drag-many");
    let mut session = studio(&dir);
    let first = session.clips()[0].clone();
    let made = session.arrange(ArrangeEdit::Add {
        lane: first.lane,
        start: PPQN * 40,
    });
    let second = made.clips[0];
    session.end_gesture();
    let ids = vec![first.id, second];
    let starts = |s: &Session| -> Vec<i64> {
        let clips = s.clips();
        ids.iter()
            .map(|id| clips.iter().find(|c| c.id == *id).unwrap().start)
            .collect()
    };
    let before = starts(&session);
    for _ in 0..20 {
        session.arrange(ArrangeEdit::Move {
            ids: ids.clone(),
            tick_delta: PPQN / 4,
            lane_delta: 0,
        });
    }
    session.end_gesture();
    assert_eq!(
        starts(&session),
        vec![before[0] + PPQN * 5, before[1] + PPQN * 5]
    );

    session.undo();
    assert_eq!(
        starts(&session),
        before,
        "one undo puts the whole drag back"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------- Shift-dragging several clips' ends ---

#[test]
fn shift_dragging_a_mixed_selection_loops_each_at_its_own_length_in_one_undo() {
    // Ty: *"if you have multiple clips selected, some looping and some not,
    // and then youre holding shift and drag the end of the clips out ... it
    // would basically be like if you dragged each one out manually. however
    // whenever i do this, it chops everything up into the same loop time and
    // then squishes it weirdly."* — and the drag is one thing to take back.
    let dir = scratch("loop-many");
    let mut session = studio(&dir);
    let first = session.clips()[0].clone();
    let made = session.arrange(ArrangeEdit::Add {
        lane: first.lane,
        start: PPQN * 64,
    });
    let second = made.clips[0];
    session.end_gesture();
    session.arrange(ArrangeEdit::SetLoop {
        ids: vec![second],
        loop_length: Some(PPQN * 2),
    });
    session.end_gesture();
    let shape = |s: &Session| -> Vec<(i64, Option<i64>)> {
        let clips = s.clips();
        [first.id, second]
            .iter()
            .map(|id| {
                let c = clips.iter().find(|c| c.id == *id).unwrap();
                (c.length, c.loop_length)
            })
            .collect()
    };
    let before = shape(&session);
    assert_ne!(before[0].0, before[1].0, "two lengths, to tell apart");

    session.arrange(ArrangeEdit::ResizeLooping {
        ids: vec![first.id, second],
        tick_delta: PPQN,
        loops: vec![Some(before[0].0), None],
    });
    for _ in 0..7 {
        session.arrange(ArrangeEdit::Resize {
            ids: vec![first.id, second],
            tick_delta: PPQN,
        });
    }
    session.end_gesture();
    assert_eq!(
        shape(&session),
        vec![
            (before[0].0 + PPQN * 8, Some(before[0].0)),
            (before[1].0 + PPQN * 8, Some(PPQN * 2)),
        ]
    );

    session.undo();
    assert_eq!(shape(&session), before, "one undo puts the whole drag back");
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------- which instrument a click on a clip means ---
//
// > *"if you end up deciding you dont like the instrument you initially
// > started with and play a different instrument in that same clip and remove
// > the first one, it will play the instrument right but clicking on that
// > clip to select it does not select the instrument thats actually playing
// > in the channel rack instead it selects the original instrument that was
// > tied to that clip."*
//
// Read off the notes, at the moment of the click (`ClipChannels`): the home
// channel stays in the file, and nothing a person sees comes from it unless
// the clip is empty.

/// Ty's clip: drawn with the first channel, every note in it the second's.
fn played_by_the_second(dir: &std::path::Path) -> Session {
    let mut session = two_instruments(dir);
    // The first clip is the first channel's; the second is selected.
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 62),
    });
    session
}

#[test]
fn a_clip_drawn_with_one_instrument_and_played_by_another_selects_the_other() {
    let dir = scratch("played-by");
    let mut session = played_by_the_second(&dir);
    let clip = session.clips()[0].id;
    session.select_channel(0);
    session.open_clip(clip);
    assert_eq!(
        session.selected_channel(),
        1,
        "the instrument actually playing, not the one it was drawn with"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn and_its_block_wears_the_instrument_playing_in_it() {
    let dir = scratch("played-by-face");
    let mut session = played_by_the_second(&dir);
    let second = session.channels()[1].name.clone();
    let block = session.clips()[0].clone();
    assert_eq!(block.name, second, "captioned with what plays");
    // And coloured with it: a block of the second's own drawing is.
    let drawn = session.arrange(ArrangeEdit::Add {
        lane: 3,
        start: PPQN * 32,
    });
    let theirs = session
        .clips()
        .into_iter()
        .find(|c| c.id == drawn.clips[0])
        .expect("drawn");
    assert_eq!(block.color, theirs.color, "coloured with what plays");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn with_several_instruments_the_one_with_the_most_notes_is_chosen() {
    let dir = scratch("most");
    let mut session = played_by_the_second(&dir);
    let clip = session.clips()[0].id;
    // One note of the first's beside the second's two, and a third channel
    // that plays nothing here selected.
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 64),
    });
    session.add_channel().expect("a third channel");
    assert_eq!(session.selected_channel(), 2);
    session.open_clip(clip);
    assert_eq!(session.selected_channel(), 1, "the part the clip is about");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn clicking_the_open_clip_again_steps_through_its_instruments() {
    let dir = scratch("cycle");
    let mut session = played_by_the_second(&dir);
    let clip = session.clips()[0].id;
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 64),
    });
    session
        .add_channel()
        .expect("a third channel, which plays nothing here");
    session.open_clip(clip);
    assert_eq!(session.selected_channel(), 1);
    session.cycle_clip_channel(clip);
    assert_eq!(
        session.selected_channel(),
        0,
        "round to the top of the rack"
    );
    session.cycle_clip_channel(clip);
    assert_eq!(session.selected_channel(), 1);
    session.cycle_clip_channel(clip);
    assert_eq!(
        session.selected_channel(),
        0,
        "never the third, which is not in it"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_empty_clip_still_selects_the_instrument_it_was_drawn_with() {
    let dir = scratch("empty");
    let mut session = two_instruments(&dir);
    let clip = session.clips()[0].id;
    assert_eq!(session.selected_channel(), 1);
    session.open_clip(clip);
    assert_eq!(session.selected_channel(), 0);
    session.cycle_clip_channel(clip);
    assert_eq!(session.selected_channel(), 0, "nothing else to step to");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_deleted_instrument_is_never_selected_and_its_undo_brings_it_back() {
    let dir = scratch("deleted");
    let mut session = played_by_the_second(&dir);
    let clip = session.clips()[0].id;
    // One note of the first's too, so the clip is not empty once the second
    // has gone.
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 64),
    });
    session.end_gesture();
    let first = session.channels()[0].name.clone();
    // The second goes. Its notes stay in the clip naming a channel that is
    // not there, and count for nothing.
    session.remove_channel(1);
    session.end_gesture();
    assert_eq!(session.channels().len(), 1);
    session.open_clip(clip);
    assert_eq!(session.selected_channel(), 0);
    assert_eq!(
        session
            .clips()
            .iter()
            .find(|c| c.id == clip)
            .map(|c| c.name.clone()),
        Some(first),
        "no +1 for an instrument that has gone"
    );
    session.cycle_clip_channel(clip);
    assert_eq!(session.selected_channel(), 0);
    // And back: the second is the one with the most notes again.
    session.undo();
    assert_eq!(session.channels().len(), 2);
    session.select_channel(0);
    session.open_clip(clip);
    assert_eq!(session.selected_channel(), 0, "kept: the first plays here");
    session.cycle_clip_channel(clip);
    assert_eq!(session.selected_channel(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_automation_clip_leaves_the_rack_alone_when_clicked_or_clicked_again() {
    let dir = scratch("automation");
    let mut session = two_instruments(&dir);
    let master = session
        .mixer_track_id(session.mixer_strips().len() - 1)
        .expect("the last strip is the master");
    let address = fontelle_types::ParamTarget::TrackGain(master).address();
    session.create_automation(&address, "Master \u{2014} gain", PPQN * 4);
    let automation = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Automation)
        .expect("an automation clip")
        .id;
    let before = session.selected_channel();
    session.open_clip(automation);
    session.cycle_clip_channel(automation);
    assert_eq!(session.selected_channel(), before);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------- deleting the first instrument ---

#[test]
fn deleting_the_instrument_a_clip_was_drawn_with_keeps_the_others_notes() {
    // Ty's case to the letter: drawn with A, B played into it, A removed.
    // *"it will play the instrument right but clicking on that clip ...
    // selects the original instrument"* — and before that, deleting A took
    // the whole clip, B's part with it.
    let dir = scratch("delete-first");
    let mut session = played_by_the_second(&dir);
    let clip = session.clips()[0].id;
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(PPQN * 2, 64),
    });
    session.end_gesture();
    let second = session.channels()[1].name.clone();
    let before = session.clips();

    session.remove_channel(0);
    session.end_gesture();
    assert_eq!(session.channels().len(), 1);
    let block = session
        .clips()
        .into_iter()
        .find(|c| c.id == clip)
        .expect("the clip with B's notes is still there");
    assert_eq!(block.name, second, "and it is B's block");
    assert_eq!(block.notes.len(), 2, "B's two notes, A's one gone");
    session.open_clip(clip);
    assert_eq!(session.selected_channel(), 0, "a click selects B");
    // It plays B: B's notes compile.
    assert!(!session.compiled().events.is_empty());

    session.undo();
    assert_eq!(session.channels().len(), 2);
    assert_eq!(session.clips(), before, "undo puts back exactly");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn deleting_an_instrument_still_takes_a_clip_of_nothing_but_its_notes() {
    let dir = scratch("delete-only");
    let mut session = two_instruments(&dir);
    // The first channel's clip, with one of its own notes in it.
    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();
    let before = session.clips().len();
    session.remove_channel(0);
    session.end_gesture();
    assert_eq!(session.clips().len(), before - 1);
    std::fs::remove_dir_all(&dir).ok();
}
