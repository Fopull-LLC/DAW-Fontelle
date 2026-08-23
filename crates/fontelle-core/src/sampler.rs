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
    sample_rate: f32,
}

impl Sampler {
    pub fn new(patch: Patch) -> Self {
        let capacity = patch.voice_config.polyphony;
        Self {
            patch,
            voices: VoicePool::with_capacity(capacity),
            sample_rate: 48_000.0,
        }
    }

    /// Off-RT: allocation permitted (matches `AudioNode::prepare` in `fontelle-engine`).
    /// The voice pool is already sized from `patch.voice_config.polyphony` at
    /// construction; `prepare` only needs to record the device's actual rate,
    /// since pitch/loop math is computed per-render from it.
    pub fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
    }

    pub fn note_on(&mut self, key: u8, velocity: u8, voice_context: u32) {
        if let Some(voice) = self.voices.allocate(self.patch.voice_config.steal_policy) {
            voice.trigger(&self.patch, key, velocity, voice_context);
        }
    }

    pub fn note_off(&mut self, key: u8, voice_context: u32) {
        if let Some(voice) = self.voices.find_active_mut(key, voice_context) {
            voice.release();
        }
    }

    /// RT. No allocation (INVARIANT 1). Zeroes `out` first, then mixes every
    /// active voice into it.
    pub fn render(&mut self, store: &SampleStore, out: &mut [f32]) {
        out.fill(0.0);
        let patch = &self.patch;
        let sample_rate = self.sample_rate;
        for voice in self.voices.iter_active_mut() {
            voice.render(patch, store, sample_rate, out);
        }
    }

    pub fn patch(&self) -> &Patch {
        &self.patch
    }
}

pub struct SamplerContext {
    pub store: SampleStore,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_matrix::ModMatrix;
    use crate::patch::{FilterSlot, Layer, Source};
    use crate::playback::{LoopMode, PlaybackConfig};
    use crate::voice::{StealPolicy, VoiceConfig};
    use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};

    const SR: f32 = 48_000.0;

    fn disabled_filter() -> FilterSlot {
        FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
        }
    }

    fn instant_envelope() -> EnvelopeConfig {
        EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
        }
    }

    fn one_voice_patch(store: &mut SampleStore, polyphony: u16) -> Patch {
        let asset = store.insert(crate::streaming::SampleBuffer {
            data: std::sync::Arc::from(vec![1.0; 100_000]),
            sample_rate: SR as u32,
        });
        Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Interpolation::Draft,
                    end_offset: 100_000.0,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(), instant_envelope()],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig {
                polyphony,
                steal_policy: StealPolicy::Oldest,
                ..VoiceConfig::default()
            },
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn note_on_then_render_produces_sound() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on(60, 127, 0);
        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut out);
        assert!(rms(&out) > 0.5);
    }

    #[test]
    fn note_off_releases_the_matching_voice_and_it_goes_silent() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on(60, 127, 0);
        let mut scratch = vec![0.0; 256];
        sampler.render(&store, &mut scratch); // settle in

        sampler.note_off(60, 0);
        // release_s = 0.01s @ 48kHz = 480 samples; render well past that.
        for _ in 0..10 {
            sampler.render(&store, &mut scratch);
        }
        assert_eq!(
            rms(&scratch),
            0.0,
            "released voice must eventually fall silent"
        );
    }

    #[test]
    fn note_off_does_not_affect_a_different_voice_context() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on(60, 127, 0);
        sampler.note_off(60, 999); // wrong voice_context — TDD §11.4 per-clip tagging

        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut out);
        assert!(
            rms(&out) > 0.5,
            "a note-off with a non-matching voice_context must not release the real voice"
        );
    }

    #[test]
    fn exhausting_polyphony_steals_the_oldest_voice() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 2);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        // Fill both voice slots, then trigger a third: stealing must free a slot
        // rather than silently dropping the new note.
        sampler.note_on(60, 127, 0);
        sampler.note_on(61, 127, 1);
        sampler.note_on(62, 127, 2);

        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut out);
        assert!(rms(&out) > 0.0, "the stolen-in third note must still sound");
    }
}
