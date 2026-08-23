#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealPolicy {
    Oldest,
    Quietest,
    LowestPriority,
}

#[derive(Debug, Clone, Copy)]
pub struct UnisonConfig {
    pub voices: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub randomise_phase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetriggerMode {
    Poly,
    Mono,
    Legato,
}

#[derive(Debug, Clone, Copy)]
pub struct VoiceConfig {
    /// 1..=256.
    pub polyphony: u16,
    pub steal_policy: StealPolicy,
    pub glide_time_s: f32,
    pub glide_legato_only: bool,
    pub unison: UnisonConfig,
    pub retrigger: RetriggerMode,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            polyphony: 64,
            steal_policy: StealPolicy::Oldest,
            glide_time_s: 0.0,
            glide_legato_only: false,
            unison: UnisonConfig {
                voices: 1,
                detune_cents: 0.0,
                spread: 0.0,
                randomise_phase: false,
            },
            retrigger: RetriggerMode::Poly,
        }
    }
}

/// TDD §7.2: "up to 16" layers. A fixed array, not a `Vec` — per-layer playback
/// state is allocated once with the voice, at pool-construction time, never on
/// note-on (INVARIANT 1, INVARIANT 6).
pub const MAX_LAYERS: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
struct LayerPlayback {
    active: bool,
    /// Fractional sample position within the layer's `SampleBuffer`.
    position: f64,
}

/// One playing note. Fixed-topology (INVARIANT 6): Layers → mix → Filter 1 → Filter 2
/// → Amp → Pan → out, with the mod matrix feeding every stage. Predictable per-voice
/// cost, zero allocation on note-on, no graph compilation on the audio thread.
///
/// **M0 scope note:** only `Source::Sample` layers render (`Source::Sf2Zone` and
/// `Source::Oscillator` are silent no-ops for now); Filter1/Filter2 pass straight
/// through unless disabled is honoured as pass-through only (the SVF itself isn't
/// implemented yet, see `fontelle_dsp::SvfFilter`); output is a single (mono)
/// buffer, so `Layer::pan` has no effect yet. All tracked in `PROGRESS.md`.
pub struct Voice {
    active: bool,
    key: u8,
    voice_context: u32,
    /// Set from `VoicePool`'s monotonic counter on every `trigger`, so the pool
    /// can find the oldest active voice to steal without a separate timestamp
    /// clock (INVARIANT 1: no syscalls on the RT thread).
    age: u64,
    layers: [LayerPlayback; MAX_LAYERS],
    amp_env: fontelle_dsp::EnvelopeGenerator,
}

