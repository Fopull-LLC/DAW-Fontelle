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

use fontelle_model::Arena;
use fontelle_model::{
    Channel, Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap, TempoSegment,
};
use fontelle_types::{ChannelId, NoteId, PPQN, Tick};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use crate::ImportError;
use crate::general_midi::general_midi_name;

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

/// MIDI controller numbers. Only these two are read; the rest are ignored, and
/// the channel listing reports what was found so nothing vanishes silently.
const CC_CHANNEL_VOLUME: u8 = 7;
const CC_PAN: u8 = 10;

/// A channel that sends no CC7 is not at full scale — MIDI's reset value for
/// channel volume is 100. Every part in a file that sends none is then equally
/// ~4 dB down, which is a uniform offset rather than a balance error.
const DEFAULT_VOLUME_CC: u8 = 100;
/// CC10's centre. 0 and 127 are the ends of the field.
const CENTRE_PAN_CC: u8 = 64;

/// The floor a CC7 of 0 lands on. Real silence is `-inf` dB, which no fader can
/// hold and no arithmetic downstream survives; -96 dB is below the noise floor
/// of 16-bit audio and is a number.
const SILENT_DB: f32 = -96.0;

/// MIDI channel volume in decibels, on the curve GM2 and DLS both specify:
/// `40 * log10(value / 127)`. A linear reading of the controller would make
/// every balance in every General MIDI file wrong by the difference.
fn volume_cc_to_db(value: u8) -> f32 {
    if value == 0 {
        return SILENT_DB;
    }
    (40.0 * (value as f32 / 127.0).log10()).max(SILENT_DB)
}

/// CC10 to Fontelle's -1.0..=1.0 pan. 64 is centre; the two halves are scaled
/// by 63 and 64 respectively so that 0 and 127 both reach the ends exactly
/// rather than one of them stopping just short.
fn pan_cc_to_pan(value: u8) -> f32 {
    let centre = CENTRE_PAN_CC as f32;
    if value >= CENTRE_PAN_CC {
        (value as f32 - centre) / (127.0 - centre)
    } else {
        (value as f32 - centre) / centre
    }
}

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
    /// What this part is called — see [`part_name`]. Carried even though it
    /// was skipped, because the thing that lists what was left out is also
    /// the thing that offers to go back and get it.
    pub name: String,
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
    /// Where the file asks for this part in the stereo field, from its first
    /// CC10: -1.0 hard left, 0.0 centre, +1.0 hard right.
    pub pan: f32,
    /// The part's level in decibels, from its first CC7 through the GM2/DLS
    /// curve. A channel that sends none takes MIDI's own default of 100,
    /// which is about -4 dB — not unity, and reading it as unity would put
    /// every silent channel above the ones that spelled the default out.
    pub volume_db: f32,
    /// What the part is called, and what the channel and its mixer track are
    /// named in the document — see [`part_name`].
    pub name: String,
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
    /// The file's **opening** tempo, for reporting. The whole curve is in
    /// `project.tempo_map`; a piece that changes tempo is not summarised by
    /// any single number, and averaging would be worse than being clear.
    pub bpm: f64,
    /// How many tempo segments the file produced. One means constant tempo.
    pub tempo_changes: usize,
}

/// One part of a MIDI file, as [`survey_midi`] reports it.
///
/// Deliberately not [`ImportedMidiChannel`]: that one names ids in a document
/// that a survey does not build.
#[derive(Debug, Clone)]
pub struct MidiPart {
    /// Zero-based, so channel 10 in a DAW's UI is 9 here.
    pub channel: u8,
    pub notes: usize,
    pub program: Option<u8>,
    pub is_percussion: bool,
    /// What to call it — see [`part_name`].
    pub name: String,
}

/// What is inside a `.mid` file, read without building anything.
///
/// A MIDI file may hold one part or sixteen, and which it is decides what
/// importing it should mean: a one-part file is a phrase to drop on the
/// instrument you have, and a sixteen-part file is a song that wants a track
/// each. Asking the user that question needs the answer first, and importing
/// the file to find out — then throwing it away if they say no — is a way to
/// lose whatever was open.
#[derive(Debug, Clone)]
pub struct MidiSurvey {
    /// The file's own name, without its extension.
    pub name: String,
    /// Every channel that carries notes, in channel order. Percussion
    /// included: the survey reports what is *there*, and what to leave out is
    /// the import's decision.
    pub parts: Vec<MidiPart>,
    /// The file's opening tempo. The whole curve is only built on import.
    pub bpm: f64,
    /// How many tempo segments the file would produce. One means constant.
    pub tempo_changes: usize,
    /// The file's own resolution, in ticks per quarter note.
    pub ticks_per_quarter: u32,
    /// How long the piece is, on **this project's** grid.
    pub length: Tick,
}

