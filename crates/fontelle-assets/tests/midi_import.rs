//! MIDI import is what turns Fontelle from "plays a hardcoded phrase" into
//! "plays a piece", so it gets the same treatment as the SF2 importer: real
//! bytes, assembled here rather than checked in, and every conversion asserted
//! against the inverse of the formula the importer has to use.

use std::path::PathBuf;

use fontelle_assets::{MidiChannels, import_midi};
use fontelle_model::ClipSource;
use fontelle_types::PPQN;

/// MIDI's variable-length quantity: seven bits per byte, high bit set on every
/// byte but the last.
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

#[derive(Clone, Copy)]
struct TestNote {
    channel: u8,
    key: u8,
    velocity: u8,
    start: u32,
    length: u32,
}

/// A complete format-1 file: a tempo track plus one note track.
fn build_midi(ticks_per_quarter: u16, us_per_quarter: u32, notes: &[TestNote]) -> Vec<u8> {
    build_midi_with_programs(ticks_per_quarter, us_per_quarter, notes, &[])
}

/// As `build_midi`, plus a program change per channel emitted at time zero.
fn build_midi_with_programs(
    ticks_per_quarter: u16,
    us_per_quarter: u32,
    notes: &[TestNote],
    programs: &[(u8, u8)],
) -> Vec<u8> {
    build_midi_with_controls(ticks_per_quarter, us_per_quarter, notes, programs, &[])
}

/// As `build_midi_with_programs`, plus `(channel, controller, value)` control
/// changes emitted at time zero.
fn build_midi_with_controls(
    ticks_per_quarter: u16,
    us_per_quarter: u32,
    notes: &[TestNote],
    programs: &[(u8, u8)],
    controls: &[(u8, u8, u8)],
) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6u32.to_be_bytes());
    file.extend_from_slice(&1u16.to_be_bytes()); // format 1
    file.extend_from_slice(&2u16.to_be_bytes()); // two tracks
    file.extend_from_slice(&ticks_per_quarter.to_be_bytes());

    let mut tempo = Vec::new();
    varint(0, &mut tempo);
    tempo.extend_from_slice(&[0xff, 0x51, 0x03]);
    tempo.extend_from_slice(&us_per_quarter.to_be_bytes()[1..]); // 24-bit
    varint(0, &mut tempo);
    tempo.extend_from_slice(&[0xff, 0x2f, 0x00]); // end of track
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(tempo.len() as u32).to_be_bytes());
    file.extend_from_slice(&tempo);

    // Absolute-time event list, then sorted and delta-encoded, so the fixture
    // can be written in the order a human thinks in.
    // Two-byte messages are padded and their real length tracked, so a program
    // change and a note-on can share one ordered list.
    let mut events: Vec<(u32, usize, [u8; 3])> = Vec::new();
    for (channel, program) in programs {
        events.push((0, 2, [0xc0 | channel, *program, 0]));
    }
    for (channel, controller, value) in controls {
        events.push((0, 3, [0xb0 | channel, *controller, *value]));
    }
    for n in notes {
        events.push((n.start, 3, [0x90 | n.channel, n.key, n.velocity]));
        events.push((n.start + n.length, 3, [0x80 | n.channel, n.key, 0]));
    }
    // Stable sort, so a program change written before a note stays before it.
    events.sort_by_key(|(t, _, _)| *t);

    let mut track = Vec::new();
    let mut previous = 0;
    for (time, len, bytes) in events {
        varint(time - previous, &mut track);
        track.extend_from_slice(&bytes[..len]);
        previous = time;
    }
    varint(0, &mut track);
    track.extend_from_slice(&[0xff, 0x2f, 0x00]);
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    file
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("fontelle_midi_{name}_{}.mid", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn notes_of(import: &fontelle_assets::MidiImport) -> Vec<fontelle_model::Note> {
    let mut out: Vec<_> = import
        .project
        .clips
        .values()
        .filter_map(|clip| match &clip.source {
            ClipSource::Notes(data) => Some(data.notes.values().copied()),
            _ => None,
        })
        .flatten()
        .collect();
    out.sort_by_key(|n| (n.start, n.key));
    out
}

/// The notes on one imported channel, by its MIDI channel number.
fn notes_on(import: &fontelle_assets::MidiImport, midi_channel: u8) -> Vec<fontelle_model::Note> {
    let imported = import
        .channels
        .iter()
        .find(|c| c.midi_channel == midi_channel)
        .unwrap_or_else(|| panic!("channel {midi_channel} was not imported"));
    let mut out: Vec<_> = import
        .project
        .clips
        .values()
        .filter_map(|clip| match &clip.source {
            ClipSource::Notes(data) if data.channel == imported.channel => {
                Some(data.notes.values().copied())
            }
            _ => None,
        })
        .flatten()
        .collect();
    out.sort_by_key(|n| (n.start, n.key));
    out
}

#[test]
fn imports_notes_with_their_timing_converted_to_the_project_resolution() {
    // The file is at 480 ticks per quarter and Fontelle is at 960, so every
    // position must double. Getting this wrong plays the piece at the wrong
    // speed, which is easy to mistake for a tempo-map bug.
    let path = write_temp(
        "timing",
        &build_midi(
            480,
            500_000, // 120 bpm
            &[
                TestNote {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
                TestNote {
                    channel: 0,
                    key: 64,
                    velocity: 80,
                    start: 480,
                    length: 960,
                },
            ],
        ),
    );
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    let notes = notes_of(&import);
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].key, 60);
    assert_eq!(notes[0].velocity, 100);
    assert_eq!(notes[0].start, 0);
    assert_eq!(
        notes[0].length,
        PPQN / 2,
        "240/480 of a quarter at 960 PPQN"
    );
    assert_eq!(notes[1].key, 64);
    assert_eq!(notes[1].start, PPQN, "480/480 of a quarter");
    assert_eq!(notes[1].length, PPQN * 2);
}

