//! Importing MIDI files and FL Studio scores, through the studio the window
//! drives.
//!
//! The other two halves are tested where they live — the file formats in
//! `fontelle-assets`, the command in `fontelle-model`, the panel geometry in
//! `fontelle-ui`. This is the seam: a folder somebody configured, a list of
//! rows, a click on one, and a document that changed.
//!
//! Everything is real except the mouse: real files on disk, the real
//! `StudioHost` the window talks through, and the real commands underneath.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::{FscNoteSpec, build_fsc};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::ClipSource;
use fontelle_types::{CompiledTimeline, FolderKind, PPQN};
use fontelle_ui::canvas::BrowserMode;
use fontelle_ui::document::{DocumentHost, LibraryKind, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-import-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn a_session(dir: &Path) -> Session {
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
    // Never the developer's own `~/.config/fontelle`.
    .with_settings_path(dir.join("settings.json"))
}

// -------------------------------------------------------- building files ---

fn varint(mut value: u32, out: &mut Vec<u8>) {
    let mut buffer = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        buffer.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    buffer.reverse();
    out.extend_from_slice(&buffer);
}

/// A format-1 `.mid`, one track per named part.
fn build_midi(parts: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(parts.len() as u16).to_be_bytes());
    out.extend_from_slice(&96u16.to_be_bytes());

    for (name, channel, keys) in parts {
        let mut track = Vec::new();
        varint(0, &mut track);
        track.extend_from_slice(&[0xff, 0x03]);
        varint(name.len() as u32, &mut track);
        track.extend_from_slice(name.as_bytes());
        for key in *keys {
            varint(0, &mut track);
            track.extend_from_slice(&[0x90 | channel, *key, 100]);
            varint(48, &mut track);
            track.extend_from_slice(&[0x80 | channel, *key, 64]);
        }
        varint(0, &mut track);
        track.extend_from_slice(&[0xff, 0x2f, 0x00]);

        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);
    }
    out
}

/// Points the session's MIDI folder at `dir` and reads it.
fn use_folder(session: &mut Session, kind: FolderKind, dir: &Path) {
    session.set_import_folder(kind, Some(dir.to_path_buf()));
    session.set_import_kind(kind);
    session.set_browser_mode(BrowserMode::Import);
}

fn row_named(session: &Session, name: &str) -> usize {
    session
        .import_files()
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no row called {name} \u{2014} the list is {:?}",
                session
                    .import_files()
                    .iter()
                    .map(|e| e.name.clone())
                    .collect::<Vec<_>>()
            )
        })
}

fn channel_names(session: &Session) -> Vec<String> {
    let mut names: Vec<String> = session
        .channels()
        .iter()
        .map(|channel| channel.name.clone())
        .collect();
    names.sort();
    names
}

// --------------------------------------------------- no folder, no browser ---

