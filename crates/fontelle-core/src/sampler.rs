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
    /// The channel's own level as a linear amplitude, applied to the whole
    /// mix on the way out. See [`Sampler::set_gain_db`].
    gain: f32,
    /// The wheels: what the hand playing this instrument is doing right now.
    /// See [`crate::Performance`], and the three setters below.
    performance: crate::Performance,
    /// The wavetables this patch's layers name, resolved in
    /// [`prepare`](Sampler::prepare).
    ///
    /// Resolved there and **nowhere else**: building a table allocates half a
    /// megabyte and asking the bank for one takes a lock, both of which are
    /// fine off the audio thread and forbidden on it (INVARIANT 1). See
    /// [`crate::WavetableSet`].
    tables: crate::WavetableSet,
}

impl Sampler {
    pub fn new(patch: Patch) -> Self {
        let capacity = patch.voice_config.polyphony;
        let mut sampler = Self {
            patch,
            voices: VoicePool::with_capacity(capacity),
            sample_rate: 48_000.0,
            quality: fontelle_dsp::Interpolation::Normal,
            pan: 0.0,
            gain: 1.0,
            performance: crate::Performance::default(),
            tables: crate::WavetableSet::new(),
        };
        // The tables the patch names, before it is ever rendered: a channel
        // whose first block was silent while a table built would be a synth
        // that stutters when you play the first note.
        sampler.tables.resolve(&sampler.patch);
        sampler
    }

    /// Off-RT: allocation permitted (matches `AudioNode::prepare` in `fontelle-engine`).
    /// The voice pool is already sized from `patch.voice_config.polyphony` at
    /// construction; `prepare` only needs to record the device's actual rate,
    /// since pitch/loop math is computed per-render from it.
    pub fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
        // Off-RT, so this may lock and allocate — and this is the one place
        // that is allowed to.
        self.tables.resolve(&self.patch);
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

    /// Sets how loud the whole part is, in decibels.
    ///
    /// A channel control like [`set_pan`](Self::set_pan), and a **different**
    /// control from the level of the bus it arrives on: several channels may
    /// share one mixer track (TDD §13.1), so a level that lived on the track
    /// would be one fader for all of them — which is exactly the bug that put
    /// this here. See `fontelle_model::Channel::gain_db`.
    ///
    /// Read per block, so moving it moves the notes already sounding.
    ///
    /// RT-safe: a plain field write, no allocation.
    /// The mod wheel, `0.0..=1.0`. A matrix source and nothing else: where
    /// a wheel goes is the patch's decision (`ModSource::ModWheel`).
    ///
    /// RT-safe, and **live** — it moves what is already sounding, which is
    /// what makes it a wheel rather than a note property.
    pub fn set_mod_wheel(&mut self, value: f32) {
        self.performance.mod_wheel = value.clamp(0.0, 1.0);
    }

    /// The pitch wheel, `-1.0..=1.0`, centred at zero.
    ///
    /// Bends every sounding note over `VoiceConfig::bend_range_semitones`,
    /// and reads as `ModSource::PitchBend` besides. RT-safe and live.
    pub fn set_pitch_bend(&mut self, value: f32) {
        self.performance.pitch_bend = value.clamp(-1.0, 1.0);
    }

    /// Channel pressure, `0.0..=1.0` — `ModSource::Aftertouch`. As the mod
    /// wheel: a source, and no destination of its own.
    pub fn set_aftertouch(&mut self, value: f32) {
        self.performance.aftertouch = value.clamp(0.0, 1.0);
    }

    /// What the hand is doing, for anything that has to put it back — the
    /// engine node keeps no copy of its own.
    pub fn performance(&self) -> crate::Performance {
        self.performance
    }

    pub fn set_gain_db(&mut self, gain_db: f32) {
        // Decibels in, amplitude kept: the conversion is a `powf` and doing it
        // here means it happens when the knob moves rather than every block.
        self.gain = if gain_db <= crate::SILENT_DB {
            0.0
        } else {
            10f32.powf(gain_db / 20.0)
        };
    }

