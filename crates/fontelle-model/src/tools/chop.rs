//! Chop: each note cut into repeats (FL Studio's Chop and Quick chop,
//! Live 12's Chop). Ty, 2026-10-20: *"chop (lke repeated chops basically)"*.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::{NoteId, PPQN, Tick};

/// The longest step pattern the dialog draws.
pub const MAX_CHOP_PATTERN: u8 = 16;

/// How the pieces' velocities go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ChopVelocity {
    #[default]
    Flat,
    /// Each piece softer than the one before.
    Fall,
    /// Each piece harder.
    Rise,
    /// The first piece of every pattern at full weight, the rest softer.
    Accent,
}

/// What Chop is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChopSpec {
    /// The length of one piece, in ticks.
    pub step: Tick,
    /// 0.05..=1: how much of its step each piece sounds for.
    pub gate: f32,
    /// One bit per step, lowest first: a clear bit is a step left silent.
    pub pattern: u16,
    /// How many of `pattern`'s bits are the pattern; it repeats after them.
    pub pattern_len: u8,
    pub velocity: ChopVelocity,
    /// Semitones added per piece — a stair rather than a stutter.
    pub pitch_step: i8,
    /// Cuts fall on the song's grid (every multiple of `step`), rather than
    /// every `step` from each note's own start.
    pub on_grid: bool,
}

impl Default for ChopSpec {
    fn default() -> Self {
        Self {
            step: PPQN / 4,
            gate: 1.0,
            pattern: u16::MAX,
            pattern_len: 4,
            velocity: ChopVelocity::Flat,
            pitch_step: 0,
            on_grid: true,
        }
    }
}

/// Every note of `scope` long enough to cut, cut. A note that would come out
/// as one piece the same as itself is left alone.
pub fn chopped(scope: &[(NoteId, Note)], spec: ChopSpec) -> ToolResult {
    if spec.step <= 0 {
        return ToolResult::default();
    }
    let pattern_len = u32::from(spec.pattern_len.clamp(1, MAX_CHOP_PATTERN));
    let gate = f64::from(spec.gate.clamp(0.05, 1.0));
    let mut result = ToolResult::default();
    for (id, note) in scope {
        let end = note.start + note.length;
        // The boundaries: the note's own start, then each cut.
        let mut bounds = vec![note.start];
        let mut cut = if spec.on_grid {
            (note.start.div_euclid(spec.step) + 1) * spec.step
        } else {
            note.start + spec.step
        };
        while cut < end {
            bounds.push(cut);
            cut += spec.step;
        }
        bounds.push(end);
        let pieces = bounds.len() - 1;
        if pieces < 2 {
            continue;
        }
        result.remove.push(*id);
        for k in 0..pieces {
            if spec.pattern & (1 << (k as u32 % pattern_len)) == 0 {
                continue;
            }
            let (from, to) = (bounds[k], bounds[k + 1]);
            // Where the piece is in the run, 0 to 1, for the ramps.
            let x = k as f64 / (pieces - 1) as f64;
            let weight = match spec.velocity {
                ChopVelocity::Flat => 1.0,
                ChopVelocity::Fall => 1.0 - 0.5 * x,
                ChopVelocity::Rise => 0.5 + 0.5 * x,
                ChopVelocity::Accent if (k as u32).is_multiple_of(pattern_len) => 1.0,
                ChopVelocity::Accent => 0.7,
            };
            result.insert.push(Note {
                start: from,
                length: (((to - from) as f64 * gate).round() as Tick).max(1),
                key: (i32::from(note.key) + i32::from(spec.pitch_step) * k as i32).clamp(0, 127)
                    as u8,
                velocity: (f64::from(note.velocity) * weight)
                    .round()
                    .clamp(1.0, 127.0) as u8,
                // A piece is a plain strike: a slide cut into four would
                // slide four times, each from the note's own key.
                slide: false,
                path: Vec::new(),
                ..note.clone()
            });
        }
    }
    result
}
