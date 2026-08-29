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
    GEN_KEY_RANGE, GEN_OVERRIDING_ROOT_KEY, GEN_SAMPLE_MODES, Sf2Fixture, ZoneSpec, build_sf2,
    gen_range, gen_val,
};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::ClipSource;
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::RollEdit;
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
    let project = blank_project(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline_source) = timeline_channel(CompiledTimeline::empty());

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
    .with_graphs(graphs)
    // Never the developer's own `~/.config/fontelle`: `open_bank` writes the
    // folders it settled on, and a test that scribbles a `/tmp` path into a
    // real config file is a test that breaks the machine it ran on. It did.
    .with_settings_path(dir.join("settings.json"));
    session.add_soundfont_dir(dir);
    session.open_bank();
    (session, source)
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
        None,
        "the open file was named by its place in the filtered list, and the \
         filter just moved under it"
    );

    session.set_query("zzzz");
    assert!(session.library_files().is_empty());

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
        tick: 0,
        key: 60,
        length: PPQN,
        velocity: 100,
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
        tick: 0,
        key: 60,
        length: PPQN,
        velocity: 100,
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

    session.toggle_mute(0);
    assert!(session.channels()[0].muted);
    assert!(
        source.take_update(),
        "a mute lives on the fader node, so it is the *graph* that has to be \
         republished, not the timeline"
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
        tick: 0,
        key: 60,
        length: PPQN / 4,
        velocity: 100,
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
        tick: 0,
        key: 60,
        length: PPQN,
        velocity: 100,
    });

    session.edit(RollEdit::SetVelocity {
        ids: ids.clone(),
        velocity: 42,
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

    session.audition_on(64, 90);
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
