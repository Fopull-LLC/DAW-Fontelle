//! Bringing a file's parts into the project that is already open.
//!
//! `fontelle_assets::import_midi` builds a **whole new `Project`**, which is
//! the right shape for opening a `.mid` as a song and the wrong one for the
//! thing people actually do: dropping a file onto a piece they are working on.
//! This is that — one command, so importing eight tracks is one entry in the
//! history and one press of Ctrl+Z takes all of it back.
//!
//! One command rather than a `Compound` of `AddChannel`/`AddLane`/`AddClip`
//! because those cannot be built in advance: the clip has to name the channel
//! id that the `AddChannel` beside it is going to mint, and a `Compound` holds
//! commands that were made before any of them ran.

use fontelle_model::{
    Command, History, ImportPart, ImportParts, Note, Project, TempoMap,
};
use fontelle_types::PPQN;

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
    }
}

fn a_part(name: &str, keys: &[u8]) -> ImportPart {
    ImportPart {
        name: name.to_string(),
        notes: keys
            .iter()
            .enumerate()
            .map(|(i, key)| a_note(i as i64 * PPQN, *key))
            .collect(),
        pan: 0.0,
        volume_db: 0.0,
        color: [0x4f, 0x8f, 0xd0, 0xff],
    }
}

fn project() -> Project {
    let mut project = Project::new("open song");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    project
}

fn snapshot(project: &Project) -> serde_json::Value {
    serde_json::to_value(project).expect("a project must serialise")
}

#[test]
fn every_part_arrives_as_its_own_instrument_row_and_clip() {
    let mut doc = project();
    let channels_before = doc.channels.len();
    let lanes_before = doc.lanes.len();

    let mut command = ImportParts::new(
        "Import Song.mid",
        vec![a_part("Bass", &[36, 38]), a_part("Lead", &[72])],
    );
    command.apply(&mut doc).expect("applies");

    assert_eq!(doc.channels.len(), channels_before + 2);
    assert_eq!(doc.lanes.len(), lanes_before + 2);
    assert_eq!(doc.clips.len(), 2);
}

#[test]
fn each_one_is_called_what_the_file_called_it() {
    // The whole point of reading the track names: *"import them all as
    // separate (named) tracks/instruments"*. A row called "Channel 4" is a
    // row you have to play to identify.
    let mut doc = project();
    let mut command = ImportParts::new(
        "Import",
        vec![a_part("Fretless Bass", &[36]), a_part("Strings", &[72])],
    );
    command.apply(&mut doc).expect("applies");

    let mut channel_names: Vec<String> =
        doc.channels.values().map(|c| c.name.clone()).collect();
    channel_names.sort();
    assert_eq!(channel_names, vec!["Fretless Bass", "Strings"]);

    let mut lane_names: Vec<String> = doc.lanes.values().map(|l| l.name.clone()).collect();
    lane_names.sort();
    assert_eq!(lane_names, vec!["Fretless Bass", "Strings"]);
}

#[test]
fn each_part_gets_a_mixer_strip_of_its_own_at_the_level_the_file_asked_for() {
    // A MIDI file carries a CC7 per channel and that is a fader, so the parts
    // really do each want a strip — see `midi_import`.
    let mut doc = project();
    let quiet = ImportPart {
        volume_db: -12.0,
        ..a_part("Quiet", &[60])
    };
    let mut command = ImportParts::new("Import", vec![quiet]);
    command.apply(&mut doc).expect("applies");

    let channel = doc.channels.values().next().expect("a channel");
    let track = channel.mixer_track.expect("a strip of its own");
    assert!((doc.mixer.tracks[track].gain_db - (-12.0)).abs() < 1e-6);
    assert_eq!(
        doc.mixer.tracks[track].output, doc.mixer.master,
        "and it plays into the master rather than into nothing"
    );
}

#[test]
fn a_clip_holds_its_own_parts_notes_and_is_as_long_as_they_are() {
    let mut doc = project();
    let mut command = ImportParts::new("Import", vec![a_part("Bass", &[36, 38, 40])]);
    command.apply(&mut doc).expect("applies");

    let clip = doc.clips.values().next().expect("a clip");
    assert_eq!(clip.start, 0);
    // Three notes a quarter apart, each a quarter long.
    assert_eq!(clip.length, PPQN * 3);
    let fontelle_model::ClipSource::Notes(data) = &clip.source else {
        panic!("an imported part is notes");
    };
    assert_eq!(data.notes.len(), 3);
}

