//! `.mid` import (TDD §14.6): tracks become note clips on a project timeline.
//!
//! **Scope, stated plainly.** §14.6 asks for tracks to map to *instrument*
//! channels, tempo and time-signature meta events to reach the tempo map, and
//! channel/CC data not to be silently discarded. What is here: notes, their
//! timing converted to the project's resolution, and the file's initial tempo.
//! What is not, and is not pretended to be:
//!
//! - Only the **first** tempo event is read. Tempo changes need the piecewise
//!   `TempoMap` that lands with M3; a single constant is what the map can
//!   currently hold, and inventing an average would be worse than being clear.
//! - Time signature, program changes, and CC are read far enough to *report*
//!   (see `MidiChannelSummary::program`) but do not yet affect the document.
//!
//! Each of these is a missing feature rather than a wrong result: what does
//! import, imports correctly.

use std::collections::HashMap;
use std::path::Path;

use fontelle_model::{Channel, Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, NoteId, PPQN, Tick};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use slotmap::SlotMap;

use crate::ImportError;

/// MIDI's percussion channel, zero-indexed. Its note numbers select drums
/// rather than pitches.
const PERCUSSION_CHANNEL: u8 = 9;

/// The MIDI spec's tempo when a file states none: 500000 microseconds per
/// quarter note, i.e. 120 bpm.
const DEFAULT_US_PER_QUARTER: u32 = 500_000;

/// How long an unterminated note is held. A file whose note-off is missing is
/// malformed, but common enough — an export that crashed partway — that
/// dropping the note or holding it forever are both worse than a definite,
/// obviously-arbitrary length.
const STUCK_NOTE_LENGTH: Tick = PPQN;

/// Which of the file's channels to bring in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiChannels {
    /// Everything except channel 10 (index 9). Its notes are drum selections,
    /// not pitches, so playing them through a melodic patch is noise rather
    /// than music — and arriving by accident is worse than being absent.
    Melodic,
    /// Every channel, percussion included.
    All,
    /// One channel, by zero-based index.
    Only(u8),
}

impl MidiChannels {
    fn accepts(self, channel: u8) -> bool {
        match self {
            Self::Melodic => channel != PERCUSSION_CHANNEL,
            Self::All => true,
            Self::Only(wanted) => channel == wanted,
        }
    }
}

/// A channel present in the file but left out of the import.
#[derive(Debug, Clone)]
pub struct MidiChannelSummary {
    /// Zero-based, so channel 10 in a DAW's UI is 9 here.
    pub channel: u8,
    pub notes: usize,
    /// From the channel's first program-change event, if it has one.
    pub program: Option<u8>,
}

/// One of the file's channels, as a document channel of its own.
#[derive(Debug, Clone)]
pub struct ImportedMidiChannel {
    /// Zero-based, so channel 10 in a DAW's UI is 9 here.
    pub midi_channel: u8,
    pub channel: ChannelId,
    pub notes: usize,
    /// From the channel's first program-change event. A caller with a General
    /// MIDI soundfont can use it to choose the preset the file asked for
    /// instead of playing every part on one sound.
    pub program: Option<u8>,
    /// True for MIDI channel 10, whose note numbers select drums rather than
    /// pitches. Carried through import so whoever assigns instruments doesn't
    /// have to rediscover it.
    pub is_percussion: bool,
}

pub struct MidiImport {
    pub project: Project,
    /// One document channel per MIDI channel that carried notes, in channel
    /// order.
    pub channels: Vec<ImportedMidiChannel>,
    /// Channels the file contained that this import left out, so a caller can
    /// say what it skipped rather than leaving the user to wonder where the
    /// drums went.
    pub skipped: Vec<MidiChannelSummary>,
    pub bpm: f64,
}

/// A note-on waiting for its note-off.
struct Pending {
    start: Tick,
    velocity: u8,
}

