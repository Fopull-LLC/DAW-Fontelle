use std::sync::Arc;

use fontelle_core::{SampleStore, Sampler};
use fontelle_types::{PanLaw, ParamAddress};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};

/// The widest render `SamplerNode` performs: a stereo pair. Surround is not a
/// feature yet, and a fixed width is what keeps the scratch allocation in
/// `prepare` (INVARIANT 1).
const MAX_CHANNELS: usize = 2;

struct EmptyParams;
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
}

impl SamplerNode {
    pub fn new(sampler: Sampler, store: Arc<SampleStore>) -> Self {
        Self {
            sampler,
            store,
            sample_rate: 0.0,
            scratch: Vec::new(),
            scratch_frames: 0,
        }
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
                _ => {}
            }
        }
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

        for (index, channel) in ctx.outputs.iter_mut().enumerate() {
            let source = &rendered[index.min(channels.saturating_sub(1))];
            for (out, sample) in channel[..frames].iter_mut().zip(source.iter()) {
                *out += *sample;
            }
        }
    }

    fn reset(&mut self) {
        // A hard cut, not a release: a release tail from before a seek would
        // play over the top of wherever playback landed.
        self.sampler.reset();
        self.scratch.fill(0.0);
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
}

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
    Eq(fontelle_fx::ParametricEq),
    Compressor(fontelle_fx::Compressor),
}

impl EffectNode {
    /// An insert set up as the document says, with no live end.
    pub fn new(config: fontelle_types::EffectConfig) -> Self {
        Self {
            state: match config {
                fontelle_types::EffectConfig::Eq(_) => {
                    EffectState::Eq(fontelle_fx::ParametricEq::new())
                }
                fontelle_types::EffectConfig::Compressor(_) => {
                    EffectState::Compressor(fontelle_fx::Compressor::new())
                }
            },
            config,
            bypassed: false,
            controls: None,
            automated: [None; MAX_EFFECT_PARAMS],
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
    fn prepare(&mut self, ctx: &PrepareContext) {
        match &mut self.state {
            EffectState::Eq(eq) => eq.prepare(ctx.sample_rate),
            EffectState::Compressor(comp) => comp.prepare(ctx.sample_rate),
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.settle();
        self.take_automation(ctx);
        self.apply_automation();
        if self.bypassed {
            return;
        }
        // In place on the bus it was given: a chain is a run of inserts
        // scheduled on the same pair of buffers, in the order they run.
        match (&mut self.state, &self.config) {
            (EffectState::Eq(eq), fontelle_types::EffectConfig::Eq(config)) => {
                eq.process(ctx.outputs, config);
            }
            (EffectState::Compressor(comp), fontelle_types::EffectConfig::Compressor(config)) => {
                // No sidechain yet: routing one track's audio to another's
                // detector is the graph's job and belongs with sends (§13.2),
                // which are not compiled. The DSP takes one already.
                comp.process(ctx.outputs, None, config);
            }
            // A config of a different kind than the state cannot arrive: the
            // chain rebuilds the graph when a slot's *kind* changes, and only
            // tunes it in place when parameters move.
            _ => {}
        }
    }

    fn reset(&mut self) {
        match &mut self.state {
            EffectState::Eq(eq) => eq.reset(),
            EffectState::Compressor(comp) => comp.reset(),
        }
        // A full stop lets go of what automation was holding: the next thing
        // played starts from the document, and a `ParamValue` will arrive to
        // say otherwise if the playhead is inside a clip.
        self.automated = [None; MAX_EFFECT_PARAMS];
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

pub struct AudioClipNode {
    // TDD §15 (M6).
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
            self.limiter.process(ctx.outputs, &self.limiter_config);
        }
        // Metered *after* the limiter, because what the meter is for is
        // showing what left the machine.
        for (index, (meter, channel)) in self.meters.iter_mut().zip(ctx.outputs.iter()).enumerate()
        {
            meter.process_block(channel);
            self.published.record(index, meter.peak());
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
        };
        let instant = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
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
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
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
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
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
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
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
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
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
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
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
