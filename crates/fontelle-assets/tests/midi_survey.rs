//! Looking inside a `.mid` file *before* importing it (TDD §14.6).
//!
//! A MIDI file may hold one part or sixteen, and which of those it is decides
//! what importing it should even mean: a one-part file is a phrase to drop on
//! the instrument you have, and a sixteen-part file is a song that wants a
//! track each. The window cannot ask that question without knowing the answer
//! first, and it must not have to *import* the file to find out — so the
//! survey is a read that builds no document.
//!
//! The other half of this is **names**. A part called `Channel 4` tells you
//! nothing; the file almost always knows better, and there are three places to
//! look: the track's own name, the General MIDI program it selects, and
//! whether it is the percussion channel.

use fontelle_assets::{MidiChannels, general_midi_name, import_midi, survey_midi};
use fontelle_types::PPQN;

// --------------------------------------------------------- a file builder ---

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

/// One thing that happens in a track, at an absolute tick.
enum Ev {
    Note { at: u32, length: u32, channel: u8, key: u8 },
    Program { at: u32, channel: u8, program: u8 },
    TrackName(&'static str),
    InstrumentName(&'static str),
}

fn track_bytes(events: &[Ev]) -> Vec<u8> {
    // Absolute-timed, then sorted and delta-encoded, so a test can write what
    // it means rather than doing the arithmetic in its head.
    let mut timed: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    let mut order = 0u32;
    for event in events {
        let mut push = |at: u32, body: Vec<u8>| {
            timed.push((at, order, body));
            order += 1;
        };
        match event {
            Ev::Note { at, length, channel, key } => {
                push(*at, vec![0x90 | channel, *key, 100]);
                push(at + length, vec![0x80 | channel, *key, 64]);
            }
            Ev::Program { at, channel, program } => push(*at, vec![0xc0 | channel, *program]),
            Ev::TrackName(name) => {
                let mut body = vec![0xff, 0x03];
                varint(name.len() as u32, &mut body);
                body.extend_from_slice(name.as_bytes());
                push(0, body);
            }
            Ev::InstrumentName(name) => {
                let mut body = vec![0xff, 0x04];
                varint(name.len() as u32, &mut body);
                body.extend_from_slice(name.as_bytes());
                push(0, body);
            }
        }
    }
    timed.sort_by_key(|(at, order, _)| (*at, *order));

    let mut out = Vec::new();
    let mut previous = 0u32;
    for (at, _, body) in timed {
        varint(at - previous, &mut out);
        out.extend_from_slice(&body);
        previous = at;
    }
    varint(0, &mut out);
    out.extend_from_slice(&[0xff, 0x2f, 0x00]);

    let mut chunk = b"MTrk".to_vec();
    chunk.extend_from_slice(&(out.len() as u32).to_be_bytes());
    chunk.extend_from_slice(&out);
    chunk
}

fn build(format: u16, ticks_per_quarter: u16, tracks: &[Vec<Ev>]) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&format.to_be_bytes());
    out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    out.extend_from_slice(&ticks_per_quarter.to_be_bytes());
    for track in tracks {
        out.extend_from_slice(&track_bytes(track));
    }
    out
}

fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "fontelle-survey-{name}-{}-{:?}.mid",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn note(at: u32, channel: u8, key: u8) -> Ev {
    Ev::Note { at, length: 48, channel, key }
}

// ------------------------------------------------------------ the survey ---

