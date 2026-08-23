use fontelle_core::Sampler;
use fontelle_types::ParamAddress;

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};

struct EmptyParams;
impl ParamSet for EmptyParams {
    fn get(&self, _addr: &ParamAddress) -> Option<f64> {
        None
    }
    fn set(&self, _addr: &ParamAddress, _value: f64) -> bool {
        false
    }
}

/// Wraps `fontelle-core::Sampler` as a graph node (TDD §5.1). This is where the
/// engine hands the sampler its slice of the compiled event timeline and reads
/// back rendered audio — the sampler itself still knows nothing about the graph.
pub struct SamplerNode {
    sampler: Sampler,
}

impl SamplerNode {
    pub fn new(sampler: Sampler) -> Self {
        Self { sampler }
    }
}

impl AudioNode for SamplerNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: ctx.sample_rate,
            max_block_size: ctx.max_block_size,
        });
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        for event in ctx.events {
            match &event.payload {
                fontelle_types::EventPayload::NoteOn {
                    key,
                    velocity,
                    voice_context,
                } => {
                    self.sampler.note_on(*key, *velocity, *voice_context);
                }
                fontelle_types::EventPayload::NoteOff { key, voice_context } => {
                    self.sampler.note_off(*key, *voice_context);
                }
                _ => {}
            }
        }
        if let Some(out) = ctx.outputs.first_mut() {
            self.sampler.render(out);
        }
    }

    fn reset(&mut self) {
        todo!("silence all active voices")
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// Wraps one `fontelle-fx` effect. Which effect is behind `Box<dyn ...>` is decided
/// when the node is built from the document's `EffectSlot` — `fontelle-fx` itself
/// exposes no shared trait, since it must not depend on this crate (TDD §4.1).
pub struct EffectNode {
    // Concrete effect + its ParamSet adapter land alongside the mixer/insert wiring
    // in M4.
}

pub struct MixerTrackNode {
    // Fader, pan, sends. TDD §13.
}

pub struct SendNode {
    // Pre/post-fader tap to another track. TDD §13.2.
}

pub struct AudioClipNode {
    // TDD §15 (M6).
}

pub struct MasterNode {
    // Metering (peak/RMS + LUFS/true-peak) lives here. TDD §13.3.
}
