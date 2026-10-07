//! MIDI with lyrics: a lyric event on a note's tick is that note's words;
//! the tracks that carry lyrics are the vocal parts. Written back out, the
//! words, notes, tempo and pitch bend come back.

mod common;

use common::{assert_near, lane};
use fontelle_model::{LaneCurve, TempoSegment};
use fontelle_sing::Lane;
use fontelle_vocal_files::{SongPart, VocalSong, midi, vocal_note};
use midly::num::{u4, u7, u14, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

fn at(delta: u32, kind: TrackEventKind<'static>) -> TrackEvent<'static> {
    TrackEvent {
        delta: u28::new(delta),
        kind,
    }
}

fn meta(m: MetaMessage<'static>) -> TrackEventKind<'static> {
    TrackEventKind::Meta(m)
}

fn ch(message: MidiMessage) -> TrackEventKind<'static> {
    TrackEventKind::Midi {
        channel: u4::new(0),
        message,
    }
}

fn on(key: u8) -> TrackEventKind<'static> {
    ch(MidiMessage::NoteOn {
        key: u7::new(key),
        vel: u7::new(90),
    })
}

fn off(key: u8) -> TrackEventKind<'static> {
    ch(MidiMessage::NoteOff {
        key: u7::new(key),
        vel: u7::new(0),
    })
}

fn cc(controller: u8, value: u8) -> TrackEventKind<'static> {
    ch(MidiMessage::Controller {
        controller: u7::new(controller),
        value: u7::new(value),
    })
}

fn bend(value: u16) -> TrackEventKind<'static> {
    ch(MidiMessage::PitchBend {
        bend: midly::PitchBend(u14::new(value)),
    })
}

fn bytes(tracks: Vec<Vec<TrackEvent<'static>>>) -> Vec<u8> {
    let smf = Smf {
        header: Header::new(Format::Parallel, Timing::Metrical(u15::new(480))),
        tracks,
    };
    let mut out = Vec::new();
    smf.write_std(&mut out).unwrap();
    out
}

/// A conductor track (100 bpm, 3/4), a sung track with lyrics and a bend,
/// and a piano with none.
fn song_file() -> Vec<u8> {
    let end = || at(0, meta(MetaMessage::EndOfTrack));
    bytes(vec![
        vec![
            at(0, meta(MetaMessage::Tempo(u24::new(600_000)))),
            at(0, meta(MetaMessage::TimeSignature(3, 2, 24, 8))),
            end(),
        ],
        vec![
            at(0, meta(MetaMessage::TrackName(b"Vocal"))),
            // A bend range of 2 semitones, said with RPN 0.
            at(0, cc(101, 0)),
            at(0, cc(100, 0)),
            at(0, cc(6, 2)),
            at(0, meta(MetaMessage::Lyric(b"Hel"))),
            at(0, on(60)),
            at(480, off(60)),
            at(0, meta(MetaMessage::Lyric(b"lo"))),
            at(0, on(62)),
            at(120, bend(8192 + 4096)),
            at(300, bend(8192)),
            at(60, off(62)),
            end(),
        ],
        vec![
            at(0, meta(MetaMessage::TrackName(b"Piano"))),
            at(0, on(48)),
            at(960, off(48)),
            end(),
        ],
    ])
}

#[test]
fn the_track_with_lyrics_is_the_vocal_part() {
    let file = song_file();
    assert!(midi::has_lyrics(&file));
    let song = midi::read(&file, "file").unwrap();
    assert_eq!(song.beats_per_bar, 3);
    assert_near("tempo", song.tempo[0].bpm as f32, 100.0, 1e-3);
    assert_eq!(song.parts.len(), 1, "the piano has no words");
    let part = &song.parts[0];
    assert_eq!(part.name, "Vocal");
    let notes: Vec<(i64, i64, u8, &str)> = part
        .notes
        .iter()
        .map(|n| (n.start, n.length, n.key, n.lyric.as_str()))
        .collect();
    assert_eq!(notes, vec![(0, 960, 60, "Hel"), (960, 960, 62, "lo")]);
    // MIDI velocity is loudness, not consonant speed: left as the voice's own.
    assert!(part.notes.iter().all(|n| n.velocity == 100));
    assert!(
        song.unmapped.iter().any(|u| u.contains("Piano")),
        "{:?}",
        song.unmapped
    );
}

