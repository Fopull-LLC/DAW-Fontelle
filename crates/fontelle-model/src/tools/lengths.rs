//! Lengths: legato, staccato, fixed and the rest — FL Studio's Articulate,
//! Live's Span, Logic's and Cubase's length functions.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::{NoteId, PPQN, Tick};

/// What Lengths does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LengthsMode {
    /// Every note reaches the next start, less `gap` — Ctrl+L with a gap.
    #[default]
    Legato,
    /// Each note `percent` of what it was.
    Staccato,
    /// Every note `length` long.
    Fixed,
    /// None shorter than `length`.
    AtLeast,
    /// None longer than `length`.
    AtMost,
    /// No note runs into the next start: each is cut back to leave `gap`
    /// before it. Never lengthens.
    Gap,
}

/// What Lengths is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LengthsSpec {
    pub mode: LengthsMode,
    pub percent: i32,
    pub length: Tick,
    pub gap: Tick,
}

impl Default for LengthsSpec {
    fn default() -> Self {
        Self {
            mode: LengthsMode::Legato,
            percent: 50,
            length: PPQN / 4,
            gap: 0,
        }
    }
}

/// `scope` with its lengths set. `clip_end` is where a legato's last notes
/// are carried to, as Ctrl+L carries them.
pub fn lengthened(
    scope: &[(NoteId, Note)],
    spec: LengthsSpec,
    clip_end: Option<Tick>,
) -> ToolResult {
    let spans: Vec<(Tick, Tick)> = scope.iter().map(|(_, n)| (n.start, n.length)).collect();
    let gap = spec.gap.max(0);
    let legato = super::legato_lengths_to(&spans, clip_end);
    // The next start after each note's, across the whole scope — a chord's
    // notes all share the same one, as legato's do.
    let next_start = |start: Tick| spans.iter().map(|(s, _)| *s).filter(|s| *s > start).min();
    let after = scope
        .iter()
        .zip(&legato)
        .map(|((_, n), reach)| {
            let length = match spec.mode {
                LengthsMode::Legato => reach - gap,
                LengthsMode::Staccato => {
                    (n.length as f64 * f64::from(spec.percent.max(1)) / 100.0).round() as Tick
                }
                LengthsMode::Fixed => spec.length,
                LengthsMode::AtLeast => n.length.max(spec.length),
                LengthsMode::AtMost => n.length.min(spec.length),
                LengthsMode::Gap => match next_start(n.start) {
                    Some(next) => n.length.min(next - n.start - gap),
                    None => n.length,
                },
            };
            Note {
                length: length.max(1),
                ..n.clone()
            }
        })
        .collect();
    ToolResult::rewriting(scope, after)
}
