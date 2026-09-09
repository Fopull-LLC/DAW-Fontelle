#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StealPolicy {
    Oldest,
    Quietest,
    LowestPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnisonConfig {
    pub voices: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub randomise_phase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RetriggerMode {
    Poly,
    Mono,
    Legato,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VoiceConfig {
    /// 1..=256.
    pub polyphony: u16,
    pub steal_policy: StealPolicy,
    pub glide_time_s: f32,
    pub glide_legato_only: bool,
    pub unison: UnisonConfig,
    pub retrigger: RetriggerMode,
    /// How far a full pitch bend goes, in semitones either way.
    ///
    /// Two by default, which is what every keyboard ships with and what
    /// SF2 2.04's always-present pitch-wheel default modulator amounts to.
    /// It is the **patch's** setting and not the wheel's, beside the glide
    /// and the polyphony: a lead that bends an octave and a pad that bends
    /// a tone are the same wheel and different instruments.
    ///
    /// `serde(default)` because a patch written before this existed is not
    /// a broken one, and the default is what it was silently doing: see
    /// [`DEFAULT_BEND_RANGE_SEMITONES`].
    #[serde(default = "default_bend_range")]
    pub bend_range_semitones: f32,
}

/// See [`VoiceConfig::bend_range_semitones`].
pub const DEFAULT_BEND_RANGE_SEMITONES: f32 = 2.0;

fn default_bend_range() -> f32 {
    DEFAULT_BEND_RANGE_SEMITONES
}

/// What the hand playing an instrument is doing right now, beside the notes.
///
/// **Channel-wide and live**, which is what separates these from the five
/// per-note properties a `NoteTrigger` carries (§16.5): a wheel moves what
/// is already sounding, so it is read at render rather than captured at
/// note-on, exactly like the channel's own pan — which is why it rides here
/// with it.
///
/// The bend is bipolar (`-1.0..=1.0`), the other two run `0.0..=1.0`, and
/// all three are what the matrix reads for `ModSource::PitchBend`,
/// `ModWheel` and `Aftertouch`. The bend is *also* applied to the note's own
/// pitch over [`VoiceConfig::bend_range_semitones`], because every keyboard
/// bends pitch and a patch should not have to wire a route to get what the
/// wheel is for. The other two go wherever the patch sends them and nowhere
/// by default: inventing a destination for a mod wheel would be a mapping
/// nobody asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Performance {
    /// The channel's placement — see [`Voice::render_with_pan`].
    pub pan: f32,
    /// `ModSource::ModWheel`, `0.0..=1.0`.
    pub mod_wheel: f32,
    /// `ModSource::PitchBend`, `-1.0..=1.0`.
    pub pitch_bend: f32,
    /// `ModSource::Aftertouch` — channel pressure, `0.0..=1.0`.
    pub aftertouch: f32,
    /// Where the transport is, for the LFOs that read it.
    ///
    /// It rides here rather than as a sixth argument to `render_performing`
    /// for the reason the wheels do: it is **channel-wide and live**, it is
    /// read at render rather than captured at note-on, and every call site
    /// that already builds a `Performance` is exactly the set of call sites
    /// that has a transport to fill it in from.
    pub clock: RenderClock,
}

/// Where the transport is, as a voice needs to know it.
///
/// Two numbers, because two things read them: a synced LFO needs the **tempo**
/// to work out its rate from its division, and a free-running LFO needs the
/// **position** to work out its phase. Both are already on
/// `ProcessContext::transport`, so nothing new is measured — it is only
/// carried one level further in than it used to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderClock {
    pub bpm: f32,
    pub position_sample: u64,
}

impl Default for RenderClock {
    fn default() -> Self {
        Self {
            bpm: fontelle_types::DEFAULT_BPM,
            position_sample: 0,
        }
    }
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
            bend_range_semitones: DEFAULT_BEND_RANGE_SEMITONES,
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

/// How often, in samples, the filter coefficients are rebuilt along the
/// within-block cutoff ramp (`docs/flopsynth-plan.md` §3.3).
///
/// Eight is 6 kHz at 48 kHz — far above anything an LFO or an envelope moves a
/// corner at, and an eighth of the cost of rebuilding per sample. A patch with
/// no route to cutoff pays it too, and pays almost nothing: the ramp's two
/// endpoints are equal and the `tan` is the same one it would have computed
/// once anyway.
pub const FILTER_STEP: usize = 8;

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

#[derive(Debug, Clone, Copy)]
struct LayerPlayback {
    active: bool,
    /// **Which** of `patch.layers` this slot is playing.
    ///
    /// The slots used to be indexed *by* patch layer — slot `n` played layer
    /// `n` — which quietly made `MAX_LAYERS` a limit on how many zones a patch
    /// could have rather than on how many may sound at once. `zip` stopped at
    /// sixteen and every zone past that never sounded: on a real 46-zone drum
    /// kit, 30 keys of silence that looked exactly like keys that should work.
    ///
    /// A note triggers only the zones covering its key and velocity — one or
    /// two, in a kit — so the slots now hold *those*, and a patch may carry as
    /// many zones as the file does.
    layer: u16,
    /// Fractional sample position within the layer's `SampleBuffer`.
    position: f64,
    /// The phase of a `Source::Oscillator` layer, which has no buffer to hold
    /// a position in.
    ///
    /// Per **slot** rather than per patch layer, and per voice rather than per
    /// patch, for the reason the filter memory is: two notes sounding together
    /// on one oscillator are two different phases, and sharing one would make
    /// a voice's output depend on which other voices rendered before it.
    osc: fontelle_dsp::Oscillator,
    /// The drum machine's per-hit state, for a `Source::Drum` layer.
    ///
    /// Beside `osc` and for exactly the same reason. It carries its own
    /// envelopes, so — unlike every other source here — the length of the
    /// sound is the *hit's* rather than the patch's amp envelope's: a kit
    /// whose kick and hat had to share one decay would not be a kit.
    drum: fontelle_dsp::DrumSynth,
    /// The Flopsynth oscillator's per-voice state, for a `Source::Synth`
    /// layer: eight unison phases, a random source and the noise filter's
    /// memory.
    ///
    /// Beside `osc` and `drum` and for exactly the same reason: two notes
    /// sounding together on one oscillator are two different phases, and
    /// sharing one would make a voice's output depend on which other voices
    /// rendered before it.
    synth: fontelle_dsp::SynthState,
    /// Whether the hit still has to be fired.
    ///
    /// A drum's envelope coefficients depend on the **sample rate**, which
    /// `trigger_note` does not know — the rate reaches a voice with the block
    /// it is asked to render. So the note-on records that a hit is owed and
    /// the first sample of the first block fires it, which is the same moment
    /// either way and needs no rate plumbed through the note path.
    drum_pending: bool,
}

impl Default for LayerPlayback {
    fn default() -> Self {
        Self {
            active: false,
            layer: 0,
            position: 0.0,
            osc: fontelle_dsp::Oscillator::new(),
            drum: fontelle_dsp::DrumSynth::new(),
            synth: fontelle_dsp::SynthState::new(),
            drum_pending: false,
        }
    }
}

/// Where one prepared layer's samples come from.
///
/// The two are resolved to completely different constants — a step through a
/// buffer against a frequency in hertz — and keeping them in one struct with
/// half its fields unused for either is how a renderer ends up silently
/// skipping the variant nobody filled in, which is what `Source::Oscillator`
/// did for as long as this enum did not exist.
#[derive(Clone, Copy)]
enum PreparedSource<'a> {
    /// PCM straight out of the `SampleStore` — no copy, no allocation.
    Sample {
        data: &'a [f32],
        step: f64,
        loop_end: f64,
        loop_len: f64,
        looping: bool,
        end_offset: f64,
        interpolation: fontelle_dsp::Interpolation,
    },
    /// A shape and the pitch to run it at. **It never ends**: an oscillator
    /// has no last sample to run off, so the note lasts exactly as long as its
    /// amplitude envelope says.
    Oscillator {
        kind: fontelle_dsp::OscKind,
        freq_hz: f32,
    },
    /// One drum hit. **It ends itself**, which is what makes it different
    /// from both of the others: a hit's length is its own `decay_s` and the
    /// patch's amp envelope is held open behind it (see
    /// `crate::drum_kit::drum_kit`), so the slot goes quiet when the drum
    /// does rather than when the note is let go.
    ///
    /// The voice is copied in rather than borrowed because it is eighty bytes
    /// of plain numbers and `PreparedLayer` is `Copy`; a reference would tie
    /// the prepared array's lifetime to the patch for no gain.
    Drum(fontelle_dsp::DrumVoice),
    /// One Flopsynth oscillator, with the table `Sampler::prepare` resolved
    /// for it and the note's own pitch.
    ///
    /// The table is borrowed rather than owned because it is a megabyte and
    /// the `Arc` that holds it is the sampler's — resolving one *here* would
    /// take a lock and allocate, which is exactly what INVARIANT 1 forbids on
    /// this thread.
    Synth {
        osc: fontelle_dsp::SynthOsc,
        table: Option<&'a fontelle_dsp::Wavetable>,
        note_hz: f32,
    },
}

