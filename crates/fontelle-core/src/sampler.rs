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
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

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
            curve: EnvelopeCurve::Linear,
        }
    }

    fn one_voice_patch(store: &mut SampleStore, polyphony: u16) -> Patch {
        patch_with_envelope(store, polyphony, instant_envelope())
    }

    /// Same flat, always-1.0 sample as `one_voice_patch`, but with a
    /// caller-chosen amp envelope — the polyphony-mixing tests below need an
    /// envelope whose level *isn't* 1.0, since multiplying by 1.0 hides
    /// exactly the class of bug they exist to catch.
    fn patch_with_envelope(
        store: &mut SampleStore,
        polyphony: u16,
        envelope: EnvelopeConfig,
    ) -> Patch {
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
            envelopes: vec![envelope, instant_envelope()],
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

    /// Each voice must apply its *own* amp envelope to its *own* contribution
    /// only. The natural-looking implementation — every voice adds its layers
    /// into the shared output buffer, then multiplies the whole buffer by its
    /// envelope — silently re-envelopes every voice mixed in before it, so
    /// with N voices the first one's output gets multiplied by all N
    /// envelopes. Every earlier test missed this because they all used
    /// `sustain_level: 1.0`, where multiplying is a no-op.
    #[test]
    fn each_voice_applies_its_envelope_only_to_its_own_contribution() {
        let mut store = SampleStore::new();
        let half = EnvelopeConfig {
            sustain_level: 0.5,
            ..instant_envelope()
        };
        let patch = patch_with_envelope(&mut store, 8, half);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        // Two voices, each rendering a constant 1.0 sample through a constant
        // 0.5 envelope: 0.5 + 0.5 = 1.0.
        sampler.note_on(60, 127, 0);
        sampler.note_on(64, 127, 1);

        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut out);

        let got = rms(&out);
        assert!(
            (got - 1.0).abs() < 1e-3,
            "two voices at 0.5 envelope each must sum to 1.0, got {got} \
             (0.75 means the second voice's envelope was applied to the first's output too)"
        );
    }

    /// The audible symptom of the bug above: hold a chord, play another note
    /// on top, and the held notes duck for the duration of the new note's
    /// attack — because the new voice multiplies the whole shared buffer,
    /// including the held notes already mixed into it, by its own
    /// near-zero attack level.
    #[test]
    fn a_new_notes_attack_does_not_duck_already_sounding_voices() {
        let mut store = SampleStore::new();
        let slow_attack = EnvelopeConfig {
            attack_s: 1.0,
            sustain_level: 1.0,
            ..instant_envelope()
        };
        let patch = patch_with_envelope(&mut store, 8, slow_attack);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        // Voice A: triggered and rendered well into its attack so it's loud.
        sampler.note_on(60, 127, 0);
        let mut out = vec![0.0; 128];
        for _ in 0..300 {
            sampler.render(&store, &mut out);
        }
        let before = rms(&out);
        assert!(
            before > 0.5,
            "voice A should be well into its attack by now, got {before}"
        );

        // Voice B: brand new, so its envelope is ~0 for this block. Voice A's
        // contribution must be unaffected — the total can only go up.
        sampler.note_on(64, 127, 1);
        sampler.render(&store, &mut out);
        let after = rms(&out);

        assert!(
            after >= before - 1e-3,
            "adding a note must not reduce the output: {before} -> {after} \
             (the new voice's near-zero attack envelope ducked the already-sounding one)"
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
