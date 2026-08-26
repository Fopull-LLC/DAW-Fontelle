//! An imported MIDI file has to reach the engine through exactly the same path
//! the built-in demo phrase does — document, sequencer, compiled timeline — or
//! "it plays a .mid" would mean a second, parallel playback route that the
//! rest of the project's invariants don't cover.

use std::path::PathBuf;

use fontelle_app::Song;
use fontelle_assets::{MidiChannels, import_midi};
use fontelle_types::{EventPayload, PPQN};

const SR: u32 = 48_000;

/// The smallest complete format-0 file: one track, one tempo, two notes.
fn tiny_midi(us_per_quarter: u32, notes: &[(u8, u8, u32, u32)]) -> Vec<u8> {
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

    let mut track = Vec::new();
    varint(0, &mut track);
    track.extend_from_slice(&[0xff, 0x51, 0x03]);
    track.extend_from_slice(&us_per_quarter.to_be_bytes()[1..]);

    let mut events: Vec<(u32, [u8; 3])> = Vec::new();
    for (key, velocity, start, length) in notes {
        events.push((*start, [0x90, *key, *velocity]));
        events.push((start + length, [0x80, *key, 0]));
    }
    events.sort_by_key(|(t, _)| *t);
    let mut previous = 0;
    for (time, bytes) in events {
        varint(time - previous, &mut track);
        track.extend_from_slice(&bytes);
        previous = time;
    }
    varint(0, &mut track);
    track.extend_from_slice(&[0xff, 0x2f, 0x00]);

    let mut file = Vec::new();
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6u32.to_be_bytes());
    file.extend_from_slice(&0u16.to_be_bytes()); // format 0
    file.extend_from_slice(&1u16.to_be_bytes());
    file.extend_from_slice(&480u16.to_be_bytes());
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    file
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("fontelle_song_{name}_{}.mid", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn an_imported_midi_file_compiles_to_a_timeline_the_engine_can_play() {
    // 120 bpm, so a quarter note is 0.5s = 24000 samples at 48 kHz.
    let path = write_temp(
        "chain",
        &tiny_midi(500_000, &[(60, 100, 0, 480), (67, 80, 480, 480)]),
    );
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    let song = Song::from_midi(import, SR);
    let timeline = song.compile();

    let note_ons: Vec<_> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, velocity, .. } => Some((e.sample, key, velocity)),
            _ => None,
        })
        .collect();

    assert_eq!(note_ons.len(), 2);
    assert_eq!(note_ons[0], (0, 60, 100));
    assert_eq!(
        note_ons[1],
        (24_000, 67, 80),
        "the second note is one quarter in, which is 24000 samples at 120 bpm"
    );

    // And the reported duration must cover the music, or playback stops early.
    assert!(song.duration_samples(PPQN) >= 48_000);
}

#[test]
fn the_files_own_tempo_drives_the_timeline() {
    // Same note positions in MIDI ticks, twice the tempo: every sample
    // position must halve. This is the assertion that catches a tempo that was
    // read but never actually applied.
    let path = write_temp("tempo", &tiny_midi(250_000, &[(60, 100, 480, 480)]));
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    let song = Song::from_midi(import, SR);
    let first = song
        .compile()
        .events
        .iter()
        .find(|e| matches!(e.payload, EventPayload::NoteOn { .. }))
        .map(|e| e.sample)
        .unwrap();
    assert_eq!(first, 12_000, "a quarter note at 240 bpm is 12000 samples");
}
