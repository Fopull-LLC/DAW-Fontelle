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
    quality: fontelle_dsp::Interpolation,
    pan: f32,
}

impl Sampler {
    pub fn new(patch: Patch) -> Self {
        let capacity = patch.voice_config.polyphony;
        Self {
            patch,
            voices: VoicePool::with_capacity(capacity),
            sample_rate: 48_000.0,
            quality: fontelle_dsp::Interpolation::Normal,
            pan: 0.0,
        }
    }

    /// Off-RT: allocation permitted (matches `AudioNode::prepare` in `fontelle-engine`).
    /// The voice pool is already sized from `patch.voice_config.polyphony` at
    /// construction; `prepare` only needs to record the device's actual rate,
    /// since pitch/loop math is computed per-render from it.
    pub fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
    }

    /// Sets the session's interpolation quality — the kernel used by every
    /// layer that does not name one of its own.
    ///
    /// TDD §7.6 makes playback and render quality independent settings, so a
    /// bounce can run at a better kernel than the session was played at
    /// without editing the document: the caller sets `High` for an export pass
    /// and leaves it at the `Normal` default the rest of the time. This is
    /// session state, not document data, because an export must not mutate the
    /// patch. A layer with `Some(mode)` in its `PlaybackConfig` ignores this.
    ///
    /// Off-RT.
    pub fn set_quality(&mut self, quality: fontelle_dsp::Interpolation) {
        self.quality = quality;
    }

    /// Places the whole part in the stereo field: -1.0 hard left, 0.0 centre,
    /// +1.0 hard right. The compiled form of MIDI CC10.
    ///
    /// This is a channel control, not a patch edit — it adds to whatever pan
    /// each layer carries of its own rather than replacing it, and it is read
    /// per block, so moving it moves the notes already sounding.
    ///
    /// RT-safe: a plain field write, no allocation.
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan;
    }

    pub fn pan(&self) -> f32 {
        self.pan
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
    ///
    /// `out` is planar and channel-major: `[left, right]` for stereo, `[mono]`
    /// for one channel. Clearing what it is handed is deliberate and is the
    /// contract a plugin host expects of `fontelle-core`'s boundary (TDD
    /// §8.1); a DAW host that needs several instruments summed onto one bus
    /// renders into scratch and adds, which is what `SamplerNode` does.
    pub fn render(&mut self, store: &SampleStore, out: &mut [&mut [f32]]) {
        for channel in out.iter_mut() {
            channel.fill(0.0);
        }
        let patch = &self.patch;
        let sample_rate = self.sample_rate;
        let quality = self.quality;
        let pan = self.pan;
        for voice in self.voices.iter_active_mut() {
            voice.render_with_pan(patch, store, sample_rate, quality, pan, out);
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
                    interpolation: Some(Interpolation::Draft),
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
        sampler.render(&store, &mut [&mut out[..]]);
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
        sampler.render(&store, &mut [&mut scratch[..]]); // settle in

        sampler.note_off(60, 0);
        // release_s = 0.01s @ 48kHz = 480 samples; render well past that.
        for _ in 0..10 {
            sampler.render(&store, &mut [&mut scratch[..]]);
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
        sampler.render(&store, &mut [&mut out[..]]);
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
        sampler.render(&store, &mut [&mut out[..]]);

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
            sampler.render(&store, &mut [&mut out[..]]);
        }
        let before = rms(&out);
        assert!(
            before > 0.5,
            "voice A should be well into its attack by now, got {before}"
        );

        // Voice B: brand new, so its envelope is ~0 for this block. Voice A's
        // contribution must be unaffected — the total can only go up.
        sampler.note_on(64, 127, 1);
        sampler.render(&store, &mut [&mut out[..]]);
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
        sampler.render(&store, &mut [&mut out[..]]);
        assert!(rms(&out) > 0.0, "the stolen-in third note must still sound");
    }

    /// A sample whose content sits high enough in the spectrum that the choice
    /// of interpolation kernel is measurable — the point of the setting.
    fn sine_patch(store: &mut SampleStore, cycles_per_sample: f64, len: usize) -> Patch {
        let data: Vec<f32> = (0..len)
            .map(|i| (std::f64::consts::TAU * cycles_per_sample * i as f64).sin() as f32)
            .collect();
        let asset = store.insert(crate::streaming::SampleBuffer {
            data: std::sync::Arc::from(data),
            sample_rate: SR as u32,
        });
        Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                // Read at a fractional rate so every output sample lands
                // between two stored ones; at an integer rate every kernel
                // returns the same thing and the test proves nothing.
                fine_tune_cents: 30.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Some(Interpolation::Normal),
                    end_offset: len as f64,
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
                polyphony: 4,
                steal_policy: StealPolicy::Oldest,
                ..VoiceConfig::default()
            },
        }
    }

    #[test]
    fn a_layer_with_no_interpolation_of_its_own_follows_the_session_quality() {
        // TDD §7.6: playback and render quality are independent session
        // settings, so a bounce runs at a better kernel than the session was
        // played at without the document being edited. A layer that names no
        // mode of its own is what makes that possible.
        let mut store = SampleStore::new();
        let mut patch = sine_patch(&mut store, 0.2, 4000);
        patch.layers[0].playback.interpolation = None;

        let normal = render_with(&patch, &store, Interpolation::Normal);
        let high = render_with(&patch, &store, Interpolation::High);

        assert!(normal.iter().any(|s| *s != 0.0), "fixture should sound");
        let difference: f32 = normal
            .iter()
            .zip(high.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            difference > 0.1,
            "session quality must reach an unpinned layer, difference was {difference}"
        );
    }

    #[test]
    fn a_layer_that_names_its_own_interpolation_keeps_it_at_any_session_quality() {
        // A pinned layer is honoured exactly, in playback and in render alike,
        // and is never quietly "upgraded" for a bounce. Draft's aliasing is a
        // legitimate character choice in a sampler, so treating the modes as a
        // quality ladder the export may climb would silently change how a
        // deliberately lo-fi patch sounds in the mix it ships in.
        let mut store = SampleStore::new();
        let mut patch = sine_patch(&mut store, 0.2, 4000);
        patch.layers[0].playback.interpolation = Some(Interpolation::Draft);

        assert_eq!(
            render_with(&patch, &store, Interpolation::Normal),
            render_with(&patch, &store, Interpolation::High),
            "a pinned layer must ignore the session quality entirely"
        );
    }

    #[test]
    fn an_unpinned_layer_defaults_to_normal_without_a_session_setting() {
        let mut store = SampleStore::new();
        let mut patch = sine_patch(&mut store, 0.2, 4000);
        patch.layers[0].playback.interpolation = None;

        let mut sampler = Sampler::new(patch.clone());
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });
        sampler.note_on(72, 127, 0);
        let mut untouched = vec![0.0; 512];
        sampler.render(&store, &mut [&mut untouched[..]]);

        assert_eq!(
            untouched,
            render_with(&patch, &store, Interpolation::Normal),
            "a fresh Sampler should play at Normal, the TDD's playback default"
        );
    }

    fn render_with(patch: &Patch, store: &SampleStore, quality: Interpolation) -> Vec<f32> {
        let mut sampler = Sampler::new(patch.clone());
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });
        sampler.set_quality(quality);
        sampler.note_on(72, 127, 0);
        let mut out = vec![0.0; 512];
        sampler.render(store, &mut [&mut out[..]]);
        out
    }

    /// The channel pan has to reach notes that are *already sounding* — it is
    /// a live control, not a note-on value. Setting it after the note-on and
    /// hearing nothing move is the bug this pins down.
    #[test]
    fn setting_the_channel_pan_moves_notes_that_are_already_sounding() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        sampler.note_on(60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        let centred = std::f32::consts::FRAC_1_SQRT_2;
        assert!((left[0] - centred).abs() < 1e-5, "starts centred");

        sampler.set_pan(1.0);
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        assert!(
            left[0].abs() < 1e-5 && (right[0] - 1.0).abs() < 1e-5,
            "the held note must move with the channel pan, got {} / {}",
            left[0],
            right[0]
        );
    }
}