#[test]
fn a_survey_says_what_is_in_the_file_without_importing_it() {
    let bytes = build(
        1,
        96,
        &[
            vec![Ev::TrackName("Bass"), note(0, 0, 36), note(96, 0, 38)],
            vec![Ev::TrackName("Lead"), note(0, 1, 72)],
        ],
    );
    let path = write_temp("two-parts", &bytes);
    let survey = survey_midi(&path).expect("reads");

    assert_eq!(survey.parts.len(), 2);
    assert_eq!(survey.parts[0].channel, 0);
    assert_eq!(survey.parts[0].notes, 2);
    assert_eq!(survey.parts[1].channel, 1);
    assert_eq!(survey.parts[1].notes, 1);
    // How long the piece is, on this project's grid: the last note ends a
    // quarter and a half in.
    assert_eq!(survey.length, PPQN + PPQN / 2);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_file_of_one_part_says_so_which_is_what_decides_whether_to_ask() {
    let bytes = build(0, 96, &[vec![note(0, 0, 60)]]);
    let path = write_temp("one-part", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert_eq!(survey.parts.len(), 1);
    assert!(!survey.is_multi_part());
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_survey_carries_the_tempo_so_the_prompt_can_say_it() {
    let bytes = build(1, 96, &[vec![note(0, 0, 60)]]);
    let path = write_temp("tempo", &bytes);
    let survey = survey_midi(&path).expect("reads");
    // No tempo event: MIDI's own default, which is what import uses too.
    assert!((survey.bpm - 120.0).abs() < 1e-9);
    assert_eq!(survey.tempo_changes, 1);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_survey_refuses_the_same_files_import_refuses() {
    let path = write_temp("rubbish", b"this is not a MIDI file");
    let error = survey_midi(&path).expect_err("must refuse");
    assert!(error.0.contains("not a readable MIDI file"), "{error}");
    std::fs::remove_file(&path).ok();
}

// ------------------------------------------------------------- the names ---

#[test]
fn a_part_is_called_what_its_track_is_called() {
    let bytes = build(
        1,
        96,
        &[
            vec![Ev::TrackName("Fretless Bass"), note(0, 0, 36)],
            vec![Ev::TrackName("Strings"), note(0, 1, 72)],
        ],
    );
    let path = write_temp("track-names", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert_eq!(survey.parts[0].name, "Fretless Bass");
    assert_eq!(survey.parts[1].name, "Strings");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_tracks_name_is_not_used_when_the_track_holds_more_than_one_part() {
    // A format-0 file is one track holding every channel, and its track name
    // is the *song's* name. Handing it to all sixteen parts would call every
    // instrument in the piece the same thing — which is worse than a number,
    // because a number at least tells two of them apart.
    let bytes = build(
        0,
        96,
        &[vec![
            Ev::TrackName("My Song"),
            Ev::Program { at: 0, channel: 0, program: 33 },
            note(0, 0, 36),
            Ev::Program { at: 0, channel: 1, program: 48 },
            note(0, 1, 72),
        ]],
    );
    let path = write_temp("format-0", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert_ne!(survey.parts[0].name, "My Song");
    assert_ne!(survey.parts[1].name, "My Song");
    // The program change is the next thing to ask, and it knows.
    assert_eq!(survey.parts[0].name, "Electric Bass (finger)");
    assert_eq!(survey.parts[1].name, "String Ensemble 1");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_part_with_no_name_falls_back_to_the_instrument_it_selects() {
    let bytes = build(
        1,
        96,
        &[vec![
            Ev::Program { at: 0, channel: 2, program: 0 },
            note(0, 2, 60),
        ]],
    );
    let path = write_temp("gm-name", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert_eq!(survey.parts[0].name, "Acoustic Grand Piano");
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_instrument_name_event_is_read_when_there_is_no_track_name() {
    let bytes = build(
        1,
        96,
        &[vec![Ev::InstrumentName("Rhodes"), note(0, 0, 60)]],
    );
    let path = write_temp("instrument-name", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert_eq!(survey.parts[0].name, "Rhodes");
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_percussion_channel_is_called_what_it_is() {
    let bytes = build(1, 96, &[vec![note(0, 9, 36)]]);
    let path = write_temp("drums", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert!(survey.parts[0].is_percussion);
    assert_eq!(survey.parts[0].name, "Drums");
    // A program change on channel 10 selects a *kit*, not a melodic patch, so
    // the General MIDI melodic name must not be used for it.
    let bytes = build(
        1,
        96,
        &[vec![Ev::Program { at: 0, channel: 9, program: 0 }, note(0, 9, 36)]],
    );
    let path2 = write_temp("drums-program", &bytes);
    let survey = survey_midi(&path2).expect("reads");
    assert_eq!(survey.parts[0].name, "Drums");
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&path2).ok();
}

#[test]
fn a_part_with_nothing_to_go_on_is_called_by_its_channel_as_a_keyboard_counts_them() {
    let bytes = build(1, 96, &[vec![note(0, 3, 60)]]);
    let path = write_temp("nameless", &bytes);
    let survey = survey_midi(&path).expect("reads");
    // Channel 4 on the front panel, not the 3 on the wire.
    assert_eq!(survey.parts[0].name, "Channel 4");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_blank_track_name_is_not_a_name() {
    let bytes = build(1, 96, &[vec![Ev::TrackName("   "), note(0, 0, 60)]]);
    let path = write_temp("blank-name", &bytes);
    let survey = survey_midi(&path).expect("reads");
    assert_eq!(survey.parts[0].name, "Channel 1");
    std::fs::remove_file(&path).ok();
}

#[test]
fn every_general_midi_program_has_a_name() {
    for program in 0u8..=127 {
        let name = general_midi_name(program);
        assert!(!name.is_empty(), "program {program} has no name");
    }
    assert_eq!(general_midi_name(0), "Acoustic Grand Piano");
    assert_eq!(general_midi_name(127), "Gunshot");
}

// -------------------------------------------- what import does with them ---

#[test]
fn an_imported_channel_carries_the_same_name_the_survey_showed() {
    // The prompt lists the parts and the import makes them; a name that
    // differed between the two would be a list of things you did not get.
    let bytes = build(
        1,
        96,
        &[
            vec![Ev::TrackName("Fretless Bass"), note(0, 0, 36)],
            vec![Ev::TrackName("Strings"), note(0, 1, 72)],
        ],
    );
    let path = write_temp("names-match", &bytes);
    let survey = survey_midi(&path).expect("survey reads");
    let import = import_midi(&path, MidiChannels::Melodic).expect("import reads");

    let surveyed: Vec<&str> = survey.parts.iter().map(|p| p.name.as_str()).collect();
    let imported: Vec<&str> = import.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(surveyed, imported);

    // And the name reaches the document, which is the point of having one.
    let named = import
        .project
        .channels
        .get(import.channels[0].channel)
        .expect("the channel is in the document");
    assert_eq!(named.name, "Fretless Bass");
    std::fs::remove_file(&path).ok();
}

#[test]
fn importing_one_part_of_a_song_brings_only_that_part() {
    let bytes = build(
        1,
        96,
        &[
            vec![Ev::TrackName("Bass"), note(0, 0, 36)],
            vec![Ev::TrackName("Lead"), note(0, 1, 72), note(48, 1, 74)],
        ],
    );
    let path = write_temp("one-of-two", &bytes);
    let import = import_midi(&path, MidiChannels::Only(1)).expect("reads");
    assert_eq!(import.channels.len(), 1);
    assert_eq!(import.channels[0].name, "Lead");
    assert_eq!(import.channels[0].notes, 2);
    // And it says what it left behind rather than leaving you to wonder.
    assert_eq!(import.skipped.len(), 1);
    assert_eq!(import.skipped[0].channel, 0);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_skipped_channel_is_named_too_so_the_prompt_can_offer_it() {
    let bytes = build(
        1,
        96,
        &[
            vec![Ev::TrackName("Bass"), note(0, 0, 36)],
            vec![Ev::TrackName("Kit"), note(0, 9, 36)],
        ],
    );
    let path = write_temp("named-skip", &bytes);
    let import = import_midi(&path, MidiChannels::Melodic).expect("reads");
    assert_eq!(import.skipped.len(), 1);
    assert_eq!(import.skipped[0].name, "Kit");
    std::fs::remove_file(&path).ok();
}