    /// Moves one of the patch's own controls by its §8.2 address — what an
    /// **automation lane** does to a cutoff, an envelope stage or an
    /// oscillator's level. Whether the address named anything.
    ///
    /// The same function the instrument panel writes through
    /// ([`crate::patch_params::set`]), so what you can right-click is what a
    /// lane can move, by construction.
    ///
    /// RT-safe: the address is split as a `&str` and everything it does is a
    /// field write. An address this build does not recognise changes nothing
    /// and is not an error (INVARIANT 7) — which on the audio thread also
    /// means it cannot panic on a project from a later version.
    pub fn set_patch_param(&mut self, address: &str, value: f32) -> bool {
        crate::patch_params::set(&mut self.patch, address, value)
    }

    /// Where the transport is, for the LFOs that read it.
    ///
    /// Set every block by whatever is driving this sampler, because both
    /// numbers are live: a synced LFO's *rate* follows a tempo change, and a
    /// free-running one's *phase* is a fact about the position — which is what
    /// makes the same bar sound the same every time it plays.
    pub fn set_clock(&mut self, clock: crate::RenderClock) {
        self.performance.clock = clock;
    }

    /// Whether `address` is one the live wire may carry, or one that has to go
    /// through a rebuild.
    ///
    /// The line §2.3 draws: almost every patch parameter can be applied to a
    /// running sampler between one block and the next, which is what makes a
    /// knob drag audible under a held chord. **A layer's table is the
    /// exception.** Choosing one means resolving it, and resolving one locks
    /// the wavetable bank and may build half a megabyte — so it goes through
    /// `prepare`, like every other structural change, and the caller has to
    /// know that before it sends the edit rather than after the layer has
    /// gone silent.
    pub fn is_live_param(address: &str) -> bool {
        !address.ends_with("/synth/table")
    }

    /// What [`set_gain_db`](Self::set_gain_db) was last given, back in
    /// decibels.
    pub fn gain_db(&self) -> f32 {
        if self.gain <= 0.0 {
            crate::SILENT_DB
        } else {
            20.0 * self.gain.log10()
        }
    }

    /// Starts a note the timeline asked for. See [`Sampler::note_on_from`]
    /// for one a player did.
    pub fn note_on(&mut self, key: u8, velocity: u8, voice_context: u32) {
        self.note_on_from(
            key,
            velocity,
            voice_context,
            fontelle_types::VoiceOrigin::Timeline,
        );
    }

    /// As [`Sampler::note_on`], recording where the note came from so that
    /// transport stop and seek can cut the timeline's voices without cutting
    /// the player's.
    pub fn note_on_from(
        &mut self,
        key: u8,
        velocity: u8,
        voice_context: u32,
        origin: fontelle_types::VoiceOrigin,
    ) {
        self.trigger(
            crate::NoteTrigger::new(key, velocity)
                .in_context(voice_context)
                .from_origin(origin),
        );
    }