/// One layer's per-render constants, resolved once before the sample loop in
/// `Voice::render` rather than recomputed per sample.
#[derive(Clone, Copy)]
struct PreparedLayer<'a> {
    source: PreparedSource<'a>,
    gain: f32,
    /// This layer's place in the stereo field, already through the pan law.
    /// `(1.0, 0.0)` on a mono render: nothing goes to a channel that isn't
    /// there, and the one that is carries the layer unattenuated.
    pan_gain: (f32, f32),
    /// Which of the four filter buses this layer is summed into. Every source
    /// but `Synth` is `Serial`, which is what the two filters have always
    /// done — so nothing that existed before this changes.
    route: fontelle_dsp::FilterRoute,
    /// Which patch layer this is, so a `Synth` layer naming another as its FM
    /// or RM modulator can find that layer's sample for this frame.
    layer_index: usize,
}

/// The pitch an oscillator layer plays at its root key: middle C, 261.6256 Hz.
///
/// A `Source::Oscillator` is transposed by exactly the arithmetic a sample is —
/// `key - root_key`, plus every tuning on the pitch path — so it needs one
/// frequency to be transposed *from*. Middle C rather than A440 so that the
/// default `root_key: 60` makes a note play its own pitch, which is the only
/// reading of a synthesiser anybody expects.
pub const OSC_ROOT_HZ: f32 = 261.625_56;

/// What a note's `release: 127` multiplies the patch's release time by.
///
/// Four rather than some larger number because the property has to stay
/// *drawable*: the roll's lane maps 0..127 across a few dozen pixels, and a
/// range wide enough to turn a pluck into a pad puts every musically useful
/// value in the bottom two pixels of it.
pub const MAX_NOTE_RELEASE: f32 = 4.0;

/// Everything a note-on says beyond "start playing".
///
/// One struct rather than seven positional arguments, and the reason is
/// §16.5: `Note` carries pan, fine pitch, release and two free modulation
/// values, and every one of them has to reach a voice. Pan arrived first and
/// the other four followed, each as a field here rather than another argument
/// threaded through four call sites — which is what this struct was shaped
/// for.
///
/// Built with [`NoteTrigger::new`] plus the `with_`/`in_`/`from_` methods, so
/// a caller says only what it means and the rest stays at the default a plain
/// note-on has always had: centred, voice context zero, from the timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteTrigger {
    pub key: u8,
    pub velocity: u8,
    /// `-1.0` hard left, `0.0` centre, `1.0` hard right. Adds to the layer's
    /// pan and the channel's; see [`Voice::render_with_pan`].
    ///
    /// A unit interval rather than the document's byte: this is the audio
    /// side of the seam, and `fontelle_types::pan_unit` is the one crossing.
    pub pan: f32,
    /// Cents off this note's own key. Adds to the layer's tuning and to the
    /// mod matrix's pitch routes, all three being cents.
    pub fine_pitch: i16,
    /// `0..=127`, lengthening this one note's release past the patch's.
    /// `0` is the patch's own — see [`EventPayload::NoteOn`].
    ///
    /// [`EventPayload::NoteOn`]: fontelle_types::EventPayload::NoteOn
    pub release: u8,
    /// §16.5's two free modulation values, `0..=127`, readable by the patch
    /// as [`ModSource::NoteModX`] and [`ModSource::NoteModY`].
    ///
    /// [`ModSource::NoteModX`]: crate::ModSource::NoteModX
    /// [`ModSource::NoteModY`]: crate::ModSource::NoteModY
    pub mod_x: u8,
    pub mod_y: u8,
    /// TDD §11.4's per-clip tag, so a note-off finds the voice it belongs to.
    pub voice_context: u32,
    pub origin: fontelle_types::VoiceOrigin,
}

impl NoteTrigger {
    /// A plain note: centred, voice context zero, from the timeline.
    pub fn new(key: u8, velocity: u8) -> Self {
        Self {
            key,
            velocity,
            pan: 0.0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
            origin: fontelle_types::VoiceOrigin::Timeline,
        }
    }

    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }

    pub fn with_fine_pitch(mut self, cents: i16) -> Self {
        self.fine_pitch = cents;
        self
    }

    pub fn with_release(mut self, release: u8) -> Self {
        self.release = release;
        self
    }

    pub fn with_mod_x(mut self, mod_x: u8) -> Self {
        self.mod_x = mod_x;
        self
    }

    pub fn with_mod_y(mut self, mod_y: u8) -> Self {
        self.mod_y = mod_y;
        self
    }

    pub fn in_context(mut self, voice_context: u32) -> Self {
        self.voice_context = voice_context;
        self
    }

    pub fn from_origin(mut self, origin: fontelle_types::VoiceOrigin) -> Self {
        self.origin = origin;
        self
    }
}