impl MidiSurvey {
    /// Whether this file holds more than one part, which is the whole question
    /// the survey exists to answer.
    pub fn is_multi_part(&self) -> bool {
        self.parts.len() > 1
    }

    /// How many notes are in the file altogether.
    pub fn notes(&self) -> usize {
        self.parts.iter().map(|part| part.notes).sum()
    }
}

/// A note-on waiting for its note-off.
struct Pending {
    start: Tick,
    velocity: u8,
}

/// Everything one walk of the file finds, before any of it becomes a document.
///
/// One walk rather than two, and one shape for both callers: a survey that
/// counted its notes differently from the import that follows it would offer a
/// list of things you do not get.
struct Scan {
    ticks_per_quarter: u32,
    /// Every tempo event, at the tick it takes effect.
    tempo_events: Vec<(u64, u32)>,
    /// Notes, per MIDI channel — only for the channels `accept` let through,
    /// and only when the caller asked for them at all.
    per_channel: HashMap<u8, Arena<NoteId, Note>>,
    /// How many notes each channel carried, **whether or not it was accepted**.
    counts: HashMap<u8, usize>,
    programs: HashMap<u8, u8>,
    controllers: HashMap<(u8, u8), u8>,
    /// What each channel's own track called itself — see [`part_name`].
    track_names: HashMap<u8, String>,
    /// The last tick any note ends on, in the file's own ticks.
    end: u64,
}

impl Scan {
    /// The channels that carried notes, in channel order.
    fn channels(&self) -> Vec<u8> {
        let mut channels: Vec<u8> = self.counts.keys().copied().collect();
        channels.sort_unstable();
        channels
    }

    fn part(&self, channel: u8) -> MidiPart {
        MidiPart {
            channel,
            notes: self.counts.get(&channel).copied().unwrap_or(0),
            program: self.programs.get(&channel).copied(),
            is_percussion: channel == PERCUSSION_CHANNEL,
            name: part_name(
                channel,
                self.track_names.get(&channel).map(String::as_str),
                self.programs.get(&channel).copied(),
            ),
        }
    }
}

/// What to call a part, best source first.
///
/// A part called *"Channel 4"* tells you nothing, and a MIDI file almost
/// always knows better. Three places to look, in the order they are worth
/// trusting:
///
/// 1. **The track's own name.** Whoever wrote the file typed it, so it beats
///    anything derived. Only when the track holds this one channel, though —
///    see [`scan`].
/// 2. **The General MIDI program** it selects. `33` is a fretless bass in
///    every file that ever selected it.
/// 3. **The percussion channel**, which is drums by definition.
///
/// And a number as the floor, counted from 1 the way a keyboard's own front
/// panel counts channels rather than the way the wire does.
pub fn part_name(channel: u8, track_name: Option<&str>, program: Option<u8>) -> String {
    if let Some(name) = track_name {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    // Before the program, because a program change on channel 10 selects a
    // *kit* and its number means nothing on the melodic list — reading it
    // there is how a drum track comes to be called "Acoustic Grand Piano".
    if channel == PERCUSSION_CHANNEL {
        return "Drums".to_string();
    }
    if let Some(program) = program {
        return general_midi_name(program).to_string();
    }
    format!("Channel {}", channel as u16 + 1)
}

/// Parses the file and settles its timing, which both entry points need.
fn read_smf<'bytes>(bytes: &'bytes [u8], what: &str) -> Result<(Smf<'bytes>, u32), ImportError> {
    let smf = Smf::parse(bytes)
        .map_err(|e| ImportError(format!("{what} is not a readable MIDI file: {e}")))?;

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
    Ok((smf, ticks_per_quarter))
}

/// A file tick on this project's grid.
///
/// Exact wherever the resolutions divide, and rounded to nearest where they do
/// not. Truncating would drag every note fractionally early and rounding up
/// every note fractionally late; either way the bias is systematic, and at an
/// odd source resolution it is a rhythm that is subtly but consistently wrong
/// rather than a random jitter.
fn to_project_ticks(midi_tick: u64, ticks_per_quarter: u32) -> Tick {
    let tpq = ticks_per_quarter.max(1) as u64;
    ((midi_tick * PPQN as u64 + tpq / 2) / tpq) as Tick
}

