//! Standard MIDI files with lyric events, read and written.
//!
//! Lyrics live on **tracks** (meta event 5, or karaoke's text events when
//! a file has no lyric events at all), notes on channels: a lyric is the
//! words of the note in the same track starting on its tick (or just
//! after). Every track with words is a vocal part; tracks without are left
//! to the ordinary MIDI import and named in `unmapped`.
//!
//! What maps, and to what:
//!
//! - Notes and their lyrics; MIDI velocity is loudness, not consonant
//!   speed, so it is left as the voice's own.
//! - Tempo changes and the first time signature.
//! - Pitch bend on the part's channel, at the range RPN 0 sets (2
//!   semitones when none) → the pitch-bend lane, held between events.
//!
//! Written: one conductor track (tempo, time signature, name) and a track
//! per part (its name, a 12-semitone bend range, the lane as pitch bend
//! every couple of ticks where it moves, then a lyric event and the note
//! for each note), at Fontelle's own 960 a quarter so nothing is rounded.

use std::collections::BTreeMap;

use fontelle_model::{LaneCurve, TempoSegment};
use fontelle_sing::Lane;
use fontelle_types::{PPQN, Tick};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use crate::{SongPart, VocalSong, vocal_note};

/// The bend range written, in semitones: wide enough for any drawn bend.
const WRITTEN_RANGE: f64 = 12.0;

/// UTF-8 when it is, else Shift-JIS (a Japanese tool's lyrics).
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(),
    }
}