impl Voice {
    pub fn new() -> Self {
        Self {
            active: false,
            key: 0,
            voice_context: 0,
            age: 0,
            layers: [LayerPlayback::default(); MAX_LAYERS],
            amp_env: fontelle_dsp::EnvelopeGenerator::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn key(&self) -> u8 {
        self.key
    }

    pub fn voice_context(&self) -> u32 {
        self.voice_context
    }

    pub fn trigger(&mut self, patch: &crate::Patch, key: u8, velocity: u8, voice_context: u32) {
        self.active = true;
        self.key = key;
        self.voice_context = voice_context;
        self.amp_env.note_on();

        for (slot, layer) in self.layers.iter_mut().zip(patch.layers.iter()) {
            let in_key_range = key >= layer.key_range.0 && key <= layer.key_range.1;
            let in_vel_range = velocity >= layer.vel_range.0 && velocity <= layer.vel_range.1;
            *slot = LayerPlayback {
                active: in_key_range && in_vel_range,
                position: layer.playback.start_offset,
            };
        }
        for slot in self.layers.iter_mut().skip(patch.layers.len()) {
            *slot = LayerPlayback::default();
        }
    }

    /// Voice stealing always ramps out over a short release rather than cutting
    /// hard (TDD §7.4) — never a click. Uses the same envelope release as a
    /// normal note-off; a shorter, dedicated steal-ramp is a later refinement.
    pub fn release(&mut self) {
        self.amp_env.note_off();
    }

    /// RT: no allocation. Mixes every active layer into `out`, applying pitch
    /// (root key + fine tune, resampled against the buffer's own rate),
    /// looping, per-layer gain, and the patch's amp envelope (`envelopes[0]`).
    /// Adds into `out` rather than overwriting it — callers mixing multiple
    /// voices into one buffer must clear it first.
    pub fn render(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        sample_rate: f32,
        out: &mut [f32],
    ) {
        if !self.active {
            return;
        }

        let amp_env_config =
            patch
                .envelopes
                .first()
                .copied()
                .unwrap_or(fontelle_dsp::EnvelopeConfig {
                    delay_s: 0.0,
                    attack_s: 0.0,
                    hold_s: 0.0,
                    decay_s: 0.0,
                    sustain_level: 1.0,
                    release_s: 0.0,
                });

        for (slot, layer) in self.layers.iter_mut().zip(patch.layers.iter()) {
            if !slot.active {
                continue;
            }
            let crate::patch::Source::Sample { file } = &layer.source else {
                // Sf2Zone/Oscillator sources aren't wired to the renderer yet.
                continue;
            };
            let Some(buffer) = store.get(*file) else {
                continue;
            };

            let semitones =
                (self.key as f32 - layer.root_key as f32) + layer.fine_tune_cents / 100.0;
            let pitch_ratio = 2f32.powf(semitones / 12.0);
            let rate_ratio = buffer.sample_rate as f32 / sample_rate;
            let step = (pitch_ratio * rate_ratio) as f64;
            let layer_gain = 10f32.powf(layer.gain_db / 20.0);

            let loop_len = layer.playback.loop_end - layer.playback.loop_start;
            let looping =
                matches!(layer.playback.loop_mode, crate::LoopMode::Forward) && loop_len > 0.0;

            for out_sample in out.iter_mut() {
                if looping && slot.position >= layer.playback.loop_end {
                    slot.position -= loop_len;
                }
                if !looping && slot.position >= layer.playback.end_offset {
                    slot.active = false;
                    break;
                }

                let value = fontelle_dsp::interpolate(
                    &buffer.data,
                    slot.position,
                    layer.playback.interpolation,
                );
                *out_sample += value * layer_gain;
                slot.position += step;
            }
        }

        for sample in out.iter_mut() {
            *sample *= self.amp_env.advance(&amp_env_config, sample_rate);
        }

        let any_layer_active = self.layers.iter().any(|s| s.active);
        if !self.amp_env.is_active() || !any_layer_active {
            self.active = false;
        }
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

/// A pre-allocated pool sized to `VoiceConfig::polyphony` at `prepare()` time
/// (TDD §7.4) — no allocation on note-on, ever.
pub struct VoicePool {
    voices: Vec<Voice>,
    next_age: u64,
}

impl VoicePool {
    pub fn with_capacity(capacity: u16) -> Self {
        Self {
            voices: (0..capacity).map(|_| Voice::new()).collect(),
            next_age: 0,
        }
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// Finds a free voice, or steals one per `policy`. `Quietest` and
    /// `LowestPriority` aren't distinguished from `Oldest` yet — neither
    /// per-voice level tracking nor a priority concept exists — so both fall
    /// back to age-based stealing for now (tracked in `PROGRESS.md`).
    pub fn allocate(&mut self, policy: StealPolicy) -> Option<&mut Voice> {
        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);

        let index = match self.voices.iter().position(|v| !v.is_active()) {
            Some(i) => i,
            None => match policy {
                StealPolicy::Oldest | StealPolicy::Quietest | StealPolicy::LowestPriority => self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)?,
            },
        };

        let voice = self.voices.get_mut(index)?;
        voice.age = age;
        Some(voice)
    }

    /// Finds the active voice matching `(key, voice_context)`, if any — used by
    /// `Sampler::note_off` (TDD §11.4: voice-context tagging keeps overlapping
    /// clips' note-offs from killing each other's voices).
    pub fn find_active_mut(&mut self, key: u8, voice_context: u32) -> Option<&mut Voice> {
        self.voices
            .iter_mut()
            .find(|v| v.is_active() && v.key() == key && v.voice_context() == voice_context)
    }

    pub fn iter_active_mut(&mut self) -> impl Iterator<Item = &mut Voice> {
        self.voices.iter_mut().filter(|v| v.is_active())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::{FilterSlot, Layer, Patch, Source};
    use crate::playback::{LoopMode, PlaybackConfig};
    use crate::streaming::{SampleBuffer, SampleStore};
    use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};

    const SR: f32 = 48_000.0;

    fn instant_envelope(sustain: f32) -> EnvelopeConfig {
        EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: sustain,
            release_s: 0.01,
        }
    }

    fn disabled_filter() -> FilterSlot {
        FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
        }
    }

    /// A one-shot (non-looping) buffer of constant amplitude `level`, `len` samples
    /// long, at the engine's own sample rate so pitch/rate conversion is 1:1 and
    /// every test assertion is about envelope/gain/range logic, not resampling.
    fn flat_patch(store: &mut SampleStore, level: f32, len: usize, gain_db: f32) -> Patch {
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(vec![level; len]),
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
                    end_offset: len as f64,
                    ..PlaybackConfig::default()
                },
                gain_db,
                pan: 0.0,
            }],
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(1.0), instant_envelope(1.0)],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: VoiceConfig::default(),
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn silent_before_trigger() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();
        assert!(!voice.is_active());

        let mut out = vec![0.0; 128];
        voice.render(&patch, &store, SR, &mut out);
        assert_eq!(rms(&out), 0.0);
    }

    #[test]
    fn triggered_note_in_key_range_produces_sound() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();

        voice.trigger(&patch, 60, 127, 0);
        assert!(voice.is_active());

        let mut out = vec![0.0; 128];
        voice.render(&patch, &store, SR, &mut out);
        assert!(
            rms(&out) > 0.5,
            "expected near-full-scale output, got rms {}",
            rms(&out)
        );
    }

    #[test]
    fn note_outside_key_range_stays_silent() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].key_range = (60, 72);
        let mut voice = Voice::new();

        voice.trigger(&patch, 40, 127, 0);

        let mut out = vec![0.0; 128];
        voice.render(&patch, &store, SR, &mut out);
        assert_eq!(
            rms(&out),
            0.0,
            "a note outside the layer's key range must produce silence"
        );
    }

    #[test]
    fn gain_db_attenuates_output() {
        let mut store_full = SampleStore::new();
        let patch_full = flat_patch(&mut store_full, 1.0, 1000, 0.0);
        let mut voice_full = Voice::new();
        voice_full.trigger(&patch_full, 60, 127, 0);
        let mut out_full = vec![0.0; 64];
        voice_full.render(&patch_full, &store_full, SR, &mut out_full);

        let mut store_quiet = SampleStore::new();
        let patch_quiet = flat_patch(&mut store_quiet, 1.0, 1000, -6.0);
        let mut voice_quiet = Voice::new();
        voice_quiet.trigger(&patch_quiet, 60, 127, 0);
        let mut out_quiet = vec![0.0; 64];
        voice_quiet.render(&patch_quiet, &store_quiet, SR, &mut out_quiet);

        let ratio = rms(&out_quiet) / rms(&out_full);
        let expected = 10f32.powf(-6.0 / 20.0); // -6dB ~= 0.5012
        assert!(
            (ratio - expected).abs() < 0.01,
            "expected ~{expected} amplitude ratio for -6dB, got {ratio}"
        );
    }

    #[test]
    fn release_fades_out_and_deactivates_the_voice() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 100_000, 0.0);
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);

        // Settle into sustain. `render` documents that it *adds* into `out`
        // rather than clearing it (so multiple voices can mix into one
        // buffer) — a test that wants to inspect a single call's output must
        // clear the buffer itself first, same as any real caller would.
        let mut scratch = vec![0.0; 256];
        for _ in 0..10 {
            scratch.fill(0.0);
            voice.render(&patch, &store, SR, &mut scratch);
        }
        assert!(voice.is_active());

        voice.release();
        // release_s = 0.01s @ 48kHz = 480 samples; render well past that.
        for _ in 0..20 {
            scratch.fill(0.0);
            voice.render(&patch, &store, SR, &mut scratch);
        }
        assert!(
            !voice.is_active(),
            "voice must deactivate once its release finishes"
        );
        assert_eq!(
            rms(&scratch),
            0.0,
            "a fully-released voice must render silence"
        );
    }

    #[test]
    fn forward_loop_keeps_the_voice_alive_past_the_natural_buffer_end() {
        let mut store = SampleStore::new();
        // Buffer shorter than one render block, looped, so a non-looping voice
        // would necessarily go silent partway through this single render call.
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(vec![1.0; 32]),
            sample_rate: SR as u32,
        });
        let patch = Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Forward,
                    loop_start: 0.0,
                    loop_end: 32.0,
                    end_offset: 32.0,
                    interpolation: Interpolation::Draft,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(1.0), instant_envelope(1.0)],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: VoiceConfig::default(),
        };

        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);

        let mut out = vec![0.0; 256]; // 8x the buffer length
        voice.render(&patch, &store, SR, &mut out);
        assert!(
            voice.is_active(),
            "a Forward-looped voice must not stop at the buffer's natural end"
        );
        assert!(
            rms(&out) > 0.9,
            "looping must keep producing full-scale output, got rms {}",
            rms(&out)
        );
    }
}
