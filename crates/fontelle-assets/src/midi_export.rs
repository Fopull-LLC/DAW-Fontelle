//! `.mid` export (TDD §14.6): the inverse of [`crate::import_midi`].
//!
//! Import turns a MIDI file into a project; this turns a project back into a
//! MIDI file. It writes a **Format 1** (parallel) SMF whose ticks-per-quarter
//! is the project's own [`PPQN`], so no note timing is rescaled in either
//! direction — a note at tick 480 exports at 480 and imports back at 480.
//!
//! The song, not the roll. What plays is what is written: every note clip is
//! walked exactly the way `fontelle-sequencer` walks it — a clip's start
//! offsets its notes, a loop is unrolled into one note per pass, and every
//! note is cut at the nearer of its pass end and the clip's end — so the file
//! sounds like the arrangement, not like the raw contents of one pattern.
//!
//! What is deliberately not written, each for a reason and none silently:
//!
//! - **Slide notes.** A slide sounds no voice of its own — it bends whatever
//!   is already playing (see [`fontelle_model::Note::slide`]). Writing it as
//!   an ordinary note would double the note it bends, so it is left out. The
//!   note it slides *from* is written; MIDI has no per-note glide to carry the
//!   bend itself.
//! - **Per-note pan, fine pitch, release and the two mod values.** MIDI has no
//!   per-note field for any of them, and spraying channel-wide CC or pitch-bend
//!   would change every other note on the channel. They stay in the `.fp`
//!   project; the `.mid` is the notes.
//! - **A time-signature track.** The one signature the project counts by is
//!   written once at tick 0; a metre that changes mid-song is §6.2's remaining
//!   gap on both sides of this module.

use std::path::Path;

use fontelle_model::{ClipSource, Project};
use fontelle_types::{ChannelId, PPQN, Tick};
use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

/// Anything that can go wrong writing a `.mid` — which is only the file write
/// itself; building the bytes in memory cannot fail.
#[derive(Debug)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExportError {}

/// The MIDI spec's default tempo, for a project whose map somehow holds none.
const DEFAULT_US_PER_QUARTER: u32 = 500_000;

/// One resolved note on the song's timeline, in absolute ticks, before it is
/// grouped onto a track.
struct PlacedNote {
    channel: ChannelId,
    on: Tick,
    off: Tick,
    key: u8,
    velocity: u8,
}

/// Writes `project` to `path` as a Standard MIDI File.
pub fn export_midi(project: &Project, path: &Path) -> Result<(), ExportError> {
    let bytes = export_project_to_midi(project);
    std::fs::write(path, bytes).map_err(|e| ExportError(format!("{}: {e}", path.display())))
}

/// The project as SMF bytes.
///
/// Infallible: the only fallible step is touching the disk, which is
/// [`export_midi`]'s job — writing MIDI into a `Vec` cannot fail.
pub fn export_project_to_midi(project: &Project) -> Vec<u8> {
    let placed = placed_notes(project);

    // Which document channels carry notes, in a stable order (the notes were
    // gathered in clip then note order). Each gets its own MIDI channel and its
    // own track; MIDI has sixteen channels, so a project with more than that
    // wraps — rare, and better than dropping a part.
    let mut order: Vec<ChannelId> = Vec::new();
    for note in &placed {
        if !order.contains(&note.channel) {
            order.push(note.channel);
        }
    }
    let midi_channel = |channel: ChannelId| -> u8 {
        (order.iter().position(|c| *c == channel).unwrap_or(0) % 16) as u8
    };

    // The track names, owned here so the borrowed `TrackName` events outlive the
    // `Smf` we hand to the writer.
    let names: Vec<Vec<u8>> = order
        .iter()
        .map(|c| {
            project
                .channels
                .get(*c)
                .map(|ch| ch.name.as_bytes().to_vec())
                .unwrap_or_default()
        })
        .collect();

    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::from(PPQN as u16)),
    ));

    // Track 0 is the tempo/metre track — the conductor track a Format 1 file
    // keeps its song metadata on.
    smf.tracks.push(tempo_track(project));

    // One track per channel that carries notes.
    for (channel, name) in order.iter().zip(names.iter()) {
        let ch = u4::from(midi_channel(*channel));
        let mut events: Vec<(Tick, bool, TrackEventKind)> = Vec::new();
        for note in placed.iter().filter(|n| n.channel == *channel) {
            events.push((
                note.on,
                true, // on
                TrackEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::NoteOn {
                        key: u7::from(note.key.min(127)),
                        vel: u7::from(note.velocity.min(127)),
                    },
                },
            ));
            events.push((
                note.off,
                false, // off
                TrackEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::NoteOff {
                        key: u7::from(note.key.min(127)),
                        vel: u7::from(0),
                    },
                },
            ));
        }
        // At one tick, an off must come before an on so a note ending exactly
        // where the next of the same key begins does not kill the new one.
        events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut track = vec![TrackEvent {
            delta: u28::from(0u32),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(name)),
        }];
        let mut previous: Tick = 0;
        for (tick, _, kind) in events {
            track.push(TrackEvent {
                delta: delta(previous, tick),
                kind,
            });
            previous = tick;
        }
        track.push(TrackEvent {
            delta: u28::from(0u32),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(track);
    }

    let mut bytes = Vec::new();
    smf.write_std(&mut bytes)
        .expect("writing MIDI into a Vec cannot fail");
    bytes
}