/// Karaoke's text events: `@` headers are not words; `/` and `\` start a
/// line or a paragraph.
fn karaoke(text: &str) -> Option<String> {
    if text.starts_with('@') || text.starts_with('%') {
        return None;
    }
    let t = text.trim_start_matches(['/', '\\']).trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// One track's walk.
#[derive(Default)]
struct TrackScan {
    name: Option<String>,
    /// (start, end, channel, key), file ticks.
    notes: Vec<(u64, u64, u8, u8)>,
    lyrics: Vec<(u64, String)>,
    texts: Vec<(u64, String)>,
    /// Pitch bend by channel: (tick, −8192..8191).
    bends: BTreeMap<u8, Vec<(u64, i16)>>,
    /// The bend range RPN 0 set, by channel.
    ranges: BTreeMap<u8, f64>,
}

struct Scan {
    tpq: u32,
    tracks: Vec<TrackScan>,
    tempo: Vec<(u64, u32)>,
    signature: Option<(u8, u8)>,
}

fn scan(bytes: &[u8]) -> Result<Scan, String> {
    let smf = Smf::parse(bytes).map_err(|e| format!("not a readable MIDI file: {e}"))?;
    let tpq = match smf.header.timing {
        Timing::Metrical(t) => u32::from(t.as_int()).max(1),
        Timing::Timecode(..) => return Err("SMPTE-timed MIDI files are not read".to_string()),
    };
    let mut out = Scan {
        tpq,
        tracks: Vec::new(),
        tempo: Vec::new(),
        signature: None,
    };
    for track in &smf.tracks {
        let mut t = TrackScan::default();
        let mut at: u64 = 0;
        let mut held: BTreeMap<(u8, u8), u64> = BTreeMap::new();
        let mut rpn: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
        for event in track {
            at += u64::from(event.delta.as_int());
            match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(us)) => out.tempo.push((at, us.as_int())),
                TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, _, _)) => {
                    out.signature.get_or_insert((n, d));
                }
                TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                    t.name
                        .get_or_insert_with(|| decode(name).trim().to_string());
                }
                TrackEventKind::Meta(MetaMessage::Lyric(text)) => {
                    let text = decode(text).replace(['\r', '\n'], "");
                    if !text.trim().is_empty() {
                        t.lyrics.push((at, text.trim().to_string()));
                    }
                }
                TrackEventKind::Meta(MetaMessage::Text(text)) => {
                    if let Some(text) = karaoke(&decode(text)) {
                        t.texts.push((at, text));
                    }
                }
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            held.insert((channel, key.as_int()), at);
                        }
                        MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                            if let Some(start) = held.remove(&(channel, key.as_int())) {
                                t.notes.push((start, at, channel, key.as_int()));
                            }
                        }
                        MidiMessage::PitchBend { bend } => {
                            t.bends
                                .entry(channel)
                                .or_default()
                                .push((at, bend.as_int()));
                        }
                        MidiMessage::Controller { controller, value } => {
                            let entry = rpn.entry(channel).or_insert((127, 127));
                            match controller.as_int() {
                                101 => entry.0 = value.as_int(),
                                100 => entry.1 = value.as_int(),
                                6 if *entry == (0, 0) => {
                                    t.ranges.insert(channel, f64::from(value.as_int()));
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        for ((channel, key), start) in held {
            t.notes.push((start, start + u64::from(tpq), channel, key));
        }
        t.notes.sort();
        out.tracks.push(t);
    }
    out.tempo.sort_by_key(|(t, _)| *t);
    Ok(out)
}

/// Each lyric matched to the note of its track starting on its tick (or
/// within a thirty-second after): `(note index, text)`.
fn matched(track: &TrackScan, words: &[(u64, String)], tpq: u32) -> Vec<(usize, String)> {
    let slack = u64::from(tpq / 8);
    let mut taken = vec![false; track.notes.len()];
    let mut out = Vec::new();
    for (at, text) in words {
        let found = track
            .notes
            .iter()
            .enumerate()
            .find(|(i, n)| !taken[*i] && n.0 >= *at && n.0 <= at + slack);
        if let Some((i, _)) = found {
            taken[i] = true;
            out.push((i, text.clone()));
        }
    }
    out
}

/// Whether any track has words on its notes.
pub fn has_lyrics(bytes: &[u8]) -> bool {
    let Ok(scan) = scan(bytes) else {
        return false;
    };
    let use_texts = scan.tracks.iter().all(|t| t.lyrics.is_empty());
    scan.tracks.iter().any(|t| {
        if use_texts {
            matched(t, &t.texts, scan.tpq).len() >= 2
        } else {
            !matched(t, &t.lyrics, scan.tpq).is_empty()
        }
    })
}

pub fn read(bytes: &[u8], name: &str) -> Result<VocalSong, String> {
    let scan = scan(bytes).map_err(|e| format!("{name}: {e}"))?;
    let tick =
        |t: u64| -> Tick { ((t as f64) * PPQN as f64 / f64::from(scan.tpq)).round() as Tick };
    let mut song = VocalSong::new(name);
    song.tempo = scan
        .tempo
        .iter()
        .map(|(t, us)| TempoSegment {
            start_tick: tick(*t),
            bpm: 60_000_000.0 / f64::from((*us).max(1)),
        })
        .collect();
    song.tempo.dedup_by_key(|s| s.start_tick);
    if let Some((n, d)) = scan.signature {
        let unit = 2u32.pow(u32::from(d).min(6));
        song.beat_unit = unit;
        song.beats_per_bar = u32::from(n).max(1);
    }
    let use_texts = scan.tracks.iter().all(|t| t.lyrics.is_empty());
    for (index, track) in scan.tracks.iter().enumerate() {
        if track.notes.is_empty() {
            continue;
        }
        let words = if use_texts {
            &track.texts
        } else {
            &track.lyrics
        };
        let lyrics = matched(track, words, scan.tpq);
        let part_name = track
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Track {}", index + 1));
        if lyrics.is_empty() || (use_texts && lyrics.len() < 2) {
            song.unmapped(format!(
                "the part \u{201c}{part_name}\u{201d}, which has no lyrics \u{2014} drop the \
                 file on an instrument row to bring it in"
            ));
            continue;
        }
        let mut part = SongPart::new(part_name);
        let mut words: BTreeMap<usize, String> = lyrics.into_iter().collect();
        for (i, (start, end, _, key)) in track.notes.iter().enumerate() {
            let (start, end) = (tick(*start), tick(*end));
            if end <= start {
                continue;
            }
            part.notes.push(vocal_note(
                start,
                end - start,
                *key,
                words.remove(&i).unwrap_or_default(),
            ));
        }
        // The bend on the channel the part's notes are on.
        let channel = track.notes[0].2;
        if let Some(bends) = track.bends.get(&channel) {
            let range = track.ranges.get(&channel).copied().unwrap_or(2.0);
            let mut points: Vec<(Tick, f32)> = Vec::new();
            let mut before = 0.0f32;
            for (t, v) in bends {
                let cents = (f64::from(*v) / 8192.0 * range * 100.0) as f32;
                if (cents - before).abs() > 1e-4 {
                    points.push((tick(*t) - 1, before));
                }
                points.push((tick(*t), cents));
                before = cents;
            }
            let curve = LaneCurve::from_samples(points, 0.0);
            if !curve.is_empty() {
                part.vocal.lanes.insert(Lane::PitchBend, curve);
            }
        }
        song.parts.push(part);
    }
    song.tidy();
    if song.parts.is_empty() {
        return Err(format!("there are no sung notes in {name}"));
    }
    Ok(song)
}

// ------------------------------------------------------------------ writing

fn vlq(mut value: u64, out: &mut Vec<u8>) {
    let mut bytes = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        bytes.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    bytes.reverse();
    out.extend(bytes);
}

/// Events at absolute ticks, ordered by tick then by `rank`.
struct Track {
    events: Vec<(u64, u8, Vec<u8>)>,
}

impl Track {
    fn new() -> Self {
        Self { events: Vec::new() }
    }

    fn push(&mut self, at: Tick, rank: u8, bytes: Vec<u8>) {
        self.events.push((at.max(0) as u64, rank, bytes));
    }

    fn meta(&mut self, at: Tick, rank: u8, kind: u8, data: &[u8]) {
        let mut bytes = vec![0xff, kind];
        vlq(data.len() as u64, &mut bytes);
        bytes.extend_from_slice(data);
        self.push(at, rank, bytes);
    }

    fn chunk(mut self) -> Vec<u8> {
        self.events.sort_by_key(|(at, rank, _)| (*at, *rank));
        let mut body = Vec::new();
        let mut last = 0u64;
        for (at, _, bytes) in &self.events {
            vlq(at - last, &mut body);
            body.extend_from_slice(bytes);
            last = *at;
        }
        body.extend_from_slice(&[0x00, 0xff, 0x2f, 0x00]);
        let mut out = b"MTrk".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }
}

pub fn write(song: &VocalSong) -> Vec<u8> {
    let mut conductor = Track::new();
    conductor.meta(0, 0, 0x03, song.name.as_bytes());
    let unit = song.beat_unit.max(1);
    let numerator = song.beats_per_bar.clamp(1, 255) as u8;
    // The file writes the beat as a power of two: the nearest one.
    let denominator = f64::from(unit).log2().round().clamp(0.0, 6.0) as u8;
    conductor.meta(0, 1, 0x58, &[numerator, denominator, 24, 8]);
    for segment in &song.tempo {
        let us = (60_000_000.0 / segment.bpm.max(0.01)).round() as u32;
        conductor.meta(segment.start_tick, 2, 0x51, &us.to_be_bytes()[1..]);
    }
    let mut tracks = vec![conductor.chunk()];
    for (index, part) in song.parts.iter().enumerate() {
        // Channels 1–16 in turn, skipping 10 (drums).
        let mut channel = (index % 15) as u8;
        if channel >= 9 {
            channel += 1;
        }
        let mut track = Track::new();
        track.meta(0, 0, 0x03, part.name.as_bytes());
        for (controller, value) in [(101u8, 0u8), (100, 0), (6, WRITTEN_RANGE as u8), (38, 0)] {
            track.push(0, 1, vec![0xb0 | channel, controller, value]);
        }
        if let Some(lane) = part.vocal.lanes.get(&Lane::PitchBend) {
            write_bend(&mut track, lane, channel);
        }
        for note in &part.notes {
            // Off before on at a shared tick, a lyric just before its note.
            track.push(
                note.start + note.length,
                3,
                vec![0x80 | channel, note.key, 0],
            );
            if !note.lyric.is_empty() {
                track.meta(note.start, 4, 0x05, note.lyric.as_bytes());
            }
            track.push(note.start, 5, vec![0x90 | channel, note.key, 100]);
        }
        tracks.push(track.chunk());
    }
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    out.extend_from_slice(&(PPQN as u16).to_be_bytes());
    for t in tracks {
        out.extend(t);
    }
    out
}

/// The lane as pitch bend, every two ticks where it moves, and back to the
/// centre after its last point.
fn write_bend(track: &mut Track, lane: &LaneCurve, channel: u8) {
    let (Some(first), Some(last)) = (lane.points().first(), lane.points().last()) else {
        return;
    };
    let value = |cents: f32| -> u16 {
        let v = f64::from(cents) / (WRITTEN_RANGE * 100.0) * 8192.0 + 8192.0;
        v.round().clamp(0.0, 16383.0) as u16
    };
    let mut sent: Option<u16> = None;
    let mut t = first.0;
    let end = last.0;
    while t <= end + 2 {
        let v = value(if t > end { 0.0 } else { lane.at(t, 0.0) });
        if sent != Some(v) && sent.is_none_or(|s| (i32::from(s) - i32::from(v)).abs() >= 3) {
            track.push(t, 2, vec![0xe0 | channel, (v & 0x7f) as u8, (v >> 7) as u8]);
            sent = Some(v);
        }
        t += 2;
    }
    if sent != Some(8192) {
        track.push(end + 2, 2, vec![0xe0 | channel, 0, 0x40]);
    }
}
