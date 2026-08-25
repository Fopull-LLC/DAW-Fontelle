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

/// The gain a note-on velocity contributes, as a linear amplitude multiplier.
///
/// SF2 2.04 §8.4.1 specifies a default modulator that is present on every zone
/// unless the file overrides it: MIDI note-on velocity -> initial attenuation,
/// concave curve, negative direction, amount 960 centibels. Feeding the
/// concave curve's 96 dB span through that amount works out to amplitude
/// proportional to the *square* of normalised velocity — velocity 64 lands
/// ~12 dB down, not 6 — which is why soundfonts played with a linear velocity
/// response sound flat and undynamic.
///
/// This is the default modulator's net effect computed directly, not the
/// general modulator machinery: `ModMatrix::evaluate` is still unimplemented,
/// so a file that *overrides* this default is not honoured yet. Every file
/// that doesn't (the overwhelming majority) is now correct. See `PROGRESS.md`.
pub fn velocity_to_gain(velocity: u8) -> f32 {
    // Velocity 0 is a note-off in MIDI, never a very quiet note.
    if velocity == 0 {
        return 0.0;
    }
    let normalised = velocity as f32 / 127.0;
    normalised * normalised
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

/// One layer's per-render constants, resolved once before the sample loop in
/// `Voice::render` rather than recomputed per sample. Borrows the layer's PCM
/// straight out of the `SampleStore` — no copy, no allocation.
#[derive(Clone, Copy)]
struct PreparedLayer<'a> {
    data: &'a [f32],
    step: f64,
    gain: f32,
    loop_end: f64,
    loop_len: f64,
    looping: bool,
    end_offset: f64,
    interpolation: fontelle_dsp::Interpolation,
}

