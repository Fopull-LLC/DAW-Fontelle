//! `.fsc` import: FL Studio's piano-roll **score** files.
//!
//! A score is a phrase — the notes of one pattern, with no instrument, no
//! tempo and no arrangement — which is exactly what the piano roll's
//! *Import score* is for: a chord set, an arpeggio, a drum fill, dropped into
//! whatever clip is open.
//!
//! # The format, and where it came from
//!
//! Image-Line publishes no specification, so this was read off FL Studio's own
//! factory score library: 609 files holding 5523 notes, written by every
//! version of FL from 3.0.0 to 20.9.0. Every rule below holds across all of
//! them with no exceptions, and `tests/fsc_import.rs` can be pointed back at
//! that library to say so.
//!
//! A score is the same container a `.flp` project is:
//!
//! ```text
//! "FLhd" <u32 length> <i16 format> <u16 channels> <u16 ppq>
//! "FLdt" <u32 length> <event>*
//! ```
//!
//! An event is a byte id and a payload whose *shape* is the id's range: `0..64`
//! one byte, `64..128` two, `128..192` four, and `192..=255` a variable-length
//! count followed by that many bytes. Three of those matter here — 199 is the
//! version string, 65 is the pattern number, and **224 is the notes** — and
//! the rest are skipped by shape, which is what lets a file written by a newer
//! FL than this code has ever seen still be read.
//!
//! # The one rule that cannot be guessed
//!
//! A note record is **20 bytes** in a file written before FL 8 and **24** from
//! FL 8 on, and the version string is the only thing in the file that says
//! which. The block's length cannot settle it: 120 bytes is six narrow notes
//! or five wide ones, and 240 of the 609 files in FL's own library are
//! ambiguous that way. A reader that guessed would, on those files, drop a
//! note and read every field of the others out of the wrong byte — silently,
//! because every byte of a real note is a plausible value of some other field.
//! So a file with no version string is refused rather than guessed at.
//!
//! # What is not read
//!
//! A score carries no instrument, tempo, or time signature, so there is
//! nothing to drop. What *is* dropped is the per-note MIDI channel and note
//! group, which this document has nowhere to put; both are recorded here
//! rather than silently ignored, and neither changes what the phrase plays.

use std::path::Path;

use fontelle_model::Note;
use fontelle_types::{PPQN, Tick};

use crate::ImportError;

/// FL's flag bit for a slide note — the same thing [`Note::slide`] is.
const FL_SLIDE: u16 = 0x0008;

/// Where FL's pan knob sits when it is centred, on its own 0..=128 scale.
const FL_PAN_CENTRE: i32 = 64;
/// Where FL's fine-pitch knob sits when it is centred, on its own 0..=240
/// scale. The knob covers one semitone either way.
const FL_FINE_CENTRE: i32 = 120;
/// How far FL's fine pitch reaches from centre, in cents.
const FL_FINE_CENTS: i32 = 100;
/// Where FL's release knob sits when it is centred, on its own 0..=128 scale.
/// At the centre it is the instrument's own release, which is what this
/// document's `0` means.
const FL_RELEASE_CENTRE: i32 = 64;
/// The top of FL's free modulation values. This document's are half as wide.
const FL_MOD_MAX: i32 = 255;

/// The event that carries the notes.
const EVENT_NOTES: u8 = 224;
/// The event that carries the version string FL wrote the file with.
const EVENT_VERSION: u8 = 199;

/// The FL version from which a note record is 24 bytes rather than 20.
const WIDE_RECORD_FROM: u32 = 8;

/// The FL version from which a note record carries a fine-pitch byte at all.
///
/// Before 3.3 the byte is present and always zero, which read as a value is a
/// full semitone flat — what every note of the oldest files in FL's own
/// library would import as. A version rule rather than "treat zero as absent"
/// because in a modern file a zero there is a real value somebody dialled in,
/// and the two cases are told apart by the only thing that can tell them
/// apart.
const FINE_PITCH_FROM: (u32, u32) = (3, 3);

/// One note out of a score, with the thing the score knows about it that this
/// document's [`Note`] does not: which instrument of the pattern it belonged
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FscNote {
    pub note: Note,
    /// FL's channel-rack slot, zero-based. Every score in FL's own library
    /// uses only slot 0 — a score saved from a pattern holding several
    /// instruments is where this is more than one value.
    pub rack_channel: u16,
}

