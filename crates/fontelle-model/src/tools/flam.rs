//! Flam and grace notes: a short note just before (or on) each note (FL
//! Studio's Flam, Live 12's Ornament).

use super::ToolResult;
use crate::note::Note;
use fontelle_types::{NoteId, PPQN, Tick};

/// The most grace notes one note gets.
pub const MAX_FLAM_COUNT: u8 = 4;

/// The grace note's pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FlamPitch {
    /// The same key — a drummer's flam.
    #[default]
    Same,
    /// A step above: the next key in the scale, or two semitones without one.
    Above,
    Below,
}

/// What Flam is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FlamSpec {
    /// How long each grace note is, and how far apart.
    pub time: Tick,
    /// Before the note, leaving it where it is; otherwise on its start,
    /// pushing the note itself later (and shorter, so it still ends where
    /// it did).
    pub before: bool,
    /// The grace notes' velocity, as a percentage of their note's.
    pub velocity: i32,
    pub pitch: FlamPitch,
    pub count: u8,
}

impl Default for FlamSpec {
    fn default() -> Self {
        Self {
            time: PPQN / 16,
            before: true,
            velocity: 60,
            pitch: FlamPitch::Same,
            count: 1,
        }
    }
}

/// `scope` with grace notes added. `key` (a mask of pitch classes) is what
/// "a step above" means when there is one.
pub fn flammed(scope: &[(NoteId, Note)], spec: FlamSpec, key: Option<u16>) -> ToolResult {
    let time = spec.time.max(1);
    let count = Tick::from(spec.count.clamp(1, MAX_FLAM_COUNT));
    let mask = key.filter(|m| m & 0xFFF != 0);
    let mut result = ToolResult::default();
    for (id, note) in scope {
        let grace_key = match spec.pitch {
            FlamPitch::Same => note.key,
            FlamPitch::Above => step(note.key, 1, mask),
            FlamPitch::Below => step(note.key, -1, mask),
        };
        // Before the note when there is room; a note at the very start of
        // the clip makes room instead, as it would on the beat — a flam
        // that silently skipped bar one is the case people notice first.
        let before = spec.before && note.start - time * count >= 0;
        let first = if before {
            note.start - time * count
        } else {
            let moved = note.start + time * count;
            let end = note.start + note.length;
            if moved >= end {
                continue;
            }
            result.rewrite.push((
                *id,
                Note {
                    start: moved,
                    length: end - moved,
                    ..note.clone()
                },
            ));
            note.start
        };
        for j in 0..count {
            result.insert.push(Note {
                start: first + time * j,
                length: time,
                key: grace_key,
                velocity: (f64::from(note.velocity) * f64::from(spec.velocity.clamp(1, 100))
                    / 100.0)
                    .round()
                    .clamp(1.0, 127.0) as u8,
                slide: false,
                path: Vec::new(),
                ..note.clone()
            });
        }
    }
    result
}

/// One step from `key`: to the next key of the scale that way, or two
/// semitones without one.
fn step(key: u8, way: i32, mask: Option<u16>) -> u8 {
    match mask {
        Some(m) => {
            let mut k = i32::from(key);
            loop {
                k += way;
                if !(0..=127).contains(&k) {
                    return key;
                }
                if m & (1 << (k % 12)) != 0 {
                    return k as u8;
                }
            }
        }
        None => (i32::from(key) + 2 * way).clamp(0, 127) as u8,
    }
}
