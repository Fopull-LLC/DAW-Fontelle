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

/// Modulation envelopes per voice, on top of `patch.envelopes[0]` — the amp
/// envelope, which every voice always has and which drives the amp stage
/// directly rather than through the matrix.
///
/// A fixed array, like the layers, because a voice's cost has to be knowable
/// before it sounds (INVARIANT 6) and note-on must not allocate (INVARIANT 1).
/// Three is one more than any SF2 file can ask for, which defines exactly one
/// modulation envelope.
pub const MAX_MOD_ENVELOPES: usize = 3;

/// LFOs per voice, for the same reason. SF2 defines two (vibrato and
/// modulation); four leaves room for a patch built in Fontelle rather than
/// imported.
pub const MAX_LFOS: usize = 4;

/// Which envelopes and LFOs at least one route reads, as `(envelopes, lfos)`.
/// Index 0 of `envelopes` is the amp envelope, which is advanced regardless
/// because it drives the amp stage; the flag is there so the indices line up
/// with `ModSource::Envelope`.
///
/// `via` counts as much as `source` does: a route whose depth is scaled by an
/// LFO needs that LFO turning even though it is not the thing being shaped.
fn sources_in_use(
    matrix: &crate::mod_matrix::ModMatrix,
) -> ([bool; MAX_MOD_ENVELOPES + 1], [bool; MAX_LFOS]) {
    let mut envelopes = [false; MAX_MOD_ENVELOPES + 1];
    let mut lfos = [false; MAX_LFOS];
    for route in &matrix.routes {
        for source in [Some(route.source), route.via].into_iter().flatten() {
            match source {
                crate::mod_matrix::ModSource::Envelope(index) => {
                    if let Some(slot) = envelopes.get_mut(index as usize) {
                        *slot = true;
                    }
                }
                crate::mod_matrix::ModSource::Lfo(index) => {
                    if let Some(slot) = lfos.get_mut(index as usize) {
                        *slot = true;
                    }
                }
                _ => {}
            }
        }
    }
    (envelopes, lfos)
}

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
    /// This layer's place in the stereo field, already through the pan law.
    /// `(1.0, 0.0)` on a mono render: nothing goes to a channel that isn't
    /// there, and the one that is carries the layer unattenuated.
    pan_gain: (f32, f32),
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
    /// Filter1 and Filter2 of the fixed voice topology (TDD §7.4), each with
    /// one filter per output channel: `filters[slot][channel]`.
    ///
    /// Per voice, not per patch: two notes sounding at once each need their
    /// own filter memory, and sharing one would make a voice's output depend
    /// on which other voices happened to render before it. Per channel for
    /// the same reason one step down — once layers are panned apart the two
    /// channels carry different signals, and one shared state would let each
    /// side's history bleed into the other, collapsing the image.
    filters: [[fontelle_dsp::SvfFilter; 2]; 2],
    /// Fixed for the life of the note, from `velocity_to_gain`. Folded into
    /// each layer's gain at the top of `render` so it costs nothing per sample.
    velocity_gain: f32,
    /// Note-on velocity and key as the mod matrix sees them: normalised to
    /// 0..1, captured once so evaluating a route never has to reach back into
    /// the event that started the note.
    velocity_norm: f32,
    key_norm: f32,
    amp_env: fontelle_dsp::EnvelopeGenerator,
    /// `patch.envelopes[1..]`, as modulation sources. Per voice, because two
    /// notes are at different points in their envelopes.
    mod_envs: [fontelle_dsp::EnvelopeGenerator; MAX_MOD_ENVELOPES],
    /// `patch.lfos`, retriggered on every note-on: a free-running LFO makes
    /// the same note sound different depending on when it was played, which is
    /// a character an instrument can want but not a default anyone can
    /// predict.
    lfos: [fontelle_dsp::Oscillator; MAX_LFOS],
    /// Samples since this note started, for `Lfo::delay_s`. One counter for
    /// the voice rather than one per LFO: they all start together.
    age_samples: u64,
}