/// A score file, read.
#[derive(Debug, Clone)]
pub struct FscScore {
    /// The file's own name, without its extension — what a clip made of this
    /// gets called.
    pub name: String,
    /// The version string FL wrote. Kept because it is what decided how the
    /// notes were read, so a file that imports oddly can be asked about.
    pub version: String,
    /// The file's own ticks per quarter note. Almost always 96.
    pub ppq: u32,
    /// Every note, earliest first and by pitch within a tick, on this
    /// project's grid and **at the position the file gives it**.
    ///
    /// Not shifted to zero: a phrase that begins on the second beat begins
    /// there on purpose, and a reader that helpfully moved it would silently
    /// rewrite the rhythm of every score with a rest at its front. Where the
    /// phrase goes is the caller's decision — see [`FscScore::phrase_on`],
    /// which is that decision made the usual way.
    pub notes: Vec<FscNote>,
    /// Which instruments the score used, in FL's own order.
    pub rack_channels: Vec<u16>,
    /// Where the first note is, so a caller can drop the phrase at a pointer
    /// without working it out again.
    pub start: Tick,
    /// The end of the last note. Measured from zero, like [`start`](Self::start).
    pub length: Tick,
}

impl FscScore {
    /// The notes on one instrument, or all of them for `None`.
    ///
    /// Handed back as plain [`Note`]s because that is what a clip holds: which
    /// rack slot they came from is a fact about the file, not about the music.
    pub fn notes_on(&self, rack_channel: Option<u16>) -> Vec<Note> {
        self.notes
            .iter()
            .filter(|held| rack_channel.is_none_or(|want| held.rack_channel == want))
            .map(|held| held.note)
            .collect()
    }

    /// The notes on one instrument, moved so the earliest of them is at zero.
    ///
    /// What pasting a score into a clip wants: the phrase lands where it was
    /// dropped rather than wherever it happened to sit in somebody else's
    /// pattern. Measured per instrument, so pulling one part out of a score
    /// does not leave it with the rest's leading rest.
    pub fn phrase_on(&self, rack_channel: Option<u16>) -> Vec<Note> {
        let mut notes = self.notes_on(rack_channel);
        let offset = notes.iter().map(|note| note.start).min().unwrap_or(0);
        for note in &mut notes {
            note.start -= offset;
        }
        notes
    }

    /// How many notes are on one instrument.
    pub fn count_on(&self, rack_channel: u16) -> usize {
        self.notes
            .iter()
            .filter(|held| held.rack_channel == rack_channel)
            .count()
    }
}

/// Reads a `.fsc` file.
pub fn import_fsc(path: &Path) -> Result<FscScore, ImportError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ImportError(format!("could not read {}: {e}", path.display())))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported score".to_string());
    read_fsc(&bytes, &name)
}

