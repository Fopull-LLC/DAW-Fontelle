use std::sync::Arc;

use fontelle_core::{SampleStore, Sampler};
use fontelle_types::{PanLaw, ParamAddress};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};

/// The widest render `SamplerNode` performs: a stereo pair. Surround is not a
/// feature yet, and a fixed width is what keeps the scratch allocation in
/// `prepare` (INVARIANT 1).
const MAX_CHANNELS: usize = 2;

pub(crate) struct EmptyParams;
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
///
/// `store` is shared (`Arc`) rather than owned: TDD §7.7 requires sample data be
/// shared across every patch/channel referencing the same file, so many
/// `SamplerNode`s legitimately point at the same store. **M0 scope:** the store
/// is populated before it's handed to the RT thread and never mutated after —
/// there's no synchronisation for a live re-import while a `SamplerNode` holding
/// it is already playing.
/// The span an automated **channel** level runs over, in decibels.
///
/// The same range the instrument panel's own volume knob has (see
/// `fontelle_app::instrument::GAIN_MIN_DB`), because an automation lane and the
/// knob it was made from have to mean the same thing by the same number — a
/// curve drawn at half height that played at a different level from the knob at
/// half travel would be a lane nobody could aim.
pub const CHANNEL_GAIN_MIN_DB: f32 = -60.0;
pub const CHANNEL_GAIN_MAX_DB: f32 = 12.0;

/// The controller a mod wheel sends, and the only one a built-in instrument
/// gives a meaning of its own — see `SamplerNode::process`.
const MOD_WHEEL_CC: u8 = 1;

pub struct SamplerNode {
    sampler: Sampler,
    store: Arc<SampleStore>,
    /// What `prepare` was told, so a slide's length — which arrives in samples,
    /// the only clock the wire has — can be turned into the seconds the
    /// sampler's glide takes.
    sample_rate: f32,
    /// Two channels' worth, laid out back to back and split in `process`.
    /// Sized in `prepare`, so `process` never allocates (INVARIANT 1). See the
    /// note in `process` for why the render can't go straight to the bus.
    scratch: Vec<f32>,
    /// Frames per channel in `scratch` — its length is twice this.
    scratch_frames: usize,
    /// **The instrument's own effects** (`docs/flopsynth-plan.md` §2.2).
    ///
    /// A Serum preset without its chorus and reverb is not that preset, so the
    /// chain belongs to the *instrument* rather than to the channel it happens
    /// to be on. The document carries it as `Patch::fx` — `EffectConfig` lives
    /// in `fontelle-types`, which `fontelle-core` already depends on — and
    /// this node, which can see `fontelle-fx` as well, is what runs it.
    ///
    /// One slot per `MAX_PATCH_FX`, built in `prepare` from the patch's
    /// configs and never touched in `process`: `None` for a slot the patch
    /// does not have.
    fx: Vec<Option<EffectState>>,
    /// The dry signal each slot's mix knob blends back in, `MAX_CHANNELS`
    /// worth per slot, sized in `prepare`.
    fx_dry: Vec<Vec<f32>>,
    /// Where this node says how many voices are sounding, for whoever is
    /// drawing the instrument (`docs/flopsynth-plan.md` §11, phase 6).
    ///
    /// `None` for a node nobody is watching — the preview voice, and every
    /// node in an offline render. One relaxed store per block when there is
    /// one, which is the same cost a track's peak meter already pays.
    meter: Option<Arc<VoiceMeter>>,
    /// A ring of the instrument's own output — after its chain, before the
    /// channel's gain — for whoever is drawing the sky through Flopsynth's
    /// canopy. The same ring an EQ's analyser reads, for the same reason:
    /// one relaxed store per frame and nothing that can block this thread.
    /// `None` for a node nobody is watching.
    scope: Option<Arc<crate::SpectrumTap>>,
}

/// How many voices an instrument is playing, read off the audio thread.
///
/// The first read-out in this program that comes off the **RT thread's own
/// state** rather than off the document: a count of live voices is not a fact
/// the document has, and polyphony is a number somebody sets — the only way to
/// know whether sixteen is enough for what they are playing is to watch it.
///
/// One atomic, written once a block and read whenever a window redraws, which
/// is `TrackControls::peaks`' arrangement exactly.
#[derive(Debug, Default)]
pub struct VoiceMeter {
    voices: std::sync::atomic::AtomicU32,
    /// Where the newest voice's LFOs are in their cycles — the dot on each
    /// picture. Four, which is `fontelle_core::MAX_LFOS`; a mismatch would be
    /// a compile error at the store below.
    lfo_phases: [std::sync::atomic::AtomicU32; fontelle_core::MAX_LFOS],
    /// The peak of the block after each effect slot ran
    /// (`docs/flopsynth-next.md` §3.6) — the rack's level meters. Zero for
    /// a slot that is empty or bypassed.
    fx_levels: [std::sync::atomic::AtomicU32; fontelle_core::MAX_PATCH_FX],
}

impl VoiceMeter {
    /// What each effect slot put out this block, as a peak in 0..=1-ish.
    pub fn fx_levels(&self) -> [f32; fontelle_core::MAX_PATCH_FX] {
        std::array::from_fn(|index| {
            f32::from_bits(self.fx_levels[index].load(std::sync::atomic::Ordering::Relaxed))
        })
    }