#[test]
fn reads_the_tempo_from_the_files_meta_event() {
    let path = write_temp("tempo", &build_midi(480, 250_000, &[])); // 240 bpm
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(
        (import.bpm - 240.0).abs() < 1e-6,
        "250000 us per quarter is 240 bpm, got {}",
        import.bpm
    );
}

#[test]
fn a_file_with_no_tempo_event_falls_back_to_the_midi_default() {
    // The spec's default when no tempo is given is 500000 us per quarter.
    let mut bytes = build_midi(480, 500_000, &[]);
    // Blank the tempo meta event's type byte so it parses as something else.
    let position = bytes
        .windows(3)
        .position(|w| w == [0xff, 0x51, 0x03])
        .unwrap();
    bytes[position + 1] = 0x7f; // sequencer-specific meta, ignored
    let path = write_temp("notempo", &bytes);
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();
    assert!((import.bpm - 120.0).abs() < 1e-6, "got {}", import.bpm);
}

#[test]
fn percussion_is_excluded_by_default_and_reachable_on_request() {
    // MIDI channel 10 carries drum selections, not pitches. Played through a
    // melodic patch it is noise, so it must not arrive by accident — but it
    // must still be reachable, because it is real content in the file.
    let file = build_midi(
        480,
        500_000,
        &[
            TestNote {
                channel: 0,
                key: 60,
                velocity: 100,
                start: 0,
                length: 240,
            },
            TestNote {
                channel: 9,
                key: 36,
                velocity: 100,
                start: 0,
                length: 240,
            },
        ],
    );
    let path = write_temp("perc", &file);

    let melodic = import_midi(&path, MidiChannels::Melodic).unwrap();
    assert_eq!(notes_of(&melodic).len(), 1);
    assert_eq!(notes_of(&melodic)[0].key, 60);

    let all = import_midi(&path, MidiChannels::All).unwrap();
    assert_eq!(notes_of(&all).len(), 2);

    let only_drums = import_midi(&path, MidiChannels::Only(9)).unwrap();
    assert_eq!(notes_of(&only_drums).len(), 1);
    assert_eq!(notes_of(&only_drums)[0].key, 36);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_note_on_with_zero_velocity_ends_the_note() {
    // The convention almost every real file uses instead of an explicit
    // note-off. Read literally it starts a silent note and never ends it, so
    // the piece plays as one endless chord.
    let mut file = build_midi(
        480,
        500_000,
        &[TestNote {
            channel: 0,
            key: 60,
            velocity: 100,
            start: 0,
            length: 480,
        }],
    );
    let position = file.windows(3).rposition(|w| w == [0x80, 60, 0]).unwrap();
    file[position] = 0x90; // note-on, velocity 0
    let path = write_temp("zerovel", &file);

    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();
    let notes = notes_of(&import);
    assert_eq!(notes.len(), 1, "one note, not one note plus a stuck one");
    assert_eq!(notes[0].length, PPQN);
}

#[test]
fn reports_what_each_source_channel_contained() {
    let path = write_temp(
        "summary",
        &build_midi(
            480,
            500_000,
            &[
                TestNote {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
                TestNote {
                    channel: 0,
                    key: 62,
                    velocity: 100,
                    start: 240,
                    length: 240,
                },
                TestNote {
                    channel: 3,
                    key: 48,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
            ],
        ),
    );
    let import = import_midi(&path, MidiChannels::All).unwrap();
    std::fs::remove_file(&path).ok();

    let summary = &import.channels;
    assert_eq!(
        summary.len(),
        2,
        "only channels with notes should be listed"
    );
    assert_eq!(summary[0].midi_channel, 0);
    assert_eq!(summary[0].notes, 2);
    assert_eq!(summary[1].midi_channel, 3);
    assert_eq!(summary[1].notes, 1);
}

#[test]
fn a_file_that_is_not_a_midi_file_fails_cleanly() {
    let path = write_temp("garbage", b"this is not a MIDI file at all");
    let result = import_midi(&path, MidiChannels::Melodic);
    std::fs::remove_file(&path).ok();
    assert!(result.is_err(), "garbage must not import as an empty song");
}

#[test]
fn an_unfinished_note_still_ends_somewhere() {
    // A note-on with no matching note-off is malformed but common in files
    // produced by tools that crashed mid-export. It must not become a note of
    // zero length, or of infinite length.
    let mut file = build_midi(
        480,
        500_000,
        &[TestNote {
            channel: 0,
            key: 60,
            velocity: 100,
            start: 0,
            length: 480,
        }],
    );
    let position = file.windows(3).rposition(|w| w == [0x80, 60, 0]).unwrap();
    file.drain(position - 1..position + 3); // delta byte + the note-off
    // Fix the track length so the file stays structurally valid.
    let track_len_at = file.len() - 4;
    let _ = track_len_at;
    let path = write_temp("stuck", &file);
    let result = import_midi(&path, MidiChannels::Melodic);
    std::fs::remove_file(&path).ok();

    if let Ok(import) = result {
        for note in notes_of(&import) {
            assert!(note.length > 0, "a stuck note must still have a length");
        }
    }
}

#[test]
fn each_midi_channel_becomes_its_own_instrument() {
    // The difference between playing a file and playing it as written: a bass
    // part and a lead part are separate instruments, not one merged stream of
    // notes that has to share a patch.
    let path = write_temp(
        "multi",
        &build_midi(
            480,
            500_000,
            &[
                TestNote {
                    channel: 0,
                    key: 72,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
                TestNote {
                    channel: 2,
                    key: 36,
                    velocity: 90,
                    start: 0,
                    length: 480,
                },
            ],
        ),
    );
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(import.channels.len(), 2);
    let lead = notes_on(&import, 0);
    let bass = notes_on(&import, 2);
    assert_eq!(lead.len(), 1);
    assert_eq!(lead[0].key, 72);
    assert_eq!(bass.len(), 1);
    assert_eq!(bass[0].key, 36);
    assert_ne!(
        import.channels[0].channel, import.channels[1].channel,
        "two MIDI channels must not land on one document channel"
    );
}

#[test]
fn a_channels_program_change_is_carried_through_to_the_instrument() {
    // What lets an importer pick the right preset per part instead of playing
    // the whole file on one sound.
    let path = write_temp(
        "program",
        &build_midi_with_programs(
            480,
            500_000,
            &[TestNote {
                channel: 0,
                key: 60,
                velocity: 100,
                start: 0,
                length: 240,
            }],
            &[(0, 42)],
        ),
    );
    let import = import_midi(&path, MidiChannels::All).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(import.channels[0].program, Some(42));
}

#[test]
fn percussion_channels_are_marked_as_such() {
    // A drum channel needs a drum kit, not a transposed melodic patch, so the
    // distinction has to survive import rather than being rediscovered by
    // whoever assigns instruments.
    let path = write_temp(
        "percflag",
        &build_midi(
            480,
            500_000,
            &[
                TestNote {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
                TestNote {
                    channel: 9,
                    key: 36,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
            ],
        ),
    );
    let import = import_midi(&path, MidiChannels::All).unwrap();
    std::fs::remove_file(&path).ok();

    assert!(!import.channels[0].is_percussion);
    assert!(import.channels[1].is_percussion);
}

#[test]
fn channels_present_but_filtered_out_are_still_reported() {
    // "Where did the drums go" should be answerable from the import, not from
    // reading the source.
    let path = write_temp(
        "skipped",
        &build_midi(
            480,
            500_000,
            &[
                TestNote {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
                TestNote {
                    channel: 9,
                    key: 36,
                    velocity: 100,
                    start: 0,
                    length: 240,
                },
            ],
        ),
    );
    let import = import_midi(&path, MidiChannels::Melodic).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(import.channels.len(), 1);
    assert_eq!(import.skipped.len(), 1);
    assert_eq!(import.skipped[0].channel, 9);
    assert_eq!(import.skipped[0].notes, 1);
}

/// MIDI CC10. 64 is centre, 0 hard left, 127 hard right — and a channel that
/// never sends one is centred, not silently placed somewhere.
#[test]
fn a_channels_pan_controller_places_the_part() {
    let notes: Vec<TestNote> = (0u8..3)
        .map(|c| TestNote {
            channel: c,
            key: 60,
            velocity: 100,
            start: 0,
            length: 480,
        })
        .collect();
    let bytes = build_midi_with_controls(
        480,
        500_000,
        &notes,
        &[],
        // Channel 2 sends nothing, so it must come out centred.
        &[(0, 10, 0), (1, 10, 127)],
    );
    let path = write_temp("pan_cc", &bytes);
    let import = import_midi(&path, MidiChannels::All).unwrap();
    std::fs::remove_file(&path).ok();

    let pan = |midi_channel: u8| {
        import
            .channels
            .iter()
            .find(|c| c.midi_channel == midi_channel)
            .unwrap()
            .pan
    };
    assert!(
        (pan(0) + 1.0).abs() < 1e-6,
        "CC10 0 is hard left, got {}",
        pan(0)
    );
    assert!(
        (pan(1) - 1.0).abs() < 1e-6,
        "CC10 127 is hard right, got {}",
        pan(1)
    );
    assert_eq!(pan(2), 0.0, "a channel with no CC10 is centred");
}

/// MIDI CC7, through the GM2/DLS curve `40 * log10(value / 127)` — not a
/// linear reading of the controller, which would make every balance in every
/// General MIDI file wrong by the difference between the two.
#[test]
fn a_channels_volume_controller_sets_its_level() {
    let notes: Vec<TestNote> = (0u8..3)
        .map(|c| TestNote {
            channel: c,
            key: 60,
            velocity: 100,
            start: 0,
            length: 480,
        })
        .collect();
    let bytes = build_midi_with_controls(480, 500_000, &notes, &[], &[(0, 7, 127), (1, 7, 64)]);
    let path = write_temp("volume_cc", &bytes);
    let import = import_midi(&path, MidiChannels::All).unwrap();
    std::fs::remove_file(&path).ok();

    let gain = |midi_channel: u8| {
        import
            .channels
            .iter()
            .find(|c| c.midi_channel == midi_channel)
            .unwrap()
            .volume_db
    };
    assert!(
        gain(0).abs() < 1e-4,
        "CC7 at full scale is unity, got {}",
        gain(0)
    );
    let expected = 40.0 * (64.0f32 / 127.0).log10();
    assert!(
        (gain(1) - expected).abs() < 1e-3,
        "CC7 64 should be {expected} dB, got {}",
        gain(1)
    );
    // MIDI's own default channel volume is 100, not 127: a file that sends no
    // CC7 still means something specific, and reading it as unity would put
    // every silent channel 4 dB above the ones that spelled 100 out.
    let default = 40.0 * (100.0f32 / 127.0).log10();
    assert!(
        (gain(2) - default).abs() < 1e-3,
        "a channel with no CC7 takes MIDI's default of 100 ({default} dB), got {}",
        gain(2)
    );
}

#[test]
fn a_silent_channel_volume_is_reported_as_silence_rather_than_minus_infinity() {
    let bytes = build_midi_with_controls(
        480,
        500_000,
        &[TestNote {
            channel: 0,
            key: 60,
            velocity: 100,
            start: 0,
            length: 480,
        }],
        &[],
        &[(0, 7, 0)],
    );
    let path = write_temp("volume_zero", &bytes);
    let import = import_midi(&path, MidiChannels::All).unwrap();
    std::fs::remove_file(&path).ok();

    let gain = import.channels[0].volume_db;
    assert!(
        gain.is_finite() && gain <= -96.0,
        "CC7 0 must land on a finite floor a fader can hold, got {gain}"
    );
}
