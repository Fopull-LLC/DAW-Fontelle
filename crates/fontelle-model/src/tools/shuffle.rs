//! Shuffle: one dimension of a phrase rearranged while the others stay —
//! Live 12's Recombine. Keep the rhythm, shuffle the pitches.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::NoteId;

/// Which of the notes' things is rearranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ShuffleWhat {
    #[default]
    Pitch,
    Velocity,
    Length,
}

/// How it is rearranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ShuffleHow {
    /// Into a seeded random order.
    #[default]
    Shuffle,
    /// Each note takes the one before's; the first takes the last's.
    Rotate,
    /// The order reversed.
    Mirror,
}

/// What Shuffle is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShuffleSpec {
    pub what: ShuffleWhat,
    pub how: ShuffleHow,
    pub seed: u64,
}

impl Default for ShuffleSpec {
    fn default() -> Self {
        Self {
            what: ShuffleWhat::Pitch,
            how: ShuffleHow::Shuffle,
            seed: 1,
        }
    }
}

/// `scope`, in time order, with `spec.what` rearranged among its notes.
pub fn shuffled(scope: &[(NoteId, Note)], spec: ShuffleSpec) -> ToolResult {
    let n = scope.len();
    if n < 2 {
        return ToolResult::default();
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|i| (scope[*i].1.start, scope[*i].1.key));
    // Where each value comes from: `from[i]` is the place in time order
    // whose value the i-th note takes.
    let identity: Vec<usize> = (0..n).collect();
    let from: Vec<usize> = match spec.how {
        ShuffleHow::Rotate => (0..n).map(|i| (i + n - 1) % n).collect(),
        ShuffleHow::Mirror => (0..n).rev().collect(),
        ShuffleHow::Shuffle => {
            // A shuffle that came out the way it went in is not an answer
            // anybody pressed the button for; roll again, a few times.
            let mut attempt = 0u64;
            loop {
                let mut p = identity.clone();
                for i in (1..n).rev() {
                    let roll = super::split_mix(
                        spec.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ attempt << 32 ^ i as u64,
                    );
                    p.swap(i, (roll % (i as u64 + 1)) as usize);
                }
                attempt += 1;
                let values_moved = p.iter().enumerate().any(|(i, j)| {
                    value(&scope[order[*j]].1, spec.what) != value(&scope[order[i]].1, spec.what)
                });
                if values_moved || attempt > 16 {
                    break p;
                }
            }
        }
    };
    let mut after: Vec<Note> = scope.iter().map(|(_, n)| n.clone()).collect();
    for (i, &j) in from.iter().enumerate() {
        let source = &scope[order[j]].1;
        let note = &mut after[order[i]];
        match spec.what {
            ShuffleWhat::Pitch => note.key = source.key,
            ShuffleWhat::Velocity => note.velocity = source.velocity,
            ShuffleWhat::Length => note.length = source.length,
        }
    }
    ToolResult::rewriting(scope, after)
}

fn value(n: &Note, what: ShuffleWhat) -> i64 {
    match what {
        ShuffleWhat::Pitch => i64::from(n.key),
        ShuffleWhat::Velocity => i64::from(n.velocity),
        ShuffleWhat::Length => n.length,
    }
}