/// One playing note. Fixed-topology (INVARIANT 6): Layers → mix → Filter 1 → Filter 2
/// → Amp → Pan → out, with the mod matrix feeding every stage. Predictable per-voice
/// cost, zero allocation on note-on, no graph compilation on the audio thread.
///
/// **Scope note:** `Source::Sample` and `Source::Oscillator` layers render;
/// `Source::Sf2Zone` is still a silent no-op, and can only reach a patch from a
/// build whose importer did not flatten one into a `Sample`. Tracked in
/// `PROGRESS.md`.
pub struct Voice {
    active: bool,
    /// Whether the key that started this voice is **still down**.
    ///
    /// Not the same question as [`active`](Voice::is_active): a voice in its
    /// release is still sounding and still active, but nobody is holding it
    /// any more. The difference is what keeps a note-off from being spent on
    /// a voice that has already had one — see
    /// [`VoicePool::find_active_mut`], where spending it that way left the
    /// note somebody *was* holding with no way to end it.
    held: bool,
    key: u8,
    voice_context: u32,
    origin: fontelle_types::VoiceOrigin,
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
    /// **Three** slots, not two, and one filter per output channel:
    /// `filters[path][channel]`.
    ///
    /// Path 0 is filter 1 as the `F1` route uses it, path 1 is filter 2, and
    /// path 2 is *filter 1 again* for the layers routed `F1→F2`. The third
    /// exists because a stateful filter cannot be in two places at once: with
    /// one instance, a patch whose sub goes through F1 alone and whose saw
    /// goes through F1 into F2 would have to split F1's single output between
    /// two destinations, and there is no split that is right — the two signals
    /// are already summed by the time the filter has run. `docs/flopsynth-plan.md`
    /// §3.3 says "four buses, two filters"; the fourth bus needs a third
    /// filter to be exact, and the cost is one more slot of state per channel.
    ///
    /// Per voice, not per patch: two notes sounding at once each need their
    /// own filter memory, and sharing one would make a voice's output depend
    /// on which other voices happened to render before it. Per channel for the
    /// same reason one step down — once layers are panned apart the two
    /// channels carry different signals.
    filters: [[fontelle_dsp::SynthFilter; 2]; 3],
    /// Fixed for the life of the note, from `velocity_to_gain`. Folded into
    /// each layer's gain at the top of `render` so it costs nothing per sample.
    velocity_gain: f32,
    /// §16.5's per-note pan, `-1.0..=1.0`, captured at note-on.
    ///
    /// Per *voice* rather than per channel because that is what "per note"
    /// means: two notes sounding together on one instrument may sit in
    /// different places. It adds to the layer's own pan and to the channel's
    /// live one — see [`Voice::render_with_pan`].
    ///
    /// **Reset on every trigger**, like the filter memory above it: a voice
    /// coming back out of the pool carrying the last note's pan puts a centred
    /// note wherever the previous one was, which is a bug that only appears
    /// once the pool wraps.
    note_pan: f32,
    /// Note-on velocity and key as the mod matrix sees them: normalised to
    /// 0..1, captured once so evaluating a route never has to reach back into
    /// the event that started the note.
    velocity_norm: f32,
    key_norm: f32,
    /// §16.5's fine pitch as semitones, captured at note-on. Added to the
    /// note's interval alongside the glide, and for the same reason: it moves
    /// the *sound* and leaves `key` — the note's identity, which a note-off
    /// names — where it is.
    note_detune: f32,
    /// What this note multiplies the patch's release time by, `>= 1.0`.
    ///
    /// A multiplier rather than a time so that it means the same thing on a
    /// plucked patch and a pad: the instrument sets the character and the note
    /// says "hold it longer than that". `1.0` is `release: 0` — the patch's
    /// own, and the default every note carries.
    note_release_scale: f32,
    /// §16.5's two free modulation values, normalised to 0..1 for the matrix.
    mod_x_norm: f32,
    mod_y_norm: f32,
    amp_env: fontelle_dsp::EnvelopeGenerator,
    /// `patch.envelopes[1..]`, as modulation sources. Per voice, because two
    /// notes are at different points in their envelopes.
    mod_envs: [fontelle_dsp::EnvelopeGenerator; MAX_MOD_ENVELOPES],
    /// `patch.lfos`, retriggered on every note-on: a free-running LFO makes
    /// the same note sound different depending on when it was played, which is
    /// a character an instrument can want but not a default anyone can
    /// predict.
    lfos: [crate::lfo::LfoState; MAX_LFOS],
    /// `ModSource::Random`: one value per note, drawn at note-on.
    ///
    /// Per note rather than per block, which is what "random" means for a
    /// modulation source — a value that changed under a held note would be
    /// noise, and there is a sample & hold LFO for that.
    random: f32,
    /// `ModSource::NoteOnCounter`, cycling `0, 1/7, …, 1` per note-on, so an
    /// eight-step alternation is one route with a quantised curve on it.
    note_counter: f32,
    /// Samples since this note started, for `Lfo::delay_s`. One counter for
    /// the voice rather than one per LFO: they all start together.
    age_samples: u64,
    /// How far the voice's pitch is from [`Voice::key`] right now, in
    /// semitones, and where it is heading.
    ///
    /// **The key itself never moves.** A slide bends the sound and leaves the
    /// note's identity alone, which is what lets the note-off the score wrote
    /// for key 60 still end a voice that is currently sounding key 67 — see
    /// [`Voice::glide_to`].
    glide_semitones: f32,
    glide_target: f32,
    /// Semitones per second. Zero is "already there".
    glide_rate: f32,
    /// The cutoff modulation, in cents, that the **previous** block ended at
    /// — one per filter slot.
    ///
    /// Modulation is resolved per block, and a corner that jumped once a block
    /// is an audible zipper under any LFO faster than a few hertz. So the
    /// corner is ramped from here to this block's value across the block, and
    /// this is the only thing that has to be remembered to do it.
    filter_cents: [f32; 2],
}