/// Walks the file once.
///
/// `collect_notes` is what tells a survey from an import: a survey wants the
/// counts and the names and nothing else, and building sixteen arenas of notes
/// to throw them away is work nobody asked for.
fn scan(
    smf: &Smf<'_>,
    ticks_per_quarter: u32,
    channels: MidiChannels,
    collect_notes: bool,
) -> Scan {
    let mut scan = Scan {
        ticks_per_quarter,
        tempo_events: Vec::new(),
        per_channel: HashMap::new(),
        counts: HashMap::new(),
        programs: HashMap::new(),
        controllers: HashMap::new(),
        track_names: HashMap::new(),
        end: 0,
    };
    let mut pending: HashMap<(u8, u8), Pending> = HashMap::new();

    for track in &smf.tracks {
        // Delta times are per track and restart at zero, so a format-1 file's
        // tracks all begin together rather than one after another.
        let mut absolute: u64 = 0;
        // What this track calls itself, and which channels it plays. A track
        // name only names a *part* when the track holds one — see below.
        let mut track_name: Option<String> = None;
        let mut instrument_name: Option<String> = None;
        let mut channels_here: Vec<u8> = Vec::new();

        for event in track {
            absolute += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(us)) => {
                    scan.tempo_events.push((absolute, us.as_int()));
                }
                // First of each wins: a track that renames itself part-way
                // through is describing a section, not a second instrument.
                TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                    track_name.get_or_insert_with(|| String::from_utf8_lossy(name).into_owned());
                }
                TrackEventKind::Meta(MetaMessage::InstrumentName(name)) => {
                    instrument_name
                        .get_or_insert_with(|| String::from_utf8_lossy(name).into_owned());
                }
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        MidiMessage::ProgramChange { program } => {
                            scan.programs.entry(channel).or_insert(program.as_int());
                        }
                        MidiMessage::Controller { controller, value } => {
                            scan.controllers
                                .entry((channel, controller.as_int()))
                                .or_insert(value.as_int());
                        }
                        // A note-on with zero velocity is a note-off. Almost
                        // every real file uses it in place of an explicit one,
                        // and read literally it starts a silent note that never
                        // ends, so the piece plays as one endless chord.
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            *scan.counts.entry(channel).or_default() += 1;
                            if !channels_here.contains(&channel) {
                                channels_here.push(channel);
                            }
                            if collect_notes && channels.accepts(channel) {
                                pending.insert(
                                    (channel, key.as_int()),
                                    Pending {
                                        start: to_project_ticks(absolute, ticks_per_quarter),
                                        velocity: vel.as_int(),
                                    },
                                );
                            }
                        }
                        MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                            scan.end = scan.end.max(absolute);
                            if let Some(start) = pending.remove(&(channel, key.as_int())) {
                                push_note(
                                    scan.per_channel.entry(channel).or_default(),
                                    key.as_int(),
                                    &start,
                                    to_project_ticks(absolute, ticks_per_quarter),
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
            push_note(
                scan.per_channel.entry(channel).or_default(),
                key,
                &start,
                end,
            );
        }

        // **A track's name only names a part when the track holds one part.**
        // A format-0 file is one track carrying every channel, and its track
        // name is the *song's* name — handing it to all sixteen would call
        // every instrument in the piece the same thing, which is worse than a
        // number because a number at least tells two of them apart.
        if let [only] = channels_here[..] {
            let name = track_name.or(instrument_name);
            if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
                scan.track_names.entry(only).or_insert(name);
            }
        }
    }

    // Sorted by tick, and by file order within a tick — `TempoMap` resolves a
    // tie by taking the last, which is what a sequencer writing over its own
    // event means.
    scan.tempo_events.sort_by_key(|(tick, _)| *tick);
    if scan.tempo_events.is_empty() {
        scan.tempo_events.push((0, DEFAULT_US_PER_QUARTER));
    }
    scan
}

/// What is in a `.mid` file, without building a document out of it.
pub fn survey_midi(path: &Path) -> Result<MidiSurvey, ImportError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ImportError(format!("could not read {}: {e}", path.display())))?;
    read_midi_survey(&bytes, &file_name(path))
}

/// [`survey_midi`] with the bytes in hand.
pub fn read_midi_survey(bytes: &[u8], name: &str) -> Result<MidiSurvey, ImportError> {
    let (smf, ticks_per_quarter) = read_smf(bytes, name)?;
    // Every channel, because a survey reports what is *there*; what to leave
    // out is the import's decision and the user's.
    let scan = scan(&smf, ticks_per_quarter, MidiChannels::All, false);
    let parts: Vec<MidiPart> = scan.channels().iter().map(|c| scan.part(*c)).collect();
    let bpm = 60_000_000.0 / scan.tempo_events[0].1 as f64;
    let tempo_changes = tempo_segments(&scan).len();
    Ok(MidiSurvey {
        name: name.to_string(),
        parts,
        bpm,
        tempo_changes,
        ticks_per_quarter,
        length: to_project_ticks(scan.end, ticks_per_quarter),
    })
}

