//! Flip: the selection mirrored in time or in pitch (FL Studio's Flip,
//! Live's Reverse and Invert, Bitwig's Reverse and Reverse Pattern).

use super::ToolResult;
use crate::note::Note;
use fontelle_types::{NoteId, Tick};

/// `scope` played backwards: mirrored across its own span.
///
/// With `keep_grid` only the order of the events reverses and every start
/// stays on a start the phrase already had — Bitwig's *Reverse Pattern*,
/// FL's *preserve start times*.
pub fn flipped_in_time(scope: &[(NoteId, Note)], keep_grid: bool) -> ToolResult {
    let Some(first) = scope.iter().map(|(_, n)| n.start).min() else {
        return ToolResult::default();
    };
    let last_end = scope
        .iter()
        .map(|(_, n)| n.start + n.length)
        .max()
        .unwrap_or(first);
    let mut starts: Vec<Tick> = scope.iter().map(|(_, n)| n.start).collect();
    starts.sort_unstable();
    starts.dedup();
    let after = scope
        .iter()
        .map(|(_, n)| {
            let mut n = n.clone();
            n.start = if keep_grid {
                // The i-th start from the front takes the i-th from the back.
                let i = starts.binary_search(&n.start).unwrap_or(0);
                starts[starts.len() - 1 - i]
            } else {
                // A mirror: what ended last starts first.
                first + last_end - (n.start + n.length)
            };
            n
        })
        .collect();
    ToolResult::rewriting(scope, after)
}

/// `scope` upside down: mirrored around the middle of its own range.
///
/// With a `key` (a mask of pitch classes, bit 0 = C) the mirror is in
/// scale steps, so a phrase in the key stays in it.
pub fn flipped_in_pitch(scope: &[(NoteId, Note)], key: Option<u16>) -> ToolResult {
    let Some(low) = scope.iter().map(|(_, n)| n.key).min() else {
        return ToolResult::default();
    };
    let high = scope.iter().map(|(_, n)| n.key).max().unwrap_or(low);
    let mask = key.filter(|m| m & 0xFFF != 0);
    let in_key = |k: u8| mask.is_some_and(|m| m & (1 << (k % 12)) != 0);
    // The mirror in scale steps runs between the lowest and highest notes
    // that are in the key; one outside it is mirrored by semitones.
    let degrees: Option<(i32, i32)> = mask.and_then(|m| {
        let lo = scope
            .iter()
            .map(|(_, n)| n.key)
            .filter(|k| in_key(*k))
            .min()?;
        let hi = scope
            .iter()
            .map(|(_, n)| n.key)
            .filter(|k| in_key(*k))
            .max()?;
        Some((degree(lo, m), degree(hi, m)))
    });
    let after = scope
        .iter()
        .map(|(_, n)| {
            let mut n = n.clone();
            n.key = match (mask, degrees) {
                (Some(m), Some((lo, hi))) if in_key(n.key) => {
                    pitch_of(lo + hi - degree(n.key, m), m).unwrap_or(n.key)
                }
                _ => (i32::from(low) + i32::from(high) - i32::from(n.key)).clamp(0, 127) as u8,
            };
            // A slide upwards becomes a slide downwards.
            for point in &mut n.path {
                point.offset = point.offset.saturating_neg();
            }
            n
        })
        .collect();
    ToolResult::rewriting(scope, after)
}

/// How many keys of the scale lie below `key`: its step number.
fn degree(key: u8, mask: u16) -> i32 {
    (0..key).filter(|k| mask & (1 << (k % 12)) != 0).count() as i32
}

/// The key that is step `d` of the scale, if the keyboard has one.
fn pitch_of(d: i32, mask: u16) -> Option<u8> {
    (0..=127u8).find(|k| mask & (1 << (k % 12)) != 0 && degree(*k, mask) == d)
}
