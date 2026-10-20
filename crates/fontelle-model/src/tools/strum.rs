//! Strum: a chord's notes spread out in time, the way a hand drags across
//! strings (FL Studio's Strum, Live 12's Strum, Logic's Q-Flam).

use super::{CHORD_CATCH, ToolResult};
use crate::note::Note;
use fontelle_types::{NoteId, PPQN, Tick};

/// Which way across the chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StrumDirection {
    /// Lowest note first — a downstroke on a guitar, which is the bass
    /// string first.
    #[default]
    Up,
    Down,
    /// Up, then down, then up: chord by chord, as strumming does.
    Alternate,
    Random,
}

/// What Strum is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StrumSpec {
    /// The gap between one string and the next, in ticks.
    pub time: Tick,
    pub direction: StrumDirection,
    /// −1..=1. Zero is even; above zero the strum speeds up as it goes (the
    /// gaps shrink), below zero it slows down.
    pub tension: f32,
    /// Velocity added per string, so a strum can get harder or softer as it
    /// crosses the chord.
    pub velocity: i32,
    /// The chord still lets go together: each note's end stays where it was
    /// and its length gives up what its start moved.
    pub keep_ends: bool,
    /// The **last** string lands on the beat rather than the first — the
    /// strum leads into it.
    pub land: bool,
    /// For [`StrumDirection::Random`].
    pub seed: u64,
}

impl Default for StrumSpec {
    fn default() -> Self {
        Self {
            // A sixty-fourth: audible as a strum at any tempo, short enough
            // not to sound like an arpeggio.
            time: PPQN / 16,
            direction: StrumDirection::Up,
            tension: 0.0,
            velocity: 0,
            keep_ends: true,
            land: false,
            seed: 1,
        }
    }
}

/// `scope`'s chords: indices into it, grouped by start (within
/// [`CHORD_CATCH`] of the group's first), each group lowest key first, the
/// groups in time order. A single note is a group of one.
pub fn chords_of(scope: &[(NoteId, Note)]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..scope.len()).collect();
    order.sort_by_key(|i| (scope[*i].1.start, scope[*i].1.key));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut first: Tick = Tick::MIN;
    for i in order {
        let start = scope[i].1.start;
        // Measured from the group's *first* start, not the last note added:
        // a chain of notes each a little after the one before would otherwise
        // be one long "chord".
        match groups.last_mut() {
            Some(group) if start - first <= CHORD_CATCH => group.push(i),
            _ => {
                first = start;
                groups.push(vec![i]);
            }
        }
    }
    for group in &mut groups {
        group.sort_by_key(|i| (scope[*i].1.key, scope[*i].1.start));
    }
    groups
}

/// Every chord of `scope` strummed. Single notes are not touched.
pub fn strummed(scope: &[(NoteId, Note)], spec: StrumSpec) -> ToolResult {
    let mut after: Vec<Note> = scope.iter().map(|(_, n)| n.clone()).collect();
    let time = spec.time.max(0);
    // The curve the offsets follow: `x^e` across the strum, `e` below one
    // for a positive tension, which front-loads the spread — wide gaps
    // first, narrow last — and so speeds up. Zero is a straight line.
    let exponent = 2f64.powf(-2.0 * f64::from(spec.tension.clamp(-1.0, 1.0)));
    let mut chord_index = 0u64;
    for group in chords_of(scope) {
        if group.len() < 2 {
            continue;
        }
        let n = group.len();
        let mut order = group.clone();
        let up = match spec.direction {
            StrumDirection::Up => true,
            StrumDirection::Down => false,
            StrumDirection::Alternate => chord_index.is_multiple_of(2),
            StrumDirection::Random => {
                shuffle(
                    &mut order,
                    spec.seed ^ chord_index.wrapping_mul(0xA24B_AED4_963E_E407),
                );
                true
            }
        };
        if !up {
            order.reverse();
        }
        chord_index += 1;
        let anchor = group.iter().map(|i| scope[*i].1.start).min().unwrap_or(0);
        let total = time * (n as Tick - 1);
        for (place, &index) in order.iter().enumerate() {
            let x = place as f64 / (n - 1) as f64;
            let mut offset = (total as f64 * x.powf(exponent)).round() as Tick;
            if spec.land {
                offset -= total;
            }
            let was = &scope[index].1;
            let note = &mut after[index];
            note.start = (anchor + offset).max(0);
            if spec.keep_ends {
                note.length = (was.start + was.length - note.start).max(1);
            }
            note.velocity =
                (i32::from(was.velocity) + spec.velocity * place as i32).clamp(1, 127) as u8;
        }
    }
    ToolResult::rewriting(scope, after)
}

/// Fisher–Yates, seeded — a strum in a random order that is the same order
/// every time the same seed is asked for it.
fn shuffle(items: &mut [usize], seed: u64) {
    for i in (1..items.len()).rev() {
        let roll = super::split_mix(seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let j = (roll % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}
