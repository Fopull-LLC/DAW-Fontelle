//! The studio behind the window: the channel rack, the soundfont bank, and
//! what happens to the running audio when either changes (item 9 of
//! `docs/first-usable-plan.md`).
//!
//! `Session` is where the model, the engine and the UI meet, so this drives it
//! through the same trait the window does and checks the document and the
//! published graph on the other side. Everything is real except the mouse and
//! the sound card.

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_assets::fixtures::{
    GEN_KEY_RANGE, GEN_OVERRIDING_ROOT_KEY, GEN_SAMPLE_MODES, KIT, Sf2Fixture, ZoneSpec,
    build_drum_kit_sf2, build_sf2, gen_range, gen_val,
};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{ClipSource, Note};
use fontelle_types::{CompiledTimeline, PPQN, Tick as PPQNTick};
use fontelle_ui::canvas::{ArrangeEdit, LaneProperty, RollEdit};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-studio-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A bank folder holding one real, loadable soundfont.
fn a_bank(name: &str) -> PathBuf {
    let dir = scratch(name);
    let fixture = Sf2Fixture {
        samples: (0..64).map(|i| (i * 500 - 16_000) as i16).collect(),
        sample_rate: 44_100,
        header_start: 0,
        header_end: 64,
        header_loop_start: 8,
        header_loop_end: 56,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_val(GEN_OVERRIDING_ROOT_KEY, 60),
                gen_val(GEN_SAMPLE_MODES, 1),
            ],
        },
        extra_zones: Vec::new(),
    };
    std::fs::write(dir.join("Test Piano.sf2"), build_sf2(&fixture))
        .expect("the fixture must be writable");
    dir
}

/// A session over an empty project, with a bank folder pointed at `dir`, and
/// the RT thread's end of both channels.
fn studio(dir: &std::path::Path) -> (Session, fontelle_engine::GraphSource) {
    let (session, graphs, _timeline) = studio_with_timeline(dir);
    (session, graphs)
}

/// The same, keeping the RT thread's end of the **timeline** channel as well —
/// which is what an edit that moves notes in time republishes, as against an
/// edit that changes an instrument, which republishes the graph.
fn studio_with_timeline(
    dir: &std::path::Path,
) -> (
    Session,
    fontelle_engine::GraphSource,
    fontelle_engine::TimelineSource,
) {
    let project = blank_project(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, timeline_source) = timeline_channel(CompiledTimeline::empty());

    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(
        &project,
        &library,
        RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        },
    )
    .expect("an empty project must realise");
    let (graphs, source) = graph_channel(realised.graph);

    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        },
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    // Never the developer's own `~/.config/fontelle`: `open_bank` writes the
    // folders it settled on, and a test that scribbles a `/tmp` path into a
    // real config file is a test that breaks the machine it ran on. It did.
    .with_settings_path(dir.join("settings.json"));
    session.add_soundfont_dir(dir);
    session.open_bank();
    (session, source, timeline_source)
}

// ------------------------------------------------------------- the bank ---