#[test]
fn pitch_bend_is_the_pitch_lane() {
    let song = midi::read(&song_file(), "file").unwrap();
    let part = &song.parts[0];
    // +4096 of 8192 at 2 semitones is +100 cents, from tick 600 (480 a
    // quarter) to 900.
    assert_near("bent", lane(part, Lane::PitchBend, 1300), 100.0, 0.5);
    assert_near("before", lane(part, Lane::PitchBend, 1190), 0.0, 0.5);
    assert_near("after", lane(part, Lane::PitchBend, 1820), 0.0, 0.5);
}

#[test]
fn a_file_without_lyrics_is_not_a_vocal_song() {
    let file = bytes(vec![vec![
        at(0, on(60)),
        at(480, off(60)),
        at(0, meta(MetaMessage::EndOfTrack)),
    ]]);
    assert!(!midi::has_lyrics(&file));
}

#[test]
fn karaoke_text_events_are_lyrics_when_there_are_no_lyric_events() {
    let file = bytes(vec![vec![
        at(0, meta(MetaMessage::Text(b"@TSome song"))),
        at(0, meta(MetaMessage::Text(b"/Twin"))),
        at(0, on(60)),
        at(480, off(60)),
        at(0, meta(MetaMessage::Text(b"kle"))),
        at(0, on(60)),
        at(480, off(60)),
        at(0, meta(MetaMessage::EndOfTrack)),
    ]]);
    assert!(midi::has_lyrics(&file));
    let song = midi::read(&file, "x").unwrap();
    let lyrics: Vec<&str> = song.parts[0]
        .notes
        .iter()
        .map(|n| n.lyric.as_str())
        .collect();
    assert_eq!(lyrics, vec!["Twin", "kle"]);
}

#[test]
fn written_and_read_back_the_words_notes_tempo_and_bend_return() {
    let mut song = VocalSong::new("Out");
    song.tempo = vec![
        TempoSegment {
            start_tick: 0,
            bpm: 96.0,
        },
        TempoSegment {
            start_tick: 3840,
            bpm: 128.0,
        },
    ];
    song.beats_per_bar = 3;
    let mut part = SongPart::new("Lead");
    part.notes = vec![
        vocal_note(0, 960, 60, "a-"),
        vocal_note(960, 480, 62, "ma-"),
        vocal_note(1440, 1440, 64, "zing"),
        vocal_note(2880, 960, 64, "-"),
        vocal_note(4800, 960, 67, "\u{3089}"),
    ];
    part.vocal.lanes.insert(
        Lane::PitchBend,
        LaneCurve::from_points(vec![(960, 0.0), (1200, -150.0), (1440, 0.0)]),
    );
    song.parts.push(part);
    let file = midi::write(&song);
    let back = midi::read(&file, "x").unwrap();
    assert_eq!(back.beats_per_bar, 3);
    let tempo: Vec<(i64, f64)> = back.tempo.iter().map(|s| (s.start_tick, s.bpm)).collect();
    assert_eq!(tempo.len(), 2);
    assert_near("tempo", tempo[0].1 as f32, 96.0, 0.01);
    assert_eq!(tempo[1].0, 3840);
    assert_near("tempo", tempo[1].1 as f32, 128.0, 0.01);
    let (want, got) = (&song.parts[0], &back.parts[0]);
    assert_eq!(got.name, "Lead");
    let pick = |p: &SongPart| -> Vec<(i64, i64, u8, String)> {
        p.notes
            .iter()
            .map(|n| (n.start, n.length, n.key, n.lyric.clone()))
            .collect()
    };
    assert_eq!(pick(got), pick(want));
    for t in [960, 1080, 1200, 1320, 1440, 2000] {
        assert_near(
            &format!("bend at {t}"),
            lane(got, Lane::PitchBend, t),
            lane(want, Lane::PitchBend, t),
            2.0,
        );
    }
}