impl Voice {
    pub fn new() -> Self {
        Self {
            active: false,
            held: false,
            key: 0,
            voice_context: 0,
            origin: fontelle_types::VoiceOrigin::Timeline,
            age: 0,
            layers: [LayerPlayback::default(); MAX_LAYERS],
            filters: [[fontelle_dsp::SynthFilter::new(); 2]; 3],
            velocity_gain: 0.0,
            note_pan: 0.0,
            velocity_norm: 0.0,
            key_norm: 0.0,
            note_detune: 0.0,
            note_release_scale: 1.0,
            mod_x_norm: 0.0,
            mod_y_norm: 0.0,
            amp_env: fontelle_dsp::EnvelopeGenerator::new(),
            mod_envs: [fontelle_dsp::EnvelopeGenerator::new(); MAX_MOD_ENVELOPES],
            lfos: [crate::lfo::LfoState::new(); MAX_LFOS],
            random: 0.0,
            note_counter: 0.0,
            age_samples: 0,
            glide_semitones: 0.0,
            glide_target: 0.0,
            glide_rate: 0.0,
            filter_cents: [0.0; 2],
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether this voice's key is still down — see [`Voice::held`].
    pub fn is_held(&self) -> bool {
        self.held
    }

    pub fn key(&self) -> u8 {
        self.key
    }

    pub fn voice_context(&self) -> u32 {
        self.voice_context
    }

    /// Whether the timeline or a player started this voice. See
    /// [`fontelle_types::VoiceOrigin`] — transport stop and seek cut one and
    /// spare the other.
    /// How long this voice has been sounding, in samples — what makes
    /// "the newest voice" a question with an answer.
    pub fn age_samples(&self) -> u64 {
        self.age_samples
    }

    /// Where each of this voice's LFOs is in its cycle, 0..1.
    pub fn lfo_phases(&self) -> [f32; MAX_LFOS] {
        std::array::from_fn(|index| self.lfos[index].phase())
    }

    pub fn origin(&self) -> fontelle_types::VoiceOrigin {
        self.origin
    }

    /// Starts a centred note the timeline asked for. See
    /// [`Voice::trigger_from`] for one a player did, and
    /// [`Voice::trigger_note`] for one that carries §16.5's per-note
    /// character.
    pub fn trigger(&mut self, patch: &crate::Patch, key: u8, velocity: u8, voice_context: u32) {
        self.trigger_note(
            patch,
            NoteTrigger::new(key, velocity).in_context(voice_context),
        );
    }

    /// As [`Voice::trigger`], recording where the note came from.
    ///
    /// The origin is set here, in the same call that starts the voice, rather
    /// than by a separate setter afterwards: a voice that is briefly active
    /// with the wrong origin is a voice a reset landing in between would treat
    /// as the wrong kind.
    pub fn trigger_from(
        &mut self,
        patch: &crate::Patch,
        key: u8,
        velocity: u8,
        voice_context: u32,
        origin: fontelle_types::VoiceOrigin,
    ) {
        self.trigger_note(
            patch,
            NoteTrigger::new(key, velocity)
                .in_context(voice_context)
                .from_origin(origin),
        );
    }

    /// The one that actually starts a voice; the two above are it with the
    /// defaults filled in.
    ///
    /// It takes a [`NoteTrigger`] rather than a fifth positional argument
    /// because pan was the *first* of `Note`'s five per-note properties to
    /// reach the audio path and was never going to be the last: fine pitch,
    /// release and the two free modulation values followed, and each one is a
    /// field on the struct rather than another rewrite of every call site.
    pub fn trigger_note(&mut self, patch: &crate::Patch, note: NoteTrigger) {
        let NoteTrigger {
            key,
            velocity,
            pan,
            fine_pitch,
            release,
            mod_x,
            mod_y,
            voice_context,
            origin,
        } = note;
        self.active = true;
        self.held = true;
        self.origin = origin;
        self.key = key;
        self.voice_context = voice_context;
        self.velocity_gain = velocity_to_gain(velocity);
        self.note_pan = pan.clamp(-1.0, 1.0);
        self.velocity_norm = velocity as f32 / 127.0;
        self.key_norm = key as f32 / 127.0;
        // Cents to semitones: the document stores cents because that is the
        // unit a musician tunes in, and every other tuning on the pitch path
        // is cents too, so nothing has to be converted twice.
        self.note_detune = fine_pitch as f32 / 100.0;
        // `0` is the patch's own release and 127 is four times it. Only ever
        // longer, because `0` is what every note ever written carries and a
        // property whose default rewrote existing projects is not one worth
        // having — see `EventPayload::NoteOn`.
        self.note_release_scale =
            1.0 + (release.min(127) as f32 / 127.0) * (MAX_NOTE_RELEASE - 1.0);
        self.mod_x_norm = mod_x.min(127) as f32 / 127.0;
        self.mod_y_norm = mod_y.min(127) as f32 / 127.0;
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
        // The **age counter is the seed** for everything random about this
        // note: the LFOs' sample & hold sequences, the unison stacks' start
        // phases and `ModSource::Random`. It is monotonic per pool, so two
        // notes never draw the same numbers and the same sequence of notes
        // draws the same numbers twice — which is what makes any of this
        // testable.
        let seed = self.age as u32;
        for (index, lfo) in patch.lfos.iter().take(MAX_LFOS).enumerate() {
            self.lfos[index].reset(lfo, seed.wrapping_add(index as u32 * 0x9e37));
        }
        for lfo in self.lfos.iter_mut().skip(patch.lfos.len().min(MAX_LFOS)) {
            *lfo = crate::lfo::LfoState::new();
        }
        // A bipolar value, so a route to pitch is as likely to go down as up.
        let mut x = seed.wrapping_mul(0x2545_f491) | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = (x >> 8) as f32 / 8_388_608.0 - 1.0;
        // Eight steps, so `Curve::Quantised { steps: 7 }` lands on each of
        // them exactly.
        self.note_counter = (self.age % 8) as f32 / 7.0;
        self.age_samples = 0;
        // A fresh note is at its own pitch. Portamento is applied *after*
        // this by whoever retriggered it (see `Sampler::trigger`), because
        // only the sampler knows what was sounding before.
        self.glide_semitones = 0.0;
        self.glide_target = 0.0;
        self.glide_rate = 0.0;

        // Every slot cleared first: what a voice plays is decided entirely by
        // this note, and a slot left over from the last one is a zone that
        // keeps sounding after the note that wanted it has gone.
        self.layers = [LayerPlayback::default(); MAX_LAYERS];
        let mut slot = 0;
        for (index, layer) in patch.layers.iter().enumerate() {
            if slot >= MAX_LAYERS {
                // The stack is full. `MAX_LAYERS` bounds what sounds *at
                // once* (TDD §7.4), and a seventeenth zone on one key is the
                // one thing it is allowed to drop.
                break;
            }
            let in_key_range = key >= layer.key_range.0 && key <= layer.key_range.1;
            let in_vel_range = velocity >= layer.vel_range.0 && velocity <= layer.vel_range.1;
            if !(in_key_range && in_vel_range) {
                continue;
            }
            // A patch with more zones than a `u16` can name is not a patch,
            // it is a corrupt file; the zones past that are dropped rather
            // than aliased onto a wrong one.
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            self.layers[slot] = LayerPlayback {
                active: true,
                layer: index,
                position: layer.playback.start_offset,
                // A fresh phase, for the reason the filter memory is reset: a
                // voice out of the pool carrying the last note's phase makes
                // the same note sound different depending on what was played
                // before it.
                osc: fontelle_dsp::Oscillator::new(),
                drum: fontelle_dsp::DrumSynth::new(),
                synth: {
                    // A fresh stack, seeded from the note's age for the same
                    // reason the LFOs are.
                    let mut state = fontelle_dsp::SynthState::new();
                    if let crate::patch::Source::Synth(osc) = &layer.source {
                        state.reset(osc, seed.wrapping_add(index as u32 * 0x85eb));
                    }
                    state
                },
                // Fired on the first sample rendered — see `drum_pending`.
                // A hat retriggered sixteen times a bar starts over each time
                // rather than adding to what is still ringing, because
                // `DrumSynth::trigger` rewrites every field.
                drum_pending: matches!(layer.source, crate::patch::Source::Drum(_)),
            };
            slot += 1;
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
        self.held = false;
        self.key = 0;
        self.voice_context = 0;
        self.origin = fontelle_types::VoiceOrigin::Timeline;
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
        self.lfos = [crate::lfo::LfoState::new(); MAX_LFOS];
        self.random = 0.0;
        self.note_counter = 0.0;
        self.age_samples = 0;
    }

    /// Moves this voice onto a new note **without restarting it** — legato.
    ///
    /// The sample keeps playing, the envelopes keep their level, and only the
    /// pitch moves; with a glide time it slides there rather than jumping,
    /// which is portamento. That is what `RetriggerMode::Legato` means and
    /// what makes a line played without gaps sound like one line.
    ///
    /// The **key is taken**, unlike [`Voice::glide_to`]: this voice is that
    /// note now, so the note-off the score wrote for it is the one that ends
    /// it. A slide is the other case — it bends the sound and leaves the
    /// note's identity alone.
    pub fn legato_to(&mut self, note: NoteTrigger, glide_seconds: f32) {
        let from = self.sounding_key();
        // A legato take-over is still a key going down, and it may take over
        // a voice that was already let go of — so this voice is held again,
        // and the note-off coming for `note.key` is the one that ends it.
        self.held = true;
        self.key = note.key;
        self.voice_context = note.voice_context;
        self.origin = note.origin;
        self.velocity_gain = velocity_to_gain(note.velocity);
        self.velocity_norm = note.velocity as f32 / 127.0;
        self.key_norm = note.key as f32 / 127.0;
        self.note_pan = note.pan.clamp(-1.0, 1.0);
        // The envelopes are deliberately not touched: that is the difference
        // between legato and a retrigger.
        self.glide_from(from - note.key as f32, glide_seconds);
    }

    /// Bends this voice to `key` over `seconds`, from wherever its pitch is.
    ///
    /// **A slide, not a note.** The voice's own [`key`](Voice::key) is left
    /// alone, so the note-off the score wrote for the key this voice *started*
    /// on still ends it — which is the whole reason the offset is a separate
    /// number rather than the key being rewritten. Without that, every slid
    /// note in a piece would hang.
    ///
    /// `seconds` of zero arrives immediately, which is what a portamento of
    /// zero has to mean.
    pub fn glide_to(&mut self, key: u8, seconds: f32) {
        self.glide_target = key as f32 - self.key as f32;
        self.set_glide_rate(seconds);
    }

    /// Starts this voice's pitch `semitones` away from its own note and lets
    /// it fall in over `seconds` — the portamento form.
    ///
    /// The other end of [`Voice::glide_to`]: that one moves the target, this
    /// one moves the *start*. A new note in a mono patch is at its own pitch
    /// as far as the score is concerned and has to sound as if it came from
    /// the last one.
    pub fn glide_from(&mut self, semitones: f32, seconds: f32) {
        self.glide_semitones = semitones;
        self.glide_target = 0.0;
        self.set_glide_rate(seconds);
    }

    /// Where this voice's pitch is now, as a key — the note it started on plus
    /// however far a glide has carried it.
    ///
    /// What a *following* glide measures from, so a chain of slides is one
    /// continuous line rather than a series of jumps back to the original key.
    pub fn sounding_key(&self) -> f32 {
        self.key as f32 + self.glide_semitones
    }

    fn set_glide_rate(&mut self, seconds: f32) {
        let distance = (self.glide_target - self.glide_semitones).abs();
        if seconds <= 0.0 || distance <= f32::EPSILON {
            self.glide_semitones = self.glide_target;
            self.glide_rate = 0.0;
            return;
        }
        self.glide_rate = distance / seconds;
    }

    /// Steps the glide on by one block.
    ///
    /// **Block rate, not sample rate**, and deliberately: the pitch a layer
    /// plays at is worked out once per block already — the mod matrix's
    /// `LayerPitch` route, which is what vibrato rides on, is computed in
    /// exactly the same place. A glide that moved per sample would be the only
    /// pitch modulation in this voice that did, and making all of it
    /// per-sample is a change to the render loop rather than to this feature.
    fn advance_glide(&mut self, frames: usize, sample_rate: f32) {
        if self.glide_rate <= 0.0 || sample_rate <= 0.0 {
            return;
        }
        let step = self.glide_rate * frames as f32 / sample_rate;
        let remaining = self.glide_target - self.glide_semitones;
        if remaining.abs() <= step {
            self.glide_semitones = self.glide_target;
            self.glide_rate = 0.0;
            return;
        }
        self.glide_semitones += step * remaining.signum();
    }

    /// Voice stealing always ramps out over a short release rather than cutting
    /// hard (TDD §7.4) — never a click. Uses the same envelope release as a
    /// normal note-off; a shorter, dedicated steal-ramp is a later refinement.
    pub fn release(&mut self) {
        self.held = false;
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
    /// whole part, distinct from the `pan` an SF2 zone carries for itself and
    /// from the one the *note* carries (§16.5, captured at note-on as
    /// `note_pan`). All three **add**, then clamp: that is what a soundfont
    /// player does, and it is the only reading under which a hard-left zone
    /// on a channel panned right ends up between them rather than at
    /// whichever was consulted last.
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
        self.render_performing(
            patch,
            store,
            // **No wavetables.** These two wrappers are the sampled and
            // oscillator path; a `Source::Synth` layer needs the tables
            // `Sampler::prepare` resolved for it, and therefore needs
            // `render_performing` — which is what the sampler calls. A layer
            // whose table is missing renders silence rather than a
            // substitute: a wrong sound is harder to diagnose than no sound.
            &crate::WavetableSet::EMPTY,
            sample_rate,
            quality,
            Performance {
                pan: channel_pan,
                ..Performance::default()
            },
            out,
        )
    }

    /// As [`Voice::render_with_pan`], with everything else the hand is doing
    /// — see [`Performance`], which is where the wheels are and why they are
    /// read here rather than captured at note-on.
    #[allow(clippy::too_many_arguments)]
    pub fn render_performing(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        tables: &crate::WavetableSet,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        performance: Performance,
        out: &mut [&mut [f32]],
    ) {
        let channel_pan = performance.pan;
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
                    sustain_level: 1.0,
                    ..Default::default()
                });
        // §16.5's per-note release, applied to the config rather than to the
        // generator: the envelope's shape is the patch's, and the note only
        // gets to say how long the last stage of it takes. A local copy, so
        // two notes on one patch can ring for different lengths.
        let amp_env_config = fontelle_dsp::EnvelopeConfig {
            release_s: amp_env_config.release_s * self.note_release_scale,
            ..amp_env_config
        };

        // Modulation runs at block rate: every source is sampled once here and
        // held for the whole block, and every destination is resolved from it
        // before the sample loop. At the engine's 128-frame blocks that is a
        // 375 Hz control rate — the same order as every hardware sampler ever
        // shipped, and cheap enough that a filter envelope costs a handful of
        // operations per block rather than per sample.
        //
        // The amp envelope is the exception: it advances per sample, because
        // it is a gain rather than a control value and a stepped one is
        // audible as a buzz on fast attacks. The **cutoff** is the second
        // exception, and it is ramped rather than advanced — see below.
        let mut env_levels = [0.0f32; MAX_MOD_ENVELOPES];
        let mut lfo_values = [0.0f32; MAX_LFOS];
        // Only the sources some route actually names are advanced. An SF2
        // import gives every patch a modulation envelope and two LFOs whether
        // it uses them or not, and one unread envelope is a stage advance per
        // sample per voice — the same order as the amp envelope, for nothing.
        let (env_used, lfo_used) = sources_in_use(&patch.mod_matrix);

        // Everything a source can be **except an LFO**, bound before the LFOs
        // are advanced because an LFO is a destination too — its rate, depth
        // and phase can be modulated, and that has to be resolved before it
        // turns.
        //
        // An LFO modulating *another* LFO reads zero here, deliberately: the
        // two would have to be ordered, and there is no order that is right
        // for a pair pointing at each other. A macro, an envelope, velocity or
        // a wheel on an LFO's rate — which is what every preset that wants
        // this actually asks for — all work.
        let (wheel, bend, pressure) = (
            performance.mod_wheel.clamp(0.0, 1.0),
            performance.pitch_bend.clamp(-1.0, 1.0),
            performance.aftertouch.clamp(0.0, 1.0),
        );
        let (velocity_norm, key_norm) = (self.velocity_norm, self.key_norm);
        let (mod_x_norm, mod_y_norm) = (self.mod_x_norm, self.mod_y_norm);
        let (random, note_counter) = (self.random, self.note_counter);
        // Four numbers copied out of the patch, so the closure does not borrow
        // it: the *name* of a macro is touched off the RT thread only, and the
        // value is all the audio path ever reads.
        let macro_values: [f32; crate::patch::MACRO_COUNT] =
            std::array::from_fn(|i| patch.macros[i].value.clamp(0.0, 1.0));
        let amp_level = self.amp_env.level();
        let scalar = move |source: crate::mod_matrix::ModSource| match source {
            crate::mod_matrix::ModSource::Velocity => velocity_norm,
            crate::mod_matrix::ModSource::Key => key_norm,
            crate::mod_matrix::ModSource::NoteModX => mod_x_norm,
            crate::mod_matrix::ModSource::NoteModY => mod_y_norm,
            crate::mod_matrix::ModSource::ModWheel => wheel,
            crate::mod_matrix::ModSource::PitchBend => bend,
            crate::mod_matrix::ModSource::Aftertouch => pressure,
            crate::mod_matrix::ModSource::Random => random,
            crate::mod_matrix::ModSource::NoteOnCounter => note_counter,
            crate::mod_matrix::ModSource::Macro(index) => {
                macro_values.get(index as usize).copied().unwrap_or(0.0)
            }
            crate::mod_matrix::ModSource::Envelope(0) => amp_level,
            crate::mod_matrix::ModSource::Envelope(_) | crate::mod_matrix::ModSource::Lfo(_) => 0.0,
        };
        // The amp envelope's stage times, modulated. Read from the sources
        // above — key, velocity, the wheels, the macros — rather than from
        // the LFOs and the other envelopes, because a duration wobbled by
        // the thing it is timing has no right answer. A piano's decay
        // following the key is what this is for.
        let amp_env_config = stage_times(amp_env_config, 0, &patch.mod_matrix, &scalar);
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
                // Its stage times, modulated the same way the amp envelope's
                // are — from the per-note and performance sources only.
                let config = stage_times(*config, (index + 1) as u8, &patch.mod_matrix, &scalar);
                for _ in 0..frames {
                    env.advance(&config, sample_rate);
                }
            }
            for (index, lfo) in patch.lfos.iter().take(MAX_LFOS).enumerate() {
                if !lfo_used[index] {
                    continue;
                }
                // An LFO is a destination as well as a source. Rate is
                // modulated in **octaves** rather than hertz, so "wobble
                // faster" means the same thing at 1/16 as at 1/2 — three
                // octaves either way at full depth, which is the span between
                // a slow sweep and a growl.
                let index_u8 = index as u8;
                // The mod envelopes have already been read this block, so an
                // envelope on an LFO's rate works; only another LFO reads
                // zero.
                let for_lfo = |source: crate::mod_matrix::ModSource| match source {
                    crate::mod_matrix::ModSource::Envelope(0) => amp_level,
                    crate::mod_matrix::ModSource::Envelope(at) => {
                        env_levels.get(at as usize - 1).copied().unwrap_or(0.0)
                    }
                    other => scalar(other),
                };
                let rate_dest = crate::mod_matrix::ModDest::LfoRate(index_u8);
                let octaves =
                    patch.mod_matrix.evaluate(rate_dest, &for_lfo) * rate_dest.full_scale() * 3.0;
                let depth_dest = crate::mod_matrix::ModDest::LfoDepth(index_u8);
                let phase_dest = crate::mod_matrix::ModDest::LfoPhase(index_u8);
                let lfo = crate::patch::Lfo {
                    depth: (lfo.depth
                        + patch.mod_matrix.evaluate(depth_dest, &for_lfo)
                            * depth_dest.full_scale())
                    .clamp(0.0, 1.0),
                    phase: lfo.phase
                        + patch.mod_matrix.evaluate(phase_dest, &for_lfo) * phase_dest.full_scale(),
                    ..*lfo
                };
                let lfo = &lfo;
                let rate =
                    crate::lfo::LfoState::rate_hz(lfo, performance.clock.bpm) * 2f32.powf(octaves);
                // A free-running LFO's phase is a fact about the transport,
                // so it is worked out from the clock rather than accumulated:
                // every voice reads the same number and the same bar sounds
                // the same every time it plays.
                let clock_phase = (lfo.mode == crate::patch::LfoMode::Free).then(|| {
                    crate::lfo::free_phase(performance.clock.position_sample, sample_rate, rate)
                });
                lfo_values[index] = self.lfos[index].advance_block(
                    lfo,
                    rate,
                    sample_rate,
                    frames,
                    self.age_samples,
                    clock_phase,
                );
            }
        }