    pub fn set_fx_level(&self, slot: usize, level: f32) {
        if let Some(cell) = self.fx_levels.get(slot) {
            cell.store(level.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn voices(&self) -> usize {
        self.voices.load(std::sync::atomic::Ordering::Relaxed) as usize
    }

    pub fn set_voices(&self, count: usize) {
        self.voices
            .store(count as u32, std::sync::atomic::Ordering::Relaxed);
    }

    /// Where each LFO of the newest voice is, 0..1.
    pub fn lfo_phases(&self) -> [f32; fontelle_core::MAX_LFOS] {
        std::array::from_fn(|index| {
            f32::from_bits(self.lfo_phases[index].load(std::sync::atomic::Ordering::Relaxed))
        })
    }

    pub fn set_lfo_phases(&self, phases: [f32; fontelle_core::MAX_LFOS]) {
        for (slot, phase) in self.lfo_phases.iter().zip(phases) {
            slot.store(phase.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl SamplerNode {
    pub fn new(sampler: Sampler, store: Arc<SampleStore>) -> Self {
        Self {
            sampler,
            store,
            sample_rate: 0.0,
            scratch: Vec::new(),
            scratch_frames: 0,
            fx: Vec::new(),
            fx_dry: Vec::new(),
            meter: None,
            scope: None,
        }
    }

    /// Reports its voice count here, once a block.
    pub fn with_meter(mut self, meter: Arc<VoiceMeter>) -> Self {
        self.meter = Some(meter);
        self
    }

    /// Copies its output here, every block — see [`Self::scope`].
    pub fn with_scope(mut self, scope: Arc<crate::SpectrumTap>) -> Self {
        self.scope = Some(scope);
        self
    }
}

impl AudioNode for SamplerNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
        self.sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: ctx.sample_rate,
            max_block_size: ctx.max_block_size,
        });
        self.scratch_frames = ctx.max_block_size as usize;
        self.scratch.resize(self.scratch_frames * MAX_CHANNELS, 0.0);

        // The chain, built here and nowhere else: a slot's *kind* is
        // structure, so changing one rebuilds the graph and comes back
        // through `prepare`. What `process` does is run what it finds.
        let slots = self
            .sampler
            .patch()
            .fx
            .iter()
            .map(|slot| {
                let mut state = EffectState::for_config(&slot.config);
                state.prepare(ctx.sample_rate, &slot.config);
                Some(state)
            })
            .collect::<Vec<_>>();
        self.fx = slots;
        self.fx_dry = (0..MAX_CHANNELS)
            .map(|_| vec![0.0; self.scratch_frames])
            .collect();
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        for (origin, event) in ctx.events_with_origin() {
            match &event.payload {
                fontelle_types::EventPayload::NoteOn {
                    key,
                    velocity,
                    pan,
                    fine_pitch,
                    release,
                    mod_x,
                    mod_y,
                    voice_context,
                } => {
                    // The origin is recorded on the voice, so a transport stop
                    // can cut what the song started without cutting what the
                    // player is holding.
                    //
                    // And this is the seam §16.5's five per-note properties
                    // cross. Only pan changes units on the way: the wire
                    // counts it in the document's bytes and everything below
                    // here in unit intervals, and `pan_unit` is the one
                    // conversion. The other four mean the same on both sides.
                    self.sampler.trigger(
                        fontelle_core::NoteTrigger::new(*key, *velocity)
                            .with_pan(fontelle_types::pan_unit(*pan))
                            .with_fine_pitch(*fine_pitch)
                            .with_release(*release)
                            .with_mod_x(*mod_x)
                            .with_mod_y(*mod_y)
                            .in_context(*voice_context)
                            .from_origin(origin),
                    );
                }
                fontelle_types::EventPayload::NoteOff { key, voice_context } => {
                    self.sampler.note_off(*key, *voice_context);
                }
                // A slide note: bend what is sounding, start nothing. The
                // wire carries samples because that is the only clock the RT
                // side has; the sampler wants seconds, and this is the one
                // place that conversion happens.
                fontelle_types::EventPayload::NoteSlide {
                    key,
                    glide_samples,
                    voice_context,
                } => {
                    let seconds = if self.sample_rate > 0.0 {
                        *glide_samples as f32 / self.sample_rate
                    } else {
                        0.0
                    };
                    self.sampler.slide(*key, seconds, *voice_context);
                }
                // The wheels (TDD §7.4). **Performance, not automation**: a
                // `ParamValue` names one of this patch's own controls by its
                // §8.2 address, while these three are the fact that a hand
                // moved — so they go to the channel's controller state, and
                // where they *land* is the patch's decision through the mod
                // matrix. The mod wheel is CC 1 and nothing else is given a
                // meaning here: a controller the matrix has no source for is
                // dropped rather than invented onto something, which is the
                // same rule the plugin host follows for a CLAP-only plugin.
                fontelle_types::EventPayload::Controller { controller, value } => {
                    if *controller == MOD_WHEEL_CC {
                        self.sampler.set_mod_wheel(f32::from(*value) / 127.0);
                    }
                }
                fontelle_types::EventPayload::PitchBend { value } => {
                    // MIDI's own asymmetry, undone: a full bend down is
                    // 8192 steps and a full bend up is 8191, and both are
                    // meant to reach the same interval.
                    let span = if *value < 0 { 8192.0 } else { 8191.0 };
                    self.sampler.set_pitch_bend(f32::from(*value) / span);
                }
                fontelle_types::EventPayload::ChannelPressure { value } => {
                    self.sampler.set_aftertouch(f32::from(*value) / 127.0);
                }
                // A channel's own level and placement, under automation
                // (§12.2). Block-rate, like the mixer track's and for the same
                // reason: the last value in the block wins, which is finer
                // than a hand moves and a fraction of the cost of applying one
                // per sample.
                //
                // Matching on the tail of the address is not a search: the
                // compiler already resolved it to *this* node, so the only
                // question left is which of this channel's two controls it
                // names.
                fontelle_types::EventPayload::ParamValue { target, value } => {
                    let value = *value as f32;
                    let address = target.as_str();
                    // A knob **inside** the instrument, addressed as
                    // `channel:<id>/patch/...` — the panel's own address for
                    // it, kept whole. Everything from `patch/` onward is what
                    // `patch_params` reads, and taking a subslice of the
                    // address rather than building a string is what keeps this
                    // allocation-free (INVARIANT 1).
                    if let Some(at) = address.find("/patch/") {
                        self.sampler.set_patch_param(&address[at + 1..], value);
                    } else if address.ends_with("/gain") {
                        self.sampler.set_gain_db(
                            CHANNEL_GAIN_MIN_DB
                                + value * (CHANNEL_GAIN_MAX_DB - CHANNEL_GAIN_MIN_DB),
                        );
                    } else if address.ends_with("/pan") {
                        self.sampler.set_pan(value * 2.0 - 1.0);
                    }
                }
                _ => {}
            }
        }
        // Where the transport is, handed down before the render so that a
        // synced LFO's rate follows a tempo change and a free-running one's
        // phase follows the position (§3.5). Both numbers are already on the
        // context; this is only carrying them one level further in.
        self.sampler.set_clock(fontelle_core::RenderClock {
            bpm: ctx.transport.bpm,
            position_sample: ctx.transport.position_sample.max(0) as u64,
        });

        // Rendered into scratch and added, not written straight to the bus:
        // several instruments share one output, and `Sampler::render` clears
        // what it is given because that is the contract a plugin host expects
        // of `fontelle-core`'s boundary (TDD §8.1). The scratch buffers are
        // allocated in `prepare`, never here (INVARIANT 1).
        //
        // The sampler renders as many channels as the bus has, up to a stereo
        // pair, so a panned layer arrives placed rather than centred. A bus
        // wider than two gets the pair fanned across it — which is what
        // feeding a stereo source into a wider bus means, and is the only
        // thing this node can honestly do until surround is a real feature.
        let frames = ctx.outputs.first().map_or(0, |o| o.len());
        let frames = frames.min(self.scratch_frames);
        let channels = ctx.outputs.len().min(MAX_CHANNELS);
        let (first, second) = self.scratch.split_at_mut(self.scratch_frames);
        let mut rendered: [&mut [f32]; MAX_CHANNELS] =
            [&mut first[..frames], &mut second[..frames]];
        self.sampler.render(&self.store, &mut rendered[..channels]);
        // After the render, so the count is what actually sounded in this
        // block rather than what was asked for before it: a note-on that ran
        // out of voices is not a voice.
        if let Some(meter) = &self.meter {
            meter.set_voices(self.sampler.active_voices());
            meter.set_lfo_phases(self.sampler.newest_lfo_phases());
        }

        // **The instrument's own chain**, on the node's scratch pair — after
        // the voice sum, before the channel's gain and before anything else
        // on this bus is added to it.
        //
        // Only zero-latency kinds are offered (§3.9), so `latency_samples`
        // stays 0 and nothing has to be compensated: a plugin *instrument*
        // with latency is the one case §5.5 deliberately does not compensate,
        // and Flopsynth is an instrument.
        //
        // Nothing here allocates. The states were built in `prepare`, the dry
        // buffers were sized there, and the configs are read straight off the
        // sampler's live patch — which is what makes a knob on `patch/fx[1]/mix`
        // heard in the same block it arrives.
        if !self.fx.is_empty() {
            let bpm = ctx.transport.bpm;
            // Whether the matrix reaches the chain at all (`ModDest::FxParam`,
            // `docs/flopsynth-next.md` §4.2), asked once a block.
            let modulated = self.sampler.modulates_fx();
            for index in 0..self.fx.len() {
                let Some(slot) = self.sampler.patch().fx.get(index) else {
                    continue;
                };
                if !slot.enabled {
                    if let Some(meter) = &self.meter {
                        meter.set_fx_level(index, 0.0);
                    }
                    continue;
                }
                let mut config = slot.config;
                // A route to one of this effect's knobs moves a **copy** of
                // the config, the way a route to a cutoff moves a copy of
                // the filter's: the patch is what the document holds, and
                // the knob stays where it was set. Resolved per block, like
                // the chain runs.
                if modulated && let Ok(slot_index) = u8::try_from(index) {
                    for (param, spec) in slot.config.specs().iter().enumerate() {
                        let Ok(param_index) = u8::try_from(param) else {
                            break;
                        };
                        let amount = self.sampler.fx_modulation(slot_index, param_index);
                        if amount != 0.0
                            && let Some(base) = config.normalised(spec.id)
                        {
                            config.set_normalised(spec.id, (base + amount).clamp(0.0, 1.0));
                        }
                    }
                }
                let mix = config.mix().clamp(0.0, 1.0);
                // Taken only when the mix asks for it: a fully wet slot must
                // cost exactly what it did before this control existed.
                let blending = mix < 1.0;
                if blending {
                    for (dry, wet) in self.fx_dry.iter_mut().zip(rendered.iter()).take(channels) {
                        dry[..frames].copy_from_slice(&wet[..frames]);
                    }
                }
                if let Some(Some(state)) = self.fx.get_mut(index) {
                    state.process(
                        &mut rendered[..channels],
                        None,
                        fontelle_fx::NoteInput::default(),
                        &config,
                        bpm,
                    );
                }
                if blending {
                    for (wet, dry) in rendered.iter_mut().zip(self.fx_dry.iter()).take(channels) {
                        for (sample, was) in wet[..frames].iter_mut().zip(dry[..frames].iter()) {
                            // A gain each rather than a crossfade law, for the
                            // reason `EffectNode` gives: parallel processing is
                            // a *sum*, and an equal-power curve would make a
                            // fully dry slot louder than the wire it claims to
                            // be.
                            *sample = *sample * mix + *was * (1.0 - mix);
                        }
                    }
                }
                // The slot's meter (§3.6): the block's peak after it ran.
                // One pass over the block per slot, the cost a track's peak
                // meter already pays.
                if let Some(meter) = &self.meter {
                    let peak = rendered[..channels]
                        .iter()
                        .flat_map(|channel| channel[..frames].iter())
                        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
                    meter.set_fx_level(index, peak);
                }
            }
        }
        // And nothing for the slots past the chain's end, so a slot taken
        // off does not leave its last level on the rack.
        if let Some(meter) = &self.meter {
            for index in self.fx.len()..fontelle_core::MAX_PATCH_FX {
                meter.set_fx_level(index, 0.0);
            }
        }

        // What the instrument put out, for the sky: after its chain, before
        // it is summed into whatever else is on the bus.
        if let Some(scope) = &self.scope {
            scope.write(&rendered[..channels]);
        }

        for (index, channel) in ctx.outputs.iter_mut().enumerate() {
            let source = &rendered[index.min(channels.saturating_sub(1))];
            for (out, sample) in channel[..frames].iter_mut().zip(source.iter()) {
                *out += *sample;
            }
        }
    }

    fn reset(&mut self) {
        // A hard cut, not a release: a release tail from before a seek would
        // play over the top of wherever playback landed — and the chain's
        // repeats and tail go with it, for the same reason.
        self.sampler.reset();
        self.scratch.fill(0.0);
        for slot in self.fx.iter_mut().flatten() {
            slot.reset_state();
        }
    }

    fn reset_sequenced(&mut self) {
        // Transport stop and seek. Same hard cut, but only for the voices the
        // timeline started — the ones a player is holding belong to them, and
        // stop is a statement about the sequencer.
        self.sampler.reset_sequenced();
        self.scratch.fill(0.0);
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// What an insert of this configuration will delay its track by, in samples
/// (TDD §5.5).
///
/// The **document's** answer, so the graph builder can line tracks up before
/// it has built a single node — and the same rule
/// [`EffectNode::latency_samples`] gives once it has, because both come
/// through here. A bypassed slot costs nothing, which is what the node does
/// too.
///
/// Two effects have a latency: the gate's is its look-ahead knob, and the
/// corrector's is a **function of its range and its mode** — see
/// [`max_insert_latency_samples`], which is where the difference between the
/// two matters.
pub fn insert_latency_samples(config: &fontelle_types::EffectConfig, sample_rate: f32) -> u32 {
    match config {
        fontelle_types::EffectConfig::Gate(gate) => {
            let ms = gate
                .lookahead_ms
                .clamp(0.0, fontelle_types::MAX_GATE_LOOKAHEAD_MS);
            (ms / 1000.0 * sample_rate.max(0.0)).round() as u32
        }
        fontelle_types::EffectConfig::Tune(tune) => tune.latency_samples(sample_rate),
        // The limiter's look-ahead is fixed, so its latency is one constant
        // rather than a knob — but it is a latency all the same, and lining the
        // rest of the mix up to it is what keeps a ducked track in time.
        fontelle_types::EffectConfig::Limiter(_) => limiter_latency_samples(sample_rate),
        _ => 0,
    }
}

/// The limiter's fixed look-ahead in samples — its latency, and the size of its
/// delay line. One place, because [`insert_latency_samples`],
/// [`max_insert_latency_samples`] and the buffer it sizes all have to agree.
fn limiter_latency_samples(sample_rate: f32) -> u32 {
    (fontelle_types::LIMITER_LOOKAHEAD_MS / 1000.0 * sample_rate.max(0.0)).round() as u32
}

/// The document limiter's settings as the DSP's, with the fixed look-ahead put
/// on. The ceiling is stored in dB and the DSP wants it linear.
fn fx_limiter_config(config: &fontelle_types::LimiterConfig) -> fontelle_fx::LimiterConfig {
    fontelle_fx::LimiterConfig {
        ceiling: 10f32.powf(config.ceiling_db / 20.0),
        lookahead_ms: fontelle_types::LIMITER_LOOKAHEAD_MS,
        release_ms: config.release_ms,
    }
}

/// The **most** an insert of this kind could ever ask for, in samples.
///
/// What [`EffectNode::prepare`] sizes its dry line from, and it has to be the
/// worst case rather than the current setting for the reason the gate sizes
/// its own line that way: moving the knob mid-song must be a change of read
/// offset and not a reallocation on the audio thread (INVARIANT 1).
///
/// Its own function beside [`insert_latency_samples`] rather than a `match`
/// inside `prepare`, because there are now two effects with a latency and
/// their worst cases are different shapes — a knob's top for one, the widest
/// range in the deeper mode for the other. Two copies of that would be one to
/// forget the next time a third effect looks ahead.
pub fn max_insert_latency_samples(config: &fontelle_types::EffectConfig, sample_rate: f32) -> u32 {
    match config {
        fontelle_types::EffectConfig::Gate(_) => {
            (fontelle_types::MAX_GATE_LOOKAHEAD_MS / 1000.0 * sample_rate.max(0.0)).ceil() as u32
        }
        // Fixed, so the worst case is the only case.
        fontelle_types::EffectConfig::Limiter(_) => limiter_latency_samples(sample_rate),
        fontelle_types::EffectConfig::Tune(tune) => {
            // The range and the mode are what set it, and a change of either
            // is a graph rebuild (`docs/tune-plan.md` §3.8) — but the live
            // wire can still carry one through before the rebuild lands, so
            // the line is sized for the widest.
            let widest = fontelle_types::TuneConfig {
                range: fontelle_types::TuneRange::Low,
                mode: fontelle_types::TuneMode::Studio,
                ..*tune
            };
            widest.latency_samples(sample_rate)
        }
        _ => 0,
    }
}

/// A fixed number of samples of nothing, on a path that arrives too early
/// (TDD §5.5).
///
/// **Not an effect.** There is no feedback, no mix and nothing to set: a
/// musical delay is `fontelle_fx::Delay` and lives in an insert. This is the
/// compensator the graph builder puts on a track whose siblings cost more
/// than it does, so that everything summing into one bus arrives together.
/// A track carrying a look-ahead gate is late by the look-ahead; without
/// this, everything *else* is early, which sounds like a loose player rather
/// than like a bug.
///
/// In place on the bus it is given, like an insert, and it reports its own
/// latency so that a compensator is never itself compensated for by mistake.
pub struct DelayNode {
    /// How far back the read head sits, in frames.
    samples: u32,
    /// The line: `MAX_CHANNELS` to a frame, laid out flat so one write index
    /// serves every channel — the same shape `fontelle_fx::Gate` uses for the
    /// same job.
    line: Vec<f32>,
    /// Frames in `line`; zero until `prepare`.
    capacity: usize,
    write: usize,
}

impl DelayNode {
    pub fn new(samples: u32) -> Self {
        Self {
            samples,
            line: Vec::new(),
            capacity: 0,
            write: 0,
        }
    }

    /// How far it holds the signal back.
    pub fn samples(&self) -> u32 {
        self.samples
    }
}

impl AudioNode for DelayNode {
    fn prepare(&mut self, _ctx: &PrepareContext) {
        // Sized from what it was built for, plus the slot the write head
        // occupies: a delay of `n` needs `n + 1` frames, so that the sample
        // being written and the one being read are never the same slot
        // unless `n` is zero — which is exactly when they should be.
        self.capacity = self.samples as usize + 1;
        self.line = vec![0.0; self.capacity * MAX_CHANNELS];
        self.write = 0;
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        if self.samples == 0 || self.capacity == 0 {
            return;
        }
        let frames = ctx.outputs.first().map_or(0, |buffer| buffer.len());
        let channels = ctx.outputs.len().min(MAX_CHANNELS);
        let delay = (self.samples as usize).min(self.capacity - 1);
        for frame in 0..frames {
            let read = (self.write + self.capacity - delay) % self.capacity;
            for channel in 0..channels {
                let slot = self.write * MAX_CHANNELS + channel;
                self.line[slot] = ctx.outputs[channel][frame];
                ctx.outputs[channel][frame] = self.line[read * MAX_CHANNELS + channel];
            }
            self.write = (self.write + 1) % self.capacity;
        }
    }

    fn reset(&mut self) {
        self.line.fill(0.0);
        self.write = 0;
    }

    fn latency_samples(&self) -> u32 {
        self.samples
    }

    fn debug_name(&self) -> &'static str {
        "delay-compensation"
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// Wraps one `fontelle-fx` effect. Which effect is behind `Box<dyn ...>` is decided
/// when the node is built from the document's `EffectSlot` — `fontelle-fx` itself
/// exposes no shared trait, since it must not depend on this crate (TDD §4.1).
pub struct EffectNode {
    /// The DSP, one variant per [`EffectKind`]. A `Box<dyn>` was the shape
    /// this was drawn with and an enum is what it wants: v1's effects ship in
    /// the binary and are known here, so the vtable would buy nothing but an
    /// indirection on the audio thread. M2's plugin hosting adds the boxed
    /// variant beside these rather than replacing them.
    ///
    /// [`EffectKind`]: fontelle_types::EffectKind
    state: EffectState,
    /// What it is set to right now. Seeded from the document at build time and
    /// replaced by whatever arrives on `controls`.
    config: fontelle_types::EffectConfig,
    bypassed: bool,
    /// The live end, when something off the audio thread is holding the other
    /// side of it — see [`EffectControls`]. `None` leaves `config` in charge,
    /// which is what an offline render wants.
    controls: Option<crate::EffectSource>,
    /// Parameters an automation lane has taken over, by their position in
    /// [`EffectConfig::specs`], holding the last normalised value each was
    /// given.
    ///
    /// **It persists across blocks**, which is TDD §12.2's second rule: after
    /// an automation clip ends the parameter holds the value it left rather
    /// than snapping back to where the knob is. Applied *after* the live
    /// channel is read, so automation wins over the knob while it has an
    /// opinion — which is the same rule, seen from the other side.
    ///
    /// A fixed array, not a map: this is the audio thread (INVARIANT 1). Sized
    /// for the largest parameter list any effect has.
    automated: [Option<f32>; MAX_EFFECT_PARAMS],
    /// The bus as it arrived, kept while the effect has the real one.
    ///
    /// One `Vec` per channel, sized in [`prepare`](AudioNode::prepare) and
    /// never resized after: this is the audio thread, and a dry/wet control
    /// that allocated per block would be a control nobody could use
    /// (INVARIANT 1). Empty until prepared, and a block that finds it too
    /// small runs fully wet rather than reaching for the heap.
    dry: Vec<Vec<f32>>,
    /// The dry path's own **delay line**, for an insert that looks ahead.
    ///
    /// > *"any lookahead insert under a mix below 100 % combs against an
    /// > undelayed dry"*
    ///
    /// A gate with look-ahead delays what it outputs so its decision is
    /// already in force when the transient arrives (`fontelle_fx::Gate`).
    /// The dry the mix control blends back in is the block as it *arrived*,
    /// so summing the two is a comb filter — not half the effect, a
    /// different effect, and on a gate doing nothing at all it is audible
    /// where it should be silent. The dry is delayed by the same look-ahead
    /// here, which makes a fully open gate the wire it claims to be at any
    /// mix.
    ///
    /// Flat, `DRY_CHANNELS` to a frame, like the gate's own line. Empty for
    /// an effect that cannot look ahead, which is every one but the gate —
    /// they pay nothing for this.
    dry_line: Vec<f32>,
    /// Where the next frame is written in `dry_line`, in frames.
    dry_write: usize,
    /// How many frames `dry_line` holds.
    dry_capacity: usize,
    /// What `prepare` was told, so this node can answer what it costs
    /// without being asked in the middle of a block — see
    /// [`EffectNode::configured_latency`].
    sample_rate: f32,
    /// Where the analyser reads its samples from, when a window is showing
    /// one. `None` is the ordinary case and costs nothing — see
    /// [`crate::SpectrumTap`].
    tap: Option<std::sync::Arc<crate::SpectrumTap>>,
    /// The **external sidechain**: another track's bus, left there by a
    /// [`crate::KeyTapNode`] that the compiler scheduled first
    /// (`docs/effects-catalogue.md` §2.1). `None` on every insert that has no
    /// detector or has not been given a key, which is nearly all of them, and
    /// then this costs one branch a block.
    key: Option<std::sync::Arc<crate::KeyTap>>,
    /// Where the key is copied to, sized in [`prepare`](AudioNode::prepare)
    /// and never resized: reading it into a fresh `Vec` per block would be an
    /// allocation on the audio thread (INVARIANT 1).
    key_buffer: Vec<f32>,
    /// The **notes**: the node whose part this insert listens to
    /// (`docs/tune-plan.md` §5.2). `None` on every insert that does not take
    /// notes or has not been given a channel, which is nearly all of them, and
    /// then this costs one branch a block.
    ///
    /// The source node's *own* events are read, rather than a copy being
    /// routed here. `ProcessContext::all_events` warns that a node reading it
    /// will play other instruments' parts — and this node is **configured** to
    /// listen to one other node's part, which is the intent. The alternative,
    /// teaching the compiler to emit a second copy of every note for every
    /// listener and teaching the live source to fan out, touches the
    /// sequencer, the live path and the router for the same result. Both nodes
    /// see the same block's slices whatever order they run in, so there is no
    /// ordering hazard — which is the hazard the key tap has and this does not.
    notes_from: Option<fontelle_types::NodeId>,
    /// What is held down on that node right now.
    held: HeldKeys,
    /// And its pitch bend, in cents.
    bend_cents: f32,
    /// Where the window reads the pitch trace, when one is open — the
    /// analyser tap's sibling, and `None` costs nothing.
    tune_tap: Option<std::sync::Arc<crate::TuneTap>>,
}

/// The keys held on the channel an insert listens to, last-note first out.
///
/// A fixed array rather than a `Vec`: this is written on the audio thread
/// (INVARIANT 1). Sixteen is more than a hand, and the seventeenth pushes the
/// **oldest** out rather than being refused — the newest is the one somebody
/// just played, and a tuner that ignored it would look broken.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeldKeys {
    keys: [u8; MAX_HELD_KEYS],
    len: usize,
}

/// How many keys one insert remembers being held.
pub const MAX_HELD_KEYS: usize = 16;

impl HeldKeys {
    pub fn press(&mut self, key: u8) {
        self.release(key);
        if self.len == MAX_HELD_KEYS {
            self.keys.copy_within(1.., 0);
            self.len -= 1;
        }
        self.keys[self.len] = key;
        self.len += 1;
    }

    pub fn release(&mut self, key: u8) {
        let Some(at) = self.keys[..self.len].iter().position(|held| *held == key) else {
            return;
        };
        self.keys.copy_within(at + 1..self.len, at);
        self.len -= 1;
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Oldest first — the order they went down in.
    pub fn as_slice(&self) -> &[u8] {
        &self.keys[..self.len]
    }

    /// The last one pressed and not let go of.
    pub fn last(&self) -> Option<u8> {
        (self.len > 0).then(|| self.keys[self.len - 1])
    }

    /// Every held key's pitch class, bit 0 = C.
    pub fn mask(&self) -> u16 {
        self.keys[..self.len]
            .iter()
            .fold(0u16, |mask, key| mask | 1 << (key % 12))
    }
}

/// How many channels the dry copy has room for. A mixer bus is stereo.
const DRY_CHANNELS: usize = 2;

/// The most parameters an effect may expose. The EQ has 48.
const MAX_EFFECT_PARAMS: usize = 64;

/// The DSP behind an [`EffectNode`].
///
/// The variants differ in size — the EQ carries sixty-four filters and the
/// compressor three floats — and that is the right shape here rather than a
/// reason to box one: there is one of these per insert, a handful per project,
/// each built once off the audio thread and then read every block. Boxing
/// would trade five hundred bytes that never add up for an indirection on the
/// hot path.
#[allow(clippy::large_enum_variant)]
enum EffectState {
    Utility(fontelle_fx::Utility),
    Eq(fontelle_fx::ParametricEq),
    Filter(fontelle_fx::Filter),
    Compressor(fontelle_fx::Compressor),
    Limiter(fontelle_fx::Limiter),
    Gate(fontelle_fx::Gate),
    Distortion(fontelle_fx::Distortion),
    Bitcrush(fontelle_fx::Bitcrush),
    Soften(fontelle_fx::Soften),
    Chorus(fontelle_fx::Chorus),
    Delay(fontelle_fx::Delay),
    Reverb(fontelle_fx::FdnReverb),
    Tune(fontelle_fx::Tune),
}

impl EffectState {
    /// The DSP one config asks for.
    ///
    /// Its own function rather than a `match` inside `EffectNode::new`,
    /// because there is now a **second** place that builds a chain: a patch
    /// carries its own effects (`docs/flopsynth-plan.md` §2.2), and a
    /// `SamplerNode` runs them after the voice sum. Two copies of this match
    /// would be two lists to keep in step, which is the defect §8.2 exists to
    /// prevent one level up.
    fn for_config(config: &fontelle_types::EffectConfig) -> Self {
        match config {
            fontelle_types::EffectConfig::Utility(_) => {
                EffectState::Utility(fontelle_fx::Utility::new())
            }
            fontelle_types::EffectConfig::Eq(_) => {
                EffectState::Eq(fontelle_fx::ParametricEq::new())
            }
            fontelle_types::EffectConfig::Filter(_) => {
                EffectState::Filter(fontelle_fx::Filter::new())
            }
            fontelle_types::EffectConfig::Compressor(_) => {
                EffectState::Compressor(fontelle_fx::Compressor::new())
            }
            fontelle_types::EffectConfig::Limiter(_) => {
                EffectState::Limiter(fontelle_fx::Limiter::new())
            }
            fontelle_types::EffectConfig::Gate(_) => EffectState::Gate(fontelle_fx::Gate::new()),
            fontelle_types::EffectConfig::Distortion(_) => {
                EffectState::Distortion(fontelle_fx::Distortion::new())
            }
            fontelle_types::EffectConfig::Bitcrush(_) => {
                EffectState::Bitcrush(fontelle_fx::Bitcrush::new())
            }
            fontelle_types::EffectConfig::Soften(_) => {
                EffectState::Soften(fontelle_fx::Soften::new())
            }
            fontelle_types::EffectConfig::Chorus(_) => {
                EffectState::Chorus(fontelle_fx::Chorus::new())
            }
            fontelle_types::EffectConfig::Delay(_) => EffectState::Delay(fontelle_fx::Delay::new()),
            fontelle_types::EffectConfig::Reverb(_) => {
                EffectState::Reverb(fontelle_fx::FdnReverb::new())
            }
            fontelle_types::EffectConfig::Tune(_) => EffectState::Tune(fontelle_fx::Tune::new()),
        }
    }

    /// The config is a parameter because one effect needs it here: the
    /// corrector's rings are sized from its range and its hop from its mode
    /// (`docs/tune-plan.md` §3.8), and neither can be resized on the audio
    /// thread. That is why a change of either is a graph rebuild — the
    /// rebuild is what calls this again.
    fn prepare(&mut self, sample_rate: f32, config: &fontelle_types::EffectConfig) {
        match self {
            Self::Tune(tune) => {
                if let fontelle_types::EffectConfig::Tune(config) = config {
                    tune.prepare(sample_rate, config);
                }
            }
            Self::Utility(utility) => utility.prepare(sample_rate),
            Self::Eq(eq) => eq.prepare(sample_rate),
            Self::Filter(filter) => filter.prepare(sample_rate),
            Self::Compressor(comp) => comp.prepare(sample_rate),
            Self::Limiter(limiter) => {
                if let fontelle_types::EffectConfig::Limiter(config) = config {
                    limiter.prepare(sample_rate, &fx_limiter_config(config));
                }
            }
            Self::Gate(gate) => gate.prepare(sample_rate),
            Self::Distortion(dist) => dist.prepare(sample_rate),
            Self::Bitcrush(crush) => crush.prepare(sample_rate),
            Self::Soften(soften) => soften.prepare(sample_rate),
            Self::Chorus(chorus) => chorus.prepare(sample_rate),
            Self::Delay(delay) => delay.prepare(sample_rate),
            Self::Reverb(reverb) => reverb.prepare(sample_rate),
        }
    }

    /// One block, in place on the buffers it is given.
    ///
    /// `key` is the external sidechain, which only the two detector effects
    /// read; `bpm` is the tempo, which the three that can be set in note
    /// values read every block so that they follow a tempo *change*.
    fn process(
        &mut self,
        outputs: &mut [&mut [f32]],
        key: Option<&[f32]>,
        notes: fontelle_fx::NoteInput,
        config: &fontelle_types::EffectConfig,
        bpm: f32,
    ) {
        match (self, config) {
            (Self::Tune(tune), fontelle_types::EffectConfig::Tune(config)) => {
                tune.process(outputs, notes, config, bpm);
            }
            (Self::Utility(utility), fontelle_types::EffectConfig::Utility(config)) => {
                utility.process(outputs, config);
            }
            (Self::Eq(eq), fontelle_types::EffectConfig::Eq(config)) => {
                eq.process(outputs, config);
            }
            (Self::Filter(filter), fontelle_types::EffectConfig::Filter(config)) => {
                filter.process(outputs, config, bpm);
            }
            (Self::Compressor(comp), fontelle_types::EffectConfig::Compressor(config)) => {
                comp.process(outputs, key, config);
            }
            (Self::Limiter(limiter), fontelle_types::EffectConfig::Limiter(config)) => {
                // The key is the ducker's whole point: given one, the limiter
                // measures it instead of the signal, so the kick pushes this
                // track down under the ceiling.
                limiter.process(outputs, key, &fx_limiter_config(config));
            }
            (Self::Gate(gate), fontelle_types::EffectConfig::Gate(config)) => {
                gate.process(outputs, key, config);
            }
            (Self::Distortion(dist), fontelle_types::EffectConfig::Distortion(config)) => {
                dist.process(outputs, config);
            }
            // The delay and the reverb write their repeats and their tail,
            // with none of the signal that caused them: the blend the caller
            // does is what puts the track back under it, and an effect that
            // mixed its own dry in would be mixed in twice. It is also why
            // these two open part dry — see `EffectKind::is_time_based`.
            (Self::Bitcrush(crush), fontelle_types::EffectConfig::Bitcrush(config)) => {
                crush.process(outputs, config);
            }
            (Self::Soften(soften), fontelle_types::EffectConfig::Soften(config)) => {
                soften.process(outputs, config);
            }
            (Self::Chorus(chorus), fontelle_types::EffectConfig::Chorus(config)) => {
                chorus.process(outputs, config, bpm);
            }
            (Self::Delay(delay), fontelle_types::EffectConfig::Delay(config)) => {
                delay.process(outputs, config, bpm);
            }
            (Self::Reverb(reverb), fontelle_types::EffectConfig::Reverb(config)) => {
                reverb.process(outputs, config);
            }
            // A config of a different kind than the state cannot arrive: the
            // chain rebuilds the graph when a slot's *kind* changes, and only
            // tunes it in place when parameters move.
            _ => {}
        }
    }

    /// Back to silence.
    ///
    /// **Transport stop drops the repeats and the tail**, and the chorus's
    /// voices with them: a delay still ringing across a seek would play the
    /// bar you left behind over the one you jumped to.
    fn reset_state(&mut self) {
        match self {
            Self::Utility(utility) => utility.reset(),
            Self::Eq(eq) => eq.reset(),
            Self::Filter(filter) => filter.reset(),
            Self::Compressor(comp) => comp.reset(),
            Self::Limiter(limiter) => limiter.reset(),
            Self::Gate(gate) => gate.reset(),
            Self::Distortion(dist) => dist.reset(),
            Self::Bitcrush(crush) => crush.reset(),
            Self::Soften(soften) => soften.reset(),
            Self::Chorus(chorus) => chorus.reset(),
            Self::Delay(delay) => delay.reset(),
            Self::Reverb(reverb) => reverb.reset(),
            Self::Tune(tune) => tune.reset(),
        }
    }
}

impl EffectNode {
    /// An insert set up as the document says, with no live end.
    pub fn new(config: fontelle_types::EffectConfig) -> Self {
        Self {
            state: EffectState::for_config(&config),
            config,
            bypassed: false,
            controls: None,
            automated: [None; MAX_EFFECT_PARAMS],
            dry: Vec::new(),
            dry_line: Vec::new(),
            dry_write: 0,
            dry_capacity: 0,
            sample_rate: 0.0,
            tap: None,
            key: None,
            key_buffer: Vec::new(),
            notes_from: None,
            held: HeldKeys::default(),
            bend_cents: 0.0,
            tune_tap: None,
        }
    }

    /// Gives this insert a live end, so a knob can move without the graph
    /// being rebuilt underneath it.
    ///
    /// The source carries the *initial* config as well as later ones, so a
    /// node with a live end reads one source of truth rather than two that
    /// agree until they do not.
    pub fn with_controls(mut self, controls: crate::EffectSource) -> Self {
        self.controls = Some(controls);
        self
    }

    /// Gives this insert an analyser tap, so an editor can draw the spectrum
    /// arriving at it.
    ///
    /// **What arrives**, not what leaves: an EQ's curve is drawn over the
    /// signal you are shaping, so a cut you have just made must leave a
    /// visible dip in the *curve* against an unchanged spectrum, rather than
    /// flattening the spectrum and leaving nothing to aim at.
    pub fn with_spectrum(mut self, tap: std::sync::Arc<crate::SpectrumTap>) -> Self {
        self.tap = Some(tap);
        self
    }

    /// Gives this insert an **external key**: another track's bus, which its
    /// detector listens to instead of the signal passing through it
    /// (`docs/effects-catalogue.md` §2.1, TDD §13.4).
    ///
    /// The tap is filled by a [`crate::KeyTapNode`] the compiler scheduled on
    /// the source track, and the compiler is what guarantees that node runs
    /// first — a key is a feeding edge like a send. On an effect with no
    /// detector this is carried and never read, which is why the document
    /// answers "is there an edge" through `EffectSlot::effective_key` rather
    /// than through the field.
    pub fn with_key(mut self, key: std::sync::Arc<crate::KeyTap>) -> Self {
        self.key = Some(key);
        self
    }

    /// Gives this insert a **channel's notes**: the melody to force, or the
    /// scale to allow (`docs/tune-plan.md` §5).
    ///
    /// A routing edge like the key, and set the same way — the document names
    /// a channel, `realise` turns that into the channel's node, and a rebuild
    /// rewires it along with everything else. Carried and never read on an
    /// effect that does not take notes, which is why the document answers "is
    /// there an edge" through `EffectSlot::effective_notes`.
    pub fn with_notes_from(mut self, source: fontelle_types::NodeId) -> Self {
        self.notes_from = Some(source);
        self
    }

    /// Gives this insert a pitch-trace tap, so a window can draw what it is
    /// doing to the note (`docs/tune-plan.md` §7.3).
    pub fn with_tune_tap(mut self, tap: std::sync::Arc<crate::TuneTap>) -> Self {
        self.tune_tap = Some(tap);
        self
    }

    /// What is held on the channel this insert listens to, oldest first.
    pub fn held_keys(&self) -> &[u8] {
        self.held.as_slice()
    }

    /// The last key pressed there and not let go of.
    pub fn last_key(&self) -> Option<u8> {
        self.held.last()
    }

    /// Reads this block's notes off the node this insert listens to.
    ///
    /// Before the DSP and before the bypass: a bypassed tuner that came back
    /// having forgotten which key was down would force the wrong note for as
    /// long as it was held.
    fn take_notes(&mut self, ctx: &ProcessContext) {
        let Some(source) = self.notes_from else {
            return;
        };
        for event in ctx.all_events.iter().chain(ctx.live_events.iter()) {
            if event.target != source {
                continue;
            }
            match &event.payload {
                fontelle_types::EventPayload::NoteOn { key, velocity, .. } => {
                    // A note-on at zero velocity is a note-off, which is what
                    // half the MIDI hardware in the world sends.
                    if *velocity == 0 {
                        self.held.release(*key);
                    } else {
                        self.held.press(*key);
                    }
                }
                fontelle_types::EventPayload::NoteOff { key, .. } => self.held.release(*key),
                fontelle_types::EventPayload::PitchBend { value } => {
                    // MIDI's own asymmetry, undone, exactly as `SamplerNode`
                    // does it — and over the same two semitones every
                    // keyboard leaves the factory set to. There is no
                    // per-patch range to read here: a tuner listening to a
                    // channel is listening to the wheel, not to whatever
                    // instrument happens to be on it.
                    let span = if *value < 0 { 8_192.0 } else { 8_191.0 };
                    self.bend_cents = f32::from(*value) / span
                        * fontelle_core::DEFAULT_BEND_RANGE_SEMITONES
                        * 100.0;
                }
                _ => {}
            }
        }
    }

    pub fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    pub fn kind(&self) -> fontelle_types::EffectKind {
        self.config.kind()
    }

    /// Reads this block's automation into `automated`.
    ///
    /// Block-rate: the last value each parameter was given wins for the whole
    /// block. Sample-accurate application would mean rebuilding an EQ's
    /// coefficients part way through, which is the same trade the voice's mod
    /// matrix makes and settles the same way (§11.1's event list is
    /// sample-timestamped; what a node *does* with that is the node's).
    ///
    /// Matching by string is deliberate and is not a search: the address is
    /// already resolved to this node by the compiler, so the only question
    /// left is which of *this effect's* parameters it names.
    fn take_automation(&mut self, ctx: &ProcessContext) {
        for event in ctx.events() {
            let fontelle_types::EventPayload::ParamValue { target, value } = &event.payload else {
                continue;
            };
            let Some(param) = param_of(target.as_str()) else {
                continue;
            };
            if let Some(index) = self
                .config
                .specs()
                .iter()
                .take(MAX_EFFECT_PARAMS)
                .position(|spec| spec.id == param)
            {
                self.automated[index] = Some(*value as f32);
            }
        }
    }

    /// Writes what automation is holding onto the config the DSP reads.
    fn apply_automation(&mut self) {
        for index in 0..self.config.specs().len().min(MAX_EFFECT_PARAMS) {
            let Some(value) = self.automated[index] else {
                continue;
            };
            let id = self.config.specs()[index].id;
            self.config.set_normalised(id, value);
        }
    }

    /// Whether the scratch taken in `prepare` is big enough for this block.
    ///
    /// A block longer than the one prepared for cannot happen in this engine —
    /// the device fixes the size — and if it ever did, running the insert
    /// fully wet is the answer that stays on the audio thread.
    fn dry_fits(&self, ctx: &ProcessContext) -> bool {
        let frames = ctx.outputs.first().map_or(0, |buffer| buffer.len());
        self.dry.len() >= ctx.outputs.len().min(DRY_CHANNELS)
            && self.dry.iter().all(|buffer| buffer.len() >= frames)
    }

    /// What this insert delays the signal by, as it is **configured** —
    /// which is what [`AudioNode::latency_samples`] reports and what the dry
    /// path above is delayed by.
    ///
    /// A **bypassed** insert costs nothing, because a bypassed insert is a
    /// wire: it returns before the effect runs. That is also what
    /// `realise`'s compensation assumes, so the two agree — and it is why
    /// flicking the *live* bypass switch, which is published without a graph
    /// rebuild, moves this track by the look-ahead until the next rebuild.
    /// Written down rather than fixed: the alternative is a bypass that
    /// still delays, which is not a bypass.
    fn configured_latency(&self) -> u32 {
        if self.bypassed {
            return 0;
        }
        insert_latency_samples(&self.config, self.sample_rate)
    }

    /// Picks up anything the live end has published. Called once per block —
    /// a swap of two indices at worst, nothing at all when nothing has moved.
    fn settle(&mut self) {
        if let Some(controls) = &mut self.controls {
            let (config, bypassed) = controls.current();
            // A config of a different kind cannot arrive: the chain rebuilds
            // the graph when a slot's *kind* changes, and only tunes it in
            // place when the parameters move.
            self.config = config;
            self.bypassed = bypassed;
        }
    }
}

impl AudioNode for EffectNode {
    /// What this insert costs the track it is on, in samples (TDD §5.5).
    ///
    /// The gate's look-ahead is the only insert latency this build has, and
    /// it was reported as zero until the dry path had to be delayed by it —
    /// see [`EffectNode::configured_latency`], which is also why the live
    /// bypass does not change the answer.
    fn latency_samples(&self) -> u32 {
        self.configured_latency()
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        // The delay and the reverb allocate their lines here, which is the
        // whole reason `prepare` exists: two seconds of memory cannot be
        // reached for on the audio thread (INVARIANT 1).
        self.state.prepare(ctx.sample_rate, &self.config);
        // And room for the key, when there is one. Sized here for the same
        // reason the dry copy is: the audio thread cannot reach for memory.
        if self.key.is_some() {
            self.key_buffer = vec![0.0; ctx.max_block_size as usize];
        }
        // Room for the dry copy the mix control blends back in — see `dry`.
        // Stereo, which is what a mixer bus is; a node handed more channels
        // than this blends the ones it has room for and runs the rest wet,
        // which cannot happen in a graph this crate builds.
        let frames = ctx.max_block_size as usize;
        self.dry = (0..DRY_CHANNELS).map(|_| vec![0.0; frames]).collect();
        // And the dry's delay line, for the one effect that can look ahead.
        // Sized from the **most** look-ahead the control allows rather than
        // from where the knob happens to sit, so winding it up mid-song
        // reaches for nothing (INVARIANT 1) — the same reason the gate sizes
        // its own line that way.
        self.sample_rate = ctx.sample_rate;
        let most = max_insert_latency_samples(&self.config, ctx.sample_rate) as usize;
        self.dry_capacity = if most > 0 { most + 1 } else { 0 };
        self.dry_line = vec![0.0; self.dry_capacity * DRY_CHANNELS];
        self.dry_write = 0;
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.settle();
        self.take_automation(ctx);
        self.apply_automation();
        self.take_notes(ctx);
        // Before the bypass, and before the effect: what the analyser draws is
        // what is arriving here, which is true of a bypassed insert too — an
        // EQ you have switched off while you look for the frequency is exactly
        // when the picture matters most.
        if let Some(tap) = &self.tap {
            tap.write(ctx.outputs);
        }
        if self.bypassed {
            return;
        }
        // The signal as it arrived, before the effect is let at it. Taken only
        // when the mix asks for it: a fully wet insert — which is every insert
        // until somebody turns the knob — must cost exactly what it did before
        // this control existed.
        let mix = self.config.mix().clamp(0.0, 1.0);
        let blending = mix < 1.0 && self.dry_fits(ctx);
        if blending {
            for (channel, buffer) in ctx.outputs.iter().enumerate().take(DRY_CHANNELS) {
                self.dry[channel][..buffer.len()].copy_from_slice(buffer);
            }
        }
        // The dry, put through the same delay the effect will put the wet
        // through — see the `dry_line` field. Run whenever this insert has a
        // look-ahead at all, **whatever the mix says**, so that turning the
        // mix down mid-song blends against the signal that was flowing
        // rather than against whatever the line held when it was last used.
        let latency = self.configured_latency() as usize;
        if latency > 0 && self.dry_capacity > 0 {
            let frames = ctx.outputs.first().map_or(0, |buffer| buffer.len());
            let channels = ctx.outputs.len().min(DRY_CHANNELS);
            for frame in 0..frames {
                let read = (self.dry_write + self.dry_capacity - latency) % self.dry_capacity;
                for channel in 0..channels {
                    let slot = self.dry_write * DRY_CHANNELS + channel;
                    self.dry_line[slot] = ctx.outputs[channel][frame];
                    if blending {
                        self.dry[channel][frame] = self.dry_line[read * DRY_CHANNELS + channel];
                    }
                }
                self.dry_write = (self.dry_write + 1) % self.dry_capacity;
            }
        }
        // The key, if this insert has one. Read out here rather than inside
        // the arms below so that the borrow of `self.key_buffer` is settled
        // before `self.state` is borrowed mutably — and so that the frames
        // the source actually wrote bound it, which is what stops a short
        // block keying off the tail of the one before it.
        let key_frames = match &self.key {
            Some(tap) if !self.key_buffer.is_empty() => {
                let frames = ctx
                    .outputs
                    .first()
                    .map_or(0, |c| c.len())
                    .min(self.key_buffer.len());
                let filled = tap.read_into(&mut self.key_buffer[..frames]);
                Some(filled.min(frames))
            }
            _ => None,
        };
        let key = key_frames.map(|frames| &self.key_buffer[..frames]);

        // In place on the bus it was given: a chain is a run of inserts
        // scheduled on the same pair of buffers, in the order they run.
        let notes = fontelle_fx::NoteInput {
            last: self.held.last(),
            mask: self.held.mask(),
            bend_cents: self.bend_cents,
        };
        self.state
            .process(ctx.outputs, key, notes, &self.config, ctx.transport.bpm);

        // What the corrector did to the note, for whatever window is open on
        // it. After the effect, because the trace is what it *did*; skipped
        // entirely when nobody is looking, like the analyser's.
        if let (Some(tap), EffectState::Tune(tune)) = (&self.tune_tap, &self.state) {
            tap.write(tune.trace());
        }

        // And the two signals, blended. A gain each rather than a crossfade
        // law: an EQ blended half and half with the signal that went into it
        // has to be the *sum* of the two — that is what parallel processing
        // means — and an equal-power curve would make a fully dry insert
        // louder than the wire it is supposed to be.
        if blending {
            for (channel, buffer) in ctx.outputs.iter_mut().enumerate().take(DRY_CHANNELS) {
                let dry = &self.dry[channel];
                for (sample, was) in buffer.iter_mut().zip(dry.iter()) {
                    *sample = *sample * mix + *was * (1.0 - mix);
                }
            }
        }
    }

    fn reset(&mut self) {
        self.state.reset_state();
        // And the dry's own line, which holds the same audio the effect's
        // does: a reset that cleared one and not the other would blend the
        // signal from before the stop under the one after it.
        self.dry_line.fill(0.0);
        self.dry_write = 0;
        // And every key the source channel had down. A transport stop that
        // left one held would leave a tuner forcing that note forever, which
        // is the note-hang defect one level up.
        self.held.clear();
        self.bend_cents = 0.0;
        // A full stop lets go of what automation was holding: the next thing
        // played starts from the document, and a `ParamValue` will arrive to
        // say otherwise if the playhead is inside a clip.
        self.automated = [None; MAX_EFFECT_PARAMS];
    }

    fn debug_name(&self) -> &'static str {
        "EffectNode"
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// The range a track's fader covers, for automation's 0..1 scale.
///
/// The same numbers the mixer panel's groove uses, and they have to be: a lane
/// at half travel and a fader at half travel are the same statement about the
/// same control, and two ranges would make them two different levels.
pub const GAIN_MIN_DB: f32 = -60.0;
pub const GAIN_MAX_DB: f32 = 6.0;

/// The live half of a mixer track: what a fader writes and what a meter reads
/// (TDD §13.1, §13.3).
///
/// Shared with the RT thread as plain atomics, exactly like [`MasterMeter`] and
/// for the same reason — these are four scalars, not a waveform, and a relaxed
/// store per block is cheaper than a ring.
///
/// # Why a fader needs one at all
///
/// A control the user *drags* has to move the sound while it is moving, and has
/// to leave one undo entry behind when it stops. Those pull in opposite
/// directions: the sound comes from a `CompiledGraph` that costs a patch
/// deserialisation per channel to rebuild, and the undo entry comes from a
/// `Command` against the document. Rebuilding the graph per frame of a drag
/// would reload every soundfont in the project sixty times a second.
///
/// So a fader writes **both** — the command, for undo and for the file, and
/// this, for the sound between now and the next rebuild. The document stays the
/// source of truth (INVARIANT 9); a rebuild seeds a fresh set of these from it,
/// so the two cannot drift.
///
/// `mute` carries the *effective* mute — the track's own switch **or** a solo
/// elsewhere silencing it. Audibility under solo is a property of the whole
/// routing graph (see `fontelle_app::realise`), which is not something a node
/// can work out from where it sits.
#[derive(Debug)]
pub struct TrackControls {
    gain_db: std::sync::atomic::AtomicU32,
    pan: std::sync::atomic::AtomicU32,
    mute: std::sync::atomic::AtomicBool,
    peaks: [std::sync::atomic::AtomicU32; MAX_CHANNELS],
}

impl TrackControls {
    pub fn new(gain_db: f32, pan: f32, mute: bool) -> Self {
        Self {
            gain_db: std::sync::atomic::AtomicU32::new(gain_db.to_bits()),
            pan: std::sync::atomic::AtomicU32::new(pan.to_bits()),
            mute: std::sync::atomic::AtomicBool::new(mute),
            peaks: Default::default(),
        }
    }

    pub fn gain_db(&self) -> f32 {
        f32::from_bits(self.gain_db.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn set_gain_db(&self, value: f32) {
        self.gain_db
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn pan(&self) -> f32 {
        f32::from_bits(self.pan.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn set_pan(&self, value: f32) {
        self.pan
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn mute(&self) -> bool {
        self.mute.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_mute(&self, value: bool) {
        self.mute.store(value, std::sync::atomic::Ordering::Relaxed);
    }

    /// The highest peak since this was last called, per channel, and resets.
    /// Reading a value takes it — the reader is the one that knows when it has
    /// drawn what it read.
    pub fn take_peaks(&self) -> [f32; MAX_CHANNELS] {
        std::array::from_fn(|index| {
            f32::from_bits(self.peaks[index].swap(0, std::sync::atomic::Ordering::Relaxed))
        })
    }

    /// RT: a load, a compare and a store. Single writer, so the read-modify-
    /// write needs no compare-exchange loop.
    fn record(&self, index: usize, value: f32) {
        let Some(slot) = self.peaks.get(index) else {
            return;
        };
        let current = f32::from_bits(slot.load(std::sync::atomic::Ordering::Relaxed));
        if value > current {
            slot.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// A mixer track's fader stage (TDD §13.1): gain, pan, mute, phase invert.
///
/// **Processes in place.** Its buffers arrive already carrying the signal
/// feeding it (see `CompiledGraph::process_block`'s in-place convention), so
/// it reads, scales, and writes back the same buffers rather than copying
/// between an input and an output set. This is how an insert chain works in
/// every plugin API, and it's what lets the RT thread avoid both a copy and
/// the disjoint-borrow problem entirely.
///
/// Stereo when given two buffers (`[left, right]`), mono when given one — in
/// mono, `pan` has nowhere to go and is ignored rather than silently
/// half-attenuating the signal.
///
/// **Not yet:** inserts and sends (TDD §13.1's `inserts`/`sends`), metering
/// (§13.3), solo. Those are M4; this is the fader only, which is what the M0
/// gate's "→ mixer track →" actually requires.
pub struct MixerTrackNode {
    pub gain_db: f32,
    /// -1.0 hard left, 0.0 centre, +1.0 hard right.
    pub pan: f32,
    pub pan_law: PanLaw,
    pub mute: bool,
    pub phase_invert: bool,
    /// Where the gain, the pan and the mute actually come from, when something
    /// off the RT thread is holding the other end of them — see
    /// [`TrackControls`]. `None` leaves the fields above in charge, which is
    /// what an offline render and every direct construction of this node want.
    pub controls: Option<Arc<TrackControls>>,
    /// What automation is holding this track's gain and pan at, in dB and in
    /// `-1.0..=1.0`.
    ///
    /// It outranks both the fields above and the live controls while it has a
    /// value, and it keeps that value after the clip that set it has ended —
    /// TDD §12.2's second rule. `None` gives the fader back.
    pub automated_gain_db: Option<f32>,
    pub automated_pan: Option<f32>,
}

impl MixerTrackNode {
    /// Unity gain, centred, unmuted — a track that passes audio through
    /// unchanged.
    pub fn new() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            pan_law: PanLaw::Minus3Db,
            mute: false,
            phase_invert: false,
            controls: None,
            automated_gain_db: None,
            automated_pan: None,
        }
    }
}

impl Default for MixerTrackNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MixerTrackNode {
    /// The gain, pan and mute in force for this block: the live control
    /// surface's when there is one, and this node's own fields otherwise.
    ///
    /// Read once per block rather than per sample, so a fader moved mid-block
    /// takes effect at a block boundary. That is the same granularity the
    /// transport's own atomics have and is inaudible at 128 frames; making it
    /// per-sample would mean a ramp, which is §13.1's smoothing work and not
    /// this.
    fn settings(&self) -> (f32, f32, bool) {
        match &self.controls {
            Some(controls) => (controls.gain_db(), controls.pan(), controls.mute()),
            None => (self.gain_db, self.pan, self.mute),
        }
    }
}

impl AudioNode for MixerTrackNode {
    fn prepare(&mut self, _ctx: &PrepareContext) {}

    fn process(&mut self, ctx: &mut ProcessContext) {
        // Automation first, so what it is holding outranks the fader for this
        // block — §12.2, and the reason a knob under automation is drawn with
        // a different ring rather than pretending it is in charge.
        for event in ctx.events() {
            let fontelle_types::EventPayload::ParamValue { target, value } = &event.payload else {
                continue;
            };
            let value = *value as f32;
            if target.as_str().ends_with("/gain") {
                self.automated_gain_db = Some(GAIN_MIN_DB + value * (GAIN_MAX_DB - GAIN_MIN_DB));
            } else if target.as_str().ends_with("/pan") {
                self.automated_pan = Some(value * 2.0 - 1.0);
            }
        }
        let (gain_db, pan, mute) = self.settings();
        let gain_db = self.automated_gain_db.unwrap_or(gain_db);
        let pan = self.automated_pan.unwrap_or(pan);
        if mute {
            for channel in ctx.outputs.iter_mut() {
                channel.fill(0.0);
            }
            // Nothing is recorded, which is the same thing as recording zero:
            // `take_peaks` resets on every read, so a meter on a muted track
            // falls to silence rather than holding whatever it last saw.
            return;
        }

        let mut gain = 10f32.powf(gain_db / 20.0);
        if self.phase_invert {
            gain = -gain;
        }

        // Pan only means something with two channels to balance between. On a
        // mono track there's nowhere for it to go, so applying the centre
        // pan-law gain would just make every mono track quietly -3dB down.
        if ctx.outputs.len() == 2 {
            let (left_gain, right_gain) = self.pan_law.gains(pan);
            let (left, right) = ctx.outputs.split_at_mut(1);
            for sample in left[0].iter_mut() {
                *sample *= gain * left_gain;
            }
            for sample in right[0].iter_mut() {
                *sample *= gain * right_gain;
            }
        } else {
            for channel in ctx.outputs.iter_mut() {
                for sample in channel.iter_mut() {
                    *sample *= gain;
                }
            }
        }

        // Metered *after* the fader, for the reason `MasterNode` gives: what a
        // meter is for is telling you what you sent on, not what arrived.
        if let Some(controls) = &self.controls {
            for (index, channel) in ctx.outputs.iter().enumerate() {
                let peak = channel.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
                controls.record(index, peak);
            }
        }
    }

    fn reset(&mut self) {}

    fn debug_name(&self) -> &'static str {
        "mixer-track"
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// Adds one bus into another — the compiled form of `MixerTrack::output`
/// (TDD §13.1), which is how every track in the song reaches the master.
///
/// The only node in the graph whose inputs are a different set from its
/// outputs, and the reason `CompiledGraph::process_block` supports that shape
/// at all. It **adds** rather than overwrites, because a destination bus has
/// as many tracks arriving at it as the user routed there, and it leaves its
/// source untouched, because a bus may be routed *and* tapped by a send.
///
/// Stateless and parameterless on purpose: a send (§13.2) is this plus a level
/// and a pan, and that is M4 work along with the rest of the send system.
pub struct BusSumNode;

impl AudioNode for BusSumNode {
    fn prepare(&mut self, _ctx: &PrepareContext) {}

    fn process(&mut self, ctx: &mut ProcessContext) {
        for (source, dest) in ctx.inputs.iter().zip(ctx.outputs.iter_mut()) {
            for (sample, out) in source.iter().zip(dest.iter_mut()) {
                *out += *sample;
            }
        }
    }

    fn reset(&mut self) {}

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// One send's level and placement, shared with the RT thread.
///
/// Atomics, like [`TrackControls`] and for the same reason: a send level is
/// something somebody **drags**, and a drag has to be audible before the mouse
/// comes up. The document is still the source of truth (INVARIANT 9) — a
/// rebuild seeds a fresh set of these from it, so the two cannot drift.
///
/// `mute` carries the *effective* mute of the track this send is taken from.
/// A solo elsewhere that silences the source has to silence its sends too, or
/// a reverb goes on ringing from a part nobody can hear — and which tracks a
/// solo leaves audible is a property of the whole routing graph, not something
/// a node can work out from where it sits.
#[derive(Debug)]
pub struct SendControls {
    level_db: std::sync::atomic::AtomicU32,
    pan: std::sync::atomic::AtomicU32,
    mute: std::sync::atomic::AtomicBool,
}

impl SendControls {
    pub fn new(level_db: f32, pan: f32, mute: bool) -> Self {
        Self {
            level_db: std::sync::atomic::AtomicU32::new(level_db.to_bits()),
            pan: std::sync::atomic::AtomicU32::new(pan.to_bits()),
            mute: std::sync::atomic::AtomicBool::new(mute),
        }
    }

    pub fn level_db(&self) -> f32 {
        f32::from_bits(self.level_db.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn set_level_db(&self, value: f32) {
        self.level_db
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn pan(&self) -> f32 {
        f32::from_bits(self.pan.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn set_pan(&self, value: f32) {
        self.pan
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn mute(&self) -> bool {
        self.mute.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_mute(&self, value: bool) {
        self.mute.store(value, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A pre- or post-fader tap from one track's bus into another's (TDD §13.2).
///
/// [`BusSumNode`] plus a level and a pan, which is exactly what its own note
/// said a send would be. Like it, this **adds** into its destination — a
/// reverb bus has as many things arriving at it as were sent there — and
/// **leaves its source untouched**, which is the whole difference between a
/// send and an output: routing an output moves the signal, a send takes a
/// copy and the dry path carries on.
///
/// Where it sits in the chain is `fontelle_app::realise`'s business: before
/// the fader for a pre-fader send, after it for a post-fader one. The node
/// itself cannot tell, and does not need to.
pub struct SendNode {
    controls: Arc<SendControls>,
    pan_law: fontelle_types::PanLaw,
}

impl SendNode {
    pub fn new(controls: Arc<SendControls>, pan_law: fontelle_types::PanLaw) -> Self {
        Self { controls, pan_law }
    }

    pub fn controls(&self) -> Arc<SendControls> {
        Arc::clone(&self.controls)
    }
}

impl AudioNode for SendNode {
    fn prepare(&mut self, _ctx: &PrepareContext) {}

    fn process(&mut self, ctx: &mut ProcessContext) {
        if self.controls.mute() {
            return;
        }
        let level_db = self.controls.level_db();
        // The bottom of the travel is off, not "very quiet": that is where
        // every send starts, and `MIN_FADER_DB` worth of a loud part is still
        // audible on a bus with nothing else on it.
        if level_db <= SEND_MIN_DB {
            return;
        }
        let gain = 10f32.powf(level_db / 20.0);

        // Read once per block, like the fader's: a level moved mid-block takes
        // effect at a block boundary, which is the same granularity every
        // other live control here has.
        //
        // Pan only means something with two channels to balance between. On a
        // mono path there is nowhere for it to go, and applying the centre
        // pan-law gain would make every mono send quietly -3 dB down — the
        // same rule `MixerTrackNode` follows.
        let stereo = ctx.inputs.len() >= 2 && ctx.outputs.len() >= 2;
        let (left, right) = if stereo {
            self.pan_law.gains(self.controls.pan())
        } else {
            (1.0, 1.0)
        };

        for (channel, (source, dest)) in ctx.inputs.iter().zip(ctx.outputs.iter_mut()).enumerate() {
            let side = if channel == 1 { right } else { left };
            for (sample, out) in source.iter().zip(dest.iter_mut()) {
                *out += *sample * gain * side;
            }
        }
    }

    fn reset(&mut self) {}

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// Where a send's level reads as off.
///
/// The bottom of the mixer's own fader travel, so the panel's scale and the
/// DSP's floor are the same number rather than two that nearly agree.
pub const SEND_MIN_DB: f32 = -60.0;

/// The player in front of a mixer track that turns audio clips into sound
/// (TDD §15).
///
/// A note clip becomes **events** and a sampler turns them into sound. An audio
/// clip is not events: it is a continuous stream that has to be at one place on
/// the song and nowhere else, so there is no moment to schedule — there is a
/// range to be inside of. This node reads the transport's own sample position
/// and the placements the sequencer compiled, and decides per sample which
/// frame of which file belongs where.
///
/// # Where the audio comes from
///
/// An [`AudioStore`], handed over when the graph is built — exactly as a
/// `SamplerNode` is handed its `SampleStore`. The placements arrive per block
/// on the compiled timeline and carry no audio at all, which is what keeps a
/// recompile (one per mouse-move while dragging a clip) from moving a hundred
/// megabytes of take around.
///
/// # The filters
///
/// §15.1 puts a filter on every clip so *"make this one clip darker"* costs no
/// mixer track and no plugin slot. A filter is stateful, so each clip needs its
/// own — kept in a fixed pool allocated at `prepare` and claimed by
/// [`ClipId`](fontelle_types::ClipId), so a clip keeps its filter's state across
/// blocks even as the placement list is rebuilt underneath it. A track with
/// more filtered clips **sounding at once** than the pool holds runs the
/// surplus dry rather than allocating, which is INVARIANT 1: a wrong tone for a
/// block is recoverable and an allocation on the audio thread is not.
pub struct AudioClipNode {
    store: std::sync::Arc<fontelle_core::AudioStore>,
    /// One per pool slot, each remembering which clip last used it.
    filters: Vec<(Option<fontelle_types::ClipId>, fontelle_fx::Filter)>,
    /// Where a shifted clip's grains were last lined up — one per pool slot,
    /// claimed like the filters. See [`GrainMemo`].
    grains: Vec<(Option<fontelle_types::ClipId>, GrainMemo)>,
    /// Scratch for one clip's block, so the filter can be run over a
    /// contiguous buffer without allocating. Sized at `prepare`.
    scratch: [Vec<f32>; MAX_CHANNELS],
    sample_rate: f32,
}

/// How many clips on one track may have a filter engaged at the same moment.
///
/// Generous: a track carrying sixteen simultaneously-filtered audio clips is
/// already an unusual arrangement, and the cost of a spare slot is one unused
/// SVF.
const CLIP_FILTER_SLOTS: usize = 16;

impl AudioClipNode {
    pub fn new(store: std::sync::Arc<fontelle_core::AudioStore>) -> Self {
        Self {
            store,
            filters: Vec::new(),
            grains: Vec::new(),
            scratch: Default::default(),
            sample_rate: 48_000.0,
        }
    }
}

/// How a shifted clip's last two grains were lined up.
///
/// A grain read on its own is right; two grains *overlapping* are right only
/// if they agree about the phase of what they are both playing, and plain
/// overlap-add gives them no reason to — the second grain starts a fixed
/// number of frames after the first (`AudioClipData::grain_offset`), which for
/// a 220 Hz tone at half speed was a hundred and fifty degrees out and
/// cancelled most of it. So each new grain is slid a little, by up to
/// [`GRAIN_ALIGN_REACH`] frames, to where it best agrees with the one before
/// it over their overlap — the WSOLA idea, done once per grain.
///
/// The search is a pure function of the grain before it, so this holds the
/// last answer and the one before that: the two grains any frame reads. A
/// grain asked for out of sequence (the transport moved) starts a fresh chain
/// from zero, which is what a jump *is*.
#[derive(Debug, Clone, Copy, Default)]
pub struct GrainMemo {
    /// Which grain `delta` belongs to; `-1` for none yet.
    index: i64,
    delta: f64,
    /// The grain before it, which the newest was aligned to.
    previous: f64,
}

impl GrainMemo {
    const NONE: Self = Self {
        index: -1,
        delta: 0.0,
        previous: 0.0,
    };

    /// The slide of grain `index` and of the grain before it, searching for
    /// them if this memo does not hold them yet.
    fn deltas(
        &mut self,
        buffer: &fontelle_core::AudioBuffer,
        clip: &fontelle_types::AudioClipData,
        index: i64,
        hop: f64,
    ) -> (f64, f64) {
        if index == self.index {
            return (self.delta, self.previous);
        }
        if index < 0 {
            return (0.0, 0.0);
        }
        let previous = if index == self.index + 1 {
            self.delta
        } else {
            // A jump: the grain before this one was never played with any
            // slide, so it gets none, and this one lines up with that.
            0.0
        };
        let delta = if index <= 0 {
            0.0
        } else {
            align_grain(buffer, clip, index, hop, previous)
        };
        *self = Self {
            index,
            delta,
            previous,
        };
        (delta, previous)
    }
}

/// How far a grain may be slid to line up with the one before it, in file
/// frames either way. A period of 100 Hz: a tone below that lines up less
/// well, which is audible as a little chorus on a sub bass and nowhere else.
const GRAIN_ALIGN_REACH: i64 = 480;
/// How many points of the overlap the two grains are compared over, and how
/// far apart they are — so `128 × 4` covers half a hop at 48 kHz.
const GRAIN_ALIGN_POINTS: usize = 128;
const GRAIN_ALIGN_STRIDE: f64 = 4.0;

/// Where grain `index` best agrees with the grain before it over their
/// overlap: the slide, in file frames, to add to its nominal offset.
///
/// A bounded search — a few hundred candidates over a hundred and
/// twenty-eight points — done once per grain and remembered, which is what
/// keeps it off the per-sample path. Integer slides, because the overlap is
/// compared at integer file frames; the read itself stays fractional.
/// Zero wins a tie, so a constant or a silence slides nothing.
fn align_grain(
    buffer: &fontelle_core::AudioBuffer,
    clip: &fontelle_types::AudioClipData,
    index: i64,
    hop: f64,
    previous: f64,
) -> f64 {
    let anchor = index as f64 * hop;
    let before = anchor - hop;
    let channel = 0;
    // The frames the previous grain plays across the overlap, and where the
    // new grain's nominal read of the same frames is.
    let mut want = [0.0f32; GRAIN_ALIGN_POINTS];
    let mut at = [0.0f64; GRAIN_ALIGN_POINTS];
    for (i, (w, a)) in want.iter_mut().zip(at.iter_mut()).enumerate() {
        let position = anchor + i as f64 * GRAIN_ALIGN_STRIDE;
        let older = clip.source_at_offset(clip.grain_offset(position, before) + previous);
        *w = buffer.at(older, channel);
        *a = clip.grain_offset(position, anchor);
    }
    let score = |delta: f64| -> f32 {
        let (mut dot, mut energy) = (0.0f32, 0.0f32);
        for (w, a) in want.iter().zip(&at) {
            let sample = buffer.at(clip.source_at_offset(a + delta), channel);
            dot += w * sample;
            energy += sample * sample;
        }
        if energy <= 1e-12 {
            0.0
        } else {
            dot / energy.sqrt()
        }
    };
    let mut best = (0.0f64, score(0.0));
    for step in 1..=GRAIN_ALIGN_REACH {
        for delta in [step as f64, -(step as f64)] {
            let value = score(delta);
            if value > best.1 {
                best = (delta, value);
            }
        }
    }
    best.0
}

/// The slot in `filters` holding `clip`'s filter, claiming a free one if it has
/// none.
///
/// A linear scan of sixteen on the audio thread, which is nothing beside the
/// buffer read it precedes. `None` when every slot is spoken for by a clip
/// sounding in this same block — see [`AudioClipNode`]'s own note.
///
/// A free function rather than a method so the caller can hold the store and
/// the scratch at the same time: `process` splits `self` into its parts exactly
/// once, and everything after that borrows only what it uses.
fn filter_slot(
    filters: &mut [(Option<fontelle_types::ClipId>, fontelle_fx::Filter)],
    clip: fontelle_types::ClipId,
    claimed: usize,
) -> Option<usize> {
    if let Some(index) = filters.iter().position(|(owner, _)| *owner == Some(clip)) {
        return Some(index);
    }
    // Never one already claimed by another clip in this block: two clips
    // sharing a filter would each hear the other's ringing.
    if claimed >= filters.len() {
        return None;
    }
    filters[claimed].0 = Some(clip);
    filters[claimed].1.reset();
    Some(claimed)
}

/// The slot in `grains` holding `clip`'s grain memo — the same pool rule as
/// [`filter_slot`]. A clip refused a slot reads its grains unaligned, which
/// is a poorer sound for a block and never an allocation.
fn grain_slot(
    grains: &mut [(Option<fontelle_types::ClipId>, GrainMemo)],
    clip: fontelle_types::ClipId,
    claimed: usize,
) -> Option<usize> {
    if let Some(index) = grains.iter().position(|(owner, _)| *owner == Some(clip)) {
        return Some(index);
    }
    if claimed >= grains.len() {
        return None;
    }
    grains[claimed].0 = Some(clip);
    grains[claimed].1 = GrainMemo::NONE;
    Some(claimed)
}

impl AudioNode for AudioClipNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate.max(1.0);
        self.filters.clear();
        self.grains.clear();
        for _ in 0..CLIP_FILTER_SLOTS {
            let mut filter = fontelle_fx::Filter::new();
            filter.prepare(self.sample_rate);
            self.filters.push((None, filter));
            self.grains.push((None, GrainMemo::NONE));
        }
        for channel in &mut self.scratch {
            channel.clear();
            channel.resize(ctx.max_block_size as usize, 0.0);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        // A clip only sounds while something is rolling. Unlike a sampler
        // there is no live half to keep going: nobody plays an audio clip with
        // their hands.
        if !ctx.transport.state.is_processing() {
            return;
        }
        let frames = ctx.outputs.first().map_or(0, |o| o.len());
        if frames == 0 || ctx.outputs.is_empty() {
            return;
        }
        let start = ctx.sample_range.start;
        let device_rate = f64::from(self.sample_rate);
        // Split once, here. The render loop holds a buffer out of the store
        // and writes the scratch at the same time, which it cannot do through
        // `self`.
        let Self {
            store,
            filters,
            grains,
            scratch,
            ..
        } = self;

        // Collected first, because rendering borrows `self` mutably and the
        // placements live in `ctx`. A fixed-size array rather than a `Vec`:
        // this is the audio thread (INVARIANT 1).
        let mut sounding: [Option<&fontelle_types::AudioPlacement>; CLIP_FILTER_SLOTS] =
            [None; CLIP_FILTER_SLOTS];
        let mut count = 0;
        for placement in ctx.audio.iter().filter(|p| p.target == ctx.node) {
            // Nothing to do for a clip that is not in this block at all, which
            // is nearly all of them on a long song.
            if placement.range.end <= start || placement.range.start >= ctx.sample_range.end {
                continue;
            }
            if count < sounding.len() {
                sounding[count] = Some(placement);
                count += 1;
            }
        }

        let mut claimed = 0;
        let mut grains_claimed = 0;
        for placement in sounding.iter().take(count).flatten() {
            let clip = &placement.data;
            let Some(buffer) = store.get(clip.asset.id) else {
                // A clip whose file has not been decoded yet: silent, and it
                // fills itself in as soon as the loader publishes a new store.
                // §15.3's "draw what exists" applied to sound.
                continue;
            };
            // How fast to read the file — the file's rate against the
            // device's, or the ratio that makes it fill its block when the
            // clip is following the tempo. Both answers live on the clip; see
            // `AudioClipData::read_ratio`.
            //
            // **One pass**, not the whole placement: a one-bar loop dragged
            // out to eight bars stretches its audio to one bar and comes round
            // eight times, rather than being smeared across all of it.
            let span = if placement.repeat > 0 {
                placement.repeat
            } else {
                placement.frames()
            };
            let ratio = clip.read_ratio(buffer.sample_rate, device_rate, span);
            let gain = clip.gain();
            let (left_gain, right_gain) = clip_pan(clip.pan);
            // Whether what is heard moves at a different rate from time —
            // a pitch moved with stretch off, or a speed moved without the
            // pitch following. Then the file is read in grains; otherwise it
            // is a plain read, sample for sample. See
            // `AudioClipData::grain_offset` for the arithmetic.
            let shifted = clip.shifts_pitch();
            let hop = clip.grain_hop();
            let memo = if shifted {
                let index = grain_slot(grains, placement.clip, grains_claimed);
                if index == Some(grains_claimed) {
                    grains_claimed += 1;
                }
                index
            } else {
                None
            };
            let mut unaligned = GrainMemo::NONE;

            let filtered = clip.filter_engaged();
            let slot = if filtered {
                let index = filter_slot(filters, placement.clip, claimed);
                if index == Some(claimed) {
                    claimed += 1;
                }
                index
            } else {
                None
            };

            // One clip's contribution, dry, into the scratch — then the filter
            // over it, then summed in. Two passes rather than one because the
            // filter wants a contiguous run and the output bus already holds
            // other clips' audio.
            let channels = ctx.outputs.len().min(MAX_CHANNELS);
            for channel in scratch.iter_mut().take(channels) {
                channel[..frames].fill(0.0);
            }
            for frame in 0..frames {
                let Some(position) = placement.position(start + frame as i64) else {
                    continue;
                };
                let clip_frame = position as f64 * ratio;
                // The clip's own fades, in its frames, and the placement's
                // crossfade, in the song's — both, because they are two
                // different facts: one goes with the clip wherever it is
                // put, the other is about the clip beside it.
                let envelope =
                    gain * clip.fade_gain(clip_frame) * placement.auto_gain(start + frame as i64);
                if !shifted {
                    let source = clip.source_position(clip_frame);
                    if source < 0.0 {
                        continue;
                    }
                    for (channel, buf) in scratch.iter_mut().take(channels).enumerate() {
                        buf[frame] = buffer.at(source, channel as u16) * envelope;
                    }
                    continue;
                }
                // **In grains.** Two overlap at any frame: the newest,
                // anchored at the last hop boundary, and the one before it.
                // Each is read under half a raised cosine — the newest rising,
                // the older falling — and the two add to exactly one, so a
                // shift of nothing is the file itself. Before the second hop
                // there is only the first grain, and it carries the whole
                // weight rather than fading the clip's front in.
                let newest = (clip_frame / hop).floor();
                // Slid to agree with each other — see `GrainMemo`. A clip
                // without a slot plays unaligned rather than not at all.
                let (delta, previous) = match memo {
                    Some(slot) => grains[slot].1.deltas(buffer, clip, newest as i64, hop),
                    None => unaligned.deltas(buffer, clip, -1, hop),
                };
                for (older, slide) in [(0.0, delta), (1.0, previous)] {
                    let index = newest - older;
                    if index < 0.0 {
                        continue;
                    }
                    let anchor = index * hop;
                    let weight = if newest < 1.0 {
                        1.0
                    } else {
                        let along = (clip_frame - anchor) / hop; // 0..2
                        0.5 - 0.5 * (std::f64::consts::PI * along).cos()
                    } as f32;
                    let source =
                        clip.source_at_offset(clip.grain_offset(clip_frame, anchor) + slide);
                    if source < 0.0 {
                        continue;
                    }
                    for (channel, buf) in scratch.iter_mut().take(channels).enumerate() {
                        buf[frame] += buffer.at(source, channel as u16) * weight * envelope;
                    }
                }
            }

            if let Some(slot) = slot {
                let (a, b) = scratch.split_at_mut(1);
                let mut sides: [&mut [f32]; MAX_CHANNELS] =
                    [&mut a[0][..frames], &mut b[0][..frames]];
                filters[slot]
                    .1
                    .process(&mut sides[..channels], &clip.filter, ctx.transport.bpm);
            }

            for (channel, out) in ctx.outputs.iter_mut().take(channels).enumerate() {
                let side = if channel == 0 { left_gain } else { right_gain };
                for frame in 0..frames {
                    out[frame] += scratch[channel][frame] * side;
                }
            }
        }
    }

    fn reset(&mut self) {
        for (owner, filter) in &mut self.filters {
            *owner = None;
            filter.reset();
        }
        for (owner, memo) in &mut self.grains {
            *owner = None;
            *memo = GrainMemo::NONE;
        }
    }

    fn debug_name(&self) -> &'static str {
        "audio-clips"
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// A clip's own pan, as a pair of channel gains.
///
/// [`PanLaw::Linear`](fontelle_types::PanLaw::Linear) — balance-style, **unity
/// at centre** — rather than the constant-power law a mixer track uses. The
/// reason is the identity property this whole feature rests on: a file dropped
/// on the arrangement has to sound exactly like the file, and a constant-power
/// centre is that file three decibels down. A track's pan is a balance control
/// over a bus and gets the -3 dB law; this is a clip's own placement and gets
/// the one that does nothing when it is not moved.
fn clip_pan(pan: f32) -> (f32, f32) {
    fontelle_types::PanLaw::Linear.gains(pan)
}

/// The metronome's switch and its beat, shared with the RT thread.
///
/// Atomics, like [`TrackControls`] and [`MasterMeter`], and for the same
/// reason: three scalars that change when somebody clicks something and are
/// read once a block.
///
/// # Why a click is not a clip
///
/// It is not document data. It is not saved, it does not bounce, it belongs to
/// no channel and it has no notes — a project sent to somebody else must not
/// arrive with a woodblock on every beat. So it is a node reading the
/// transport's own position, switched from outside like every other live
/// control here.
#[derive(Debug)]
pub struct Metronome {
    on: std::sync::atomic::AtomicBool,
    /// How many samples one beat is. **Zero is "no tempo yet"** and clicks
    /// nothing, rather than dividing by it.
    samples_per_beat: std::sync::atomic::AtomicU32,
    beats_per_bar: std::sync::atomic::AtomicU32,
}

impl Metronome {
    /// Off, with no tempo. **Off is the default and has to be**: a window that
    /// clicks at you the first time you press play is one you have to go and
    /// find the switch for.
    pub fn new() -> Self {
        Self {
            on: std::sync::atomic::AtomicBool::new(false),
            samples_per_beat: std::sync::atomic::AtomicU32::new(0),
            beats_per_bar: std::sync::atomic::AtomicU32::new(4),
        }
    }

    pub fn is_on(&self) -> bool {
        self.on.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// Where the beats are: how long one is, and how many make a bar.
    ///
    /// Written by the model side whenever the tempo or the signature changes.
    /// **A constant beat**, which is exact for a song at one tempo — every
    /// song this build can create — and drifts across a tempo *change*, where
    /// the correct answer needs the map the RT thread may not read
    /// (INVARIANT 3). Worth writing down rather than hiding: a piece with a
    /// tempo curve gets a click that is right until the first change.
    pub fn set_beat(&self, samples_per_beat: u32, beats_per_bar: u32) {
        self.samples_per_beat
            .store(samples_per_beat, std::sync::atomic::Ordering::Relaxed);
        self.beats_per_bar
            .store(beats_per_bar.max(1), std::sync::atomic::Ordering::Relaxed);
    }

    /// How long one beat is, in samples. Zero is "no tempo yet" and clicks
    /// nothing — which is exactly the state a metronome nobody told the tempo
    /// to sits in, so it is worth being able to ask.
    pub fn samples_per_beat(&self) -> u32 {
        self.samples_per_beat
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.beats_per_bar
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn beat(&self) -> (i64, i64) {
        (
            i64::from(
                self.samples_per_beat
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            i64::from(
                self.beats_per_bar
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .max(1),
        )
    }
}

impl Default for Metronome {
    fn default() -> Self {
        Self::new()
    }
}

/// How long a click lasts. Thirty milliseconds: long enough to hear, short
/// enough that it is over well before the next one at any tempo anybody
/// counts in.
const CLICK_SECONDS: f32 = 0.03;

/// The downbeat, and the beats between it.
const ACCENT_HZ: f32 = 1_600.0;
const BEAT_HZ: f32 = 1_000.0;

/// And how loud, which is a judgement: loud enough to hear over a mix, quiet
/// enough not to be the loudest thing in the room.
const ACCENT_GAIN: f32 = 0.5;
const BEAT_GAIN: f32 = 0.28;

/// The click itself (item 9 of `docs/first-usable-plan.md`).
///
/// **Adds into its bus**, like every source in this graph: it shares the master
/// pair with the music, and a node that overwrote would mute the song on every
/// beat.
///
/// Reads the transport's **position** rather than counting its own elapsed
/// blocks, which is what makes a seek and a loop land on the beat: the click
/// is a function of where the playhead is, not of how long the node has been
/// running.
pub struct MetronomeNode {
    metronome: Arc<Metronome>,
    sample_rate: f32,
    /// How many samples of the current click are left, and its shape.
    remaining: u32,
    length: u32,
    phase: f32,
    step: f32,
    gain: f32,
    /// Which beat last fired, so one beat does not click twice when a block
    /// boundary falls inside it.
    last_beat: i64,
}

impl MetronomeNode {
    pub fn new(metronome: Arc<Metronome>) -> Self {
        Self {
            metronome,
            sample_rate: 0.0,
            remaining: 0,
            length: 0,
            phase: 0.0,
            step: 0.0,
            gain: 0.0,
            last_beat: i64::MIN,
        }
    }

    fn start(&mut self, accent: bool) {
        let hz = if accent { ACCENT_HZ } else { BEAT_HZ };
        self.gain = if accent { ACCENT_GAIN } else { BEAT_GAIN };
        self.length = (CLICK_SECONDS * self.sample_rate).max(1.0) as u32;
        self.remaining = self.length;
        self.phase = 0.0;
        self.step = std::f32::consts::TAU * hz / self.sample_rate.max(1.0);
    }
}

impl AudioNode for MetronomeNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
        self.remaining = 0;
        self.last_beat = i64::MIN;
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        // Only while something is rolling: a click over a stopped transport is
        // a metronome nobody asked for.
        if !ctx.transport.state.is_processing() || !self.metronome.is_on() {
            self.remaining = 0;
            return;
        }
        let (per_beat, per_bar) = self.metronome.beat();
        if per_beat <= 0 || self.sample_rate <= 0.0 {
            return; // no tempo yet — silent, rather than dividing by zero
        }

        let frames = ctx.outputs.first().map_or(0, |o| o.len());
        let start = ctx.sample_range.start;
        for frame in 0..frames {
            let at = start + frame as i64;
            // The beat this sample belongs to. Floor division, so a position
            // before the start of the song counts backwards rather than
            // clustering every negative sample onto beat zero.
            let beat = at.div_euclid(per_beat);
            if at.rem_euclid(per_beat) == 0 && beat != self.last_beat {
                self.last_beat = beat;
                self.start(beat.rem_euclid(per_bar) == 0);
            }
            if self.remaining == 0 {
                continue;
            }
            // A straight linear decay. An envelope with a shape would be a
            // nicer click and a longer explanation; what this has to do is
            // start hard and be gone.
            let fade = self.remaining as f32 / self.length.max(1) as f32;
            let value = self.phase.sin() * self.gain * fade;
            self.phase += self.step;
            self.remaining -= 1;
            for channel in ctx.outputs.iter_mut() {
                channel[frame] += value;
            }
        }
    }

    fn reset(&mut self) {
        self.remaining = 0;
        self.last_beat = i64::MIN;
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// What the master bus publishes for a meter to read: peak per channel and
/// the most gain reduction the limiter applied, in positive decibels.
///
/// Shared with the RT thread as plain atomics rather than through the
/// downsampled ring TDD §13.3 describes, because these are three scalars
/// rather than a waveform: a relaxed store per block is cheaper than a ring,
/// and there is nothing here whose *history* matters. Reading a value takes
/// it — the reader is the one that knows when it has drawn what it read.
#[derive(Debug, Default)]
pub struct MasterMeter {
    peaks: [std::sync::atomic::AtomicU32; MAX_CHANNELS],
    max_reduction_db: std::sync::atomic::AtomicU32,
}

impl MasterMeter {
    /// The highest peak since this was last called, per channel, and resets.
    pub fn take_peaks(&self) -> [f32; MAX_CHANNELS] {
        std::array::from_fn(|index| {
            f32::from_bits(self.peaks[index].swap(0, std::sync::atomic::Ordering::Relaxed))
        })
    }

    /// The most gain reduction since this was last called, in positive
    /// decibels, and resets. Zero means the limiter never engaged.
    pub fn take_max_reduction_db(&self) -> f32 {
        f32::from_bits(
            self.max_reduction_db
                .swap(0, std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// RT: a load, a compare and a store. Single writer, so the read-modify-
    /// write needs no compare-exchange loop.
    fn record(&self, index: usize, value: f32) {
        let Some(slot) = self.peaks.get(index) else {
            return;
        };
        let current = f32::from_bits(slot.load(std::sync::atomic::Ordering::Relaxed));
        if value > current {
            slot.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn record_reduction(&self, value: f32) {
        let current = f32::from_bits(
            self.max_reduction_db
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        if value > current {
            self.max_reduction_db
                .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// The master bus: a brickwall limiter, then peak/RMS metering (TDD §13.3).
///
/// **Processes in place**, at the end of the schedule, after every track has
/// summed into the master pair.
///
/// The limiter is here rather than in an insert slot because it is not an
/// effect the user chose — it is the thing that makes "play any file and it
/// does not clip" true without a judgement about the material. Bypassable, for
/// when a mix is going somewhere that wants the peaks intact.
///
/// **Not yet:** LUFS-M/S/I and true-peak metering, which §13.3 also asks of
/// the master. Both need their own filters and an oversampled peak detector;
/// the peak/RMS pair is what a fader needs to be usable.
pub struct MasterNode {
    limiter: fontelle_fx::Limiter,
    pub limiter_config: fontelle_fx::LimiterConfig,
    pub limiter_enabled: bool,
    meters: [fontelle_dsp::PeakRmsMeter; MAX_CHANNELS],
    /// The half of the metering anything off the RT thread can read.
    published: Arc<MasterMeter>,
    /// Kept from `prepare` so `reset` can rebuild the limiter without being
    /// handed a `PrepareContext` it has no way to obtain.
    sample_rate: f32,
}

impl MasterNode {
    pub fn new() -> Self {
        Self {
            limiter: fontelle_fx::Limiter::new(),
            limiter_config: fontelle_fx::LimiterConfig::default(),
            limiter_enabled: true,
            meters: [fontelle_dsp::PeakRmsMeter::new(); MAX_CHANNELS],
            published: Arc::new(MasterMeter::default()),
            sample_rate: 48_000.0,
        }
    }

    /// A handle on the master's levels that outlives handing this node to the
    /// RT thread — which is the only way anything can read them once the graph
    /// is in the audio callback.
    pub fn meter(&self) -> Arc<MasterMeter> {
        self.published.clone()
    }

    /// Publishes into `meter` instead of a fresh one.
    ///
    /// What a **rebuild** hands in: the transport bar's meter was given the
    /// first graph's `Arc` and goes on reading it, so the node in every later
    /// graph has to write to that same one or the bar reads silence for the
    /// rest of the session — *"it seems to show sometimes but not always."*
    /// The same wire the metronome switch is kept on (`Metronome`).
    pub fn with_meter(mut self, meter: Arc<MasterMeter>) -> Self {
        self.published = meter;
        self
    }

    /// Peak and RMS per channel, for a meter. The peak is held until
    /// [`MasterNode::reset_peaks`].
    pub fn channel_meter(&self, channel: usize) -> Option<&fontelle_dsp::PeakRmsMeter> {
        self.meters.get(channel)
    }

    pub fn reset_peaks(&mut self) {
        for meter in &mut self.meters {
            meter.reset_peak();
            meter.clear_clip_latch();
        }
    }
}

impl Default for MasterNode {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioNode for MasterNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
        self.limiter.prepare(ctx.sample_rate, &self.limiter_config);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        if self.limiter_enabled {
            self.limiter
                .process(ctx.outputs, None, &self.limiter_config);
        }
        // Metered *after* the limiter, because what the meter is for is
        // showing what left the machine.
        //
        // **This block's own peak** goes to the published meter, not the
        // `PeakRmsMeter`'s, which is held — the highest sample since the last
        // reset. Publishing the held one is what made the transport bar's
        // meter *"leave it hanging too long when nothings on anymore"*: every
        // frame read the loudest moment of the session over again, and the
        // bar came down only when a rebuild replaced the node. The reader
        // already folds the block peaks into its own hold and release, the
        // way it does for the track meters (`MixerTrackNode`).
        for (index, (meter, channel)) in self.meters.iter_mut().zip(ctx.outputs.iter()).enumerate()
        {
            meter.process_block(channel);
            let peak = channel.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
            self.published.record(index, peak);
        }
        self.published
            .record_reduction(self.limiter.take_max_reduction_db());
    }

    fn reset(&mut self) {
        // Re-preparing is what clears the delay line and the gain state; a
        // separate "flush" would be one more thing to keep in step with it.
        let config = self.limiter_config;
        self.limiter.prepare(self.sample_rate, &config);
        self.reset_peaks();
    }

    fn latency_samples(&self) -> u32 {
        if self.limiter_enabled {
            self.limiter.latency_samples()
        } else {
            0
        }
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// The parameter name out of an insert's address — the part after
/// `/param/` in `mixer:<track>/insert[<n>]/param/<name>`.
///
/// A `str` slice of the address, so nothing is allocated: this runs on the
/// audio thread (INVARIANT 1), and `ParamTarget::parse` builds a `String`.
fn param_of(address: &str) -> Option<&str> {
    let param = address.rsplit_once("/param/")?.1;
    (!param.is_empty()).then_some(param)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontelle_core::{
        FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, Source, VoiceConfig,
    };
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
    use fontelle_types::{EventPayload, TimedEvent};

    use crate::transport::{TransportSnapshot, TransportState};

    const SR: f32 = 48_000.0;

    fn test_patch(store: &mut SampleStore) -> Patch {
        let asset = store.insert(fontelle_core::SampleBuffer {
            data: Arc::from(vec![1.0; 10_000]),
            sample_rate: SR as u32,
        });
        let disabled_filter = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
            ..Default::default()
        };
        let instant = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
            ..Default::default()
        };
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
                    end_offset: 10_000.0,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter, disabled_filter],
            envelopes: vec![instant, instant],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig::default(),
            ..Default::default()
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    /// Drives a `MixerTrackNode` over buffers pre-filled with `input`,
    /// returning what it left behind (it processes in place).
    fn run_mixer(node: &mut MixerTrackNode, channels: usize, input: f32) -> Vec<Vec<f32>> {
        let mut buffers: Vec<Vec<f32>> = (0..channels).map(|_| vec![input; 64]).collect();
        {
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                    bpm: fontelle_types::DEFAULT_BPM,
                },
                sample_range: 0..64,
            };
            node.process(&mut ctx);
        }
        buffers
    }

    /// A centred stereo track is *not* transparent, and shouldn't be: the
    /// default -3dB pan law puts 0.707 on each side so that total power is
    /// preserved as a source pans across the field. Asserting "unchanged"
    /// here would be asserting that the pan law does nothing.
    #[test]
    fn a_centred_stereo_track_applies_the_constant_power_pan_law() {
        let mut node = MixerTrackNode::new();
        let out = run_mixer(&mut node, 2, 1.0);

        let expected = std::f32::consts::FRAC_1_SQRT_2; // -3dB
        for (channel, samples) in out.iter().enumerate() {
            for &s in samples {
                assert!(
                    (s - expected).abs() < 1e-5,
                    "channel {channel}: centre pan at -3dB law should read {expected}, got {s}"
                );
            }
        }

        let power = out[0][0] * out[0][0] + out[1][0] * out[1][0];
        assert!(
            (power - 1.0).abs() < 1e-5,
            "the whole point of the -3dB law: total power stays 1.0, got {power}"
        );
    }

    /// The transparency claim belongs to a *mono* track, where no pan law
    /// applies — unity gain in, unity gain out, bit for bit.
    #[test]
    fn a_default_mono_track_passes_audio_through_unchanged() {
        let mut node = MixerTrackNode::new();
        let out = run_mixer(&mut node, 1, 0.5);
        for &s in &out[0] {
            assert_eq!(s, 0.5, "a default mono track must be bit-transparent");
        }
    }

    #[test]
    fn gain_db_scales_the_signal() {
        let mut node = MixerTrackNode {
            gain_db: -6.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 1.0);
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!(
            (out[0][0] - expected).abs() < 1e-4,
            "-6dB should scale 1.0 to ~{expected}, got {}",
            out[0][0]
        );
    }

    #[test]
    fn mute_silences_every_channel() {
        let mut node = MixerTrackNode {
            mute: true,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 2, 1.0);
        for channel in &out {
            assert!(
                channel.iter().all(|&s| s == 0.0),
                "mute must zero the buffer"
            );
        }
    }

    #[test]
    fn phase_invert_flips_the_sign_without_changing_magnitude() {
        let mut node = MixerTrackNode {
            phase_invert: true,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 0.25);
        assert!(
            (out[0][0] + 0.25).abs() < 1e-6,
            "phase invert should give -0.25, got {}",
            out[0][0]
        );
    }

    #[test]
    fn panning_hard_left_silences_the_right_channel() {
        let mut node = MixerTrackNode {
            pan: -1.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 2, 1.0);
        assert!(
            (out[0][0] - 1.0).abs() < 1e-5,
            "left must stay at full scale, got {}",
            out[0][0]
        );
        assert!(
            out[1][0].abs() < 1e-5,
            "right must be silent, got {}",
            out[1][0]
        );
    }

    /// A mono track has nowhere to pan to. Attenuating by the centre pan-law
    /// gain anyway would make every mono track quietly 3dB down for no reason
    /// the user can see.
    #[test]
    fn pan_is_ignored_on_a_mono_track_rather_than_attenuating_it() {
        let mut node = MixerTrackNode {
            pan: 0.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 1.0);
        assert!(
            (out[0][0] - 1.0).abs() < 1e-6,
            "a centred mono track must stay at unity, got {}",
            out[0][0]
        );
    }

    // --- the live half of a fader (TDD §13.1, §13.3) -----------------------

    /// A track whose values come from a [`TrackControls`] instead of its own
    /// fields — what `fontelle_app::realise` builds for every mixer track.
    fn live_track(controls: &Arc<TrackControls>) -> MixerTrackNode {
        MixerTrackNode {
            controls: Some(Arc::clone(controls)),
            ..MixerTrackNode::new()
        }
    }

    #[test]
    fn a_fader_moved_between_blocks_is_heard_on_the_next_one() {
        // The whole reason this type exists: a level has to change while the
        // graph is playing, without rebuilding the graph and without a lock.
        let controls = Arc::new(TrackControls::new(0.0, 0.0, false));
        let mut node = live_track(&controls);

        let before = run_mixer(&mut node, 1, 1.0);
        assert!((before[0][0] - 1.0).abs() < 1e-6, "unity to start with");

        controls.set_gain_db(-6.0);
        let after = run_mixer(&mut node, 1, 1.0);
        assert!(
            (after[0][0] - 0.5011872).abs() < 1e-4,
            "-6 dB is about half, got {}",
            after[0][0]
        );
    }

    #[test]
    fn a_live_pan_moves_the_signal_across_the_field() {
        let controls = Arc::new(TrackControls::new(0.0, 0.0, false));
        let mut node = live_track(&controls);

        controls.set_pan(-1.0);
        let out = run_mixer(&mut node, 2, 1.0);
        assert!(out[0][0] > 0.99, "hard left keeps the left channel");
        assert!(out[1][0].abs() < 1e-6, "and empties the right");
    }

    #[test]
    fn a_live_mute_silences_the_track() {
        let controls = Arc::new(TrackControls::new(0.0, 0.0, false));
        let mut node = live_track(&controls);

        controls.set_mute(true);
        let out = run_mixer(&mut node, 2, 1.0);
        assert!(out.iter().all(|c| c.iter().all(|s| *s == 0.0)));

        controls.set_mute(false);
        let out = run_mixer(&mut node, 2, 1.0);
        assert!(out[0][0] > 0.0, "and unmuting brings it back");
    }

    #[test]
    fn a_track_without_controls_still_uses_its_own_fields() {
        // Offline renders and every existing test build the node directly.
        // Attaching a control surface must not become a requirement for
        // getting sound out of one.
        let mut node = MixerTrackNode {
            gain_db: -6.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 1.0);
        assert!((out[0][0] - 0.5011872).abs() < 1e-4);
    }

    #[test]
    fn a_track_meters_what_it_actually_sent_on() {
        // After the fader, not before: a meter that reads the input tells you
        // nothing about whether you have pulled the track down far enough.
        // Mono, so the -3 dB pan law is not in the picture and the number
        // being checked is the fader's alone.
        let controls = Arc::new(TrackControls::new(-20.0, 0.0, false));
        let mut node = live_track(&controls);
        run_mixer(&mut node, 1, 1.0);

        let peaks = controls.take_peaks();
        assert!(
            (peaks[0] - 0.1).abs() < 0.005,
            "-20 dB of a full-scale signal is 0.1, got {}",
            peaks[0]
        );
        assert_eq!(
            controls.take_peaks(),
            [0.0; MAX_CHANNELS],
            "reading a peak takes it, like the master meter's"
        );
    }

    #[test]
    fn a_muted_track_meters_silence() {
        let controls = Arc::new(TrackControls::new(0.0, 0.0, true));
        let mut node = live_track(&controls);
        run_mixer(&mut node, 2, 1.0);
        assert_eq!(controls.take_peaks(), [0.0; MAX_CHANNELS]);
    }

    #[test]
    fn a_note_on_event_produces_sound_through_the_audio_node_interface() {
        let mut store = SampleStore::new();
        let patch = test_patch(&mut store);
        let store = Arc::new(store);

        let mut node = SamplerNode::new(Sampler::new(patch), store);
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        let events = [TimedEvent {
            sample: 0,
            target: fontelle_types::NodeId::default(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 127,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            },
        }];

        let mut channel = vec![0.0; 128];
        {
            let mut out_slices: Vec<&mut [f32]> = vec![&mut channel];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut out_slices,
                all_events: &events,
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                    bpm: fontelle_types::DEFAULT_BPM,
                },
                sample_range: 0..128,
            };
            node.process(&mut ctx);
        }

        assert!(
            rms(&channel) > 0.5,
            "expected near-full-scale output from the NoteOn, got rms {}",
            rms(&channel)
        );
    }

    /// Drives a `MasterNode` over buffers pre-filled with `input`.
    fn run_master(node: &mut MasterNode, frames: usize, input: f32) -> Vec<Vec<f32>> {
        let mut buffers: Vec<Vec<f32>> = (0..2).map(|_| vec![input; frames]).collect();
        {
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                    bpm: fontelle_types::DEFAULT_BPM,
                },
                sample_range: 0..frames as i64,
            };
            node.process(&mut ctx);
        }
        buffers
    }

    /// The whole reason the master track exists: an arrangement summing onto
    /// one bus peaks wherever the material puts it, and a fader set by hand
    /// either clips or throws away headroom.
    #[test]
    fn the_master_holds_the_bus_under_full_scale() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 4_096,
        });
        let out = run_master(&mut node, 4_096, 3.0);
        let peak = out.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak <= node.limiter_config.ceiling + 1e-4,
            "3x full scale in should come out at the ceiling, got {peak}"
        );
    }

    #[test]
    fn a_bypassed_master_limiter_is_transparent() {
        let mut node = MasterNode {
            limiter_enabled: false,
            ..MasterNode::new()
        };
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 256,
        });
        let out = run_master(&mut node, 256, 0.5);
        assert!(out[0].iter().all(|&s| s == 0.5));
        assert_eq!(node.latency_samples(), 0, "and it costs no latency either");
    }

    /// Metered after the limiter, because what a master meter is for is
    /// showing what left the machine.
    #[test]
    fn the_master_meters_what_it_actually_output() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 4_096,
        });
        run_master(&mut node, 4_096, 3.0);

        let meter = node
            .channel_meter(0)
            .expect("a stereo master has a left meter");
        assert!(
            meter.peak() <= node.limiter_config.ceiling + 1e-4,
            "the meter must read the limited signal, not the 3.0 that arrived: \
             {}",
            meter.peak()
        );
        assert!(
            !meter.clip_latched(),
            "and nothing should have clipped in the first place"
        );
        // The published handle is the only way to read this once the node is
        // inside the graph on the RT thread, so it is what the test reads.
        let published = node.meter();
        assert!(
            published.take_max_reduction_db() > 5.0,
            "the limiter worked hard and must be able to say so"
        );
        assert_eq!(
            published.take_max_reduction_db(),
            0.0,
            "reading it resets it, or a meter shows the loudest moment of the \
             session forever"
        );
    }

    /// The meter handle has to be taken *before* the node is boxed into the
    /// schedule and handed to the audio thread, and keep working afterwards.
    #[test]
    fn the_master_meter_handle_outlives_handing_the_node_to_the_graph() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 256,
        });
        let meter = node.meter();

        let mut boxed: Box<dyn AudioNode> = Box::new(node);
        let mut buffers: Vec<Vec<f32>> = (0..2).map(|_| vec![0.5; 256]).collect();
        {
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                    bpm: fontelle_types::DEFAULT_BPM,
                },
                sample_range: 0..256,
            };
            boxed.process(&mut ctx);
        }

        let peaks = meter.take_peaks();
        assert!(
            (peaks[0] - 0.5).abs() < 1e-6,
            "the handle must still be reading the node's output, got {}",
            peaks[0]
        );
        assert_eq!(meter.take_peaks()[0], 0.0, "and reading it resets it");
    }

    /// The transport bar's meter *"leaves it hanging too long when nothings
    /// on anymore"*: what the node published was `PeakRmsMeter::peak`, which
    /// is **held** — the highest sample since the node was last reset — so
    /// every frame after the loudest moment read that moment again and the
    /// bar never fell until the graph was rebuilt. What a meter wants per
    /// block is that block's own peak, which is what the track meters
    /// publish.
    #[test]
    fn the_master_meter_publishes_each_blocks_own_peak_not_the_highest_ever() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 256,
        });
        let meter = node.meter();
        let mut boxed: Box<dyn AudioNode> = Box::new(node);
        let mut run = |level: f32| {
            let mut buffers: Vec<Vec<f32>> = (0..2).map(|_| vec![level; 256]).collect();
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                    bpm: fontelle_types::DEFAULT_BPM,
                },
                sample_range: 0..256,
            };
            boxed.process(&mut ctx);
        };
        run(0.5);
        assert!((meter.take_peaks()[0] - 0.5).abs() < 1e-6);
        // Two silent blocks, read between them: the limiter's look-ahead
        // carries the tail of the loud one a couple of milliseconds into the
        // first, and the meter is a highest-since-last-read.
        run(0.0);
        let _ = meter.take_peaks();
        run(0.0);
        assert_eq!(
            meter.take_peaks()[0],
            0.0,
            "a silent block after a loud one must publish silence, not the loud one again"
        );
        run(0.1);
        assert!(
            (meter.take_peaks()[0] - 0.1).abs() < 1e-6,
            "and a quieter block publishes its own level"
        );
    }

    /// The last step of the per-note pan's journey: the wire carries it as the
    /// document's byte, and this is the one place that turns it into the
    /// field position a voice is triggered with.
    #[test]
    fn a_note_ons_pan_reaches_the_voice_it_starts() {
        let mut store = SampleStore::new();
        let patch = test_patch(&mut store);
        let store = Arc::new(store);

        let mut node = SamplerNode::new(Sampler::new(patch), store);
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        let events = [TimedEvent {
            sample: 0,
            target: fontelle_types::NodeId::default(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 127,
                pan: -127,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            },
        }];

        let mut left = vec![0.0; 128];
        let mut right = vec![0.0; 128];
        {
            let mut out_slices: Vec<&mut [f32]> = vec![&mut left, &mut right];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut out_slices,
                all_events: &events,
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                    bpm: fontelle_types::DEFAULT_BPM,
                },
                sample_range: 0..128,
            };
            node.process(&mut ctx);
        }

        assert!(
            rms(&left) > 0.5,
            "a note panned hard left has to be audible on the left, got rms {}",
            rms(&left)
        );
        assert!(
            rms(&right) < 1e-4,
            "and silent on the right, got rms {}",
            rms(&right)
        );
    }
}
