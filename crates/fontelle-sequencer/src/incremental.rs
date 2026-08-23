use std::collections::HashSet;

use fontelle_model::Project;
use fontelle_types::{CompiledTimeline, Tick};

/// Full recompilation of a large project is too slow to run on every keystroke.
/// Compilation is segmented by bar; a mutation dirties only the bars it touches,
/// plus any bar containing a prefab instance whose source changed (TDD §11.3).
#[derive(Debug, Default)]
pub struct DirtyBars {
    bars: HashSet<i64>,
}

impl DirtyBars {
    pub fn mark(&mut self, bar: i64) {
        self.bars.insert(bar);
    }

    pub fn mark_range(&mut self, _start_tick: Tick, _end_tick: Tick) {
        todo!("tick range -> bar range via the tempo map's time signature track")
    }

    pub fn drain(&mut self) -> impl Iterator<Item = i64> + '_ {
        self.bars.drain()
    }
}

/// Recompiles only the dirty segments and splices them into the existing
/// `CompiledTimeline`, on a background task. Handoff to the RT thread happens by
/// `triple_buffer`: the model thread publishes atomically, the RT thread picks the
/// new timeline up at the next block boundary, and the old one is dropped on the
/// model thread — never the RT thread, since dropping deallocates (TDD §11.3).
pub fn recompile_dirty(
    _project: &Project,
    _dirty: &mut DirtyBars,
    _existing: &CompiledTimeline,
) -> CompiledTimeline {
    todo!("recompile only dirtied bar segments and splice into a clone of `existing`")
}
