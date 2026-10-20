//! Thin out: every Nth slice of time emptied — FL Studio's Claw machine,
//! which turns a steady roll into a stutter or a fill.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::{NoteId, PPQN, Tick};

/// What Thin out is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ThinSpec {
    /// The slice, in ticks, counted from the scope's first start.
    pub period: Tick,
    /// Every this-many-th slice goes (2 = every second one).
    pub every: u8,
    /// What is left closes up to fill the span the scope had.
    pub stretch: bool,
}

impl Default for ThinSpec {
    fn default() -> Self {
        Self {
            period: PPQN / 4,
            every: 2,
            stretch: false,
        }
    }
}

/// `scope` with every `spec.every`-th slice's notes taken out.
pub fn thinned(scope: &[(NoteId, Note)], spec: ThinSpec) -> ToolResult {
    let every = Tick::from(spec.every.max(2));
    let Some(first) = scope.iter().map(|(_, n)| n.start).min() else {
        return ToolResult::default();
    };
    if spec.period <= 0 {
        return ToolResult::default();
    }
    let slice = |n: &Note| (n.start - first).div_euclid(spec.period);
    let gone = |s: Tick| (s + 1) % every == 0;
    let slices = scope.iter().map(|(_, n)| slice(n)).max().unwrap_or(0) + 1;
    let kept_slices = (0..slices).filter(|s| !gone(*s)).count().max(1) as f64;
    let factor = slices as f64 / kept_slices;
    let mut result = ToolResult::default();
    for (id, n) in scope {
        let s = slice(n);
        if gone(s) {
            result.remove.push(*id);
        } else if spec.stretch {
            // Its place among the slices that stay, spread to fill them all.
            let rank = (0..s).filter(|x| !gone(*x)).count() as f64;
            let within = (n.start - first - s * spec.period) as f64;
            let moved = Note {
                start: first + ((rank * spec.period as f64 + within) * factor).round() as Tick,
                length: ((n.length as f64 * factor).round() as Tick).max(1),
                ..n.clone()
            };
            if moved != *n {
                result.rewrite.push((*id, moved));
            }
        }
    }
    result
}
