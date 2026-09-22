//! The live end of one Lapse's **curves** (`docs/lapse-plan.md` §3.3).
//!
//! `effect_channel.rs` beside this one carries an insert's knobs. This
//! carries the drawn material, and it exists as a second channel rather than
//! as more of the first for one reason: an `EffectConfig` is `Copy` and tens
//! of bytes, and a [`LapseGrid`] is 48 KB. `EffectSource::current` hands its
//! state back **by value**, which is right for a config and would be 19 MB/s
//! of memcpy a second per insert for this.
//!
//! Why a channel at all, when a notepad's pages cross to the audio thread
//! never and a patch's wavetable crosses on a rebuild: because a curve editor
//! whose sound arrives on the next graph rebuild is a curve editor nobody can
//! use. A rebuild costs a patch deserialisation per channel — `effect_channel`
//! says so in its own module comment — and that is not available sixty times
//! a second while somebody drags a point.

use fontelle_types::LapseGrid;

/// The writing end, held by whatever is driving the UI.
pub struct LapseControls {
    input: triple_buffer::Input<LapseGrid>,
}

/// The reading end, held by the [`EffectNode`](crate::EffectNode) in the graph.
///
/// [`EffectNode`]: crate::EffectNode
pub struct LapseSource {
    output: triple_buffer::Output<LapseGrid>,
}

/// Opens a live channel for one Lapse, at `grid`.
pub fn lapse_channel(grid: LapseGrid) -> (LapseControls, LapseSource) {
    let (input, output) = triple_buffer::triple_buffer(&grid);
    (LapseControls { input }, LapseSource { output })
}

impl LapseControls {
    /// Publishes the realised bank, atomically as far as the audio thread is
    /// concerned.
    ///
    /// Called once per applied **edit**, never per frame: realising a bank is
    /// 48 KB and a drag makes sixty edits a second, which is fine, and a
    /// frame loop doing it would be 3 MB/s for nothing.
    pub fn publish(&mut self, grid: LapseGrid) {
        self.input.write(grid);
    }
}

impl LapseSource {
    /// **RT.** The newest published curves, taking them if new ones have
    /// arrived.
    ///
    /// By reference, which is the whole difference from
    /// [`EffectSource::current`](crate::EffectSource::current) — see the
    /// module comment.
    pub fn current(&mut self) -> &LapseGrid {
        self.output.read()
    }
}