        // The mod matrix's full view of this voice: the scalar sources above,
        // plus the two whose values this block has just worked out.
        //
        // Bound to locals rather than reaching through `self`, so the closure
        // holds no borrow of the voice and the sample loop below is free to
        // take `self.layers` mutably.
        let sources = move |source: crate::mod_matrix::ModSource| match source {
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
            other => scalar(other),
        };

        // Which layers some *other* layer reads as its modulator.
        //
        // A layer at the floor is normally not rendered at all (see the skip
        // below), and this is the exception: a modulator's level is how much
        // of it you **hear**, not whether it modulates, so an FM operator
        // turned all the way down is still a full-strength operator. "FM
        // Growl" and "EP Tine" are both an oscillator nobody hears.
        let mut is_modulator = [false; MAX_LAYERS];
        for layer in &patch.layers {
            if let crate::patch::Source::Synth(osc) = &layer.source
                && let Some(which) = osc.modulator
                && let Some(flag) = is_modulator.get_mut(usize::from(which))
            {
                *flag = true;
            }
        }

        // Per-layer constants resolved once, not once per sample: a fixed-size
        // stack array (INVARIANT 1 — no `Vec`, nothing heap-touching).
        let mut prepared: [Option<PreparedLayer<'_>>; MAX_LAYERS] = [None; MAX_LAYERS];
        for (prepared_index, slot) in self.layers.iter().enumerate() {
            if !slot.active {
                continue;
            }
            // The slot names its zone; `prepared` is indexed by *slot*, while
            // every `ModDest` below is addressed by the zone's own index,
            // because that is what a saved mod route names.
            let index = usize::from(slot.layer);
            let Some(layer) = patch.layers.get(index) else {
                continue;
            };
            let sample_file = match &layer.source {
                crate::patch::Source::Sample { file } => Some(*file),
                crate::patch::Source::Sf2Zone { .. } => continue,
                // None of these names a file, so none has a buffer to look up
                // — see the source match below, which is where they are
                // resolved instead.
                crate::patch::Source::Oscillator(_)
                | crate::patch::Source::Drum(_)
                | crate::patch::Source::Synth(_) => None,
            };
            let buffer = match sample_file {
                Some(file) => match store.get(file) {
                    Some(buffer) => Some(buffer),
                    None => continue,
                },
                None => None,
            };

            // `ModDest` names a layer with a `u8`, so a patch with more than
            // 256 zones has no way to address the ones past that. They get no
            // per-layer modulation rather than being aliased onto layer 0's
            // routes, which is the failure that would be impossible to see.
            let dest_index = u8::try_from(index).ok();
            let layer_mod = |make: fn(u8) -> crate::mod_matrix::ModDest| {
                dest_index.map_or(0.0, |i| {
                    let dest = make(i);
                    patch.mod_matrix.evaluate(dest, &sources) * dest.full_scale()
                })
            };

            // Pitch modulation is in cents, like the tuning it adds to, so a
            // route means the same interval wherever the note sits.
            let pitch_cents = layer_mod(crate::mod_matrix::ModDest::LayerPitch);
            // The glide adds to the note's own interval rather than moving
            // the key: the key is the note's *identity*, and a note-off names
            // it (see `glide_to`). The bend adds the same way, and a patch
            // that also *routes* the bend gets both, which is what a route is
            // for.
            let semitones = (self.key as f32 - layer.root_key as f32)
                + self.glide_semitones
                + bend * patch.voice_config.bend_range_semitones
                + self.note_detune
                + (layer.fine_tune_cents + pitch_cents) / 100.0;
            let pitch_ratio = 2f32.powf(semitones / 12.0);

            // Gain modulation is in decibels, so a tremolo is symmetric in
            // loudness rather than lopsided the way a linear one would be.
            let gain_db = layer_mod(crate::mod_matrix::ModDest::LayerGain);

            // **A layer at the floor is off**, which is what `SILENT_DB` means
            // everywhere else in this program — `Sampler::set_gain_db` reads
            // the same threshold as a gain of exactly zero, and the presets
            // use it to say "this oscillator is not in use" (§6: the Init
            // patch is five layers with one of them up).
            //
            // Skipping it here is the difference between a Flopsynth voice
            // costing one oscillator and costing five: everything below —
            // the table read, the unison stack, the pan, the filter feed — is
            // per sample. The one exception is above: a layer somebody
            // modulates with is rendered whatever its level.
            //
            // Resolved per block like every other modulation, so a route that
            // brings a layer up brings it back on the next block, which is
            // the control rate everything else here runs at.
            if layer.gain_db + gain_db <= crate::SILENT_DB
                && !is_modulator[index.min(MAX_LAYERS - 1)]
            {
                continue;
            }

            let pan_gain = if stereo {
                // Three pans, and they add: the zone's own placement inside
                // the instrument, the note's placement inside the part, and
                // the part's placement in the mix. Any other reading throws
                // one of them away — see `render_with_pan`'s docs.
                let pan = layer.pan
                    + self.note_pan
                    + channel_pan
                    + layer_mod(crate::mod_matrix::ModDest::LayerPan);
                fontelle_types::PanLaw::Minus3Db.gains(pan)
            } else {
                (1.0, 0.0)
            };

            let mut route = fontelle_dsp::FilterRoute::Serial;
            let source = match (&layer.source, buffer) {
                (crate::patch::Source::Oscillator(kind), _) => {
                    // The note's own pitch, read off the same `semitones` a
                    // sample is transposed by: at the default root of middle C
                    // a note plays itself, a higher root plays it lower, and
                    // every tuning on the pitch path is already in there.
                    let freq_hz = OSC_ROOT_HZ * pitch_ratio;
                    PreparedSource::Oscillator {
                        kind: *kind,
                        // Above Nyquist there is no waveform left to draw,
                        // only aliases folding back down.
                        freq_hz: freq_hz.clamp(0.0, sample_rate * 0.5),
                    }
                }
                // Before the buffer arm: a drum names no file, so `buffer`
                // is `None` for one and it would otherwise fall through to
                // the `continue` at the bottom and render silence.
                (crate::patch::Source::Drum(voice), _) => PreparedSource::Drum(*voice),
                (crate::patch::Source::Synth(osc), _) => {
                    // The four knobs the matrix can move on an oscillator,
                    // resolved into a **copy** of the patch's description:
                    // the patch is what the document holds and a route is not
                    // an edit to it.
                    let mut osc = *osc;
                    osc.position = (osc.position
                        + layer_mod(crate::mod_matrix::ModDest::OscPosition))
                    .clamp(0.0, 1.0);
                    osc.warp_amount = (osc.warp_amount
                        + layer_mod(crate::mod_matrix::ModDest::OscWarp))
                    .clamp(0.0, 1.0);
                    osc.unison.detune_cents = (osc.unison.detune_cents
                        + layer_mod(crate::mod_matrix::ModDest::OscUnisonDetune))
                    .max(0.0);
                    osc.unison.blend = (osc.unison.blend
                        + layer_mod(crate::mod_matrix::ModDest::OscUnisonBlend))
                    .clamp(0.0, 1.0);
                    route = osc.filter_route;
                    let table = match osc.source {
                        fontelle_dsp::SynthSource::Table(id) => tables.get(id),
                        // One the patch carries itself — see `UserWavetable`.
                        fontelle_dsp::SynthSource::User(at) => tables.get_user(at as usize),
                        fontelle_dsp::SynthSource::Noise => None,
                    };
                    PreparedSource::Synth {
                        osc,
                        table,
                        note_hz: OSC_ROOT_HZ * pitch_ratio,
                    }
                }
                (_, Some(buffer)) => {
                    let rate_ratio = buffer.sample_rate as f32 / sample_rate;
                    let loop_len = layer.playback.loop_end - layer.playback.loop_start;
                    PreparedSource::Sample {
                        data: &buffer.data,
                        step: (pitch_ratio * rate_ratio) as f64,
                        loop_end: layer.playback.loop_end,
                        loop_len,
                        // `loop_len > 0.0` also guards the wrap loop below
                        // against spinning forever on a degenerate
                        // zero-length loop.
                        looping: matches!(layer.playback.loop_mode, crate::LoopMode::Forward)
                            && loop_len > 0.0,
                        end_offset: layer.playback.end_offset,
                        interpolation: layer.playback.interpolation.unwrap_or(quality),
                    }
                }
                (_, None) => continue,
            };

            prepared[prepared_index] = Some(PreparedLayer {
                source,
                gain: 10f32.powf((layer.gain_db + gain_db) / 20.0) * self.velocity_gain,
                pan_gain,
                route,
                layer_index: index,
            });
        }

        // --- the two filter slots, and the ramp that keeps them quiet -------
        //
        // Cutoff is resolved per block like everything else, but **applied**
        // as a ramp from the previous block's value to this one's, rebuilt
        // every `FILTER_STEP` samples along the way. With an LFO on cutoff at
        // the engine's 375 Hz block rate, a corner that jumped once a block
        // would be an audible zipper; the SVF's zero-delay topology is what
        // makes moving it this often safe.
        let mut settings: [fontelle_dsp::SynthFilterSettings; 2] = Default::default();
        let mut target_cents = [0.0f32; 2];
        for index in 0..2 {
            let slot = patch.filters[index];
            let dest = crate::mod_matrix::ModDest::FilterCutoff(index as u8);
            // Cutoff modulation is in cents, so it scales the corner rather
            // than shifting it — an octave down means the same thing at
            // 200 Hz as at 8 kHz, which a linear offset would not.
            let cents = patch.mod_matrix.evaluate(dest, &sources) * dest.full_scale();
            target_cents[index] = cents;
            let q_dest = crate::mod_matrix::ModDest::FilterResonance(index as u8);
            let resonance =
                slot.resonance + patch.mod_matrix.evaluate(q_dest, &sources) * q_dest.full_scale();
            let drive_dest = crate::mod_matrix::ModDest::FilterDrive(index as u8);
            let character_dest = crate::mod_matrix::ModDest::FilterCharacter(index as u8);
            settings[index] = fontelle_dsp::SynthFilterSettings {
                model: slot.model,
                mode: slot.mode,
                slope: slot.slope,
                // Key tracking is applied here rather than stored, so one
                // patch sounds the same at every pitch without the document
                // carrying a different cutoff per note.
                cutoff_hz: fontelle_dsp::key_tracked_cutoff(
                    slot.cutoff_hz,
                    self.key,
                    slot.key_track,
                ),
                resonance,
                drive: (slot.drive
                    + patch.mod_matrix.evaluate(drive_dest, &sources) * drive_dest.full_scale())
                .clamp(0.0, 1.0),
                character: (slot.character
                    + patch.mod_matrix.evaluate(character_dest, &sources)
                        * character_dest.full_scale())
                .clamp(0.0, 1.0),
            };
        }
        let enabled = [patch.filters[0].enabled, patch.filters[1].enabled];
        let from_cents = self.filter_cents;
        self.filter_cents = target_cents;

        // The voice-wide gain the matrix can move, in decibels — one route
        // for a tremolo rather than one per layer.
        let amp_dest = crate::mod_matrix::ModDest::Amp;
        let amp_db = patch.mod_matrix.evaluate(amp_dest, &sources) * amp_dest.full_scale();
        let amp_gain = 10f32.powf(amp_db / 20.0);

        // Split once, outside the loop: `out[0]` and `out[1]` are distinct
        // slices, and taking both mutably per sample would be a reborrow the
        // compiler can't see through.
        let (left, rest) = out.split_at_mut(1);
        let left = &mut *left[0];
        let mut right = rest.first_mut();

        // Every layer's sample this frame, indexed by **patch layer index**,
        // so an oscillator naming a later layer as its FM or RM modulator can
        // find it. Fixed-size and on the stack, like everything else here.
        let mut layer_out = [0.0f32; MAX_LAYERS];
        let mut live: [fontelle_dsp::SynthFilterSettings; 2] = settings;

        for frame in 0..frames {
            let env = self.amp_env.advance(&amp_env_config, sample_rate);

            // The cutoff ramp, rebuilt every `FILTER_STEP` samples.
            if frame % FILTER_STEP == 0 {
                let t = frame as f32 / frames.max(1) as f32;
                for index in 0..2 {
                    let cents = from_cents[index] + (target_cents[index] - from_cents[index]) * t;
                    live[index].cutoff_hz = settings[index].cutoff_hz * 2f32.powf(cents / 1200.0);
                }
            }

            // Four buses, summed before the filters: to F1, to F2, to F1→F2,
            // and around both. Per layer, which is what lets a sub bypass a
            // closed low-pass while the saw above it is being swept.
            let mut to_f1 = (0.0f32, 0.0f32);
            let mut to_f2 = (0.0f32, 0.0f32);
            let mut to_serial = (0.0f32, 0.0f32);
            let mut dry = (0.0f32, 0.0f32);

            // **Last to first.** An oscillator's FM or RM modulator is always
            // a *later* layer (the panel refuses anything else), so walking
            // backwards means the modulator's sample for this frame already
            // exists by the time the layer that reads it is evaluated — with
            // no second pass and no one-sample delay.
            for index in (0..MAX_LAYERS).rev() {
                let Some(prep) = prepared[index] else {
                    continue;
                };
                let slot = &mut self.layers[index];
                if !slot.active {
                    continue;
                }

                let (mut sample_l, mut sample_r) = match prep.source {
                    PreparedSource::Sample {
                        data,
                        step,
                        loop_end,
                        loop_len,
                        looping,
                        end_offset,
                        interpolation,
                    } => {
                        if looping {
                            // `while`, not `if`: one subtraction isn't enough
                            // when the playback step exceeds the loop length,
                            // which real extreme upward transposition of a
                            // short loop does.
                            while slot.position >= loop_end {
                                slot.position -= loop_len;
                            }
                        } else if slot.position >= end_offset {
                            slot.active = false;
                            continue;
                        }
                        let sample = fontelle_dsp::interpolate(data, slot.position, interpolation);
                        slot.position += step;
                        (sample, sample)
                    }
                    // No end to run off and no buffer to walk: the phase is
                    // the whole of its position, and it advances itself.
                    PreparedSource::Oscillator { kind, freq_hz } => {
                        let sample = slot.osc.next_sample(kind, freq_hz, sample_rate);
                        (sample, sample)
                    }
                    // The hit ends itself. Marking the slot inactive when it
                    // does is what stops a finished drum costing a `tanh` and
                    // a filter per sample for the rest of the note.
                    PreparedSource::Drum(voice) => {
                        if slot.drum_pending {
                            slot.drum.trigger(&voice, sample_rate);
                            slot.drum_pending = false;
                        }
                        if slot.drum.is_done() {
                            slot.active = false;
                            continue;
                        }
                        let sample = slot.drum.next_sample(&voice, sample_rate);
                        (sample, sample)
                    }
                    PreparedSource::Synth {
                        osc,
                        table,
                        note_hz,
                    } => {
                        // The modulator's sample from *this* frame, already
                        // computed because the walk is backwards. A layer
                        // naming a modulator that is not there reads zero,
                        // which makes an FM knob with nothing to modulate a
                        // silent knob rather than a broken one.
                        let modulator = osc
                            .modulator
                            .map(|m| layer_out.get(usize::from(m)).copied().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        slot.synth
                            .next_sample(&osc, table, note_hz, sample_rate, modulator)
                    }
                };
                // **Before the level knob**, and before the pan.
                //
                // A modulator's level is how much of it you *hear*; the warp
                // amount is how hard it modulates. Reading the post-gain
                // sample would fold the two into one knob and — worse — make a
                // modulator turned down to silence stop modulating, which is
                // exactly the setup a dedicated FM operator wants ("FM Growl"
                // and "EP Tine" are both an oscillator nobody hears). Before
                // the pan for the same kind of reason: FM by one side of a
                // panned stack is not a thing anybody means.
                if prep.layer_index < MAX_LAYERS {
                    layer_out[prep.layer_index] = (sample_l + sample_r) * 0.5;
                }
                sample_l *= prep.gain;
                sample_r *= prep.gain;

                let placed = (sample_l * prep.pan_gain.0, sample_r * prep.pan_gain.1);
                let bus = match prep.route {
                    fontelle_dsp::FilterRoute::F1 => &mut to_f1,
                    fontelle_dsp::FilterRoute::F2 => &mut to_f2,
                    fontelle_dsp::FilterRoute::Serial => &mut to_serial,
                    fontelle_dsp::FilterRoute::Bypass => &mut dry,
                };
                bus.0 += placed.0;
                bus.1 += placed.1;
            }

            // buses -> Filter1 / Filter2 -> Amp (TDD §7.4). The filters sit
            // ahead of the amp stage and operate on this voice's own mixed
            // sample rather than on the shared output buffer.
            let mut mixed = dry;
            if enabled[0] {
                mixed.0 += self.filters[0][0].process(to_f1.0, &live[0], sample_rate);
                if right.is_some() {
                    mixed.1 += self.filters[0][1].process(to_f1.1, &live[0], sample_rate);
                }
            } else {
                mixed.0 += to_f1.0;
                mixed.1 += to_f1.1;
            }
            // The serial path through its **own** copy of filter 1 — see
            // `Voice::filters`, which is where the third slot is argued.
            let mut serial = to_serial;
            if enabled[0] {
                serial.0 = self.filters[2][0].process(serial.0, &live[0], sample_rate);
                if right.is_some() {
                    serial.1 = self.filters[2][1].process(serial.1, &live[0], sample_rate);
                }
            }
            let into_f2 = (to_f2.0 + serial.0, to_f2.1 + serial.1);
            if enabled[1] {
                mixed.0 += self.filters[1][0].process(into_f2.0, &live[1], sample_rate);
                if right.is_some() {
                    mixed.1 += self.filters[1][1].process(into_f2.1, &live[1], sample_rate);
                }
            } else {
                mixed.0 += into_f2.0;
                mixed.1 += into_f2.1;
            }

            let gain = env * amp_gain;
            left[frame] += mixed.0 * gain;
            if let Some(right) = right.as_deref_mut() {
                right[frame] += mixed.1 * gain;
            }
        }

        self.age_samples += frames as u64;
        self.advance_glide(frames, sample_rate);

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
    /// How many of `voices` new notes may use, `1..=voices.len()`.
    ///
    /// **The live polyphony**, which is not the same thing as the pool's size:
    /// `patch/voice/polyphony` is automatable (§12.3), and growing a `Vec` on
    /// the audio thread is exactly what INVARIANT 1 forbids. So the pool keeps
    /// the size it was built at and this moves inside it, which is what a
    /// polyphony limit means anyway — a note that finds nothing free under the
    /// limit steals, exactly as it does when the pool is full.
    ///
    /// The pool is built from the patch, so the knob's value is the ceiling; a
    /// lane can go down from there and back up, and turning the knob rebuilds
    /// the graph and raises it.
    limit: usize,
}

impl VoicePool {
    pub fn with_capacity(capacity: u16) -> Self {
        Self {
            voices: (0..capacity).map(|_| Voice::new()).collect(),
            next_age: 0,
            limit: usize::from(capacity),
        }
    }

    /// How many voices the pool physically has — the ceiling a lane cannot
    /// raise the limit past.
    pub fn capacity(&self) -> usize {
        self.voices.len()
    }

    /// Sets how many voices new notes may use, clamped into the pool.
    ///
    /// Voices already sounding **above** the new limit are left alone rather
    /// than cut: they ring out and their slots come back as they finish, which
    /// is what lowering a polyphony knob does everywhere else. Cutting them
    /// would put a click exactly where somebody was reaching for a swell.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.clamp(1, self.voices.len().max(1));
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

        // Only the voices under the limit are candidates — see `limit`. A
        // note arriving while the ones above it are still ringing out steals
        // from inside the limit rather than reaching past it, which is what
        // keeps a lowered polyphony *lowered* while the tail of the old
        // setting decays.
        let usable = self.limit.min(self.voices.len());
        let index = match self.voices[..usable].iter().position(|v| !v.is_active()) {
            Some(i) => i,
            None => match policy {
                StealPolicy::Oldest | StealPolicy::Quietest | StealPolicy::LowestPriority => self
                    .voices[..usable]
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

    /// Finds the voice a note-off for `(key, voice_context)` should end — used
    /// by `Sampler::note_off` (TDD §11.4: voice-context tagging keeps
    /// overlapping clips' note-offs from killing each other's voices).
    ///
    /// **A voice that has already been released is not a candidate, and among
    /// those still held the newest wins.** Both halves were a hung note:
    ///
    /// - Press a key, let go, press it again before the first press has
    ///   finished ringing out, and that key has two voices — one releasing,
    ///   one held. Taking the first in pool order spent the note-off on the
    ///   one that was already released, and the held one was left sounding
    ///   with nothing left that could address it. On a patch that sustains
    ///   that is a drone; low notes reached it first, their tails being the
    ///   longest. Reported as *"it seems to want to often just hold a note
    ///   forever if i spam lower notes"*.
    /// - And when two presses of one key really are both down, the newest is
    ///   the one to let go of, so that a lost note-off strands an old voice
    ///   the steal path will reclaim rather than the one being played now.
    pub fn find_active_mut(&mut self, key: u8, voice_context: u32) -> Option<&mut Voice> {
        self.voices
            .iter_mut()
            .filter(|v| {
                v.is_active() && v.is_held() && v.key() == key && v.voice_context() == voice_context
            })
            .max_by_key(|v| v.age)
    }

    /// Every sounding voice, in pool order.
    pub fn iter_active(&self) -> impl DoubleEndedIterator<Item = &Voice> {
        self.voices.iter().filter(|v| v.is_active())
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

    /// Silences only the voices the timeline started, leaving live ones
    /// sounding — transport stop and seek (TDD §6.3 against §14).
    ///
    /// A sequenced voice belongs to a moment in the song the playhead has
    /// left. A live voice belongs to a key somebody is still holding, and
    /// stop is a statement about the sequencer rather than about the player.
    pub fn reset_sequenced(&mut self) {
        for voice in &mut self.voices {
            if voice.origin() == fontelle_types::VoiceOrigin::Timeline {
                voice.reset();
            }
        }
    }
}

/// `config` with every stage time the matrix routes to scaled into place.
///
/// `ModDest::EnvelopeStageTime(envelope, stage)` numbers the AHDSR stages
/// from one — attack, hold, decay, sustain, release — and its unit is octaves
/// of time (`ModDest::full_scale`), so a route's value is an exponent on the
/// stored time and a route at zero leaves it exactly alone. Sustain is a level
/// and not a time, so stage four has no entry here.
///
/// Called once per block per envelope, which is what makes reading it from
/// the block-rate sources rather than per sample the right cost: four
/// evaluations over a matrix of a dozen routes, beside a filter.
fn stage_times(
    config: fontelle_dsp::EnvelopeConfig,
    envelope: u8,
    matrix: &crate::mod_matrix::ModMatrix,
    sources: &dyn Fn(crate::mod_matrix::ModSource) -> f32,
) -> fontelle_dsp::EnvelopeConfig {
    let scale = |stage: u8, seconds: f32| {
        let dest = crate::mod_matrix::ModDest::EnvelopeStageTime(envelope, stage);
        let octaves = matrix.evaluate(dest, sources);
        if octaves == 0.0 {
            seconds
        } else {
            seconds * 2f32.powf(octaves * dest.full_scale())
        }
    };
    fontelle_dsp::EnvelopeConfig {
        attack_s: scale(1, config.attack_s),
        hold_s: scale(2, config.hold_s),
        decay_s: scale(3, config.decay_s),
        release_s: scale(5, config.release_s),
        ..config
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
            ..Default::default()
        }
    }

    fn disabled_filter() -> FilterSlot {
        FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
        };
        let growth = |routed: bool| {
            let mut store = SampleStore::new();
            let mut patch = tone_patch(&mut store, 6_000.0, 48_000);
            patch.filters[0] = FilterSlot {
                mode: SvfMode::Lowpass,
                cutoff_hz: 400.0,
                resonance: std::f32::consts::FRAC_1_SQRT_2,
                enabled: true,
                ..Default::default()
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
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
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
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
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
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
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
            wave: fontelle_types::LfoWave::Sine,
            // Half a second: past the LFO's first two peaks.
            delay_s: 0.5,
            ..Default::default()
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
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
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