    /// Starts a note carrying §16.5's per-note character — today that is pan,
    /// and see [`crate::NoteTrigger`] for why it is a struct.
    ///
    /// The one the other two delegate to. A note-on that finds no free voice
    /// and cannot steal one is dropped, which is what a polyphony limit means.
    pub fn trigger(&mut self, note: crate::NoteTrigger) {
        let config = self.patch.voice_config;
        // The live polyphony, read here rather than at construction, which is
        // what makes `patch/voice/polyphony` an automation lane that does
        // something. The pool cannot grow on this thread (INVARIANT 1), so
        // this moves a limit inside it — see `VoicePool::set_limit`.
        self.voices.set_limit(usize::from(config.polyphony));
        // **Mono and legato**, which `RetriggerMode` has named since the patch
        // format was written and nothing read. One voice per context: a note
        // arriving while one is sounding takes it over rather than stacking on
        // top of it, which is what makes a bass line a line.
        if matches!(
            config.retrigger,
            crate::RetriggerMode::Mono | crate::RetriggerMode::Legato
        ) && let Some(voice) = self
            .voices
            .iter_active_mut()
            .find(|v| v.voice_context() == note.voice_context)
        {
            // **`glide_legato_only`**: portamento between notes that overlap
            // and not between notes that merely follow one another, which is
            // how every mono synth with a legato switch on it behaves and what
            // all twenty-one of Flopsynth's `mono` presets ask for. Read here
            // because this is where the config is; the voice only needs to be
            // told how long to take, and zero is "not at all".
            //
            // Read *before* the take-over, because both branches below make
            // the voice held again.
            let was_held = voice.is_held();
            let glide_s = if config.glide_legato_only && !was_held {
                0.0
            } else {
                config.glide_time_s
            };
            match config.retrigger {
                // Legato keeps the envelope and the sample position; mono
                // starts the note again and only the *pitch* is carried over.
                crate::RetriggerMode::Legato => voice.legato_to(note, glide_s),
                _ => {
                    let from = voice.sounding_key();
                    voice.trigger_note(&self.patch, note);
                    if glide_s > 0.0 {
                        voice.glide_from(from - note.key as f32, glide_s);
                    }
                }
            }
            return;
        }

        // Poly, or nothing sounding to glide from. **Portamento does not
        // apply here on purpose**: in a poly patch it would mean every note of
        // a chord sliding from whichever one happened to be last, which is not
        // a statement anybody makes musically.
        if let Some(voice) = self.voices.allocate(config.steal_policy) {
            voice.trigger_note(&self.patch, note);
        }
    }

    pub fn note_off(&mut self, key: u8, voice_context: u32) {
        if let Some(voice) = self.voices.find_active_mut(key, voice_context) {
            voice.release();
        }
    }

    /// Bends whatever is sounding in `voice_context` to `key`, over `seconds`.
    ///
    /// **This is the slide note**, FL Studio's: it starts no voice and ends
    /// none. A slide with nothing sounding does nothing, which is what makes
    /// it safe to write one at the top of a bar and then delete the note in
    /// front of it.
    ///
    /// Every voice in the context, not just one: a slide under a chord moves
    /// the chord.
    pub fn slide(&mut self, key: u8, seconds: f32, voice_context: u32) {
        for voice in self.voices.iter_active_mut() {
            if voice.voice_context() == voice_context {
                voice.glide_to(key, seconds);
            }
        }
    }

    /// How many voices are sounding. What a test asserting "a slide is not a
    /// note-on" needs, and what a voice-count read-out would use.
    pub fn active_voices(&self) -> usize {
        self.voices.active_count()
    }

    /// Where the **newest** voice's LFOs are in their cycles, 0..1 each.
    ///
    /// The newest, because the LFOs are per voice and retriggered per note
    /// (§3.5), so a chord has four of each turning at four offsets and there
    /// is no one answer. The newest is the one somebody just played, which is
    /// the one they are looking at the picture to understand.
    ///
    /// All zero when nothing is sounding: a dot parked at the start is honest
    /// about a synthesiser that is not playing.
    pub fn newest_lfo_phases(&self) -> [f32; crate::MAX_LFOS] {
        self.voices
            .iter_active()
            .min_by_key(|voice| voice.age_samples())
            .map(|voice| voice.lfo_phases())
            .unwrap_or([0.0; crate::MAX_LFOS])
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
        let performance = crate::Performance {
            pan: self.pan,
            ..self.performance
        };
        let tables = &self.tables;
        for voice in self.voices.iter_active_mut() {
            voice.render_performing(patch, store, tables, sample_rate, quality, performance, out);
        }
        // The patch's own trim, before the channel's level and after the voice
        // sum: it exists so a bank of presets can be **loudness-matched**
        // without anybody rebalancing their layers to do it (§3.8).
        let trim = 10f32.powf(self.patch.output_db / 20.0);
        if (trim - 1.0).abs() > f32::EPSILON {
            for channel in out.iter_mut() {
                for sample in channel.iter_mut() {
                    *sample *= trim;
                }
            }
        }
        // The channel's own level, over the summed voices. After the mix
        // rather than folded into each voice's gain so that a level moved
        // mid-note moves what is already sounding, which is what a fader does.
        if self.gain != 1.0 {
            for channel in out.iter_mut() {
                for sample in channel.iter_mut() {
                    *sample *= self.gain;
                }
            }
        }
    }