/// [`import_fsc`] with the bytes in hand, so the format can be tested without
/// a file on disk.
pub fn read_fsc(bytes: &[u8], name: &str) -> Result<FscScore, ImportError> {
    if bytes.len() < 8 || &bytes[0..4] != b"FLhd" {
        return Err(ImportError(
            "this is not an FL Studio score: it does not begin with FL Studio's own \
             file header"
                .into(),
        ));
    }
    let header_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let mut at = 8usize;
    if header_len < 6 || bytes.len() < at + header_len {
        return Err(ImportError(
            "this FL Studio file's header is too short to read".into(),
        ));
    }
    let ppq = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap()) as u32;
    at += header_len;

    if ppq == 0 {
        return Err(ImportError(
            "this FL Studio score declares a resolution of zero ticks per quarter \
             note, so there is no grid to place its notes on"
                .into(),
        ));
    }

    if bytes.len() < at + 8 || &bytes[at..at + 4] != b"FLdt" {
        return Err(ImportError(
            "this FL Studio file has no data section after its header".into(),
        ));
    }
    let data_len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
    at += 8;
    // The shorter of what the file claims and what it actually holds: a
    // truncated download declares the length it was going to have.
    let end = at.saturating_add(data_len).min(bytes.len());

    let mut version: Option<String> = None;
    let mut block: Option<&[u8]> = None;

    while at < end {
        let id = bytes[at];
        at += 1;
        let payload = match id {
            0..=63 => 1,
            64..=127 => 2,
            128..=191 => 4,
            // A variable-length count, seven bits a byte, little end first —
            // the opposite order to MIDI's.
            192..=255 => {
                let mut length = 0usize;
                let mut shift = 0u32;
                loop {
                    if at >= end {
                        return Err(ImportError(
                            "this FL Studio score ends in the middle of an event".into(),
                        ));
                    }
                    let byte = bytes[at];
                    at += 1;
                    length |= ((byte & 0x7f) as usize) << shift;
                    shift += 7;
                    if byte & 0x80 == 0 {
                        break;
                    }
                    if shift > 28 {
                        return Err(ImportError(
                            "this FL Studio score declares an event longer than any \
                             file can hold"
                                .into(),
                        ));
                    }
                }
                length
            }
        };
        if at + payload > end {
            // Named per event, because the notes are the one whose truncation
            // is worth saying out loud: everything else can be skipped.
            if id == EVENT_NOTES {
                return Err(ImportError(
                    "this FL Studio score's note block is cut short — the file says it \
                     holds more notes than are in it"
                        .into(),
                ));
            }
            return Err(ImportError(
                "this FL Studio score is cut short in the middle of an event".into(),
            ));
        }
        match id {
            EVENT_VERSION => {
                let text = &bytes[at..at + payload];
                let text = text.split(|b| *b == 0).next().unwrap_or(text);
                version = Some(String::from_utf8_lossy(text).into_owned());
            }
            EVENT_NOTES => block = Some(&bytes[at..at + payload]),
            // Everything else is skipped by shape rather than by name, which
            // is what lets a file from a newer FL still be read.
            _ => {}
        }
        at += payload;
    }

    let Some(version) = version else {
        return Err(ImportError(
            "this FL Studio score does not say which version wrote it, and that is \
             the only thing in the file that says how wide a note is"
                .into(),
        ));
    };
    let Some(block) = block.filter(|block| !block.is_empty()) else {
        return Err(ImportError(
            "there are no notes in this FL Studio score".into(),
        ));
    };

    let width = record_bytes(&version);
    let fine_written = has_fine_pitch(&version);
    if block.len() % width != 0 {
        return Err(ImportError(format!(
            "this FL Studio score's note block is {} bytes, which is not a whole \
             number of the {width}-byte notes a file written by FL {version} holds",
            block.len()
        )));
    }

    let mut notes: Vec<FscNote> = block
        .chunks_exact(width)
        .map(|record| read_note(record, width, ppq, fine_written))
        .collect();

    // Earliest first, and by pitch within a tick so a chord reads top to
    // bottom the same way every time.
    notes.sort_by_key(|held| (held.note.start, held.note.key, held.rack_channel));

    let start = notes.first().map(|held| held.note.start).unwrap_or(0);
    let length = notes
        .iter()
        .map(|held| held.note.start + held.note.length)
        .max()
        .unwrap_or(0);

    let mut rack_channels: Vec<u16> = notes.iter().map(|held| held.rack_channel).collect();
    rack_channels.sort_unstable();
    rack_channels.dedup();

    Ok(FscScore {
        name: name.to_string(),
        version,
        ppq,
        notes,
        rack_channels,
        start,
        length,
    })
}

/// How wide a note record is in a file written by `version`.
///
/// See this module's own note on why this cannot be worked out from the block
/// length instead.
fn record_bytes(version: &str) -> usize {
    if major_minor(version).0 >= WIDE_RECORD_FROM {
        24
    } else {
        20
    }
}

/// Whether a file written by `version` has a fine-pitch byte worth reading.
/// See [`FINE_PITCH_FROM`].
fn has_fine_pitch(version: &str) -> bool {
    major_minor(version) >= FINE_PITCH_FROM
}

