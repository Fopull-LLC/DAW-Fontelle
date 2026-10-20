//! Choosing notes by what they are — FL Studio's Select menu, Live's Find
//! and Select. Not an edit: it answers with ids, and the roll selects them.

use crate::note::Note;
use fontelle_types::{NoteId, Tick};

/// Which notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectHow {
    /// Every note on a key any chosen note is on.
    SameKey,
    /// Every note sharing time with another on its key — the hatched ones.
    Overlapping,
    /// Every note that does not start on a beat.
    OffBeat,
    /// About half, at random.
    Random,
    /// Everything that is not chosen.
    Invert,
    /// Shorter than the grid.
    Short,
    /// Longer than the grid.
    Long,
}

/// The ids of `scope` that `how` picks, given what is `chosen` now.
pub fn selected_where(
    scope: &[(NoteId, Note)],
    chosen: &[NoteId],
    how: SelectHow,
    grid: Tick,
    seed: u64,
) -> Vec<NoteId> {
    let pick = |keep: &dyn Fn(usize, &Note) -> bool| -> Vec<NoteId> {
        scope
            .iter()
            .enumerate()
            .filter(|(i, (_, n))| keep(*i, n))
            .map(|(_, (id, _))| *id)
            .collect()
    };
    match how {
        SelectHow::SameKey => {
            let keys: Vec<u8> = scope
                .iter()
                .filter(|(id, _)| chosen.contains(id))
                .map(|(_, n)| n.key)
                .collect();
            pick(&|_, n| keys.contains(&n.key))
        }
        SelectHow::Overlapping => pick(&|i, a| {
            scope.iter().enumerate().any(|(j, (_, b))| {
                i != j
                    && a.key == b.key
                    && a.start < b.start + b.length
                    && b.start < a.start + a.length
            })
        }),
        SelectHow::OffBeat => pick(&|_, n| n.start % fontelle_types::PPQN != 0),
        SelectHow::Invert => scope
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !chosen.contains(id))
            .collect(),
        SelectHow::Short => pick(&|_, n| n.length < grid),
        SelectHow::Long => pick(&|_, n| n.length > grid),
        SelectHow::Random => {
            let mut got = pick(&|i, _| {
                super::split_mix(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ i as u64) & 1 == 1
            });
            // "Some of them": never none, never all, while there are two.
            if got.is_empty() {
                got.extend(scope.first().map(|(id, _)| *id));
            } else if got.len() == scope.len() && got.len() > 1 {
                got.pop();
            }
            got
        }
    }
}
