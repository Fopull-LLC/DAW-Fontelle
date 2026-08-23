use crate::patch::Patch;
use crate::streaming::SampleStore;
use crate::voice::VoicePool;

pub struct PrepareContext {
    pub sample_rate: f32,
    pub max_block_size: u32,
}

/// The whole public surface of `fontelle-core` (TDD §8.1): construct from a patch
/// description, receive events, render into a buffer, report parameters. Nothing
/// else — this is the entire boundary a plugin host or the DAW's `SamplerNode`
/// plugs into.
pub struct Sampler {
    patch: Patch,
    voices: VoicePool,
}

impl Sampler {
    pub fn new(patch: Patch) -> Self {
        let capacity = patch.voice_config.polyphony;
        Self {
            patch,
            voices: VoicePool::with_capacity(capacity),
        }
    }

    /// Off-RT: allocation permitted (matches `AudioNode::prepare` in `fontelle-engine`).
    pub fn prepare(&mut self, _ctx: &PrepareContext) {
        todo!("resize voice pool / pre-allocate per-voice scratch buffers")
    }

    pub fn note_on(&mut self, key: u8, velocity: u8, voice_context: u32) {
        if let Some(voice) = self.voices.allocate(self.patch.voice_config.steal_policy) {
            voice.trigger(&self.patch, key, velocity, voice_context);
        }
    }

    pub fn note_off(&mut self, _key: u8, _voice_context: u32) {
        todo!("find matching active voice by (key, voice_context) and release it")
    }

    /// RT. No allocation (INVARIANT 1).
    pub fn render(&mut self, out: &mut [f32]) {
        let patch = &self.patch;
        for voice in self.voices.iter_active_mut() {
            voice.render(patch, out);
        }
    }

    pub fn patch(&self) -> &Patch {
        &self.patch
    }
}

pub struct SamplerContext {
    pub store: SampleStore,
}