/// The first two numbers of an FL version string, missing parts read as zero.
fn major_minor(version: &str) -> (u32, u32) {
    let mut parts = version.split('.').map(|p| p.parse().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// One note record, in this document's units.
///
/// The two widths share their first thirteen bytes; what the wide one adds is
/// the note group, the MIDI channel, and a release the narrow one has no room
/// for. Everything after that is at a different offset in each, which is the
/// whole reason the width has to be known before a single field is read.
fn read_note(record: &[u8], width: usize, ppq: u32, fine_written: bool) -> FscNote {
    let position = u32::from_le_bytes(record[0..4].try_into().unwrap());
    let flags = u16::from_le_bytes(record[4..6].try_into().unwrap());
    let rack_channel = u16::from_le_bytes(record[6..8].try_into().unwrap());
    let length = u32::from_le_bytes(record[8..12].try_into().unwrap());
    let key = record[12];

    let (fine, release, pan, velocity, mod_x, mod_y) = if width == 24 {
        (
            record[16], record[18], record[20], record[21], record[22], record[23],
        )
    } else {
        (
            record[13],
            FL_RELEASE_CENTRE as u8,
            record[16],
            record[17],
            record[18],
            record[19],
        )
    };

    FscNote {
        rack_channel,
        note: Note {
            start: to_project_ticks(position, ppq),
            // The shortest thing this grid can hold. FL writes no zero-length
            // notes, but a file that did would otherwise import as silence.
            length: to_project_ticks(length, ppq).max(1),
            key: key.min(127),
            // FL counts velocity 0..=128 where this document counts 1..=127,
            // so exactly one value at each end has nowhere to go. Squeezing
            // the scale would move *every* note instead of those two, and
            // 100 — the velocity on almost every note FL ever wrote — would
            // arrive as 99.
            velocity: (velocity as i32).clamp(1, 127) as u8,
            pan: scale_from_centre(pan as i32, FL_PAN_CENTRE, 0, 128, 127) as i8,
            fine_pitch: read_fine_pitch(fine, fine_written) as i16,
            release: read_release(release),
            // Both are free modulation values with no agreed neutral at
            // either end, so the scale is preserved rather than the centre:
            // what was drawn keeps its shape.
            mod_x: ((mod_x as i32) * 127 / FL_MOD_MAX) as u8,
            mod_y: ((mod_y as i32) * 127 / FL_MOD_MAX) as u8,
            slide: flags & FL_SLIDE != 0,
            channel: None,
        },
    }
}

/// FL's fine-pitch byte, in cents — or in tune, in a file too old to have one.
/// See [`FINE_PITCH_FROM`].
fn read_fine_pitch(fine: u8, written: bool) -> i32 {
    if !written {
        return 0;
    }
    scale_from_centre(fine as i32, FL_FINE_CENTRE, 0, 240, FL_FINE_CENTS)
}

/// FL's release knob, as this document's per-note release.
///
/// FL's knob shortens below its centre and lengthens above it; this document's
/// field only lengthens, and `0` is *the patch's own* release rather than the
/// shortest one (see `EventPayload::NoteOn::release`). So the bottom half
/// collapses onto zero. Reading it as a fraction of the top instead would make
/// every note FL shortened ring *longer* than its instrument asks for, which
/// is the opposite of what was written.
fn read_release(release: u8) -> u8 {
    let above = (release as i32 - FL_RELEASE_CENTRE).max(0);
    (above * 127 / FL_RELEASE_CENTRE).min(127) as u8
}

/// A value on a `low..=high` scale centred at `centre`, onto `-reach..=reach`.
///
/// The two halves are scaled separately so both ends land exactly on `reach`
/// rather than one of them stopping just short — the same rule
/// `midi_import::pan_cc_to_pan` follows, and for the same reason.
fn scale_from_centre(value: i32, centre: i32, low: i32, high: i32, reach: i32) -> i32 {
    let value = value.clamp(low, high);
    if value >= centre {
        let span = (high - centre).max(1);
        (value - centre) * reach / span
    } else {
        let span = (centre - low).max(1);
        (value - centre) * reach / span
    }
}

/// An FL tick on this project's grid.
///
/// Exact wherever the resolutions divide and rounded to nearest where they do
/// not — the same rule MIDI import follows, and for the same reason: a
/// truncation would drag every note fractionally early, which is a rhythm that
/// is subtly and consistently wrong rather than one that is jittery.
fn to_project_ticks(fl_tick: u32, ppq: u32) -> Tick {
    let ppq = ppq.max(1) as u64;
    ((fl_tick as u64 * PPQN as u64 + ppq / 2) / ppq) as Tick
}