    /// Silences every voice at once, with no release tail — what transport
    /// stop and seek need. See `Voice::reset`; `release_all` is the graceful
    /// counterpart.
    ///
    /// RT-safe: touches only preallocated state.
    ///
    /// **The wheels are let go of too.** A transport stop that left a bend
    /// on would start the next note bent, and nobody is holding the wheel:
    /// what a reset means is that nothing is being played.
    pub fn reset(&mut self) {
        self.voices.reset();
        self.performance = crate::Performance {
            pan: self.performance.pan,
            ..crate::Performance::default()
        };
    }

    /// Silences only what the timeline started — transport stop and seek.
    /// A note a player is holding keeps sounding, because they have not let
    /// go of it. See [`fontelle_types::VoiceOrigin`].
    ///
    /// RT-safe.
    pub fn reset_sequenced(&mut self) {
        self.voices.reset_sequenced();
    }

    /// Note-offs every sounding voice, so each rings out through its own
    /// release. The right answer for a stuck-note panic, or for stopping at
    /// the end of a phrase rather than mid-note.
    ///
    /// RT-safe.
    pub fn release_all(&mut self) {
        for voice in self.voices.iter_active_mut() {
            voice.release();
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
    use crate::voice::{NoteTrigger, StealPolicy, VoiceConfig};
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

    const SR: f32 = 48_000.0;

    fn disabled_filter() -> FilterSlot {
        FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn a_note_on_defaults_to_being_the_timelines() {
        // Anything that does not say otherwise is the sequencer, so the
        // existing call sites keep their meaning.
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on(60, 127, 0);
        sampler.reset_sequenced();
        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        assert_eq!(
            rms(&out),
            0.0,
            "a plain note_on is the timeline's, and a stop cuts it"
        );
    }

    #[test]
    fn a_stop_cuts_the_timelines_voices_and_spares_the_players() {
        // The whole point of `VoiceOrigin`: transport stop is a statement
        // about the sequencer, not about the person holding a key down.
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on_from(60, 127, 0, fontelle_types::VoiceOrigin::Timeline);
        sampler.note_on_from(67, 127, 1, fontelle_types::VoiceOrigin::Live);

        let mut both = vec![0.0; 128];
        sampler.render(&store, &mut [&mut both[..]]);
        let with_both = rms(&both);

        sampler.reset_sequenced();
        let mut after = vec![0.0; 128];
        sampler.render(&store, &mut [&mut after[..]]);
        let after_stop = rms(&after);

        assert!(after_stop > 0.0, "the held key must still sound");
        assert!(
            after_stop < with_both,
            "the sequenced voice must be gone ({after_stop} against {with_both})"
        );
    }

    #[test]
    fn a_full_reset_still_takes_everything_including_live_voices() {
        // Device teardown and the panic button are not transport stop: there
        // is nobody left holding anything.
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on_from(60, 127, 0, fontelle_types::VoiceOrigin::Live);
        sampler.reset();
        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        assert_eq!(rms(&out), 0.0);
    }

    #[test]
    fn a_voice_slot_reused_after_a_live_note_is_not_still_marked_live() {
        // The pool reuses slots. If the origin survived into the next note,
        // one live note would make every voice that later landed in that slot
        // immune to transport stop — a note stuck through every stop and seek
        // for the rest of the session.
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        sampler.note_on_from(60, 127, 0, fontelle_types::VoiceOrigin::Live);
        sampler.reset();
        sampler.note_on(60, 127, 0);
        sampler.reset_sequenced();

        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        assert_eq!(rms(&out), 0.0, "the reused slot is the timeline's again");
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

    /// **The stuck note.** Reported from playing the studio: *"after playing
    /// notes it seems to want to often just hold a note forever if i spam
    /// lower notes"* — and once it had happened, every note after it hung
    /// too.
    ///
    /// A key pressed again while the last press is still ringing out has
    /// **two** voices on it: the old one in its release, the new one held.
    /// The note-off that follows must reach the one that is still held. When
    /// it went to the first voice in pool order it went to the one already
    /// released — a no-op — and the new voice was left holding a note nobody
    /// could address again. On a patch that sustains (every synth lead in the
    /// bank) that is a drone until the channel is rebuilt, and low notes
    /// reached it first because their tails are the longest.
    #[test]
    fn a_key_pressed_again_while_it_rings_out_still_gets_its_note_off() {
        let mut store = SampleStore::new();
        // A release long enough that the first voice is unmistakably still
        // sounding when the second note arrives.
        let lingering = EnvelopeConfig {
            release_s: 0.5,
            ..instant_envelope()
        };
        let patch = patch_with_envelope(&mut store, 8, lingering);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });
        let mut scratch = vec![0.0; 256];

        sampler.note_on_from(60, 127, 0, fontelle_types::VoiceOrigin::Live);
        sampler.render(&store, &mut [&mut scratch[..]]);
        sampler.note_off(60, 0);
        sampler.render(&store, &mut [&mut scratch[..]]);
        assert!(
            sampler.active_voices() > 0,
            "the rig is wrong if the first voice has already finished"
        );

        // The same key again, while that release is still audible.
        sampler.note_on_from(60, 127, 0, fontelle_types::VoiceOrigin::Live);
        sampler.render(&store, &mut [&mut scratch[..]]);
        sampler.note_off(60, 0);

        // Well past the longest release either voice could have.
        for _ in 0..300 {
            sampler.render(&store, &mut [&mut scratch[..]]);
        }
        assert_eq!(
            sampler.active_voices(),
            0,
            "both presses were let go of, so nothing may still be sounding"
        );
        assert_eq!(rms(&scratch), 0.0, "and it is silent");
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
            ..Default::default()
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

    /// Transport stop and seek both need every voice gone *now*: the audio
    /// after a seek belongs to a different part of the song, and a release
    /// tail from before it would play over the top.
    #[test]
    fn reset_silences_every_sounding_voice_immediately() {
        let mut store = SampleStore::new();
        let patch = patch_with_envelope(
            &mut store,
            8,
            EnvelopeConfig {
                // A long release, so a voice that merely got a note-off would
                // still be plainly audible.
                release_s: 5.0,
                ..instant_envelope()
            },
        );
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });
        sampler.note_on(60, 127, 0);
        sampler.note_on(64, 127, 0);

        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        assert!(out[0] > 0.5, "the notes must be sounding first");

        sampler.reset();
        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        assert!(
            out.iter().all(|&s| s == 0.0),
            "reset must leave nothing at all, got {}",
            out[0]
        );
    }

    /// `release_all` is the other half: every note gets a note-off and rings
    /// out. A "stop at the end of the bar" or a stuck-note panic wants this,
    /// not the hard cut above.
    #[test]
    fn release_all_lets_notes_ring_out_instead_of_cutting_them() {
        let mut store = SampleStore::new();
        let patch = patch_with_envelope(
            &mut store,
            8,
            EnvelopeConfig {
                release_s: 1.0,
                ..instant_envelope()
            },
        );
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });
        sampler.note_on(60, 127, 0);

        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        sampler.release_all();
        let mut out = vec![0.0; 128];
        sampler.render(&store, &mut [&mut out[..]]);
        assert!(
            out[0] > 0.5,
            "a one-second release has barely started after 128 samples, got {}",
            out[0]
        );
        assert!(
            out[127] < out[0],
            "but it must be on its way down: {} then {}",
            out[0],
            out[127]
        );
    }

    /// A voice comes back out of the pool carrying whatever the last note left
    /// in its filter. After a reset that memory belongs to a note that no
    /// longer exists, and it discharges into the next one as a click.
    #[test]
    fn reset_clears_filter_memory_rather_than_carrying_it_into_the_next_note() {
        let mut store = SampleStore::new();
        let mut patch = patch_with_envelope(&mut store, 1, instant_envelope());
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 200.0,
            resonance: 4.0,
            enabled: true,
            ..Default::default()
        };
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });

        // Charge the filter up on a note, then reset and start a fresh one.
        sampler.note_on(60, 127, 0);
        let mut scratch = vec![0.0; 512];
        sampler.render(&store, &mut [&mut scratch[..]]);
        sampler.reset();
        sampler.note_on(60, 127, 1);
        let mut after_reset = vec![0.0; 512];
        sampler.render(&store, &mut [&mut after_reset[..]]);

        // And the same note from a sampler that never played anything.
        let mut fresh_store = SampleStore::new();
        let mut fresh_patch = patch_with_envelope(&mut fresh_store, 1, instant_envelope());
        fresh_patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 200.0,
            resonance: 4.0,
            enabled: true,
            ..Default::default()
        };
        let mut fresh = Sampler::new(fresh_patch);
        fresh.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 512,
        });
        fresh.note_on(60, 127, 0);
        let mut untouched = vec![0.0; 512];
        fresh.render(&fresh_store, &mut [&mut untouched[..]]);

        for (i, (a, b)) in after_reset.iter().zip(untouched.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-5,
                "sample {i}: a note after a reset must be identical to the \
                 first note a sampler ever plays, got {a} against {b}"
            );
        }
    }

    /// §16.5's per-note pan: where *this note* sits, as against
    /// [`Sampler::set_pan`], which is where the whole part sits.
    #[test]
    fn a_notes_own_pan_places_it_in_the_field() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        sampler.trigger(NoteTrigger::new(60, 127).with_pan(-1.0));
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);

        assert!(
            (left[0] - 1.0).abs() < 1e-5 && right[0].abs() < 1e-5,
            "a note panned hard left must be on the left alone, got {} / {}",
            left[0],
            right[0]
        );
    }

    /// The note's pan and the channel's **add**, exactly as the layer's and
    /// the channel's already do. Any other reading throws one of them away:
    /// panning a part right would flatten every note that was leaning left
    /// against it, which is the whole point of writing them apart.
    #[test]
    fn a_notes_pan_adds_to_the_channels_rather_than_replacing_it() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 8);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        sampler.set_pan(1.0);
        sampler.trigger(NoteTrigger::new(60, 127).with_pan(-1.0));
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);

        let centred = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (left[0] - centred).abs() < 1e-5 && (right[0] - centred).abs() < 1e-5,
            "a note hard left on a channel hard right belongs between them, \
             got {} / {}",
            left[0],
            right[0]
        );
    }

    /// A voice comes back out of the pool carrying the last note's pan, and a
    /// centred note landing where a hard-left one used to be is a bug that
    /// only appears once the pool wraps. The same class as the filter memory
    /// `trigger_from` already resets.
    #[test]
    fn a_reused_voice_does_not_inherit_the_last_notes_pan() {
        let mut store = SampleStore::new();
        let patch = one_voice_patch(&mut store, 1);
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        sampler.trigger(NoteTrigger::new(60, 127).with_pan(-1.0));
        sampler.note_off(60, 0);
        sampler.reset();
        sampler.note_on(60, 127, 0);

        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);

        let centred = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (left[0] - centred).abs() < 1e-5 && (right[0] - centred).abs() < 1e-5,
            "a plain note-on is centred whatever the voice played last, \
             got {} / {}",
            left[0],
            right[0]
        );
    }

    /// A drum kit is one zone per hit, and real ones have far more than
    /// sixteen: `Setzer's_SPC_Soundfont.sf2`'s Standard kit has 46 zones,
    /// `Nokia_30.sf2`'s has 47, `MN64 Drums` 46.
    ///
    /// Reported from using the window: *"a lot of notes are showing as ones
    /// that should be playable but just aren't producing any sound at all —
    /// it's making a lot of kits just incomplete to use."*
    ///
    /// `Voice::trigger_note` zipped its **fixed sixteen** playback slots
    /// against `patch.layers`, and `zip` stops at the shorter one. Every zone
    /// past index 15 therefore never got a slot, never became active, and
    /// never rendered — silently. On the three kits above that is 30, 37 and
    /// 68 keys respectively: audible only as a kit that half works.
    ///
    /// `MAX_LAYERS` is the number of layers that may sound **at once** (TDD
    /// §7.4: "stacked, or split by key/velocity"), and a key split is not a
    /// stack. A patch may hold as many zones as the file does.
    fn kit_patch(store: &mut SampleStore, keys: &[u8]) -> Patch {
        let asset = store.insert(crate::streaming::SampleBuffer {
            data: std::sync::Arc::from(vec![1.0; 100_000]),
            sample_rate: SR as u32,
        });
        Patch {
            layers: keys
                .iter()
                .map(|key| Layer {
                    source: Source::Sample { file: asset },
                    // One key wide, which is what makes it a kit.
                    key_range: (*key, *key),
                    vel_range: (0, 127),
                    root_key: *key,
                    fine_tune_cents: 0.0,
                    playback: PlaybackConfig {
                        loop_mode: LoopMode::Off,
                        interpolation: Some(Interpolation::Draft),
                        end_offset: 100_000.0,
                        ..PlaybackConfig::default()
                    },
                    gain_db: 0.0,
                    pan: 0.0,
                })
                .collect(),
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(), instant_envelope()],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig {
                polyphony: 8,
                steal_policy: StealPolicy::Oldest,
                ..VoiceConfig::default()
            },
            ..Default::default()
        }
    }

    fn sounds(patch: &Patch, store: &SampleStore, key: u8) -> bool {
        let mut sampler = Sampler::new(patch.clone());
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });
        sampler.note_on(key, 100, 0);
        let mut out = vec![0.0; 64];
        sampler.render(store, &mut [&mut out[..]]);
        out.iter().any(|s| s.abs() > 1e-6)
    }

    #[test]
    fn every_zone_of_a_kit_sounds_however_many_zones_it_has() {
        let mut store = SampleStore::new();
        // 47 hits, the size of a real GM kit, on the keys one uses.
        let keys: Vec<u8> = (35..35 + 47).collect();
        let patch = kit_patch(&mut store, &keys);

        let silent: Vec<u8> = keys
            .iter()
            .copied()
            .filter(|key| !sounds(&patch, &store, *key))
            .collect();

        assert!(
            silent.is_empty(),
            "these keys have a zone of their own and made no sound: {silent:?}"
        );
    }

    /// The other half of the same rule, and the reason `MAX_LAYERS` is still
    /// sixteen: it bounds what may sound *together*. A key covered by no zone
    /// is still silent, however many zones the patch has.
    #[test]
    fn a_key_no_zone_covers_is_still_silent_in_a_large_kit() {
        let mut store = SampleStore::new();
        let patch = kit_patch(&mut store, &[36, 38, 42, 46]);
        assert!(sounds(&patch, &store, 36));
        assert!(
            !sounds(&patch, &store, 37),
            "key 37 has no zone and must stay silent"
        );
    }

    /// A stack deeper than the voice has slots takes the first `MAX_LAYERS` of
    /// it rather than dropping the note — the documented limit, applied to the
    /// thing it is actually about.
    #[test]
    fn more_than_max_layers_stacked_on_one_key_still_sounds() {
        let mut store = SampleStore::new();
        let keys: Vec<u8> = std::iter::repeat_n(60u8, crate::voice::MAX_LAYERS + 8).collect();
        let patch = kit_patch(&mut store, &keys);
        assert!(
            sounds(&patch, &store, 60),
            "a deep stack must sound, not fall off the end"
        );
    }
}
