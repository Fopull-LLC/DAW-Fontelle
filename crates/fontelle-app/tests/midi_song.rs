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
    tiny_midi_on(
        us_per_quarter,
        &notes
            .iter()
            .map(|n| (0u8, n.0, n.1, n.2, n.3))
            .collect::<Vec<_>>(),
    )
}

/// As `tiny_midi`, with an explicit MIDI channel per note.
fn tiny_midi_on(us_per_quarter: u32, notes: &[(u8, u8, u8, u32, u32)]) -> Vec<u8> {
    tiny_midi_with_controls(us_per_quarter, notes, &[])
}

/// As `tiny_midi_on`, plus `(channel, controller, value)` control changes at
/// time zero.
fn tiny_midi_with_controls(
    us_per_quarter: u32,
    notes: &[(u8, u8, u8, u32, u32)],
    controls: &[(u8, u8, u8)],
) -> Vec<u8> {
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
    for (channel, controller, value) in controls {
        events.push((0, [0xb0 | channel, *controller, *value]));
    }
    for (channel, key, velocity, start, length) in notes {
        events.push((*start, [0x90 | channel, *key, *velocity]));
        events.push((start + length, [0x80 | channel, *key, 0]));
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

#[test]
fn every_midi_channel_gets_its_own_node_in_the_song() {
    // Multi-timbral playback rests on this: one document channel per part, one
    // engine node each, and a distinct target on every event. Sharing a node
    // would put both parts through one instrument again.
    let path = write_temp(
        "multi",
        &tiny_midi_on(500_000, &[(0, 72, 100, 0, 240), (2, 36, 90, 0, 240)]),
    );
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    let song = Song::from_midi(import, SR);
    assert_eq!(song.channels.len(), 2);
    assert_ne!(
        song.channels[0].node, song.channels[1].node,
        "each part needs a node of its own"
    );

    let timeline = song.compile();
    let targets: std::collections::HashSet<_> = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::NoteOn { .. }))
        .map(|e| e.target)
        .collect();
    assert_eq!(
        targets.len(),
        2,
        "the two parts must be addressed separately"
    );
}

#[test]
fn each_part_is_addressed_to_the_node_holding_its_own_instrument() {
    // The mapping has to be right, not merely one-to-one: the bass notes must
    // reach the bass node.
    let path = write_temp(
        "addressing",
        &tiny_midi_on(500_000, &[(0, 72, 100, 0, 240), (2, 36, 90, 0, 240)]),
    );
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    // MIDI channels come back in order, and `Song` keeps that order.
    let song = Song::from_midi(import, SR);
    let node_for_channel_1 = song.channels[0].node;
    let node_for_channel_3 = song.channels[1].node;

    let timeline = song.compile();
    for event in &timeline.events {
        if let EventPayload::NoteOn { key, .. } = event.payload {
            let expected = if key == 72 {
                node_for_channel_1
            } else {
                node_for_channel_3
            };
            assert_eq!(event.target, expected, "key {key} went to the wrong node");
        }
    }
}

#[test]
fn each_part_gets_its_own_fader_from_the_files_own_volume_controller() {
    // A General MIDI file balances its parts with CC7. Before this the whole
    // song went through one fader, so the file's balance was discarded and
    // every part played at whatever level its instrument happened to have.
    let bytes = tiny_midi_with_controls(
        500_000,
        &[(0, 72, 100, 0, 240), (2, 36, 90, 0, 240)],
        &[(0, 7, 127), (2, 7, 40)],
    );
    let path = write_temp("faders", &bytes);
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    let quiet_part = import
        .channels
        .iter()
        .find(|c| c.midi_channel == 2)
        .unwrap()
        .volume_db;
    let song = Song::from_midi(import, SR);
    assert_eq!(song.channels.len(), 2);
    assert!(
        (song.channels[1].gain_db - quiet_part).abs() < 1e-6,
        "the part's fader must carry the level the file asked for: expected \
         {quiet_part}, got {}",
        song.channels[1].gain_db
    );
    assert!(
        song.channels[0].gain_db > song.channels[1].gain_db,
        "and the two parts must not end up at the same level"
    );
}