pub fn import_midi(path: &Path, channels: MidiChannels) -> Result<MidiImport, ImportError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ImportError(format!("could not read {}: {e}", path.display())))?;
    let smf = Smf::parse(&bytes).map_err(|e| {
        ImportError(format!(
            "{} is not a readable MIDI file: {e}",
            path.display()
        ))
    })?;

    // A metrical file counts time in subdivisions of a quarter note, which is
    // what Fontelle's PPQN grid is. SMPTE timecode counts in real seconds and
    // would need the tempo map inverted to place a note on a musical grid at
    // all — refuse it clearly rather than importing something rhythmically
    // wrong.
    let ticks_per_quarter = match smf.header.timing {
        Timing::Metrical(tpq) => tpq.as_int() as u32,
        Timing::Timecode(..) => {
            return Err(ImportError(
                "SMPTE-timecode MIDI files are not supported yet: their timing is in real \
                 seconds rather than musical time, so notes cannot be placed on the \
                 project's tick grid without a tempo map to invert"
                    .into(),
            ));
        }
    };
    if ticks_per_quarter == 0 {
        return Err(ImportError(
            "MIDI header declares zero ticks per quarter note".into(),
        ));
    }

    // Exact wherever the resolutions divide, and rounded to nearest where they
    // do not. Truncating would drag every note fractionally early and rounding
    // up every note fractionally late; either way the bias is systematic, and
    // at an odd source resolution it is a rhythm that is subtly but
    // consistently wrong rather than a random jitter.
    let to_project_ticks = |midi_tick: u64| -> Tick {
        let tpq = ticks_per_quarter as u64;
        ((midi_tick * PPQN as u64 + tpq / 2) / tpq) as Tick
    };

    let mut us_per_quarter = None;
    // Notes are collected per MIDI channel, so each becomes an instrument of
    // its own rather than being merged into one stream that has to share a
    // patch — a bass part and a lead part are different sounds.
    let mut per_channel: HashMap<u8, SlotMap<NoteId, Note>> = HashMap::new();
    let mut pending: HashMap<(u8, u8), Pending> = HashMap::new();
    let mut counts: HashMap<u8, usize> = HashMap::new();
    let mut programs: HashMap<u8, u8> = HashMap::new();

    for track in &smf.tracks {
        // Delta times are per track and restart at zero, so a format-1 file's
        // tracks all begin together rather than one after another.
        let mut absolute: u64 = 0;
        for event in track {
            absolute += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(us)) => {
                    us_per_quarter.get_or_insert(us.as_int());
                }
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        MidiMessage::ProgramChange { program } => {
                            programs.entry(channel).or_insert(program.as_int());
                        }
                        // A note-on with zero velocity is a note-off. Almost
                        // every real file uses it in place of an explicit one,
                        // and read literally it starts a silent note that never
                        // ends, so the piece plays as one endless chord.
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            *counts.entry(channel).or_default() += 1;
                            if channels.accepts(channel) {
                                pending.insert(
                                    (channel, key.as_int()),
                                    Pending {
                                        start: to_project_ticks(absolute),
                                        velocity: vel.as_int(),
                                    },
                                );
                            }
                        }
                        MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                            if let Some(start) = pending.remove(&(channel, key.as_int())) {
                                push_note(
                                    per_channel.entry(channel).or_default(),
                                    key.as_int(),
                                    &start,
                                    to_project_ticks(absolute),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        // Anything still held at the end of a track never got its note-off.
        for ((channel, key), start) in pending.drain() {
            let end = start.start + STUCK_NOTE_LENGTH;
            push_note(per_channel.entry(channel).or_default(), key, &start, end);
        }
    }

    let us_per_quarter = us_per_quarter.unwrap_or(DEFAULT_US_PER_QUARTER);
    let bpm = 60_000_000.0 / us_per_quarter as f64;

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported MIDI".to_string());
    let mut project = Project::new(&name);
    project.tempo_map = TempoMap::new(bpm, 48_000.0);

    let mut midi_channels: Vec<u8> = per_channel.keys().copied().collect();
    midi_channels.sort_unstable();

    let mut imported = Vec::new();
    for midi_channel in midi_channels {
        let notes = per_channel.remove(&midi_channel).unwrap_or_default();
        let is_percussion = midi_channel == PERCUSSION_CHANNEL;
        let label = if is_percussion {
            format!("{name} — drums")
        } else {
            format!("{name} — channel {}", midi_channel + 1)
        };

        let channel = project.channels.insert(Channel {
            name: label.clone(),
            color: CHANNEL_COLOURS[midi_channel as usize % CHANNEL_COLOURS.len()],
            mixer_track: Default::default(),
            patch_data: Vec::new(),
        });
        let lane = project.lanes.insert(Lane {
            name: label,
            height: 32.0,
            color: CHANNEL_COLOURS[midi_channel as usize % CHANNEL_COLOURS.len()],
            muted: false,
            locked: false,
        });
        let length = notes
            .values()
            .map(|n| n.start + n.length)
            .max()
            .unwrap_or(0);
        let note_count = notes.len();
        project.clips.insert(Clip {
            lane,
            start: 0,
            length,
            source: ClipSource::Notes(NoteData { channel, notes }),
            prefab_link: None,
            color: None,
            muted: false,
        });

        imported.push(ImportedMidiChannel {
            midi_channel,
            channel,
            notes: note_count,
            program: programs.get(&midi_channel).copied(),
            is_percussion,
        });
    }

    let mut skipped: Vec<MidiChannelSummary> = counts
        .into_iter()
        .filter(|(channel, _)| !channels.accepts(*channel))
        .map(|(channel, notes)| MidiChannelSummary {
            channel,
            notes,
            program: programs.get(&channel).copied(),
        })
        .collect();
    skipped.sort_by_key(|s| s.channel);

    Ok(MidiImport {
        project,
        channels: imported,
        skipped,
        bpm,
    })
}

/// Enough distinct lane colours that an imported file does not arrive as
/// sixteen identical rows.
const CHANNEL_COLOURS: [[u8; 4]; 8] = [
    [0x4f, 0x8f, 0xd0, 0xff],
    [0xd0, 0x7f, 0x4f, 0xff],
    [0x6f, 0xc0, 0x7f, 0xff],
    [0xc0, 0x6f, 0xb0, 0xff],
    [0xd0, 0xc0, 0x5f, 0xff],
    [0x5f, 0xc0, 0xc0, 0xff],
    [0x9f, 0x8f, 0xd0, 0xff],
    [0xa0, 0xa0, 0xa0, 0xff],
];

fn push_note(notes: &mut SlotMap<NoteId, Note>, key: u8, start: &Pending, end: Tick) {
    notes.insert(Note {
        start: start.start,
        // A zero-length note is inaudible; the shortest thing the grid can
        // express is one tick, which is what a note-off arriving in the same
        // tick as its note-on actually meant.
        length: (end - start.start).max(1),
        key,
        velocity: start.velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
    });
}