/// The conductor track: a time signature at the top, then every tempo change.
fn tempo_track(project: &Project) -> Vec<TrackEvent<'static>> {
    let mut events: Vec<(Tick, TrackEventKind)> = Vec::new();

    // 4/4 unless the project counts otherwise; the denominator is always four
    // (PPQN is ticks per quarter — see `Project::beats_per_bar`). The two
    // trailing fields are the file-standard 24 MIDI clocks per metronome click
    // and 8 thirty-second notes per quarter.
    let numerator = project.beats_per_bar.clamp(1, 255) as u8;
    events.push((
        0,
        TrackEventKind::Meta(MetaMessage::TimeSignature(numerator, 2, 24, 8)),
    ));

    for segment in project.tempo_map.segments() {
        let us = if segment.bpm > 0.0 {
            (60_000_000.0 / segment.bpm).round() as u32
        } else {
            DEFAULT_US_PER_QUARTER
        };
        events.push((
            segment.start_tick.max(0),
            TrackEventKind::Meta(MetaMessage::Tempo(u24::from(us.min(0x00FF_FFFF)))),
        ));
    }

    // Stable by tick; a metre at 0 sorts before a tempo at 0, which is the
    // conventional order and the one a reader expects.
    events.sort_by_key(|(tick, _)| *tick);

    let mut track = Vec::new();
    let mut previous: Tick = 0;
    for (tick, kind) in events {
        track.push(TrackEvent {
            delta: delta(previous, tick),
            kind,
        });
        previous = tick;
    }
    track.push(TrackEvent {
        delta: u28::from(0u32),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    track
}

/// The delta from `previous` to `tick`, clamped into a `u28`. A negative delta
/// (events out of order) is impossible after the sort, and a delta past a
/// `u28`'s range needs a song longer than any real one — both clamp rather than
/// panic.
fn delta(previous: Tick, tick: Tick) -> u28 {
    let raw = (tick - previous).clamp(0, u28::max_value().as_int() as Tick);
    u28::from(raw as u32)
}

/// Every note the project plays, on the song's timeline, resolved exactly the
/// way `fontelle-sequencer::compile` resolves it: clip start, loop unrolling,
/// and the two-boundary cut. Read through [`Project::clip_source`] so a prefab
/// instance exports the prefab's notes.
fn placed_notes(project: &Project) -> Vec<PlacedNote> {
    let mut placed = Vec::new();
    // Deterministic clip order, so the same project always writes the same file.
    let mut clip_ids: Vec<_> = project.clips.keys().collect();
    clip_ids.sort();
    for clip_id in clip_ids {
        let Some(clip) = project.clips.get(clip_id) else {
            continue;
        };
        if clip.muted {
            continue;
        }
        let Some(source) = project.clip_source(clip_id) else {
            continue;
        };
        let ClipSource::Notes(note_data) = source.as_ref() else {
            continue; // audio and automation clips are not MIDI
        };

        let period = clip.loop_length.filter(|p| *p > 0);
        for repeat in 0..clip.repeats() {
            let offset = clip.repeat_start(repeat);
            // A stable note order within a pass keeps the file byte-identical
            // run to run.
            let mut notes: Vec<_> = note_data.notes.values().collect();
            notes.sort_by_key(|n| (n.start, n.key));
            for note in notes {
                // A slide sounds no voice of its own — see the module docs.
                if note.slide {
                    continue;
                }
                // Notes past the period are not part of a repeat.
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                let start = note.start + offset;
                // The clip's end is the end, looped or not.
                if start >= clip.length {
                    continue;
                }
                let on_tick = clip.start + start;
                let pass_end = period.map_or(i64::MAX, |p| clip.start + offset + p);
                let off_tick = (on_tick + note.length)
                    .min(pass_end)
                    .min(clip.start + clip.length);
                // A zero-length note after the cut would be a note-off on top of
                // its note-on; give it at least one tick so it sounds.
                let off_tick = off_tick.max(on_tick + 1);

                placed.push(PlacedNote {
                    channel: note.channel_or(note_data.channel),
                    on: on_tick,
                    off: off_tick,
                    key: note.key,
                    velocity: note.velocity,
                });
            }
        }
    }
    placed
}
