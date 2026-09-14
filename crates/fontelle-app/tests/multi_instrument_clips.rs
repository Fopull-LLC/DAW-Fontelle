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
fn opening_a_clip_holding_several_instruments_does_not_move_the_rack() {
    // The rack is the instrument you chose; a clip is the place you are
    // writing. Opening the drum clip while the bass is selected means "write
    // bass in here" — and only a clip that already holds both leaves the
    // question open, so only that clip leaves the rack alone.
    let dir = scratch("open-several");
    let mut session = two_instruments(&dir);
    // The first clip is the first channel's; a note from the second makes it
    // hold both.
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
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