impl Voice {
    pub fn new() -> Self {
        Self {
            active: false,
            key: 0,
            voice_context: 0,
            age: 0,
            layers: [LayerPlayback::default(); MAX_LAYERS],
            filters: [[fontelle_dsp::SvfFilter::new(); 2]; 2],
            velocity_gain: 0.0,
            velocity_norm: 0.0,
            key_norm: 0.0,
            amp_env: fontelle_dsp::EnvelopeGenerator::new(),
            mod_envs: [fontelle_dsp::EnvelopeGenerator::new(); MAX_MOD_ENVELOPES],
            lfos: [fontelle_dsp::Oscillator::new(); MAX_LFOS],
            age_samples: 0,
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
        for slot in &mut self.filters {
            for filter in slot {
                filter.reset();
            }
        }
        self.amp_env.note_on();
        for env in &mut self.mod_envs {
            env.note_on();
        }
        for lfo in &mut self.lfos {
            lfo.reset();
        }
        self.age_samples = 0;

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

    /// Silences the voice at once and returns it to the state it had before
    /// it ever played: no tail, no filter memory, no envelope position.
    ///
    /// This is what transport stop and seek need. **It is a hard cut**, and on
    /// a sounding note that is a click — which is right when the audio after
    /// the cut belongs to a different part of the song, and wrong as a way to
    /// end a note. `Sampler::release_all` is the graceful one.
    ///
    /// The filter state matters as much as the envelope: left alone it
    /// discharges into the next note as a transient belonging to one that no
    /// longer exists.
    pub fn reset(&mut self) {
        self.active = false;
        self.key = 0;
        self.voice_context = 0;
        self.age = 0;
        self.layers = [LayerPlayback::default(); MAX_LAYERS];
        for slot in &mut self.filters {
            for filter in slot {
                filter.reset();
            }
        }
        self.velocity_gain = 0.0;
        self.velocity_norm = 0.0;
        self.key_norm = 0.0;
        self.amp_env = fontelle_dsp::EnvelopeGenerator::new();
        self.mod_envs = [fontelle_dsp::EnvelopeGenerator::new(); MAX_MOD_ENVELOPES];
        self.lfos = [fontelle_dsp::Oscillator::new(); MAX_LFOS];
        self.age_samples = 0;
    }

    /// Voice stealing always ramps out over a short release rather than cutting
    /// hard (TDD §7.4) — never a click. Uses the same envelope release as a
    /// normal note-off; a shorter, dedicated steal-ramp is a later refinement.
    pub fn release(&mut self) {
        self.amp_env.note_off();
        // A modulation envelope releases with the note too: a filter envelope
        // that stayed open through the release would keep the tail brighter
        // than the note that produced it.
        for env in &mut self.mod_envs {
            env.note_off();
        }
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
    ///
    /// `out` is planar: `[left, right]` for stereo, `[mono]` for one channel,
    /// and anything past the second slice is left alone. **A mono render
    /// ignores `Layer::pan` rather than folding it down**, because the centre
    /// pan-law gain applied to a signal with nowhere to pan is just a
    /// uniform 3 dB of attenuation the caller never asked for — the same call
    /// `MixerTrackNode` makes for a mono track.
    pub fn render(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        out: &mut [&mut [f32]],
    ) {
        self.render_with_pan(patch, store, sample_rate, quality, 0.0, out)
    }

    /// As [`Voice::render`], with the channel's own pan folded into every
    /// layer's placement.
    ///
    /// `channel_pan` is the compiled form of MIDI CC10 — a control over the
    /// whole part, distinct from the `pan` an SF2 zone carries for itself.
    /// The two **add**, then clamp: that is what a soundfont player does, and
    /// it is the only reading under which a hard-left zone on a channel panned
    /// right ends up between them rather than at whichever was consulted last.
    ///
    /// It is read here rather than captured at note-on because it is live:
    /// moving a part's pan has to move the notes already sounding.
    pub fn render_with_pan(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        channel_pan: f32,
        out: &mut [&mut [f32]],
    ) {
        if !self.active || out.is_empty() {
            return;
        }
        let stereo = out.len() >= 2;
        let frames = out.iter().take(2).map(|c| c.len()).min().unwrap_or(0);

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

        // Modulation runs at block rate: every source is sampled once here and
        // held for the whole block, and every destination is resolved from it
        // before the sample loop. At the engine's 128-frame blocks that is a
        // 375 Hz control rate — the same order as every hardware sampler ever
        // shipped, and cheap enough that a filter envelope costs a handful of
        // operations per block rather than per sample.
        //
        // The amp envelope is the exception: it advances per sample, because
        // it is a gain rather than a control value and a stepped one is
        // audible as a buzz on fast attacks.
        let mut env_levels = [0.0f32; MAX_MOD_ENVELOPES];
        let mut lfo_values = [0.0f32; MAX_LFOS];
        // Only the sources some route actually names are advanced. An SF2
        // import gives every patch a modulation envelope and two LFOs whether
        // it uses them or not, and one unread envelope is a stage advance per
        // sample per voice — the same order as the amp envelope, for nothing.
        // Scanning the routes to find out is O(routes) per block against
        // O(frames) saved.
        let (env_used, lfo_used) = sources_in_use(&patch.mod_matrix);
        {
            for (index, config) in patch
                .envelopes
                .iter()
                .skip(1)
                .take(MAX_MOD_ENVELOPES)
                .enumerate()
            {
                if !env_used[index + 1] {
                    continue;
                }
                let env = &mut self.mod_envs[index];
                // Read before advancing: the value a destination uses this
                // block is the one at its start, not its end.
                env_levels[index] = env.level();
                for _ in 0..frames {
                    env.advance(config, sample_rate);
                }
            }
            for (index, lfo) in patch.lfos.iter().take(MAX_LFOS).enumerate() {
                if !lfo_used[index] {
                    continue;
                }
                let value =
                    self.lfos[index].advance_block(lfo.shape, lfo.rate_hz, sample_rate, frames);
                // Held at rest until the delay elapses, and the oscillator is
                // advanced regardless — one that only started turning after
                // its delay would always begin at the same point in its cycle
                // as one with no delay at all, which is not what a delay is.
                let delay_samples = (lfo.delay_s.max(0.0) * sample_rate) as u64;
                lfo_values[index] = if self.age_samples >= delay_samples {
                    value * lfo.depth
                } else {
                    0.0
                };
            }
        }

        // The mod matrix's view of this voice, bound to locals rather than
        // reaching through `self`, so the closure holds no borrow of the voice
        // and the sample loop below is free to take `self.layers` mutably.
        //
        // Aftertouch, the mod wheel, pitch bend, `Random` and `NoteOnCounter`
        // read as at-rest: no MIDI controller state reaches a voice yet, and a
        // plausible-looking number would be worse than an honest zero.
        let (velocity_norm, key_norm) = (self.velocity_norm, self.key_norm);
        let amp_level = self.amp_env.level();
        let sources = move |source: crate::mod_matrix::ModSource| match source {
            crate::mod_matrix::ModSource::Velocity => velocity_norm,
            crate::mod_matrix::ModSource::Key => key_norm,
            // Envelope 0 is the amp envelope. It drives the amp stage
            // directly, and is readable here as well because "louder means
            // brighter" is a route a patch legitimately wants and there is no
            // reason to make it add a second envelope to get it.
            crate::mod_matrix::ModSource::Envelope(0) => amp_level,
            crate::mod_matrix::ModSource::Envelope(index) => {
                env_levels.get(index as usize - 1).copied().unwrap_or(0.0)
            }
            crate::mod_matrix::ModSource::Lfo(index) => {
                lfo_values.get(index as usize).copied().unwrap_or(0.0)
            }
            _ => 0.0,
        };

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

            // Pitch modulation is in cents, like the tuning it adds to, so a
            // route means the same interval wherever the note sits.
            let pitch_dest = crate::mod_matrix::ModDest::LayerPitch(index as u8);
            let pitch_cents =
                patch.mod_matrix.evaluate(pitch_dest, &sources) * pitch_dest.full_scale();
            let semitones = (self.key as f32 - layer.root_key as f32)
                + (layer.fine_tune_cents + pitch_cents) / 100.0;
            let pitch_ratio = 2f32.powf(semitones / 12.0);
            let rate_ratio = buffer.sample_rate as f32 / sample_rate;
            let loop_len = layer.playback.loop_end - layer.playback.loop_start;

            // SF2 pans a zone on a constant-power taper, and that is also
            // what keeps a layer's loudness steady as a route sweeps it
            // across the field. `ModDest::LayerPan`'s full scale is 1.0 —
            // half the field — so a full-depth route moves a centred layer
            // all the way to one side.
            // Gain modulation is in decibels, so a tremolo is symmetric in
            // loudness rather than lopsided the way a linear one would be.
            let gain_dest = crate::mod_matrix::ModDest::LayerGain(index as u8);
            let gain_db = patch.mod_matrix.evaluate(gain_dest, &sources) * gain_dest.full_scale();

            let pan_gain = if stereo {
                let dest = crate::mod_matrix::ModDest::LayerPan(index as u8);
                let pan = layer.pan
                    + channel_pan
                    + patch.mod_matrix.evaluate(dest, &sources) * dest.full_scale();
                fontelle_types::PanLaw::Minus3Db.gains(pan)
            } else {
                (1.0, 0.0)
            };

            prepared[index] = Some(PreparedLayer {
                data: &buffer.data,
                step: (pitch_ratio * rate_ratio) as f64,
                gain: 10f32.powf((layer.gain_db + gain_db) / 20.0) * self.velocity_gain,
                pan_gain,
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

        // Resolved once per block: nothing modulates cutoff or resonance
        // faster than that yet. When something does, this moves inside the
        // sample loop — the zero-delay-feedback topology exists precisely so
        // that it can.
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
            // Resonance is a Q, and a route offsets it directly: full scale is
            // 1.0 because the useful span between "barely damped" and "on the
            // edge of self-oscillation" is a couple of units, not decades.
            let q_dest = crate::mod_matrix::ModDest::FilterResonance(index as u8);
            let resonance =
                slot.resonance + patch.mod_matrix.evaluate(q_dest, &sources) * q_dest.full_scale();
            Some(fontelle_dsp::SvfFilter::coeffs(
                slot.mode,
                cutoff,
                resonance,
                0.0,
                sample_rate,
            ))
        });

        // Split once, outside the loop: `out[0]` and `out[1]` are distinct
        // slices, and taking both mutably per sample would be a reborrow the
        // compiler can't see through.
        let (left, rest) = out.split_at_mut(1);
        let left = &mut *left[0];
        let mut right = rest.first_mut();

        for frame in 0..frames {
            let env = self.amp_env.advance(&amp_env_config, sample_rate);

            // Layers -> pan -> mix: the pan is per layer because that is where
            // the format puts it (SF2's `pan` is a zone generator), and a
            // stereo SF2 sample is a pair of mono zones panned hard apart —
            // panning after the mix would fold every such instrument to the
            // centre.
            let mut mixed = (0.0f32, 0.0f32);
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

                let sample =
                    fontelle_dsp::interpolate(prep.data, slot.position, prep.interpolation)
                        * prep.gain;
                mixed.0 += sample * prep.pan_gain.0;
                mixed.1 += sample * prep.pan_gain.1;
                slot.position += prep.step;
            }

            // mix -> Filter1 -> Filter2 -> Amp (TDD §7.4). The filters sit
            // ahead of the amp stage, and operate on this voice's own mixed
            // sample rather than on the shared output buffer. One filter per
            // channel per slot: the two channels carry different signals the
            // moment layers are panned apart.
            for (coeffs, filters) in filter_coeffs.iter().zip(self.filters.iter_mut()) {
                if let Some(coeffs) = coeffs {
                    mixed.0 = filters[0].process(mixed.0, coeffs);
                    if right.is_some() {
                        mixed.1 = filters[1].process(mixed.1, coeffs);
                    }
                }
            }

            left[frame] += mixed.0 * env;
            if let Some(right) = right.as_deref_mut() {
                right[frame] += mixed.1 * env;
            }
        }

        self.age_samples += frames as u64;

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

    /// Silences every voice at once, including the inactive ones — an inactive
    /// voice still carries the filter and envelope state of whatever it last
    /// played, and that is exactly what a reset is for.
    ///
    /// The age counter is left alone: it only orders voices against each
    /// other, and restarting it would make the first voice allocated after a
    /// reset look older than one allocated before it.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
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
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
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
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
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
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
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
            &mut [&mut out_full[..]],
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
            &mut [&mut out_quiet[..]],
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
            voice.render(
                &patch,
                &store,
                SR,
                Interpolation::Normal,
                &mut [&mut scratch[..]],
            );
        }
        assert!(voice.is_active());

        voice.release();
        // release_s = 0.01s @ 48kHz = 480 samples; render well past that.
        for _ in 0..20 {
            scratch.fill(0.0);
            voice.render(
                &patch,
                &store,
                SR,
                Interpolation::Normal,
                &mut [&mut scratch[..]],
            );
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
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
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
            voice.render(
                &patch,
                &store,
                SR,
                Interpolation::Normal,
                &mut [&mut out[..]],
            );
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
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
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
        voice.render(patch, store, SR, Interpolation::Draft, &mut [&mut out[..]]);
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
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut first[..]],
        );

        // Same voice, second note.
        voice.trigger(&patch, 60, 127, 1);
        let mut second = vec![0.0; 256];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut second[..]],
        );

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
        voice_a.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut alone[..]],
        );

        let mut together = vec![0.0; 256];
        let mut first = Voice::new();
        let mut second = Voice::new();
        first.trigger(&patch, 60, 127, 0);
        second.trigger(&patch, 60, 127, 1);
        first.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut together[..]],
        );
        second.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut together[..]],
        );

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
        voice.render(patch, store, SR, Interpolation::Draft, &mut [&mut out[..]]);
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

    // --- Stereo: `Layer::pan` becomes real -----------------------------------

    /// Renders one block of `frames` into a fresh stereo pair.
    fn render_stereo(
        patch: &Patch,
        store: &SampleStore,
        key: u8,
        frames: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut voice = Voice::new();
        voice.trigger(patch, key, 127, 0);
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        voice.render(
            patch,
            store,
            SR,
            Interpolation::Draft,
            &mut [&mut left[..], &mut right[..]],
        );
        (left, right)
    }

    #[test]
    fn a_hard_left_layer_is_silent_on_the_right() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;

        let (left, right) = render_stereo(&patch, &store, 60, 64);
        assert!(
            (left[0] - 1.0).abs() < 1e-5,
            "hard left keeps the left channel at full scale, got {}",
            left[0]
        );
        assert!(
            right[0].abs() < 1e-5,
            "hard left must silence the right channel, got {}",
            right[0]
        );
    }

    /// SF2 pans a zone on a constant-power taper, so a centred layer reads
    /// 0.707 a side rather than 1.0 — the total power, not the per-channel
    /// level, is what stays put as it sweeps.
    #[test]
    fn a_centred_layer_splits_at_constant_power() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);

        let (left, right) = render_stereo(&patch, &store, 60, 64);
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!((left[0] - expected).abs() < 1e-5, "left {}", left[0]);
        assert!((right[0] - expected).abs() < 1e-5, "right {}", right[0]);
        let power = left[0] * left[0] + right[0] * right[0];
        assert!((power - 1.0).abs() < 1e-5, "total power {power}");
    }

    /// Two layers of one patch pointed opposite ways. **Deliberately unequal
    /// levels:** with both at the same level a layer that ignored its own pan
    /// and took the other's would be indistinguishable from correct.
    #[test]
    fn layers_panned_apart_land_in_different_channels() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let quiet = store.insert(SampleBuffer {
            data: std::sync::Arc::from(vec![0.25; 1000]),
            sample_rate: SR as u32,
        });
        patch.layers[0].pan = -1.0;
        let mut second = patch.layers[0].clone();
        second.source = Source::Sample { file: quiet };
        second.pan = 1.0;
        patch.layers.push(second);

        let (left, right) = render_stereo(&patch, &store, 60, 64);
        assert!(
            (left[0] - 1.0).abs() < 1e-5,
            "the loud layer belongs on the left alone, got {}",
            left[0]
        );
        assert!(
            (right[0] - 0.25).abs() < 1e-5,
            "the quiet layer belongs on the right alone, got {}",
            right[0]
        );
    }

    /// A mono caller has nowhere to pan to. Applying the centre pan-law gain
    /// anyway would drop every mono render 3 dB for no reason it can see —
    /// the same call `MixerTrackNode` makes for a mono track.
    #[test]
    fn a_mono_render_ignores_pan_rather_than_attenuating() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;

        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut out = vec![0.0; 64];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut out[..]],
        );
        assert!(
            (out[0] - 1.0).abs() < 1e-5,
            "a mono render must carry the layer at full level whatever its pan, got {}",
            out[0]
        );
    }

    #[test]
    fn the_mod_matrix_can_move_a_layers_pan() {
        use crate::mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};

        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.mod_matrix = ModMatrix {
            routes: vec![ModRoute {
                source: ModSource::Velocity,
                destination: ModDest::LayerPan(0),
                depth: 1.0,
                curve: Curve::Linear,
                via: None,
                invert: false,
            }],
        };

        // Velocity 127 is 1.0 normalised, so a full-depth route sweeps a
        // centred layer the whole way to hard right.
        let (left, right) = render_stereo(&patch, &store, 60, 64);
        assert!(
            left[0].abs() < 1e-4,
            "the route should have emptied the left channel, got {}",
            left[0]
        );
        assert!(
            (right[0] - 1.0).abs() < 1e-4,
            "and filled the right, got {}",
            right[0]
        );
    }

    /// Both channels of a stereo voice must filter independently. Sharing one
    /// filter's state between them makes each channel's output depend on the
    /// other's — an image that collapses and smears the moment the filter is
    /// on and the layers are panned apart.
    #[test]
    fn each_channel_carries_its_own_filter_state() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.layers[0].pan = -1.0;
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 1_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };

        let (_left, right) = render_stereo(&patch, &store, 60, 2048);
        let leak = rms(&right);
        assert!(
            leak < 1e-6,
            "a hard-left voice must stay silent on the right through the filter, got {leak}"
        );
    }

    #[test]
    fn a_channel_pan_places_a_centred_layer() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        voice.render_with_pan(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            -1.0,
            &mut [&mut left[..], &mut right[..]],
        );
        assert!((left[0] - 1.0).abs() < 1e-5, "left {}", left[0]);
        assert!(right[0].abs() < 1e-5, "right {}", right[0]);
    }

    /// MIDI CC10 and an SF2 zone's own `pan` generator are two different
    /// controls over one placement, and a soundfont player adds them: a hard-
    /// left zone on a channel panned right should end up somewhere in
    /// between, not at whichever of the two was consulted last.
    #[test]
    fn a_channel_pan_and_a_layer_pan_combine() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        voice.render_with_pan(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            1.0,
            &mut [&mut left[..], &mut right[..]],
        );
        let centred = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (left[0] - centred).abs() < 1e-5 && (right[0] - centred).abs() < 1e-5,
            "hard left plus hard right is centre, got {} / {}",
            left[0],
            right[0]
        );
    }

    #[test]
    fn a_channel_pan_past_the_ends_of_the_field_clamps() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        voice.render_with_pan(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            -1.0,
            &mut [&mut left[..], &mut right[..]],
        );
        assert!(
            (left[0] - 1.0).abs() < 1e-5 && right[0].abs() < 1e-5,
            "-2.0 of combined pan is still hard left, not past it, got {} / {}",
            left[0],
            right[0]
        );
    }

    // --- Envelopes and LFOs as modulation sources ----------------------------

    use crate::mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};

    fn mod_route(source: ModSource, destination: ModDest, depth: f32) -> ModMatrix {
        ModMatrix {
            routes: vec![ModRoute {
                source,
                destination,
                depth,
                curve: Curve::Linear,
                via: None,
                invert: false,
            }],
        }
    }

    /// Renders `frames` into one mono buffer, block by block at `block`, the
    /// way the engine drives it — modulation is sampled per block, so a test
    /// that renders one giant buffer would see exactly one modulation value
    /// and prove nothing about anything that moves.
    fn render_blocks(patch: &Patch, store: &SampleStore, frames: usize, block: usize) -> Vec<f32> {
        let mut voice = Voice::new();
        voice.trigger(patch, 60, 127, 0);
        let mut out = vec![0.0; frames];
        let mut at = 0;
        while at < frames {
            let n = block.min(frames - at);
            voice.render(
                patch,
                store,
                SR,
                Interpolation::Draft,
                &mut [&mut out[at..at + n]],
            );
            at += n;
        }
        out
    }

    /// Envelope-to-cutoff is what makes a filter sing, and until the matrix had
    /// an envelope to read it was unreachable: a patch could only be as bright
    /// as its velocity made it, fixed for the length of the note.
    #[test]
    fn a_modulation_envelope_opens_the_filter_over_the_length_of_a_note() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 48_000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 400.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
        };
        // Envelope 1 is the modulation envelope: a slow attack to full, held
        // there.
        patch.envelopes = vec![
            instant_envelope(1.0),
            EnvelopeConfig {
                delay_s: 0.0,
                attack_s: 0.2,
                hold_s: 0.0,
                decay_s: 0.0,
                sustain_level: 1.0,
                release_s: 0.01,
                curve: EnvelopeCurve::Linear,
            },
        ];
        // Four octaves up at full envelope.
        patch.mod_matrix = mod_route(ModSource::Envelope(1), ModDest::FilterCutoff(0), 0.5);

        let out = render_blocks(&patch, &store, 19_200, 128);
        let early = rms(&out[..2_400]);
        let late = rms(&out[14_400..]);
        assert!(
            late > early * 4.0,
            "the envelope must open the filter as the note develops: {early} \
             at the start against {late} at the end"
        );
    }

    /// The amp envelope is readable as `Envelope(0)` as well as driving the
    /// amp stage, so "louder means brighter" needs no second envelope.
    ///
    /// Measured against the *same patch without the route*, because the amp
    /// envelope raises the level either way: on a single-frequency tone a
    /// lowpass changes amplitude and not shape, so any proxy for brightness is
    /// really a proxy for level, and only the difference between the two
    /// renders isolates what the route did.
    #[test]
    fn the_amp_envelope_is_available_as_a_modulation_source() {
        let swell = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.2,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
        };
        let growth = |routed: bool| {
            let mut store = SampleStore::new();
            let mut patch = tone_patch(&mut store, 6_000.0, 48_000);
            patch.filters[0] = FilterSlot {
                mode: SvfMode::Lowpass,
                cutoff_hz: 400.0,
                resonance: std::f32::consts::FRAC_1_SQRT_2,
                enabled: true,
            };
            patch.envelopes = vec![swell];
            if routed {
                patch.mod_matrix = mod_route(ModSource::Envelope(0), ModDest::FilterCutoff(0), 0.5);
            }
            let out = render_blocks(&patch, &store, 19_200, 128);
            rms(&out[14_400..16_800]) / rms(&out[2_400..4_800]).max(1e-9)
        };

        let (with_route, without) = (growth(true), growth(false));
        assert!(
            with_route > without * 4.0,
            "routing the amp envelope to the cutoff must open the tone well \
             past what the envelope's own level explains: {with_route} against \
             {without}"
        );
    }

    /// Tremolo. An LFO that reaches nothing is a data shape, which is what
    /// `Lfo` was until now.
    #[test]
    fn an_lfo_makes_a_layers_gain_rise_and_fall_at_its_own_rate() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            delay_s: 0.0,
        }];
        // ±12 dB: unmistakable, and well short of the 96 dB full scale.
        patch.mod_matrix = mod_route(ModSource::Lfo(0), ModDest::LayerGain(0), 12.0 / 96.0);

        // One LFO cycle at 4 Hz is 12 000 samples; the peak is a quarter of the
        // way in and the trough three quarters.
        let out = render_blocks(&patch, &store, 24_000, 128);
        let peak = rms(&out[2_600..3_400]);
        let trough = rms(&out[8_600..9_400]);
        let ratio = peak / trough;
        // 24 dB between them, less whatever the window averages away.
        assert!(
            ratio > 8.0,
            "a ±12 dB tremolo should swing about 16x peak to trough, got {ratio}"
        );
        // And it must come back: a one-way ramp would pass the check above.
        let second_peak = rms(&out[14_600..15_400]);
        assert!(
            (second_peak / peak - 1.0).abs() < 0.1,
            "the LFO must be periodic: {peak} then {second_peak}"
        );
    }

    /// Vibrato. Measured as the pitch itself rather than the level, since a
    /// pitch route that quietly did nothing would still pass a loudness check.
    #[test]
    fn an_lfo_bends_a_layers_pitch_both_ways() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 1_000.0, 96_000);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 2.0,
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            delay_s: 0.0,
        }];
        // ±1200 cents: an octave each way, so the zero-crossing count moves
        // far enough to read off a short window.
        patch.mod_matrix = mod_route(ModSource::Lfo(0), ModDest::LayerPitch(0), 1200.0 / 9600.0);

        let out = render_blocks(&patch, &store, 24_000, 128);
        let crossings = |window: &[f32]| {
            window
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        // A 2 Hz LFO cycles in 24 000 samples: sharp a quarter in, flat three
        // quarters in.
        let sharp = crossings(&out[5_600..6_400]);
        let flat = crossings(&out[17_600..18_400]);
        assert!(
            sharp > flat * 3,
            "an octave of vibrato should roughly quadruple the crossing rate \
             between the two extremes: {sharp} against {flat}"
        );
    }

    /// Nothing routed anywhere means nothing moves — and, just as importantly,
    /// nothing is advanced: an envelope no route reads costs a `Vec` emptiness
    /// check rather than a stage advance per sample per voice.
    #[test]
    fn a_patch_with_no_routes_is_unmodulated() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            delay_s: 0.0,
        }];

        let out = render_blocks(&patch, &store, 24_000, 128);
        let first = rms(&out[2_600..3_400]);
        let later = rms(&out[8_600..9_400]);
        assert!(
            (first - later).abs() < 1e-5,
            "an LFO nothing routes must not reach the output: {first} against {later}"
        );
    }

    /// Vibrato that begins on the note's first sample is the single most
    /// recognisable way a sampled string section sounds synthetic.
    #[test]
    fn an_lfos_delay_holds_it_at_rest_before_it_starts() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            // Half a second: past the LFO's first two peaks.
            delay_s: 0.5,
        }];
        patch.mod_matrix = mod_route(ModSource::Lfo(0), ModDest::LayerGain(0), 12.0 / 96.0);

        let out = render_blocks(&patch, &store, 48_000, 128);
        // The LFO's first peak is 3 000 samples in and its first trough 9 000.
        let early_peak = rms(&out[2_600..3_400]);
        let early_trough = rms(&out[8_600..9_400]);
        assert!(
            (early_peak / early_trough - 1.0).abs() < 0.05,
            "nothing may move before the delay elapses: {early_peak} against \
             {early_trough}"
        );

        // 0.5 s is 24 000 samples, and the LFO's phase has kept turning: two
        // full cycles at 4 Hz, so it emerges where it would have been anyway.
        let late_peak = rms(&out[26_600..27_400]);
        let late_trough = rms(&out[32_600..33_400]);
        assert!(
            late_peak / late_trough > 8.0,
            "and it must be at full depth after: {late_peak} against {late_trough}"
        );
    }

    /// An LFO used only as a route's `via` still has to turn. It is not the
    /// thing being shaped, so a scan that looked at `source` alone would leave
    /// it parked at rest — and a route scaled by a source that never moves is
    /// a route that never fires.
    #[test]
    fn an_lfo_used_only_to_scale_another_route_still_runs() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            delay_s: 0.0,
        }];
        patch.mod_matrix = ModMatrix {
            routes: vec![ModRoute {
                source: ModSource::Velocity,
                destination: ModDest::LayerGain(0),
                depth: 12.0 / 96.0,
                curve: Curve::Linear,
                via: Some(ModSource::Lfo(0)),
                invert: false,
            }],
        };

        let out = render_blocks(&patch, &store, 24_000, 128);
        let peak = rms(&out[2_600..3_400]);
        let trough = rms(&out[8_600..9_400]);
        assert!(
            peak / trough > 8.0,
            "the via LFO must be running: {peak} against {trough}"
        );
    }
}