/// The file's tempo curve, as the document's own segments.
fn tempo_segments(scan: &Scan) -> Vec<TempoSegment> {
    let mut segments: Vec<TempoSegment> = scan
        .tempo_events
        .iter()
        .map(|(tick, us)| TempoSegment {
            start_tick: to_project_ticks(*tick, scan.ticks_per_quarter),
            bpm: 60_000_000.0 / *us as f64,
        })
        .collect();
    segments.dedup_by_key(|segment| segment.start_tick);
    segments
}

fn file_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported MIDI".to_string())
}

pub fn import_midi(path: &Path, channels: MidiChannels) -> Result<MidiImport, ImportError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ImportError(format!("could not read {}: {e}", path.display())))?;
    read_midi(&bytes, &file_name(path), channels)
}

/// [`import_midi`] with the bytes in hand.
pub fn read_midi(
    bytes: &[u8],
    name: &str,
    channels: MidiChannels,
) -> Result<MidiImport, ImportError> {
    let (smf, ticks_per_quarter) = read_smf(bytes, name)?;
    let mut scan = scan(&smf, ticks_per_quarter, channels, true);

    let segments = tempo_segments(&scan);
    let bpm = segments[0].bpm;

    let mut project = Project::new(name);
    // 48 kHz is a placeholder: the rate belongs to the audio device, and the
    // caller replaces it with `TempoMap::set_sample_rate` once it knows one.
    project.tempo_map = TempoMap::from_segments(segments, 48_000.0);
    let tempo_changes = project.tempo_map.segments().len();

    let mut midi_channels: Vec<u8> = scan.per_channel.keys().copied().collect();
    midi_channels.sort_unstable();

    let mut imported = Vec::new();
    for midi_channel in midi_channels {
        let notes = scan.per_channel.remove(&midi_channel).unwrap_or_default();
        let part = scan.part(midi_channel);
        let is_percussion = part.is_percussion;
        // The part's own name, which is what the survey showed and what the
        // prompt offered. Everything the import makes for this part — the
        // channel, its mixer track, its arrangement row — is called it.
        let label = part.name.clone();

        let controller = |number: u8, default: u8| {
            scan.controllers
                .get(&(midi_channel, number))
                .copied()
                .unwrap_or(default)
        };
        let pan = pan_cc_to_pan(controller(CC_PAN, CENTRE_PAN_CC));
        let volume_db = volume_cc_to_db(controller(CC_CHANNEL_VOLUME, DEFAULT_VOLUME_CC));

        // The file's balance between its parts, landing on the document
        // rather than being reported and thrown away. CC7 is the part's
        // fader, so it goes on a mixer track of its own; CC10 is where the
        // part sits in the field, which is the channel's own placement and
        // not a balance control over a bus (see `Channel::pan`).
        let mixer_track = project
            .mixer
            .tracks
            .insert(fontelle_model::MixerTrack::new(label.clone()));
        project.mixer.tracks[mixer_track].gain_db = volume_db;
        project.mixer.tracks[mixer_track].output = project.mixer.master;

        let channel = project.channels.insert(Channel {
            preset: None,
            name: label.clone(),
            color: CHANNEL_COLOURS[midi_channel as usize % CHANNEL_COLOURS.len()],
            // A `.mid` part is notes and a program number; what plays it is a
            // soundfont, so that is what the row says it is.
            instrument: Some(fontelle_types::InstrumentKind::SoundFont),
            // A MIDI file's parts really do each want a strip: the file
            // carries a CC7 per channel and that is a fader, so the importer
            // is the one caller that asks `AddChannel` for a track rather than
            // taking the master.
            mixer_track: Some(mixer_track),
            patch_data: None,
            plugin: None,
            pan,
            muted: false,
            soloed: false,
            named_keys: false,
            gain_db: 0.0,
        });
        let lane = project.lanes.insert(Lane {
            name: label.clone(),
            height: 32.0,
            color: CHANNEL_COLOURS[midi_channel as usize % CHANNEL_COLOURS.len()],
            muted: false,
            locked: false,
            order: 0,
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
            loop_length: None,
        });

        imported.push(ImportedMidiChannel {
            midi_channel,
            channel,
            notes: note_count,
            program: part.program,
            is_percussion,
            pan,
            volume_db,
            name: label,
        });
    }

    let skipped: Vec<MidiChannelSummary> = scan
        .channels()
        .into_iter()
        .filter(|channel| !channels.accepts(*channel))
        .map(|channel| {
            let part = scan.part(channel);
            MidiChannelSummary {
                channel,
                notes: part.notes,
                program: part.program,
                name: part.name,
            }
        })
        .collect();

    Ok(MidiImport {
        project,
        channels: imported,
        skipped,
        bpm,
        tempo_changes,
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

fn push_note(notes: &mut Arena<NoteId, Note>, key: u8, start: &Pending, end: Tick) {
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
        // An imported note is an ordinary one: MIDI has no slide.
        slide: false,
        channel: None,
    });
}
