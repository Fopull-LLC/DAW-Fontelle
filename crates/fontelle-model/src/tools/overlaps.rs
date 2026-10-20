//! Cleaning up overlaps (Cubase's Delete Doubles and Delete Overlaps, Logic's
//! overlap correction) — what the roll's cross-hatching (0.30.0) points at.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::NoteId;

/// Notes on the same key at the same start, down to one: the longest.
pub fn without_doubles(scope: &[(NoteId, Note)]) -> ToolResult {
    ToolResult {
        remove: doubles(scope),
        ..ToolResult::default()
    }
}

/// The notes that are a double of a longer one (the first of equals stays).
fn doubles(scope: &[(NoteId, Note)]) -> Vec<NoteId> {
    let mut order: Vec<usize> = (0..scope.len()).collect();
    // Longest first within a key and a start, so the one kept is first.
    order.sort_by_key(|i| {
        let n = &scope[*i].1;
        (n.key, n.start, std::cmp::Reverse(n.length), *i)
    });
    order
        .windows(2)
        .filter(|w| {
            let (a, b) = (&scope[w[0]].1, &scope[w[1]].1);
            a.key == b.key && a.start == b.start
        })
        .map(|w| scope[w[1]].0)
        .collect()
}

/// Same-key notes that run into the next one on their key, cut back to meet
/// it. Doubles go too, since they cannot be cut back to anything.
pub fn trimmed_overlaps(scope: &[(NoteId, Note)]) -> ToolResult {
    let remove = doubles(scope);
    let mut order: Vec<usize> = (0..scope.len())
        .filter(|i| !remove.contains(&scope[*i].0))
        .collect();
    order.sort_by_key(|i| (scope[*i].1.key, scope[*i].1.start));
    let mut rewrite = Vec::new();
    for w in order.windows(2) {
        let (a, b) = (&scope[w[0]].1, &scope[w[1]].1);
        if a.key == b.key && a.start + a.length > b.start {
            let mut cut = a.clone();
            cut.length = b.start - a.start;
            rewrite.push((scope[w[0]].0, cut));
        }
    }
    ToolResult {
        rewrite,
        remove,
        insert: Vec::new(),
    }
}

/// One note at a time, across every key: each chord down to its top note and
/// every note cut back to the next start — for a lead or a bass that a
/// mono synth will play.
pub fn made_mono(scope: &[(NoteId, Note)]) -> ToolResult {
    let mut kept: Vec<usize> = Vec::new();
    let mut remove = Vec::new();
    for group in super::chords_of(scope) {
        // Lowest key first, so the top note is the last.
        let (&top, rest) = group.split_last().expect("a chord has a note");
        kept.push(top);
        remove.extend(rest.iter().map(|i| scope[*i].0));
    }
    let mut rewrite = Vec::new();
    for w in kept.windows(2) {
        let (a, b) = (&scope[w[0]].1, &scope[w[1]].1);
        if a.start + a.length > b.start {
            let mut cut = a.clone();
            cut.length = (b.start - a.start).max(1);
            rewrite.push((scope[w[0]].0, cut));
        }
    }
    ToolResult {
        rewrite,
        remove,
        insert: Vec::new(),
    }
}
