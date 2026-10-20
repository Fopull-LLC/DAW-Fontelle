//! Glue: touching notes on one key become one note (FL Studio's Glue,
//! Live's Join). Chop's inverse.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::NoteId;

/// Every run of same-key notes in `scope` that touch or overlap, joined into
/// its first note.
pub fn glued(scope: &[(NoteId, Note)]) -> ToolResult {
    let mut order: Vec<usize> = (0..scope.len()).collect();
    order.sort_by_key(|i| (scope[*i].1.key, scope[*i].1.start));
    let mut result = ToolResult::default();
    let mut at = 0;
    while at < order.len() {
        let first = order[at];
        let key = scope[first].1.key;
        let mut end = scope[first].1.start + scope[first].1.length;
        let mut next = at + 1;
        // Touching is enough: a chopped note's pieces meet exactly, and a
        // gap of even one tick is a gap you can hear on a percussive sound.
        while next < order.len()
            && scope[order[next]].1.key == key
            && scope[order[next]].1.start <= end
        {
            let n = &scope[order[next]].1;
            end = end.max(n.start + n.length);
            result.remove.push(scope[order[next]].0);
            next += 1;
        }
        if next > at + 1 {
            let mut joined = scope[first].1.clone();
            joined.length = end - joined.start;
            result.rewrite.push((scope[first].0, joined));
        }
        at = next;
    }
    result
}