#[test]
fn the_browser_finds_the_soundfonts_in_the_bank_folder() {
    let dir = a_bank("finds");
    let (session, _source) = studio(&dir);

    let files = session.library_files();
    assert_eq!(files.len(), 1, "the bank folder holds one soundfont");
    assert_eq!(files[0].name, "Test Piano");
    assert!(
        !files[0].detail.is_empty(),
        "the size is shown, because a 325 MB soundfont is a choice worth making \
         knowingly"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opening_a_file_lists_its_presets_without_loading_any_audio() {
    let dir = a_bank("presets");
    let (mut session, _source) = studio(&dir);

    assert!(
        session.library_presets().is_empty(),
        "nothing is open yet, so there is nothing to list"
    );
    session.open_file(0).expect("the fixture must open");
    assert_eq!(session.selected_file(), Some(0));
    assert!(!session.library_presets().is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_search_filters_the_bank_and_forgets_the_open_file() {
    let dir = a_bank("search");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();

    session.set_query("piano");
    assert_eq!(session.library_files().len(), 1);
    assert_eq!(
        session.selected_file(),
        Some(0),
        "the open file is remembered by its **path** now, so it keeps its \
         highlight wherever the list moves it to — see `tests/browsing.rs`"
    );

    session.set_query("zzzz");
    assert!(session.library_files().is_empty());
    assert_eq!(
        session.selected_file(),
        None,
        "a search that hides the open file shows no highlight, which is true"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ----------------------------------------------------- the channel rack ---

#[test]
fn a_blank_project_starts_with_one_channel_and_no_instrument_on_it() {
    let dir = a_bank("blank");
    let (session, _source) = studio(&dir);

    let channels = session.channels();
    assert_eq!(channels.len(), 1);
    assert!(
        !channels[0].has_instrument,
        "a fresh channel plays nothing rather than playing a default"
    );
    assert!(!channels[0].muted && !channels[0].soloed);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn choosing_a_preset_puts_it_on_the_selected_channel_and_republishes_the_graph() {
    let dir = a_bank("choose");
    let (mut session, mut source) = studio(&dir);
    session.open_file(0).unwrap();

    let before = session.revision();
    session
        .set_channel_instrument(0)
        .expect("the fixture's preset must load");

    assert!(session.channels()[0].has_instrument);
    assert!(
        session.is_dirty(),
        "the document changed, so it needs saving"
    );
    assert!(session.revision() > before, "the panels have to be redrawn");
    assert!(
        source.take_update(),
        "the instrument has to reach the running stream — otherwise choosing a \
         soundfont means restarting the audio device, which is the whole of \
         item 9"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn adding_a_channel_gives_it_a_clip_and_opens_it_in_the_roll() {
    let dir = a_bank("add");
    let (mut session, mut source) = studio(&dir);
    session.open_file(0).unwrap();

    session.add_channel_with(0).expect("must add");
    assert_eq!(session.channels().len(), 2);
    assert_eq!(
        session.selected_channel(),
        1,
        "the channel you just added is the one you want to write on"
    );
    assert!(session.channels()[1].has_instrument);

    // The roll follows it, and the clip it opened is empty and belongs to the
    // new channel.
    assert!(session.notes().is_empty());
    session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    assert_eq!(session.notes().len(), 1);
    let clips: Vec<usize> = session
        .project()
        .clips
        .values()
        .map(|clip| match &clip.source {
            ClipSource::Notes(data) => data.notes.len(),
            _ => 0,
        })
        .collect();
    assert_eq!(
        clips,
        vec![0, 1],
        "the note went into the new channel's clip, not the first one's"
    );
    assert!(source.take_update());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn selecting_a_channel_opens_that_channels_clip() {
    let dir = a_bank("select");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.add_channel_with(0).unwrap();

    // A note on each, so the two clips are tellable apart.
    session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    session.select_channel(0);
    assert_eq!(
        session.notes().len(),
        0,
        "the roll follows the rack — that is what they are next to each other for"
    );
    session.select_channel(1);
    assert_eq!(session.notes().len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn muting_a_channel_is_a_command_and_reaches_the_audio_thread() {
    let dir = a_bank("mute");
    let (mut session, mut source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    while source.take_update() {}

    let controls = session.track_controls();
    session.toggle_mute(0);
    assert!(session.channels()[0].muted);
    // The rack's mute is the **channel's**, and a sequencer mute: the
    // compiler drops the channel's clips rather than a fader zeroing its bus.
    // It has to be — a channel plays through the master by default now, so a
    // switch that reached for the mixer track would silence the whole song.
    // See `fontelle_model::Channel::muted` and `tests/mixer.rs`.
    assert!(
        session.compiled().events.is_empty(),
        "a muted channel puts nothing on the timeline"
    );
    assert!(
        !controls[0].mute(),
        "and the master, which this channel plays through, stays open"
    );
    assert!(
        !source.take_update(),
        "and it did not cost a new graph to get there"
    );

    // And it is undoable, because it went through the history like everything
    // else (INVARIANT 9).
    session.undo();
    assert!(!session.channels()[0].muted);

    session.toggle_solo(0);
    assert!(session.channels()[0].soloed);

    std::fs::remove_dir_all(&dir).ok();
}

/// INVARIANT 9 has no exceptions, and choosing an instrument is a document
/// mutation like any other. It went through `Command::apply` directly at first,
/// which made it the one edit in the app that Ctrl+Z could not reach.
#[test]
fn choosing_an_instrument_can_be_taken_back() {
    let dir = a_bank("undo-instrument");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    assert!(session.channels()[0].has_instrument);

    session.undo();
    assert!(
        !session.channels()[0].has_instrument,
        "putting the wrong soundfont on a channel has to be undoable"
    );
    session.redo();
    assert!(session.channels()[0].has_instrument);

    std::fs::remove_dir_all(&dir).ok();
}

/// The roll's ruler is clicked to move the playhead, and only the document
/// knows where a tick is in samples once there is a tempo change (INVARIANT 5).
#[test]
fn a_clip_tick_converts_to_a_sample_through_the_documents_own_tempo_map() {
    let dir = a_bank("seek");
    let (session, _source) = studio(&dir);

    // 120 bpm: one beat is half a second.
    let one_beat = session.sample_of_clip_tick(PPQN);
    assert_eq!(one_beat, SR as i64 / 2);
    assert_eq!(session.sample_of_clip_tick(0), 0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn adding_a_channel_can_be_taken_back() {
    let dir = a_bank("undo");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.add_channel_with(0).unwrap();
    assert_eq!(session.channels().len(), 2);

    // Three entries — the patch, the clip, the channel — because a compound
    // command does not exist yet. Each one undoes.
    session.undo();
    assert!(
        !session.channels()[1].has_instrument,
        "the patch comes off first"
    );
    session.undo();
    session.undo();
    assert_eq!(
        session.channels().len(),
        1,
        "adding an instrument by mistake has to be reachable by Ctrl+Z"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// -------------------------------------------------------- drawing a note ---

#[test]
fn drawing_a_note_hands_back_the_id_the_drag_needs() {
    let dir = a_bank("ids");
    let (mut session, _source) = studio(&dir);

    let ids = session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN / 4,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    assert_eq!(
        ids.len(),
        1,
        "drawing and sizing is one gesture, and its second half needs this id"
    );
    assert!(session.notes().get(ids[0]).is_some());

    // A paste hands back one per note, so the pasted phrase arrives selected.
    let pasted = session.edit(RollEdit::Insert(
        session.notes().values().copied().collect(),
    ));
    assert_eq!(pasted.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_velocity_edit_reaches_the_document() {
    let dir = a_bank("velocity");
    let (mut session, _source) = studio(&dir);
    let ids = session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });

    session.edit(RollEdit::SetProperty {
        ids: ids.clone(),
        property: LaneProperty::Velocity,
        value: 42,
    });
    assert_eq!(session.notes()[ids[0]].velocity, 42);
    session.undo();
    assert_eq!(session.notes()[ids[0]].velocity, 100);

    std::fs::remove_dir_all(&dir).ok();
}

// -------------------------------------------------------------- audition ---

#[test]
fn an_audition_reaches_the_live_channel_addressed_to_the_selected_channel() {
    let dir = a_bank("audition");
    let (mut session, _source) = studio(&dir);
    let (mut live, mut ports) = fontelle_engine::live_event_channel(1, 32);
    let port = ports.claim().expect("one port");
    let session = session_with_audition(&mut session, Box::new(port));

    session.audition_on(64, 90, 0);
    session.audition_off(64);

    let drained = live.drain(0, false);
    assert_eq!(drained.len(), 2, "a note on and its note off");
    assert!(matches!(
        drained[0].payload,
        fontelle_types::EventPayload::NoteOn { key: 64, .. }
    ));
    assert!(
        drained[0].target != Default::default(),
        "an audition has to be addressed to the selected channel's node, or it \
         reaches whichever node has the null id"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// `with_audition` consumes the session; this is the same thing in place, so a
/// test can build a session and then give it a sink.
fn session_with_audition(
    session: &mut Session,
    sink: Box<dyn fontelle_types::EventSink>,
) -> &mut Session {
    session.set_audition(sink);
    session
}

// -------------------------------------------------- where the bank lives ---

/// The user has to be able to say where their soundfonts are without knowing
/// that a `--soundfonts` flag exists, and to open the folder Fontelle picked so
/// they can put files in it. Both reported from the window as gaps.
#[test]
fn the_bank_folder_is_reachable_and_changeable_from_inside_the_studio() {
    let dir = a_bank("folders");
    let (mut session, _source) = studio(&dir);
    assert_eq!(session.library_dirs(), vec![dir.clone()]);

    // Changing it replaces the list, rescans, and is remembered on disk — so a
    // relaunch opens on the folder that was chosen and not on the default.
    let other = scratch("folders-other");
    session.set_library_dirs(vec![other.clone()], false);
    assert_eq!(session.library_dirs(), vec![other.clone()]);
    assert!(
        session.library_files().is_empty(),
        "the new folder has no soundfonts in it, and the old one's must be gone"
    );
    let (saved, error) = fontelle_app::settings::Settings::load_from(&dir.join("settings.json"));
    assert!(error.is_none());
    assert_eq!(saved.soundfont_dirs, vec![other.clone()]);

    // Adding keeps what was there.
    session.set_library_dirs(vec![dir.clone()], true);
    assert_eq!(session.library_dirs(), vec![other, dir.clone()]);
    // With **two** configured folders the browser's top level is the two
    // folders, not a flat list of everything under them — that is the whole of
    // `tests/browsing.rs`. The fixture being back is a search away.
    assert_eq!(session.library_files().len(), 2, "one row per folder");
    session.set_query("piano");
    assert_eq!(session.library_files().len(), 1, "the fixture is back");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_status_line_says_where_the_bank_is_rather_than_running_off_the_panel() {
    let dir = a_bank("status");
    let (session, _source) = studio(&dir);

    let status = session.library_status();
    assert!(
        status.chars().count() <= 40,
        "the browser is 248 pixels wide; {status:?} does not fit in it"
    );
    assert!(
        status.contains(dir.file_name().unwrap().to_str().unwrap()),
        "it still has to say which folder: {status:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------- saying what is chosen ---
//
// Reported from using the window: *"I was flicking through instruments on a
// soundfont and it was letting me — I could hear the sound change — but it
// wasn't highlighting the selected instrument."* Two things were missing, and
// both of them are the same thing: nothing on screen said which preset a
// channel was playing.

#[test]
fn the_studio_says_which_preset_the_selected_channel_is_playing() {
    let dir = a_bank("selected-preset");
    let (mut session, _source) = studio(&dir);

    assert_eq!(
        session.selected_preset(),
        None,
        "nothing is open and nothing is chosen"
    );
    session.open_file(0).unwrap();
    assert_eq!(
        session.selected_preset(),
        None,
        "opening a file lists its presets — it does not choose one"
    );

    session.set_channel_instrument(0).unwrap();
    assert_eq!(
        session.selected_preset(),
        Some(0),
        "the preset that was put on this channel is the one to highlight"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_channel_has_its_own_chosen_preset() {
    let dir = a_bank("selected-preset-two");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();

    // A second channel with nothing on it: the highlight must follow the rack's
    // selection, not stay on whatever was chosen last.
    session.add_channel_with(0).unwrap();
    assert_eq!(session.selected_preset(), Some(0));
    session.select_channel(0);
    assert_eq!(session.selected_preset(), Some(0));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn changing_a_channels_instrument_renames_it_to_the_preset() {
    let dir = a_bank("rename");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();

    let preset = session.library_presets()[0].name.clone();
    session.set_channel_instrument(0).unwrap();
    assert_eq!(
        session.channels()[0].name,
        preset,
        "a channel that goes on saying what it used to be is the loudest \
         \"nothing happened\" a rack can give somebody who just changed an \
         instrument"
    );
    assert!(session.channels()[0].has_instrument);

    // And it is one undo away, like every other document change (INVARIANT 9).
    session.undo();
    assert_ne!(session.channels()[0].name, preset);

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------- the arrangement ---
//
// The window had a piano roll and no view of the piece. These are the
// document's half of the arrangement canvas: what it shows, and what an edit
// dragged on it does to the project.

#[test]
fn the_arrangement_lists_every_clip_on_its_own_lane() {
    let dir = a_bank("arrange-list");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    session.add_channel_with(0).unwrap();

    let lanes = session.lanes();
    let clips = session.clips();
    assert!(lanes.len() >= 2, "two channels, two lanes: {lanes:?}");
    assert_eq!(clips.len(), 2, "one clip each: {clips:?}");
    for clip in &clips {
        assert!(
            clip.lane < lanes.len(),
            "clip on lane {} of {}",
            clip.lane,
            lanes.len()
        );
        assert!(clip.length > 0);
        assert!(!clip.name.is_empty(), "a block with no name is unreadable");
    }
    assert_eq!(
        clips.iter().filter(|c| c.open).count(),
        1,
        "exactly one clip is the one the roll has open"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn clicking_a_clip_on_the_arrangement_opens_it_in_the_roll() {
    let dir = a_bank("arrange-open");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    session.add_channel_with(0).unwrap();
    assert_eq!(session.selected_channel(), 1);

    let first = session
        .clips()
        .into_iter()
        .find(|c| c.lane == 0)
        .expect("a clip on the first lane");
    session.open_clip(first.id);
    assert_eq!(
        session.selected_channel(),
        0,
        "opening a clip selects the channel that owns it — the rack, the roll \
         and the arrangement are three views of one selection"
    );
    assert!(session.clips().iter().any(|c| c.id == first.id && c.open));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dragging_a_clip_on_the_arrangement_is_a_command_and_reaches_the_audio_thread() {
    let dir = a_bank("arrange-move");
    let (mut session, _source, mut timeline) = studio_with_timeline(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    let was = timeline
        .current()
        .events
        .first()
        .expect("the drawn note is on the timeline")
        .sample;

    let clip = session.clips().remove(0);
    session.arrange(ArrangeEdit::Move {
        ids: vec![clip.id],
        tick_delta: PPQN * 4,
        lane_delta: 0,
    });
    assert_eq!(session.clips()[0].start, clip.start + PPQN * 4);
    // Moving a clip moves its notes in time, so the compiled timeline the audio
    // thread reads has actually changed — two seconds later at 120 bpm.
    assert_eq!(
        timeline.current().events.first().map(|e| e.sample),
        Some(was + SR as i64 * 2),
        "the moved clip's notes did not reach the audio thread"
    );

    // And it is one Ctrl+Z away, like every other document change.
    session.undo();
    assert_eq!(session.clips()[0].start, clip.start);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_arrangement_can_resize_duplicate_mute_and_delete_clips() {
    let dir = a_bank("arrange-edits");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    let clip = session.clips().remove(0);

    session.arrange(ArrangeEdit::Resize {
        ids: vec![clip.id],
        tick_delta: PPQN * 4,
    });
    assert_eq!(session.clips()[0].length, clip.length + PPQN * 4);

    session.arrange(ArrangeEdit::SetMuted {
        ids: vec![clip.id],
        muted: true,
    });
    assert!(session.clips()[0].muted);

    session.arrange(ArrangeEdit::Duplicate {
        ids: vec![clip.id],
        tick_offset: PPQN * 16,
    });
    assert_eq!(session.clips().len(), 2, "the copy is on the arrangement");

    session.arrange(ArrangeEdit::Remove(vec![clip.id]));
    assert_eq!(session.clips().len(), 1);
    assert!(
        session.clips()[0].id != clip.id,
        "the one that was removed is the one that is gone"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_song_tick_converts_to_a_sample_and_back_through_the_tempo_map() {
    let dir = a_bank("arrange-time");
    let (session, _source) = studio(&dir);

    // 120 bpm: one beat is half a second, and the arrangement's ruler is in
    // song ticks rather than the open clip's.
    assert_eq!(session.sample_of_song_tick(PPQN), SR as i64 / 2);
    assert_eq!(session.playhead_song_tick(SR as i64 / 2), PPQN);
    assert_eq!(
        session.sample_of_song_tick(-500),
        0,
        "never before the start"
    );
    assert!(session.song_length() > 0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn muting_a_lane_silences_it_without_touching_its_clips() {
    let dir = a_bank("arrange-lane-mute");
    let (mut session, _source, mut timeline) = studio_with_timeline(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    assert!(!timeline.current().events.is_empty());

    session.toggle_lane_mute(0);
    assert!(session.lanes()[0].muted);
    assert!(
        !session.clips()[0].muted,
        "a lane mute is a sequencer mute (TDD §10.3), not a change to the clip"
    );
    assert!(
        timeline.current().events.is_empty(),
        "a muted lane emits no events at all (TDD §10.3), and the audio thread \
         is reading the timeline that says so"
    );

    session.toggle_lane_mute(0);
    assert!(!session.lanes()[0].muted);
    assert!(!timeline.current().events.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------ the instrument editor ---
//
// TDD §7.2's whole point: the SF2 file supplies defaults and the user owns
// every parameter afterwards. Until this existed an imported preset played what
// the file said and nothing could be changed, which is the one thing Fontelle is
// for. Reported as *"why can't I open up the VST options?"* — because there
// were none to open.

fn param_of<'a>(
    view: &'a fontelle_ui::canvas::InstrumentView,
    ends_with: &str,
) -> &'a fontelle_ui::canvas::InstrumentParam {
    view.groups
        .iter()
        .flat_map(|g| &g.params)
        .find(|p| p.address.as_str().ends_with(ends_with))
        .unwrap_or_else(|| {
            panic!(
                "no parameter ending in {ends_with:?}; there are {:?}",
                view.groups
                    .iter()
                    .flat_map(|g| &g.params)
                    .map(|p| p.address.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn a_channel_with_no_instrument_has_nothing_to_edit() {
    let dir = a_bank("vst-empty");
    let (session, _source) = studio(&dir);
    assert!(
        session.instrument().is_none(),
        "an empty channel must not offer knobs that write to nothing"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_instrument_editor_exposes_the_patch_the_soundfont_seeded() {
    let dir = a_bank("vst-view");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();

    let view = session
        .instrument()
        .expect("a channel with a patch has one");
    assert_eq!(view.title, session.channels()[0].name);
    assert!(view.groups.len() >= 4, "got {:?}", view.groups.len());

    // The four things §7.2 says a person owns after an import.
    for wanted in [
        "voice/polyphony",
        "voice/glide",
        "filter[0]/cutoff",
        "filter[0]/resonance",
        "filter[0]/enabled",
        "env[0]/attack",
        "env[0]/decay",
        "env[0]/sustain",
        "env[0]/release",
        "quality",
        "mixer/gain",
        "mixer/pan",
    ] {
        let param = param_of(&view, wanted);
        assert!(
            (0.0..=1.0).contains(&param.value),
            "{wanted} is {} — every value the panel draws is normalised",
            param.value
        );
        assert!(!param.display.is_empty(), "{wanted} has no read-out");
        assert!(!param.label.is_empty(), "{wanted} has no caption");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn moving_a_knob_changes_the_patch_and_reaches_the_audio_thread() {
    let dir = a_bank("vst-edit");
    let (mut session, mut source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    while source.take_update() {}

    let cutoff = param_of(&session.instrument().unwrap(), "filter[0]/cutoff").clone();
    let was = cutoff.display.clone();
    session.set_instrument_param(&cutoff.address, 0.25);

    let now = session.instrument().unwrap();
    let after = param_of(&now, "filter[0]/cutoff");
    assert!((after.value - 0.25).abs() < 0.01, "got {}", after.value);
    assert_ne!(after.display, was, "the read-out did not follow the knob");
    assert!(
        source.take_update(),
        "a patch change rebuilds the graph — otherwise you cannot hear what you \
         just turned"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_switch_and_a_choice_both_write_through() {
    use fontelle_ui::canvas::ParamKind;

    let dir = a_bank("vst-switch");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();

    let view = session.instrument().unwrap();
    let enabled = param_of(&view, "filter[0]/enabled").clone();
    assert_eq!(enabled.kind, ParamKind::Switch);
    let flipped = if enabled.value >= 0.5 { 0.0 } else { 1.0 };
    session.set_instrument_param(&enabled.address, flipped);
    assert_eq!(
        param_of(&session.instrument().unwrap(), "filter[0]/enabled").value,
        flipped
    );

    let quality = param_of(&session.instrument().unwrap(), "quality").clone();
    let ParamKind::Choice(options) = &quality.kind else {
        panic!("the quality control is not a choice: {:?}", quality.kind);
    };
    assert!(options.len() >= 4, "got {options:?}");
    // The last option, whatever it is, and the read-out has to say so.
    session.set_instrument_param(&quality.address, 1.0);
    let now = param_of(&session.instrument().unwrap(), "quality").clone();
    assert_eq!(now.display, *options.last().unwrap());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn turning_a_knob_is_one_undo_rather_than_one_per_pixel() {
    let dir = a_bank("vst-undo");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();

    let cutoff = param_of(&session.instrument().unwrap(), "filter[0]/cutoff").clone();
    let was = cutoff.value;
    // One drag, forty steps.
    for step in 1..=40 {
        session.set_instrument_param(&cutoff.address, step as f32 / 40.0);
    }
    session.end_gesture();
    session.undo();

    let after = param_of(&session.instrument().unwrap(), "filter[0]/cutoff").value;
    assert!(
        (after - was).abs() < 0.01,
        "one undo went back to {after} rather than to {was} — a drag left forty \
         entries on the history"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_parameter_address_that_means_nothing_is_ignored_rather_than_a_panic() {
    use fontelle_types::ParamAddress;

    let dir = a_bank("vst-nonsense");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();

    let before = session.instrument().unwrap();
    session.set_instrument_param(&ParamAddress::new("patch/filter[99]/cutoff"), 0.5);
    session.set_instrument_param(&ParamAddress::new("nonsense"), 0.5);
    assert_eq!(
        session.instrument().unwrap(),
        before,
        "an address nobody minted changed the patch"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ onion skins ---

#[test]
fn the_onion_skin_shows_other_channels_notes_lined_up_in_time() {
    use fontelle_ui::document::GhostFilter;

    let dir = a_bank("ghosts");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();

    // A note on the first channel, then a second channel with its own.
    let a_note = |start: PPQNTick, key: u8| RollEdit::Add {
        note: Note {
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
        },
    };
    session.edit(a_note(PPQN * 2, 60));
    session.add_channel_with(0).unwrap();
    session.edit(a_note(PPQN * 4, 67));

    // Looking at the second channel, the first channel's note is a ghost.
    assert!(
        session.ghost_notes(GhostFilter::Off).is_empty(),
        "off means off"
    );
    let all = session.ghost_notes(GhostFilter::All);
    assert_eq!(all.len(), 1, "got {all:?}");
    assert_eq!(all[0].key, 60);
    assert_eq!(
        all[0].start,
        PPQN * 2,
        "the ghost is placed in the open clip's own tick space, or it does not \
         line up with the notes it is meant to be read against"
    );
    assert!(all[0].length > 0);

    // And the other way round.
    session.select_channel(0);
    let all = session.ghost_notes(GhostFilter::All);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].key, 67);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_onion_skin_can_be_filtered_to_one_instrument() {
    use fontelle_ui::document::GhostFilter;

    let dir = a_bank("ghost-filter");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    session.add_channel_with(0).unwrap();
    session.add_channel_with(0).unwrap();

    let a_note = |start: PPQNTick, key: u8| RollEdit::Add {
        note: Note {
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
        },
    };
    for (channel, key) in [(0usize, 60u8), (1, 64), (2, 67)] {
        session.select_channel(channel);
        session.edit(a_note(0, key));
    }

    session.select_channel(2);
    let all = session.ghost_notes(GhostFilter::All);
    assert_eq!(all.len(), 2, "both of the others: {all:?}");

    let one = session.ghost_notes(GhostFilter::Channel(0));
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].key, 60, "only the instrument that was asked for");

    assert!(
        session.ghost_notes(GhostFilter::Channel(2)).is_empty(),
        "the channel you are editing is never its own ghost"
    );
    assert!(session.ghost_notes(GhostFilter::Channel(9)).is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_ghost_carries_the_colour_of_the_instrument_it_came_from() {
    use fontelle_ui::document::GhostFilter;

    let dir = a_bank("ghost-colour");
    let (mut session, _source) = studio(&dir);
    session.open_file(0).unwrap();
    session.set_channel_instrument(0).unwrap();
    session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    session.add_channel_with(0).unwrap();

    let ghosts = session.ghost_notes(GhostFilter::All);
    assert_eq!(ghosts.len(), 1);
    assert_ne!(
        ghosts[0].color,
        [0, 0, 0, 0],
        "a ghost with no colour cannot be told from any other ghost"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------- the key map ---

/// A bank folder holding one real drum kit — zones one key wide, with gaps.
fn a_kit_bank(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(dir.join("Test Kit.sf2"), build_drum_kit_sf2(KIT))
        .expect("the fixture must be writable");
    dir
}

/// The whole point of the key map, end to end: pick a drum kit in the browser
/// and the roll is told which four rows do anything and what each one is.
/// Everything under this has its own test; this is the one that proves the
/// wiring between them, which is where a feature like this actually breaks.
#[test]
fn a_drum_kit_on_a_channel_names_its_hits_and_greys_the_rest() {
    let dir = a_kit_bank("kit-map");
    let (mut session, _graph) = studio(&dir);
    session.open_file(0).unwrap();
    session
        .set_channel_instrument(0)
        .expect("the kit must load");

    let map = session.key_map();
    assert!(map.is_known(), "there is an instrument on the channel");
    assert!(map.is_named(), "and it is a kit, so its keys have names");

    for (name, key) in KIT {
        assert!(map.plays(*key), "{name} sits on key {key}");
        assert_eq!(map.name(*key), Some(*name));
    }
    for key in [35, 37, 39, 60, 127] {
        assert!(
            !map.plays(key),
            "key {key} is not in the kit and has to be greyed"
        );
    }
}

/// A channel with nothing on it is not a channel that plays nothing.
#[test]
fn a_channel_with_no_instrument_reports_an_unknown_key_map() {
    let dir = a_bank("no-instrument-map");
    let (session, _graph) = studio(&dir);

    let map = session.key_map();
    assert!(!map.is_known());
    assert!(map.plays(60), "nothing known greys nothing");
}

// ------------------------------------------------ the arrangement clipboard ---

/// The clips are the **host's** to hold, because a canvas may not see one
/// (INVARIANT 2) — so this is where copy and paste actually happen.
#[test]
fn copying_a_clip_and_pasting_it_puts_a_second_one_in_the_document() {
    let dir = a_bank("clip-copy");
    let (mut session, _graph) = studio(&dir);
    let first = Session::first_clip(session.project()).expect("the blank project has a clip");
    let before = session.project().clips.len();

    session.arrange(ArrangeEdit::Copy(vec![first]));
    session.arrange(ArrangeEdit::Paste { at: PPQN * 8 });

    assert_eq!(
        session.project().clips.len(),
        before + 1,
        "a paste puts a clip in the document"
    );
    let pasted = session
        .project()
        .clips
        .values()
        .find(|c| c.start == PPQN * 8)
        .expect("the pasted clip is where it was asked for");
    let source = session.project().clips.get(first).expect("the source");
    assert_eq!(pasted.lane, source.lane, "and on the lane it came from");
    assert_eq!(pasted.length, source.length);

    std::fs::remove_dir_all(&dir).ok();
}

/// The case a clipboard of ids could not have handled, and the reason the
/// host holds real clips: paste has to work after the thing it copied from
/// has been deleted.
#[test]
fn a_cut_clip_can_still_be_pasted_back() {
    let dir = a_bank("clip-cut");
    let (mut session, _graph) = studio(&dir);
    let first = Session::first_clip(session.project()).expect("a clip");
    let length = session.project().clips.get(first).expect("it").length;

    session.arrange(ArrangeEdit::Copy(vec![first]));
    session.arrange(ArrangeEdit::Remove(vec![first]));
    assert!(
        session.project().clips.get(first).is_none(),
        "the cut clip is gone"
    );

    session.arrange(ArrangeEdit::Paste { at: PPQN * 4 });
    let back = session
        .project()
        .clips
        .values()
        .find(|c| c.start == PPQN * 4)
        .expect("what was cut comes back");
    assert_eq!(back.length, length);

    std::fs::remove_dir_all(&dir).ok();
}

/// Pasting the same thing twice puts two of it down — a clipboard is not
/// consumed by being used, which is what makes "paste, move, paste" work.
#[test]
fn the_clip_clipboard_survives_being_pasted() {
    let dir = a_bank("clip-repaste");
    let (mut session, _graph) = studio(&dir);
    let first = Session::first_clip(session.project()).expect("a clip");
    let before = session.project().clips.len();

    session.arrange(ArrangeEdit::Copy(vec![first]));
    assert_eq!(session.clip_clipboard_len(), 1, "one clip is held");
    session.arrange(ArrangeEdit::Paste { at: PPQN * 8 });
    session.arrange(ArrangeEdit::Paste { at: PPQN * 16 });

    assert_eq!(session.project().clips.len(), before + 2);
    assert_eq!(session.clip_clipboard_len(), 1, "and still held afterwards");

    std::fs::remove_dir_all(&dir).ok();
}

/// A paste with nothing copied is not an error and not a clip — it is
/// nothing, the same way `Ctrl+V` on an empty clipboard is nothing everywhere
/// else.
#[test]
fn pasting_an_empty_clipboard_changes_nothing() {
    let dir = a_bank("clip-empty-paste");
    let (mut session, _graph) = studio(&dir);
    let before = session.project().clips.len();

    session.arrange(ArrangeEdit::Paste { at: PPQN * 8 });

    assert_eq!(session.project().clips.len(), before);
    assert_eq!(session.clip_clipboard_len(), 0);

    std::fs::remove_dir_all(&dir).ok();
}

/// Several clips keep their shape relative to one another: copying a two-bar
/// pattern of two clips and pasting it puts the pair down, still two bars
/// apart, with the *earliest* landing on the paste point.
#[test]
fn a_multi_clip_copy_keeps_its_shape() {
    let dir = a_bank("clip-shape");
    let (mut session, _graph) = studio(&dir);
    let first = Session::first_clip(session.project()).expect("a clip");

    // A second clip four bars along, so there is a shape to keep.
    session.arrange(ArrangeEdit::Duplicate {
        ids: vec![first],
        tick_offset: PPQN * 16,
    });
    let ids: Vec<_> = session.project().clips.keys().collect();
    assert_eq!(ids.len(), 2);

    session.arrange(ArrangeEdit::Copy(ids));
    session.arrange(ArrangeEdit::Paste { at: PPQN * 32 });

    let mut starts: Vec<_> = session
        .project()
        .clips
        .values()
        .map(|c| c.start)
        .filter(|s| *s >= PPQN * 32)
        .collect();
    starts.sort_unstable();
    assert_eq!(
        starts,
        vec![PPQN * 32, PPQN * 48],
        "the pair keeps its sixteen-beat gap, earliest on the paste point"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The contract that makes "repeat" walk along the arrangement instead of
/// stacking copies in one place: the host says what it created, and the
/// canvas selects it (see `Timeline::clips_inserted`).
#[test]
fn duplicating_and_pasting_report_the_clips_they_made() {
    let dir = a_bank("clip-ids");
    let (mut session, _graph) = studio(&dir);
    let first = Session::first_clip(session.project()).expect("a clip");

    let made = session.arrange(ArrangeEdit::Duplicate {
        ids: vec![first],
        tick_offset: PPQN * 16,
    });
    assert_eq!(made.len(), 1, "a duplicate makes one clip and says which");
    assert_ne!(made[0], first, "and it is not the one it copied");
    assert!(session.project().clips.get(made[0]).is_some());

    session.arrange(ArrangeEdit::Copy(vec![first]));
    let pasted = session.arrange(ArrangeEdit::Paste { at: PPQN * 32 });
    assert_eq!(pasted.len(), 1, "a paste reports what it put down");
    assert_eq!(
        session.project().clips.get(pasted[0]).map(|c| c.start),
        Some(PPQN * 32)
    );

    // The edits that create nothing say so.
    assert!(session.arrange(ArrangeEdit::Copy(vec![first])).is_empty());
    assert!(
        session
            .arrange(ArrangeEdit::SetMuted {
                ids: vec![first],
                muted: true
            })
            .is_empty()
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------- making clips, and cutting notes ---

#[test]
fn drawing_on_the_arrangement_makes_a_real_clip_on_that_lane() {
    // The report this answers: *"it's way too difficult to just make a new
    // clip in the arrangement — I can't even figure out how."* It was not
    // difficult, it was impossible: every press on empty grid started a
    // marquee.
    let dir = a_bank("draw-clip");
    let (mut session, _source) = studio(&dir);
    let before = session.clips().len();

    let made = session.arrange(ArrangeEdit::Add {
        lane: 0,
        start: PPQN * 8,
    });

    assert_eq!(made.len(), 1, "one clip, and its id came back");
    let clips = session.clips();
    assert_eq!(clips.len(), before + 1);
    let clip = clips
        .iter()
        .find(|c| c.id == made[0])
        .expect("the new clip is in the list");
    assert_eq!(clip.start, PPQN * 8);
    assert_eq!(clip.lane, 0);
    assert!(clip.length > 0);
    assert!(
        clip.open,
        "a clip you just drew opens in the roll — you made it to put notes in"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_drawn_clip_plays_whatever_else_is_on_that_lane() {
    // What a lane *means* to somebody looking at it. A second clip on the drum
    // lane that played the piano would be a trap.
    let dir = a_bank("draw-channel");
    let (mut session, _source) = studio(&dir);
    let existing = session.clips()[0].name.clone();

    let made = session.arrange(ArrangeEdit::Add {
        lane: 0,
        start: PPQN * 16,
    });
    let clips = session.clips();
    let drawn = clips
        .iter()
        .find(|c| c.id == made[0])
        .expect("the new clip");
    assert_eq!(
        drawn.name, existing,
        "the block is captioned with the channel it plays"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn drawing_a_clip_is_undoable_like_every_other_edit() {
    let dir = a_bank("draw-undo");
    let (mut session, _source) = studio(&dir);
    let before = session.clips().len();

    session.arrange(ArrangeEdit::Add {
        lane: 0,
        start: PPQN * 8,
    });
    assert_eq!(session.clips().len(), before + 1);

    session.undo();
    assert_eq!(session.clips().len(), before, "one Ctrl+Z takes it back");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cutting_a_note_makes_two_that_meet_where_it_was_cut() {
    // The cut tool, all the way through: the roll hands the session a tick per
    // note and the document ends up with two notes and one undo entry.
    let dir = a_bank("cut");
    let (mut session, _source) = studio(&dir);

    let ids = session.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN * 4,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    assert_eq!(ids.len(), 1);

    session.edit(RollEdit::Slice {
        cuts: vec![(ids[0], PPQN)],
    });

    let mut spans: Vec<(i64, i64)> = session
        .notes()
        .values()
        .map(|n| (n.start, n.length))
        .collect();
    spans.sort();
    assert_eq!(spans, vec![(0, PPQN), (PPQN, PPQN * 3)]);

    session.undo();
    let spans: Vec<(i64, i64)> = session
        .notes()
        .values()
        .map(|n| (n.start, n.length))
        .collect();
    assert_eq!(spans, vec![(0, PPQN * 4)], "one undo, one note back");

    std::fs::remove_dir_all(&dir).ok();
}
