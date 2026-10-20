//! Speed: a phrase made faster or slower, anchored at its first start
//! (Logic's Double/Half speed, Live's ×2 and ÷2, Bitwig's Scale).

use super::ToolResult;
use crate::note::Note;
use fontelle_types::NoteId;

/// `scope` with every start's distance from the first, and every length,
/// multiplied by `by` — 2 is half speed, 0.5 double.
pub fn stretched(scope: &[(NoteId, Note)], by: f32) -> ToolResult {
    let Some(first) = scope.iter().map(|(_, n)| n.start).min() else {
        return ToolResult::default();
    };
    if !(by.is_finite() && by > 0.0) {
        return ToolResult::default();
    }
    let by = f64::from(by);
    let scale = |t: i64| (t as f64 * by).round() as i64;
    let after = scope
        .iter()
        .map(|(_, n)| {
            let mut n = n.clone();
            n.start = first + scale(n.start - first);
            n.length = scale(n.length).max(1);
            // A slide's points are times from the note's start: they keep
            // pace with it.
            for point in &mut n.path {
                point.at = scale(point.at).min(n.length);
            }
            n
        })
        .collect();
    ToolResult::rewriting(scope, after)
}