#[test]
fn a_part_with_no_notes_is_left_out_rather_than_arriving_as_an_empty_row() {
    let mut doc = project();
    let mut command = ImportParts::new(
        "Import",
        vec![a_part("Real", &[60]), a_part("Empty", &[])],
    );
    command.apply(&mut doc).expect("applies");
    assert_eq!(doc.clips.len(), 1);
    assert_eq!(doc.channels.len(), 1);
}

#[test]
fn importing_nothing_at_all_is_refused_rather_than_landing_as_an_empty_entry() {
    let mut doc = project();
    let before = snapshot(&doc);
    let mut command = ImportParts::new("Import", Vec::new());
    assert!(command.apply(&mut doc).is_err());
    assert_eq!(before, snapshot(&doc));
}

#[test]
fn undoing_an_import_takes_the_whole_file_back_out() {
    // The load-bearing property, and the reason this is one command: eight
    // tracks in, one press of Ctrl+Z out, and the document byte-for-byte
    // where it started.
    let mut doc = project();
    let before = snapshot(&doc);
    let mut command: Box<dyn Command> = Box::new(ImportParts::new(
        "Import",
        vec![
            a_part("Bass", &[36, 38]),
            a_part("Lead", &[72]),
            a_part("Pad", &[48, 55, 60]),
        ],
    ));
    command.apply(&mut doc).expect("applies");
    assert_ne!(before, snapshot(&doc));

    command.invert().apply(&mut doc).expect("the inverse applies");
    assert_eq!(before, snapshot(&doc), "back exactly where it started");
}

#[test]
fn a_redone_import_puts_back_the_same_ids_it_made_the_first_time() {
    // The rule every creating command in this file follows: a redo that
    // minted fresh ids would leave anything stacked above it pointing at
    // nothing.
    let mut doc = project();
    let mut history = History::new();
    history
        .apply(
            Box::new(ImportParts::new("Import", vec![a_part("Bass", &[36])])),
            &mut doc,
        )
        .expect("applies");
    let after_first = snapshot(&doc);

    history.undo(&mut doc).expect("undoes").expect("applies");
    history.redo(&mut doc).expect("redoes").expect("applies");
    assert_eq!(after_first, snapshot(&doc));
}

#[test]
fn an_import_is_one_entry_in_the_history_however_many_parts_it_had() {
    let mut doc = project();
    let mut history = History::new();
    history
        .apply(
            Box::new(ImportParts::new(
                "Import",
                vec![a_part("A", &[60]), a_part("B", &[62]), a_part("C", &[64])],
            )),
            &mut doc,
        )
        .expect("applies");
    assert_eq!(history.depth(), 1);
}

#[test]
fn an_import_says_what_it_brought_in() {
    let command = ImportParts::new("Song.mid", vec![a_part("Bass", &[36])]);
    assert!(
        command.label().contains("Song.mid"),
        "the history entry should name the file: {}",
        command.label()
    );
}

#[test]
fn what_it_made_is_readable_afterwards_so_the_window_can_go_there() {
    // The roll opens on what you just imported, which needs the clip's id.
    let mut doc = project();
    let mut command = ImportParts::new("Import", vec![a_part("Bass", &[36])]);
    assert!(command.made().is_empty(), "nothing until it has run");
    command.apply(&mut doc).expect("applies");

    let made = command.made();
    assert_eq!(made.len(), 1);
    assert!(doc.clips.contains_key(made[0].clip));
    assert!(doc.channels.contains_key(made[0].channel));
    assert!(doc.lanes.contains_key(made[0].lane));
}

#[test]
fn imported_rows_go_under_the_ones_that_are_already_there() {
    // Not on top of them: a file dropped onto a song you are working on
    // should not push everything you have down the arrangement.
    let mut doc = project();
    let mut first = ImportParts::new("First", vec![a_part("Existing", &[60])]);
    first.apply(&mut doc).expect("applies");
    let existing_order = doc.lanes[first.made()[0].lane].order;

    let mut second = ImportParts::new("Second", vec![a_part("New", &[62])]);
    second.apply(&mut doc).expect("applies");
    assert!(
        doc.lanes[second.made()[0].lane].order > existing_order,
        "the new row landed above the old one"
    );
}