/// One playing note. Fixed-topology (INVARIANT 6): Layers → mix → Filter 1 → Filter 2
/// → Amp → Pan → out, with the mod matrix feeding every stage. Predictable per-voice
/// cost, zero allocation on note-on, no graph compilation on the audio thread.
///
/// **M0 scope note:** only `Source::Sample` layers render (`Source::Sf2Zone` and
/// `Source::Oscillator` are silent no-ops for now); output is a single (mono)
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
    /// Filter1 and Filter2 of the fixed voice topology (TDD §7.4). Per voice,
    /// not per patch: two notes sounding at once each need their own filter
    /// memory, and sharing one would make a voice's output depend on which
    /// other voices happened to render before it.
    filters: [fontelle_dsp::SvfFilter; 2],
    /// Fixed for the life of the note, from `velocity_to_gain`. Folded into
    /// each layer's gain at the top of `render` so it costs nothing per sample.
    velocity_gain: f32,
    /// Note-on velocity and key as the mod matrix sees them: normalised to
    /// 0..1, captured once so evaluating a route never has to reach back into
    /// the event that started the note.
    velocity_norm: f32,
    key_norm: f32,
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
            filters: [fontelle_dsp::SvfFilter::new(); 2],
            velocity_gain: 0.0,
            velocity_norm: 0.0,
            key_norm: 0.0,
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
        self.velocity_gain = velocity_to_gain(velocity);
        self.velocity_norm = velocity as f32 / 127.0;
        self.key_norm = key as f32 / 127.0;
        // A voice comes back out of the pool carrying the last note's filter
        // memory. Left alone, that discharges into the new note as a transient
        // belonging to a note that already ended — a click that only shows up
        // once voices start being reused.
        for filter in &mut self.filters {
            filter.reset();
        }
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
    ///
    /// **The loop is sample-major, not layer-major, and that is load-bearing.**
    /// The obvious layer-major shape — add every layer across the whole
    /// buffer, then multiply the buffer by the envelope — applies this voice's
    /// envelope to whatever *other* voices already mixed into `out`, since
    /// `out` is shared. With N voices the first one's output ends up
    /// multiplied by all N envelopes; audibly, holding a chord and adding a
    /// note ducks the held notes for the length of the new note's attack.
    /// That was a real bug here, caught by
    /// `sampler::tests::a_new_notes_attack_does_not_duck_already_sounding_voices`.
    /// Advancing the envelope once per output sample and scaling only this
    /// voice's own mixed sample before adding it is what keeps the additive
    /// contract honest.
    /// `quality` is the session's interpolation setting, used by any layer
    /// that names none of its own — see `Sampler::set_quality`.
    pub fn render(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
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
                    curve: fontelle_dsp::EnvelopeCurve::Linear,
                });

        // Per-layer constants resolved once, not once per sample: a fixed-size
        // stack array (INVARIANT 1 — no `Vec`, nothing heap-touching).
        let mut prepared: [Option<PreparedLayer<'_>>; MAX_LAYERS] = [None; MAX_LAYERS];
        for (index, (slot, layer)) in self.layers.iter().zip(patch.layers.iter()).enumerate() {
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
            let loop_len = layer.playback.loop_end - layer.playback.loop_start;

            prepared[index] = Some(PreparedLayer {
                data: &buffer.data,
                step: (pitch_ratio * rate_ratio) as f64,
                gain: 10f32.powf(layer.gain_db / 20.0) * self.velocity_gain,
                loop_end: layer.playback.loop_end,
                loop_len,
                // `loop_len > 0.0` also guards the wrap loop below against
                // spinning forever on a degenerate zero-length loop.
                looping: matches!(layer.playback.loop_mode, crate::LoopMode::Forward)
                    && loop_len > 0.0,
                end_offset: layer.playback.end_offset,
                interpolation: layer.playback.interpolation.unwrap_or(quality),
            });
        }

        // Resolved once per block: nothing modulates cutoff or resonance yet
        // (`ModMatrix::evaluate` is still unimplemented). When something does,
        // this moves inside the sample loop — the zero-delay-feedback topology
        // exists precisely so that it can.
        // The mod matrix's view of this voice. Only the note-on sources are
        // live: LFOs aren't built, and envelopes are not yet exposed as
        // sources (`Envelope(0)` drives the amp stage directly). Everything
        // else reads as at-rest rather than as a plausible-looking number.
        let sources = |source: crate::mod_matrix::ModSource| match source {
            crate::mod_matrix::ModSource::Velocity => self.velocity_norm,
            crate::mod_matrix::ModSource::Key => self.key_norm,
            _ => 0.0,
        };

        let filter_coeffs: [Option<fontelle_dsp::SvfCoeffs>; 2] = std::array::from_fn(|index| {
            let slot = patch.filters[index];
            if !slot.enabled {
                return None;
            }
            // Cutoff modulation is in cents, so it scales the corner rather
            // than shifting it — an octave down means the same thing at 200 Hz
            // as at 8 kHz, which a linear offset would not.
            let dest = crate::mod_matrix::ModDest::FilterCutoff(index as u8);
            let cents = patch.mod_matrix.evaluate(dest, &sources) * dest.full_scale();
            let cutoff = slot.cutoff_hz * 2f32.powf(cents / 1200.0);
            Some(fontelle_dsp::SvfFilter::coeffs(
                slot.mode,
                cutoff,
                slot.resonance,
                0.0,
                sample_rate,
            ))
        });

        for out_sample in out.iter_mut() {
            let env = self.amp_env.advance(&amp_env_config, sample_rate);

            let mut mixed = 0.0;
            for (index, prep) in prepared.iter().enumerate() {
                let Some(prep) = prep else { continue };
                let slot = &mut self.layers[index];
                if !slot.active {
                    continue;
                }

                if prep.looping {
                    // `while`, not `if`: one subtraction isn't enough when the
                    // playback step exceeds the loop length, which real
                    // extreme upward transposition of a short loop does.
                    while slot.position >= prep.loop_end {
                        slot.position -= prep.loop_len;
                    }
                } else if slot.position >= prep.end_offset {
                    slot.active = false;
                    continue;
                }

                mixed += fontelle_dsp::interpolate(prep.data, slot.position, prep.interpolation)
                    * prep.gain;
                slot.position += prep.step;
            }

            // Layers -> mix -> Filter1 -> Filter2 -> Amp (TDD §7.4). The
            // filters sit ahead of the amp stage, and operate on this voice's
            // own mixed sample rather than on the shared output buffer.
            for (coeffs, filter) in filter_coeffs.iter().zip(self.filters.iter_mut()) {
                if let Some(coeffs) = coeffs {
                    mixed = filter.process(mixed, coeffs);
                }
            }

            *out_sample += mixed * env;
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
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

    const SR: f32 = 48_000.0;

    fn instant_envelope(sustain: f32) -> EnvelopeConfig {
        EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: sustain,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
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
                    interpolation: Some(Interpolation::Draft),
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
        voice.render(&patch, &store, SR, Interpolation::Normal, &mut out);
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
        voice.render(&patch, &store, SR, Interpolation::Normal, &mut out);
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
        voice.render(&patch, &store, SR, Interpolation::Normal, &mut out);
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
        voice_full.render(
            &patch_full,
            &store_full,
            SR,
            Interpolation::Normal,
            &mut out_full,
        );

        let mut store_quiet = SampleStore::new();
        let patch_quiet = flat_patch(&mut store_quiet, 1.0, 1000, -6.0);
        let mut voice_quiet = Voice::new();
        voice_quiet.trigger(&patch_quiet, 60, 127, 0);
        let mut out_quiet = vec![0.0; 64];
        voice_quiet.render(
            &patch_quiet,
            &store_quiet,
            SR,
            Interpolation::Normal,
            &mut out_quiet,
        );

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
            voice.render(&patch, &store, SR, Interpolation::Normal, &mut scratch);
        }
        assert!(voice.is_active());

        voice.release();
        // release_s = 0.01s @ 48kHz = 480 samples; render well past that.
        for _ in 0..20 {
            scratch.fill(0.0);
            voice.render(&patch, &store, SR, Interpolation::Normal, &mut scratch);
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
                    interpolation: Some(Interpolation::Draft),
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
        voice.render(&patch, &store, SR, Interpolation::Normal, &mut out);
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

    #[test]
    fn velocity_scales_output_on_the_sf2_default_curve() {
        // Two identical patches, two velocities. SF2's always-present default
        // velocity -> initial-attenuation modulator makes amplitude scale with
        // the square of normalised velocity, so half velocity is roughly a
        // quarter of the amplitude (-12 dB), not half and certainly not the
        // same.
        let render_at = |velocity: u8| {
            let mut store = SampleStore::new();
            let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
            let mut voice = Voice::new();
            voice.trigger(&patch, 60, velocity, 0);
            let mut out = vec![0.0; 64];
            voice.render(&patch, &store, SR, Interpolation::Normal, &mut out);
            rms(&out)
        };

        let full = render_at(127);
        let half = render_at(64);
        let ratio = half / full;
        let expected = (64.0f32 / 127.0).powi(2);
        assert!(
            (ratio - expected).abs() < 0.01,
            "velocity 64 against 127 should be ~{expected} of the amplitude, got {ratio}"
        );
    }

    #[test]
    fn full_velocity_is_unity_gain() {
        // The velocity curve must not quietly attenuate everything: 127 is the
        // reference point, so a full-scale sample at velocity 127 and 0 dB
        // layer gain still comes out at full scale.
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut out = vec![0.0; 64];
        voice.render(&patch, &store, SR, Interpolation::Normal, &mut out);
        assert!(
            (rms(&out) - 1.0).abs() < 1e-4,
            "velocity 127 must be unity gain, got {}",
            rms(&out)
        );
    }

    #[test]
    fn velocity_to_gain_spans_the_full_sf2_attenuation_range() {
        assert_eq!(velocity_to_gain(127), 1.0);
        assert!((velocity_to_gain(64) - 0.253_9).abs() < 1e-3);
        assert_eq!(
            velocity_to_gain(0),
            0.0,
            "velocity 0 is a note-off in MIDI and must never make sound"
        );
        assert!(
            velocity_to_gain(1) < 1e-4,
            "the default modulator's 960 cB amount puts velocity 1 ~84 dB down"
        );
    }

    /// A buffer alternating +1/-1: content sitting exactly at Nyquist, which no
    /// lowpass worth the name lets through.
    fn bright_patch(store: &mut SampleStore, len: usize) -> Patch {
        let data: Vec<f32> = (0..len)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(data),
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
                    end_offset: len as f64,
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
        }
    }

    /// A steady tone at `freq_hz`, for measuring a rolloff. Unlike the
    /// alternating-sample fixture above this sits *below* Nyquist, where a
    /// bilinear-transform lowpass has a finite response — the bilinear map puts
    /// a double zero exactly at Nyquist, so Nyquist content is annihilated by
    /// the first filter and says nothing about the second.
    fn tone_patch(store: &mut SampleStore, freq_hz: f32, len: usize) -> Patch {
        let data: Vec<f32> = (0..len)
            .map(|i| (std::f32::consts::TAU * freq_hz * i as f32 / SR).sin())
            .collect();
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(data),
            sample_rate: SR as u32,
        });
        let mut patch = bright_patch(&mut SampleStore::new(), len);
        patch.layers[0].source = Source::Sample { file: asset };
        patch
    }

    fn render_patch(patch: &Patch, store: &SampleStore, len: usize) -> Vec<f32> {
        let mut voice = Voice::new();
        voice.trigger(patch, 60, 127, 0);
        let mut out = vec![0.0; len];
        voice.render(patch, store, SR, Interpolation::Draft, &mut out);
        out
    }

    #[test]
    fn an_enabled_lowpass_removes_high_frequency_content() {
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        let unfiltered = rms(&render_patch(&patch, &store, 256));

        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };
        let filtered = rms(&render_patch(&patch, &store, 256));

        assert!(unfiltered > 0.9, "the fixture should be full scale");
        assert!(
            filtered < unfiltered * 0.05,
            "a 500 Hz lowpass should all but remove Nyquist content: {filtered} vs {unfiltered}"
        );
    }

    #[test]
    fn a_disabled_filter_slot_changes_nothing() {
        // `enabled` has to be a real bypass, not a filter set wide open — an
        // "off" filter that still runs costs CPU on every voice and colours
        // the signal at the top of the band.
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        let baseline = render_patch(&patch, &store, 256);

        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 500.0,
            resonance: 4.0,
            enabled: false,
        };
        assert_eq!(baseline, render_patch(&patch, &store, 256));
    }

    #[test]
    fn both_filter_slots_are_applied_in_series() {
        let mut store = SampleStore::new();
        // Two octaves above the corner: about -24 dB through one 2-pole
        // section and -48 dB through the pair, both comfortably measurable.
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        let slot = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 1_500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };
        patch.filters[0] = slot;
        let one_pole_pair = rms(&render_patch(&patch, &store, 256)[128..]);
        patch.filters[1] = slot;
        let two_pole_pairs = rms(&render_patch(&patch, &store, 256)[128..]);

        assert!(
            two_pole_pairs < one_pole_pair * 0.5,
            "a second identical lowpass must steepen the rolloff: \
             {two_pole_pairs} vs {one_pole_pair}"
        );
    }

    #[test]
    fn filter_state_does_not_leak_from_the_previous_note() {
        // A voice is reused from the pool, so a note that inherits the last
        // note's filter memory starts with a transient that has nothing to do
        // with it — a click, and one that only appears under voice reuse.
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 800.0,
            resonance: 6.0,
            enabled: true,
        };

        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut first = vec![0.0; 256];
        voice.render(&patch, &store, SR, Interpolation::Draft, &mut first);

        // Same voice, second note.
        voice.trigger(&patch, 60, 127, 1);
        let mut second = vec![0.0; 256];
        voice.render(&patch, &store, SR, Interpolation::Draft, &mut second);

        assert_eq!(
            first, second,
            "a retriggered voice must start from a clean filter, not the last note's tail"
        );
    }

    #[test]
    fn each_voice_filters_only_its_own_contribution() {
        // The same shared-output-buffer trap the amp envelope fell into: the
        // filter must run on this voice's mixed sample, not on whatever is
        // already sitting in `out`.
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };

        let mut alone = vec![0.0; 256];
        let mut voice_a = Voice::new();
        voice_a.trigger(&patch, 60, 127, 0);
        voice_a.render(&patch, &store, SR, Interpolation::Draft, &mut alone);

        let mut together = vec![0.0; 256];
        let mut first = Voice::new();
        let mut second = Voice::new();
        first.trigger(&patch, 60, 127, 0);
        second.trigger(&patch, 60, 127, 1);
        first.render(&patch, &store, SR, Interpolation::Draft, &mut together);
        second.render(&patch, &store, SR, Interpolation::Draft, &mut together);

        for (i, (one, two)) in alone.iter().zip(together.iter()).enumerate() {
            assert!(
                (two - one * 2.0).abs() < 1e-4,
                "sample {i}: two identical voices should sum to twice one, got {two} vs {}",
                one * 2.0
            );
        }
    }

    fn cutoff_route(depth: f32, invert: bool) -> crate::mod_matrix::ModMatrix {
        crate::mod_matrix::ModMatrix {
            routes: vec![crate::mod_matrix::ModRoute {
                source: crate::mod_matrix::ModSource::Velocity,
                destination: crate::mod_matrix::ModDest::FilterCutoff(0),
                depth,
                curve: crate::mod_matrix::Curve::Linear,
                via: None,
                invert,
            }],
        }
    }

    /// Brightness independent of loudness: velocity scales amplitude too, so a
    /// raw level comparison would measure the velocity curve rather than the
    /// filter. Dividing by the known velocity gain isolates the cutoff.
    fn brightness_at(patch: &Patch, store: &SampleStore, velocity: u8) -> f32 {
        let mut voice = Voice::new();
        voice.trigger(patch, 60, velocity, 0);
        let mut out = vec![0.0; 512];
        voice.render(patch, store, SR, Interpolation::Draft, &mut out);
        rms(&out[256..]) / velocity_to_gain(velocity)
    }

    #[test]
    fn a_velocity_to_cutoff_route_makes_soft_notes_darker() {
        // What every sampler does and Fontelle did not: play quietly and the
        // tone closes down, not just the level.
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 6_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };
        // SF2's own default: full velocity leaves the cutoff alone, and it
        // falls away as velocity drops.
        patch.mod_matrix = cutoff_route(-0.25, true);

        let loud = brightness_at(&patch, &store, 127);
        let soft = brightness_at(&patch, &store, 20);
        assert!(
            soft < loud * 0.5,
            "a soft note must be audibly darker once the cutoff is modulated: \
             {soft} against {loud}"
        );
    }

    #[test]
    fn without_a_route_velocity_leaves_the_cutoff_alone() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 6_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };

        let loud = brightness_at(&patch, &store, 127);
        let soft = brightness_at(&patch, &store, 20);
        assert!(
            (soft - loud).abs() < loud * 0.02,
            "with no route, velocity must change loudness only: {soft} against {loud}"
        );
    }

    #[test]
    fn an_uninverted_route_brightens_hard_notes_instead() {
        // The same route without SF2's negative direction: the modulation
        // rises with velocity rather than falling away from full scale, so a
        // hard note opens up past the patch's own cutoff.
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 1_500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };
        patch.mod_matrix = cutoff_route(0.25, false);

        assert!(brightness_at(&patch, &store, 127) > brightness_at(&patch, &store, 20) * 2.0);
    }

    #[test]
    fn cutoff_modulation_is_ignored_when_the_filter_is_off() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.mod_matrix = cutoff_route(-0.25, true);
        assert!(
            (brightness_at(&patch, &store, 20) - brightness_at(&patch, &store, 127)).abs() < 1e-3,
            "a disabled filter slot has no cutoff to modulate"
        );
    }
}
