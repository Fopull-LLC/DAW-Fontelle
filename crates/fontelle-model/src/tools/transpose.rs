//! Transpose, grown: in semitones or in scale steps, and kept inside a range.

use super::ToolResult;
use crate::note::Note;
use fontelle_types::NoteId;

/// `scope` moved `by`: semitones, or scale steps when there is a `key` (a
/// mask of pitch classes). With a `range` every note that leaves it is
/// folded back by octaves (Cubase's *Keep notes in range*, FL's Limit).
pub fn transposed(
    scope: &[(NoteId, Note)],
    by: i32,
    key: Option<u16>,
    range: Option<(u8, u8)>,
) -> ToolResult {
    let mask = key.filter(|m| m & 0xFFF != 0);
    let after = scope
        .iter()
        .map(|(_, n)| {
            let mut k = match mask {
                None => i32::from(n.key) + by,
                // Step by step, each to the next key of the scale that
                // way — which also brings a note outside the key in on its
                // first step.
                Some(m) => {
                    let way = by.signum();
                    let mut k = i32::from(n.key);
                    for _ in 0..by.abs() {
                        let mut next = k + way;
                        while (0..=127).contains(&next) && m & (1 << (next % 12)) == 0 {
                            next += way;
                        }
                        if !(0..=127).contains(&next) {
                            break;
                        }
                        k = next;
                    }
                    k
                }
            };
            if let Some((low, high)) = range {
                let (low, high) = (i32::from(low.min(high)), i32::from(low.max(high)));
                while k > high && k - 12 >= low {
                    k -= 12;
                }
                while k < low && k + 12 <= high {
                    k += 12;
                }
                k = k.clamp(low, high);
            }
            Note {
                key: k.clamp(0, 127) as u8,
                ..n.clone()
            }
        })
        .collect();
    ToolResult::rewriting(scope, after)
}