#[test]
fn with_no_folder_set_there_is_nothing_to_browse_and_it_says_so() {
    // INVARIANT 10: Fontelle reads nothing the user has not named, so there
    // is no folder to fall back on. The window's answer to this is to send
    // you to the settings — see `WindowApp::open_import_browser`.
    let dir = scratch("no-folder");
    let session = a_session(&dir);
    for kind in FolderKind::ALL {
        assert!(!session.has_import_dir(kind), "{kind:?} invented a folder");
    }
    assert!(session.import_files().is_empty());
    assert!(
        session.import_status().to_lowercase().contains("settings"),
        "the status should say where to go: {}",
        session.import_status()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_folder_already_in_the_settings_file_is_read_without_anything_being_changed() {
    // The fresh-launch path, and the one every other test here bypasses by
    // going through `set_import_folder`. On a real launch the settings are
    // read at construction and *nothing changes afterwards*: the kind is
    // set to MIDI, the folder is already set, and the Import tab is simply
    // opened. The version of this that rescanned "when the kind changed"
    // browsed a bank that had never been read and reported "no sf2 files"
    // over a folder full of MIDI.
    let dir = scratch("already-configured");
    std::fs::write(dir.join("Ready.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();
    // Written before the session exists, exactly as a real settings file is.
    let mut settings = fontelle_app::settings::Settings::default();
    settings.set_folder(FolderKind::Midi, Some(dir.clone()));
    settings
        .save_to(&dir.join("settings.json"))
        .expect("the settings must be writable");

    let mut session = a_session(&dir);
    // The only thing the window does: open the tab, and pick MIDI (the tab
    // opens on Audio now — see the launch test below).
    session.set_browser_mode(BrowserMode::Import);
    session.set_import_kind(FolderKind::Midi);

    let names: Vec<String> = session
        .import_files()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, vec!["Ready"], "the configured folder was never read");
    assert!(
        !session.import_status().contains("sf2"),
        "it is describing the soundfont bank: {}",
        session.import_status()
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- the lists ---

#[test]
fn the_import_list_shows_the_files_of_the_kind_it_is_on_and_no_others() {
    let dir = scratch("mixed");
    std::fs::write(dir.join("Song.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();
    std::fs::write(
        dir.join("Chords.fsc"),
        build_fsc("20.9.0", 96, &[a_fsc_note(0, 60)]),
    )
    .unwrap();
    std::fs::write(dir.join("Notes.txt"), b"not a music file").unwrap();

    let mut session = a_session(&dir);
    // Both folders at the same place, which is the case that makes the
    // filtering load-bearing: the same directory holds all three files, and
    // each tab must show only its own.
    session.set_import_folder(FolderKind::Scores, Some(dir.clone()));
    use_folder(&mut session, FolderKind::Midi, &dir);
    let names: Vec<String> = session
        .import_files()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, vec!["Song"], "only the .mid, and by its stem");

    session.set_import_kind(FolderKind::Scores);
    let names: Vec<String> = session
        .import_files()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, vec!["Chords"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn switching_to_a_kind_with_no_folder_says_so_rather_than_showing_the_other_ones_files() {
    // The two folders are two folders. Falling back to the one that *is* set
    // would list `.mid` files under a tab that says Scores.
    let dir = scratch("one-of-two-folders");
    std::fs::write(dir.join("Song.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();
    std::fs::write(
        dir.join("Chords.fsc"),
        build_fsc("20.9.0", 96, &[a_fsc_note(0, 60)]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    assert_eq!(session.import_files().len(), 1);

    session.set_import_kind(FolderKind::Scores);
    assert!(
        session.import_files().is_empty(),
        "it borrowed the other folder"
    );
    assert!(
        session.import_status().to_lowercase().contains("settings"),
        "got {:?}",
        session.import_status()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_import_list_walks_into_subdirectories() {
    // *"should work cleanly with subdirectories"* — a collection organised as
    // `Drums/Fills/` is organised that way on purpose.
    let dir = scratch("nested");
    std::fs::create_dir_all(dir.join("Drums/Fills")).unwrap();
    std::fs::write(
        dir.join("Drums/Fills/Roll.mid"),
        build_midi(&[("Kit", 9, &[36])]),
    )
    .unwrap();
    std::fs::write(dir.join("Top.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);

    // The folder row says how much is under it, which is the one number worth
    // showing: an empty folder looks exactly like a full one until you click.
    let drums = session
        .import_files()
        .into_iter()
        .find(|entry| entry.name == "Drums")
        .expect("the subfolder is listed");
    assert_eq!(drums.kind, LibraryKind::Folder);
    assert!(drums.detail.contains('1'), "got {:?}", drums.detail);

    // Into it, and into the one inside that.
    session.open_import(row_named(&session, "Drums")).unwrap();
    session.open_import(row_named(&session, "Fills")).unwrap();
    let names: Vec<String> = session
        .import_files()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert!(names.contains(&"Roll".to_string()), "got {names:?}");
    // And back out, which is the row that makes a tree navigable rather than
    // a one-way trip.
    assert!(
        names.contains(&"..".to_string()),
        "no way back up: {names:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_search_looks_across_the_whole_folder_tree() {
    let dir = scratch("search");
    std::fs::create_dir_all(dir.join("Deep/Down")).unwrap();
    std::fs::write(
        dir.join("Deep/Down/Fanfare.mid"),
        build_midi(&[("Brass", 0, &[60])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.set_query("fanf");
    let names: Vec<String> = session
        .import_files()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, vec!["Fanfare"], "found from the top of the tree");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_import_search_is_not_the_soundfont_search() {
    // Two lists, two boxes. A query typed against MIDI files means nothing
    // against soundfonts, and one shared string would carry each into the
    // other every time the tab changed.
    let dir = scratch("two-queries");
    let mut session = a_session(&dir);
    session.set_browser_mode(BrowserMode::Sounds);
    session.set_query("piano");
    session.set_browser_mode(BrowserMode::Import);
    assert_eq!(session.query(), "", "the import box started empty");
    session.set_query("drums");
    session.set_browser_mode(BrowserMode::Sounds);
    assert_eq!(
        session.query(),
        "piano",
        "and the soundfont box kept its own"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_row_of_the_import_list_is_shown_as_the_open_one() {
    // Nothing is "open" here: these are files you act on rather than a file
    // you are looking inside. Answering with the soundfont's index would
    // light whichever import row happened to sit at that position.
    let dir = scratch("no-selection");
    std::fs::write(dir.join("A.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();
    std::fs::write(dir.join("B.mid"), build_midi(&[("Lead", 0, &[60])])).unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    assert_eq!(session.selected_file(), None);
    session.open_import(row_named(&session, "A")).unwrap();
    assert_eq!(session.selected_file(), None, "importing is not selecting");
    std::fs::remove_dir_all(&dir).ok();
}

// -------------------------------------------------------- importing MIDI ---

#[test]
fn a_midi_file_of_one_part_imports_without_asking_anything() {
    // A prompt with a single answer is a click somebody has to make to get
    // what they already asked for.
    let dir = scratch("one-part");
    std::fs::write(
        dir.join("Line.mid"),
        build_midi(&[("Fretless Bass", 0, &[36, 38, 40])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Line")).unwrap();

    assert!(session.import_prompt().is_none(), "it asked a question");
    assert!(
        channel_names(&session).contains(&"Fretless Bass".to_string()),
        "got {:?}",
        channel_names(&session)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_midi_file_of_several_parts_asks_before_it_does_anything() {
    // *"if I import a midi with multiple instruments inside that midi song it
    // will prompt me if I want to import them all as separate (named)
    // tracks/instruments or only import a single instrument"*.
    let dir = scratch("several");
    std::fs::write(
        dir.join("Song.mid"),
        build_midi(&[("Bass", 0, &[36]), ("Lead", 1, &[72]), ("Pad", 2, &[60])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    let channels_before = session.channels().len();
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Song")).unwrap();

    let prompt = session.import_prompt().expect("it should have asked");
    assert!(prompt.title.contains("Song.mid"), "got {:?}", prompt.title);
    // One line for "all of them", then one per part, each named and counted.
    assert_eq!(prompt.choices.len(), 4);
    assert!(prompt.choices[0].to_lowercase().contains("all"));
    for name in ["Bass", "Lead", "Pad"] {
        assert!(
            prompt.choices.iter().any(|line| line.contains(name)),
            "no line offers {name}: {:?}",
            prompt.choices
        );
    }
    assert_eq!(
        session.channels().len(),
        channels_before,
        "nothing was imported before the question was answered"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn answering_all_brings_every_part_in_under_its_own_name() {
    let dir = scratch("all-parts");
    std::fs::write(
        dir.join("Song.mid"),
        build_midi(&[("Bass", 0, &[36]), ("Lead", 1, &[72])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    // Whatever the document started with — a blank project's row count is a
    // default that has moved once already (`fontelle_app::STARTING_LANES`) and
    // is not what this test is about. The invariant is **a row per part**.
    let lanes_before = session.lanes().len();
    session.open_import(row_named(&session, "Song")).unwrap();
    session.answer_import(0);

    let names = channel_names(&session);
    assert!(names.contains(&"Bass".to_string()), "got {names:?}");
    assert!(names.contains(&"Lead".to_string()), "got {names:?}");
    assert!(
        session.import_prompt().is_none(),
        "the question is answered"
    );
    assert_eq!(
        session.lanes().len(),
        lanes_before + 2,
        "a row each for Bass and Lead, on top of what was there"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn answering_with_one_part_brings_in_only_that_part() {
    let dir = scratch("one-of-three");
    std::fs::write(
        dir.join("Song.mid"),
        build_midi(&[("Bass", 0, &[36]), ("Lead", 1, &[72]), ("Pad", 2, &[60])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Song")).unwrap();
    // Choice 0 is "all"; the parts start at 1, and the prompt lists them in
    // channel order.
    session.answer_import(2);

    let names = channel_names(&session);
    assert!(names.contains(&"Lead".to_string()), "got {names:?}");
    assert!(!names.contains(&"Bass".to_string()), "got {names:?}");
    assert!(!names.contains(&"Pad".to_string()), "got {names:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_question_dropped_imports_nothing() {
    let dir = scratch("cancelled");
    std::fs::write(
        dir.join("Song.mid"),
        build_midi(&[("Bass", 0, &[36]), ("Lead", 1, &[72])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    let before = session.channels().len();
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Song")).unwrap();
    session.cancel_import();

    assert!(session.import_prompt().is_none());
    assert_eq!(session.channels().len(), before);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn undoing_an_import_takes_the_whole_file_back_out_in_one_press() {
    let dir = scratch("undo");
    std::fs::write(
        dir.join("Song.mid"),
        build_midi(&[("Bass", 0, &[36]), ("Lead", 1, &[72])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    let before = channel_names(&session);
    let lanes_before = session.lanes().len();
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Song")).unwrap();
    session.answer_import(0);
    assert_ne!(channel_names(&session), before);

    session.undo();
    assert_eq!(
        channel_names(&session),
        before,
        "one press took it all back"
    );
    assert_eq!(session.lanes().len(), lanes_before);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------ importing scores ---

fn a_fsc_note(position: u32, key: u8) -> FscNoteSpec {
    FscNoteSpec {
        position,
        key,
        ..Default::default()
    }
}

#[test]
fn a_score_lands_in_the_clip_that_is_open_rather_than_as_a_track() {
    // A score is a *phrase* — no instrument, no tempo, no arrangement — so it
    // goes in the roll. That is what FL's own "import score" does with one,
    // and it is the difference between the two formats rather than an
    // inconsistency between them.
    let dir = scratch("score");
    let bytes = build_fsc(
        "20.9.0",
        96,
        &[a_fsc_note(0, 60), a_fsc_note(48, 64), a_fsc_note(96, 67)],
    );
    std::fs::write(dir.join("Triad.fsc"), bytes).unwrap();

    let mut session = a_session(&dir);
    let channels_before = session.channels().len();
    let notes_before = session.notes().len();
    use_folder(&mut session, FolderKind::Scores, &dir);
    session.open_import(row_named(&session, "Triad")).unwrap();

    assert_eq!(
        session.notes().len(),
        notes_before + 3,
        "the notes went into the open clip"
    );
    assert_eq!(
        session.channels().len(),
        channels_before,
        "and it made no instrument of its own"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_imported_scores_notes_arrive_with_the_pitches_and_timing_it_wrote() {
    let dir = scratch("score-values");
    let bytes = build_fsc("20.9.0", 96, &[a_fsc_note(0, 60), a_fsc_note(96, 72)]);
    std::fs::write(dir.join("Two.fsc"), bytes).unwrap();

    let mut session = a_session(&dir);
    let before: Vec<u8> = session.notes().values().map(|note| note.key).collect();
    use_folder(&mut session, FolderKind::Scores, &dir);
    session.open_import(row_named(&session, "Two")).unwrap();

    let mut arrived: Vec<(i64, u8)> = session
        .notes()
        .values()
        .filter(|note| !before.contains(&note.key))
        .map(|note| (note.start, note.key))
        .collect();
    arrived.sort();
    assert_eq!(arrived, vec![(0, 60), (PPQN, 72)]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn undoing_an_imported_score_takes_its_notes_back_out() {
    let dir = scratch("score-undo");
    let bytes = build_fsc("20.9.0", 96, &[a_fsc_note(0, 60), a_fsc_note(48, 64)]);
    std::fs::write(dir.join("Pair.fsc"), bytes).unwrap();

    let mut session = a_session(&dir);
    let before = session.notes().len();
    use_folder(&mut session, FolderKind::Scores, &dir);
    session.open_import(row_named(&session, "Pair")).unwrap();
    assert_eq!(session.notes().len(), before + 2);

    session.undo();
    assert_eq!(session.notes().len(), before, "one press took both back");
    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------------- dropped files ---

#[test]
fn a_dropped_midi_file_imports_the_way_a_clicked_one_does() {
    let dir = scratch("drop-midi");
    let path = dir.join("Dropped.mid");
    std::fs::write(&path, build_midi(&[("Strings", 0, &[60, 64])])).unwrap();

    let mut session = a_session(&dir);
    // No folder configured at all: a drop is a file you handed over, not one
    // Fontelle went looking for.
    assert!(!session.has_import_dir(FolderKind::Midi));
    let message = session.drop_file(&path).expect("it imports");
    assert!(message.contains("Strings"), "got {message:?}");
    assert!(channel_names(&session).contains(&"Strings".to_string()));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_score_goes_into_the_open_clip() {
    let dir = scratch("drop-score");
    let path = dir.join("Dropped.fsc");
    std::fs::write(&path, build_fsc("20.9.0", 96, &[a_fsc_note(0, 60)])).unwrap();

    let mut session = a_session(&dir);
    let before = session.notes().len();
    session.drop_file(&path).expect("it imports");
    assert_eq!(session.notes().len(), before + 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_midi_of_several_parts_asks_the_same_question() {
    let dir = scratch("drop-several");
    let path = dir.join("Song.mid");
    std::fs::write(&path, build_midi(&[("A", 0, &[60]), ("B", 1, &[62])])).unwrap();

    let mut session = a_session(&dir);
    let message = session.drop_file(&path).expect("it is a file we can read");
    assert!(message.is_empty(), "nothing to announce yet: {message:?}");
    assert!(session.import_prompt().is_some(), "it should have asked");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_file_of_a_kind_fontelle_does_not_read_says_what_it_does_read() {
    let dir = scratch("drop-junk");
    let path = dir.join("Notes.txt");
    std::fs::write(&path, b"not a music file").unwrap();

    let mut session = a_session(&dir);
    let error = session.drop_file(&path).expect_err("it must refuse");
    for extension in [".mid", ".fsc", ".sf2"] {
        assert!(error.contains(extension), "got {error:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_file_that_is_not_there_says_so_rather_than_failing_obscurely() {
    let dir = scratch("drop-missing");
    let mut session = a_session(&dir);
    let error = session
        .drop_file(&dir.join("Gone.mid"))
        .expect_err("it must refuse");
    assert!(error.contains("Gone.mid"), "got {error:?}");
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ the tempo ---

#[test]
fn importing_into_a_song_that_has_something_in_it_leaves_its_tempo_alone() {
    // A file's tempo is right for the file and wrong for the piece you are
    // working on. Changing the song's tempo is an edit nobody asked for — but
    // it is worth *saying*, so the difference is not a mystery.
    let dir = scratch("tempo");
    std::fs::write(dir.join("Fast.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();

    let mut session = a_session(&dir);
    session.set_tempo(140.0);
    let before = session.tempo();
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Fast")).unwrap();

    assert!(
        (session.tempo() - before).abs() < 1e-9,
        "the song's tempo moved to {}",
        session.tempo()
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------- what it says ---

#[test]
fn the_status_line_names_the_folder_you_are_standing_in() {
    let dir = scratch("status");
    std::fs::write(dir.join("One.mid"), build_midi(&[("Bass", 0, &[36])])).unwrap();
    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    let status = session.import_status();
    assert!(status.contains("1 file"), "got {status:?}");
}

#[test]
fn an_empty_folder_says_it_is_empty_rather_than_looking_broken() {
    let dir = scratch("empty");
    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    let status = session.import_status();
    assert!(status.contains("no mid"), "got {status:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_clip_of_an_imported_part_holds_that_parts_notes() {
    let dir = scratch("clip-notes");
    std::fs::write(
        dir.join("Song.mid"),
        build_midi(&[("Bass", 0, &[36, 38]), ("Lead", 1, &[72])]),
    )
    .unwrap();

    let mut session = a_session(&dir);
    use_folder(&mut session, FolderKind::Midi, &dir);
    session.open_import(row_named(&session, "Song")).unwrap();
    session.answer_import(0);

    // The roll opens on what just arrived, which is what somebody who pressed
    // "import" is looking for — and its clip holds the notes of one part
    // rather than of both.
    let counts: Vec<usize> = session
        .project()
        .clips
        .values()
        .filter_map(|clip| match &clip.source {
            ClipSource::Notes(data) if !data.notes.is_empty() => Some(data.notes.len()),
            _ => None,
        })
        .collect();
    assert!(counts.contains(&2), "the bass's two notes: {counts:?}");
    assert!(counts.contains(&1), "the lead's one note: {counts:?}");
    std::fs::remove_dir_all(&dir).ok();
}

// A `.mid` with no row of its own arrives where the window is looking, the
// way a sound does (`audio_import.rs`, "where the row turns up").
#[test]
fn a_midi_file_arrives_on_the_row_the_window_is_looking_at() {
    let dir = scratch("arrival-midi");
    let mut session = a_session(&dir);
    let path = dir.join("Song.mid");
    std::fs::write(
        &path,
        build_midi(&[("Strings", 0, &[60, 64]), ("Brass", 1, &[48, 50])]),
    )
    .unwrap();
    for name in ["Drums", "Bass", "Keys", "Vox"] {
        session.add_lane();
        let last = session.lanes().len() - 1;
        session.rename_lane(last, name);
    }
    let before: Vec<String> = session.lanes().into_iter().map(|lane| lane.name).collect();
    session.set_arrival_row(3);
    session.drop_file(&path).expect("a one-question file");
    // Two parts: all of them.
    session.answer_import(0);
    let after: Vec<String> = session.lanes().into_iter().map(|lane| lane.name).collect();
    assert_eq!(
        &after[3..5],
        &["Strings".to_string(), "Brass".to_string()],
        "the parts are a block at the arrival index: {after:?}"
    );
    assert_eq!(
        &after[5..],
        &before[3..],
        "and the rows under it moved down"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------- what a launch reads ---
//
// > *"make your audio files load on startup instead of being when you open
// > the import audio section ... we should also make it start out opened
// > on the audio import tab by default instead of the soundfonts tab"*
//
// The Import tab is where a session starts, on Audio, and the folder it
// shows is read when the settings are — the same rule the plugin scan
// follows (`Session::scan_plugins`): startup is where a wait is expected,
// and a tab that fills in a beat after you look at it is not.

#[test]
fn a_launch_reads_the_audio_folder_before_anybody_opens_the_tab() {
    let dir = scratch("launch-audio");
    std::fs::write(
        dir.join("Kick.wav"),
        fontelle_assets::fixtures::build_wav(48_000, 1, &[0.0; 4800]),
    )
    .unwrap();
    let mut settings = fontelle_app::settings::Settings::default();
    settings.set_folder(FolderKind::Audio, Some(dir.clone()));
    settings
        .save_to(&dir.join("settings.json"))
        .expect("the settings must be writable");

    let session = a_session(&dir);
    assert_eq!(
        session.import_kind(),
        FolderKind::Audio,
        "the tab starts on sounds"
    );
    let names: Vec<String> = session
        .import_files()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(
        names,
        vec!["Kick"],
        "the folder was read at launch, with no tab opened and no kind picked"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A folder of sounds counts them as "sounds": the one noun in the browser
/// that is a word rather than an extension takes its plural — "675 sound"
/// read as a clipped column.
#[test]
fn a_folder_of_sounds_counts_them_in_words() {
    use fontelle_app::bank::BankFilter;
    assert_eq!(BankFilter::Files(FolderKind::Audio).count(1), "1 sound");
    assert_eq!(
        BankFilter::Files(FolderKind::Audio).count(675),
        "675 sounds"
    );
    assert_eq!(BankFilter::Files(FolderKind::Midi).count(2), "2 mid");
}
