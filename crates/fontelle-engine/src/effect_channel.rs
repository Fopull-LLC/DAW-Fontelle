//! The live end of one insert (TDD §13.4, and the same problem §13.1's fader
//! had).
//!
//! An EQ knob is a control somebody *drags*. That pulls in two directions at
//! once, exactly as a fader does: the sound has to move while the hand is
//! moving, and one undo entry has to be left behind when it stops. The undo
//! entry is a `Command` against the document; the sound comes from a
//! `CompiledGraph` that costs a patch deserialisation per channel to rebuild.
//! Rebuilding it per frame of a drag reloads every soundfont in the project
//! sixty times a second.
//!
//! So an insert writes **both** — the command, for undo and for the file, and
//! this, for the sound between now and the next rebuild. The document stays
//! the source of truth (INVARIANT 9), and a rebuild seeds a fresh channel from
//! it, so the two cannot drift.
//!
//! # Why not atomics, as the fader uses
//!
//! A fader is four scalars and fits in four atomics. An `EqConfig` is eight
//! bands of six fields, which is neither atomic nor `Copy`-into-a-`u64`, and a
//! mutex on the audio thread is not an option (INVARIANT 1). A triple buffer
//! is the mechanism that fits: wait-free on the reading side, no allocation on
//! either, and already what the compiled timeline crosses on.

use fontelle_types::EffectConfig;

/// What crosses the buffer: everything about an insert that can change without
/// the graph being rebuilt.
///
/// Bypass rides along with the parameters rather than in an atomic of its own,
/// because it is a control on the same panel and gets switched mid-listen just
/// as often — and two channels for one effect is two things that can arrive in
/// the wrong order.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EffectState {
    config: EffectConfig,
    bypassed: bool,
}

/// The writing end, held by whatever is driving the UI.
pub struct EffectControls {
    input: triple_buffer::Input<EffectState>,
    /// The last thing published, so `set_bypassed` can change one field
    /// without the caller having to know the other.
    last: EffectState,
}

/// The reading end, held by the [`EffectNode`](crate::EffectNode) in the graph.
///
/// [`EffectNode`]: crate::EffectNode
pub struct EffectSource {
    output: triple_buffer::Output<EffectState>,
}

/// Opens a live channel for one insert, at `config` and not bypassed.
pub fn effect_channel(config: EffectConfig) -> (EffectControls, EffectSource) {
    let initial = EffectState {
        config,
        bypassed: false,
    };
    let (input, output) = triple_buffer::triple_buffer(&initial);
    (
        EffectControls {
            input,
            last: initial,
        },
        EffectSource { output },
    )
}

impl EffectControls {
    /// Publishes new parameters, atomically as far as the audio thread is
    /// concerned.
    pub fn publish(&mut self, config: EffectConfig) {
        self.last.config = config;
        self.input.write(self.last);
    }

    pub fn set_bypassed(&mut self, bypassed: bool) {
        self.last.bypassed = bypassed;
        self.input.write(self.last);
    }

    /// What was last published. The document is still the source of truth;
    /// this is what the sound is doing right now.
    pub fn config(&self) -> EffectConfig {
        self.last.config
    }

    pub fn bypassed(&self) -> bool {
        self.last.bypassed
    }
}

impl EffectSource {
    /// **RT.** The newest published state, taking it if one has arrived.
    pub fn current(&mut self) -> (EffectConfig, bool) {
        let state = self.output.read();
        (state.config, state.bypassed)
    }
}
