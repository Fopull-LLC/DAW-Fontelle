//! A plugin somebody else wrote, in the graph (TDD §8.4).
//!
//! §8.4 said the `AudioNode` trait was *"the entire boundary a future CLAP
//! host would plug into"*, and this file is the whole of what that turned out
//! to cost: a node that hands its block to [`fontelle_host`] instead of to a
//! `fontelle-fx` effect or a `fontelle-core` sampler. Everything else about it
//! — where it sits in the schedule, how automation reaches it, what a bypass
//! does — is what it already was for the built-ins.

use std::sync::Arc;

use fontelle_host::{HostedProcessor, ParamValues, ProcessorBay};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};
use crate::nodes::{CHANNEL_GAIN_MAX_DB, CHANNEL_GAIN_MIN_DB, EmptyParams};

/// Which way round a hosted plugin is wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRole {
    /// It is played: notes in, sound out, nothing on its input.
    Instrument,
    /// It sits in an insert chain: the bus goes through it.
    Effect,
}

/// One hosted plugin, rendering.
///
/// # Where the processor comes from
///
/// Not from the constructor, which is the one thing about this node that is
/// not like the others. A plugin may be activated once, and the graph is built
/// and prepared *while the old one is still playing* — so at the moment this
/// node is made, the processor it needs is still inside the node it replaces.
/// It arrives through [`ProcessorBay`], which the retired node parks it in
/// when the main thread frees the old graph.
///
/// Until then this node is a **pass-through** rather than a silence: an insert
/// that briefly stops processing is a chain that sounds unprocessed for a
/// handful of blocks, where one that briefly stopped passing signal would be a
/// hole in the mix.
///
/// The same bay can also **ask for the processor back** while this node holds
/// it — the main thread reading an LV2 plugin's own state, which lives on the
/// instance inside the processor and may not be read while it runs. The node
/// parks it at the top of the next block and carries on without it until it
/// is parked again; see [`ProcessorBay::recall`].
///
/// # An instrument adds, it does not write
///
/// > *"when adding a plugin instrument i cannot hear my other instruments
/// > anymore at the same time"*
///
/// The graph clears every bus once a block and each source **adds** into it —
/// that is what lets several channels share a track. A plugin renders into
/// its own port buffers and the host copies them out, and copying them *over*
/// the bus silenced every instrument scheduled before this one on the same
/// bus. So an instrument renders into scratch of its own (sized in
/// [`prepare`](AudioNode::prepare), never here), takes the channel's level and
/// placement there, and adds the result to whatever the bus already carries.
pub struct PluginNode {
    bay: Arc<ProcessorBay>,
    processor: Option<HostedProcessor>,
    values: Arc<ParamValues>,
    role: PluginRole,
    /// Switched out of the chain. The **live end** when there is one, so a
    /// bypass can be flicked while listening rather than costing a graph
    /// rebuild — the same shape `EffectNode` takes its config through, and for
    /// the same reason.
    bypass: Option<Arc<std::sync::atomic::AtomicBool>>,
    bypassed: bool,
    /// Where in the song this block starts, so a note's sample can be turned
    /// into an offset inside it.
    block_start: fontelle_types::Sample,
    /// The **channel's** own level and placement, applied after the plugin has
    /// written its block (`fontelle_model::Channel::gain_db`).
    ///
    /// Not its mixer track's, and not the plugin's: several channels may share
    /// a track, so these are per-channel, and a plugin has no idea it is on
    /// one. `SamplerNode` applies the same two at the *voice*, where a patch
    /// can place each note of a chord separately; a plugin gives back a
    /// finished stereo pair, so this is a gain and a pan law over that pair,
    /// which is the most that can honestly be done to somebody else's output.
    ///
    /// Read on an instrument only. An insert is a stage of a strip, and the
    /// strip's fader is what sets its level.
    gain_db: f32,
    pan: f32,
    /// Where an instrument renders before being added to the bus: two
    /// channels of `scratch_frames`, allocated in `prepare` (INVARIANT 1).
    /// Empty until then, and an instrument that has not been prepared adds
    /// nothing — the graph prepares every node before its first block.
    scratch: Vec<f32>,
    scratch_frames: usize,
    /// What is sounding, so a **slide** has something to bend.
    ///
    /// A slide note names only the key it goes *to*; which note it moves is
    /// whatever is already sounding in its voice context, exactly as
    /// `fontelle_core::Sampler::slide` decides it for a built-in instrument.
    /// A plugin will not tell a host what it is playing, so the node keeps
    /// the score's own answer: every note it has sent and not yet ended.
    ///
    /// A fixed array, never a `Vec`: this is written on the audio thread
    /// (INVARIANT 1). Thirty-two notes is more than a hand, and a
    /// thirty-third is played and simply cannot be slid.
    sounding: [Option<Sounding>; MAX_SOUNDING],
    /// The **external sidechain**: another track's bus, left in a tap by a
    /// [`crate::KeyTapNode`] the compiler scheduled first — the same tap a
    /// built-in compressor reads (`docs/effects-catalogue.md` §2.1). Handed
    /// to the plugin's sidechain port; a plugin with none ignores it.
    key: Option<Arc<crate::KeyTap>>,
    /// Where the key is copied to, sized in `prepare` (INVARIANT 1).
    key_buffer: Vec<f32>,
    /// What the plugin said it delays by — see
    /// [`latency_samples`](AudioNode::latency_samples).
    latency: u32,
}

/// How many channels the scratch holds. A mixer bus is at most stereo.
const SCRATCH_CHANNELS: usize = 2;

/// How many notes at once can be bent — see [`PluginNode::sounding`].
const MAX_SOUNDING: usize = 32;

/// One note this node has started and not yet ended, and where its pitch is.
#[derive(Debug, Clone, Copy)]
struct Sounding {
    key: u8,
    /// §16.5's voice context: which notes a slide is allowed to move.
    context: u32,
    /// How far this note has been bent from the key it started on, in
    /// semitones.
    semitones: f32,
    /// Where the glide is going, and how fast it gets there in semitones per
    /// sample. A rate of zero is a note that is not gliding — either it has
    /// arrived, or the slide that moved it had no length.
    target: f32,
    rate: f32,
    /// What the plugin was last told, so a tuning is sent when it moves and
    /// not once a block for a note standing still.
    sent: f32,
}

impl PluginNode {
    pub fn new(bay: Arc<ProcessorBay>, values: Arc<ParamValues>, role: PluginRole) -> Self {
        Self {
            bay,
            processor: None,
            values,
            role,
            bypass: None,
            bypassed: false,
            block_start: 0,
            gain_db: 0.0,
            pan: 0.0,
            scratch: Vec::new(),
            scratch_frames: 0,
            sounding: [None; MAX_SOUNDING],
            key: None,
            key_buffer: Vec::new(),
            latency: 0,
        }
    }

    /// What the plugin reports it delays by, from
    /// [`fontelle_host::HostedPlugin::latency_samples`]. The graph builder
    /// reads the same number to hold the rest of the mix back (TDD §5.5);
    /// this is how it reaches anything that walks the built schedule.
    pub fn with_latency(mut self, latency: u32) -> Self {
        self.latency = latency;
        self
    }

    /// The track this insert's sidechain listens to — see the field.
    pub fn with_key(mut self, key: Arc<crate::KeyTap>) -> Self {
        self.key = Some(key);
        self
    }

    /// The channel's own level and placement — see the fields.
    pub fn on_channel(mut self, gain_db: f32, pan: f32) -> Self {
        self.gain_db = gain_db;
        self.pan = pan;
        self
    }

    /// A fixed bypass, for a render that has nobody to flick it.
    pub fn bypassed(mut self, bypassed: bool) -> Self {
        self.bypassed = bypassed;
        self
    }

    /// The live bypass switch this node reads every block.
    pub fn with_bypass(mut self, bypass: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.bypass = Some(bypass);
        self
    }

    fn is_bypassed(&self) -> bool {
        match &self.bypass {
            Some(bypass) => bypass.load(std::sync::atomic::Ordering::Relaxed),
            None => self.bypassed,
        }
    }

    pub fn role(&self) -> PluginRole {
        self.role
    }

    /// Tries to collect the processor, if this node has not got it yet —
    /// or hands it back, if the main thread has asked for it.
    ///
    /// RT-safe: a try that loses does nothing and is tried again next block.
    /// See [`ProcessorBay`] for why waiting would be the wrong answer, and
    /// [`ProcessorBay::recall`] for the request. While the request stands the
    /// node **does not take** the processor even if it is home: the main
    /// thread is on its way to it.
    fn claim(&mut self) {
        if self.bay.wants_return() {
            if let Some(processor) = self.processor.take()
                && let Err(processor) = self.bay.try_park(processor)
            {
                self.processor = Some(processor);
            }
            return;
        }
        if self.processor.is_none() {
            self.processor = self.bay.take();
        }
    }

    /// Reads this block's automation onto the parameter wire.
    ///
    /// The plugin's own id is the tail of the address, which is
    /// `.../param/<id>` for an insert and `.../patch/plugin/param/<id>` for an
    /// instrument — one rule, because §8.2 forbids a second addressing scheme
    /// and a plugin did not need one. An id this plugin does not have is
    /// ignored rather than refused (INVARIANT 7).
    fn take_automation(&mut self, ctx: &ProcessContext) {
        for event in ctx.events() {
            let fontelle_types::EventPayload::ParamValue { target, value } = &event.payload else {
                continue;
            };
            // The channel's own level and placement, under automation
            // (§12.2). Block-rate, like every other node's, and matched on the
            // tail of the address for the reason `SamplerNode` gives: the
            // compiler has already resolved it to this node, so the only
            // question left is which of this channel's controls it names.
            let address = target.as_str();
            if address.ends_with("/gain") {
                self.gain_db = CHANNEL_GAIN_MIN_DB
                    + *value as f32 * (CHANNEL_GAIN_MAX_DB - CHANNEL_GAIN_MIN_DB);
                continue;
            }
            if address.ends_with("/pan") {
                self.pan = *value as f32 * 2.0 - 1.0;
                continue;
            }
            let Some(id) = param_id(address) else {
                continue;
            };
            // **Normalised**, because every automation lane in this program is
            // — see `ParamValues::set_normalised`, which is where the plugin's
            // own range turns a lane's fraction back into a value it
            // understands.
            self.values.set_normalised(id, *value);
        }
    }

    fn take_notes(&mut self, ctx: &ProcessContext) {
        let Self {
            processor: Some(processor),
            sounding,
            block_start,
            ..
        } = self
        else {
            return;
        };
        let block_start = *block_start;
        let frames = ctx.outputs.first().map_or(0, |buffer| buffer.len());
        // **Before this block's events**, at frame zero, because everything
        // below is at a frame of its own and both formats want their events
        // in time order. A glide moves at block rate for the reason
        // `fontelle_core::Voice::advance_glide` gives: the pitch a note
        // plays at is worked out once a block already, and a glide that
        // moved per sample would be the only pitch modulation here that did.
        advance_glide(processor, sounding, frames);
        for event in ctx.events() {
            let frame = event.sample.saturating_sub(block_start) as usize;
            let frame = frame.min(frames.saturating_sub(1));
            match &event.payload {
                fontelle_types::EventPayload::NoteOn {
                    key,
                    velocity,
                    voice_context,
                    ..
                } => {
                    // A note starts at its own pitch. If this key was left
                    // bent — a retrigger of a note a slide had moved — the
                    // plugin is told so before the note begins, which is
                    // what puts a channel-wide bend back at zero.
                    forget(processor, sounding, frame, *key, *voice_context);
                    processor.note_on(frame, *key, *velocity as f64 / 127.0);
                    remember(sounding, *key, *voice_context);
                }
                fontelle_types::EventPayload::NoteOff { key, voice_context } => {
                    processor.note_off(frame, *key);
                    forget(processor, sounding, frame, *key, *voice_context);
                }
                // The wheels, whole: the host turns them into the MIDI or the
                // note expression the plugin speaks (see
                // `HostedProcessor::controller`).
                fontelle_types::EventPayload::Controller { controller, value } => {
                    processor.controller(frame, *controller, *value);
                }
                fontelle_types::EventPayload::PitchBend { value } => {
                    processor.pitch_bend(frame, *value);
                }
                fontelle_types::EventPayload::ChannelPressure { value } => {
                    processor.channel_pressure(frame, *value);
                }
                // A note's own pressure, bend or slide (MPE, §4.2), handed
                // to the plugin as the channel's: the router turned a
                // member channel's messages into per-note ones, and a
                // plugin that speaks no note expression still hears what it
                // would have heard. Per-note delivery for CLAP note
                // expressions is not built.
                fontelle_types::EventPayload::NoteMod {
                    pressure,
                    bend,
                    slide,
                    ..
                } => {
                    if let Some(value) = pressure {
                        processor.channel_pressure(frame, *value);
                    }
                    if let Some(value) = bend {
                        processor.pitch_bend(frame, *value);
                    }
                    if let Some(value) = slide {
                        processor.controller(frame, 74, *value);
                    }
                }
                // A slide bends what is already sounding to its key and
                // starts nothing — the same thing a slide does to a
                // built-in instrument (`fontelle_core::Sampler::slide`),
                // and a slide with nothing sounding still does nothing.
                // Every note in the context, because a slide under a chord
                // moves the chord.
                fontelle_types::EventPayload::NoteSlide {
                    key,
                    glide_samples,
                    voice_context,
                } => {
                    for note in sounding.iter_mut().flatten() {
                        if note.context != *voice_context {
                            continue;
                        }
                        note.target = f32::from(*key) - f32::from(note.key);
                        note.rate = if *glide_samples == 0 {
                            note.semitones = note.target;
                            0.0
                        } else {
                            (note.target - note.semitones).abs() / *glide_samples as f32
                        };
                    }
                    // Whatever arrived at once — a slide with no length —
                    // is heard at the frame it happened on rather than at
                    // the top of the next block.
                    send_tuning(processor, sounding, frame);
                }
                _ => {}
            }
        }
    }
}

/// Steps every gliding note on by one block, and tells the plugin.
///
/// At frame zero: see the call site.
fn advance_glide(
    processor: &mut HostedProcessor,
    sounding: &mut [Option<Sounding>; MAX_SOUNDING],
    frames: usize,
) {
    for note in sounding.iter_mut().flatten() {
        if note.rate <= 0.0 {
            continue;
        }
        let step = note.rate * frames as f32;
        let remaining = note.target - note.semitones;
        if remaining.abs() <= step {
            note.semitones = note.target;
            note.rate = 0.0;
        } else {
            note.semitones += step * remaining.signum();
        }
    }
    send_tuning(processor, sounding, 0);
}

/// Tells the plugin about every note whose pitch has moved since it was last
/// told, at `frame`.
fn send_tuning(
    processor: &mut HostedProcessor,
    sounding: &mut [Option<Sounding>; MAX_SOUNDING],
    frame: usize,
) {
    for note in sounding.iter_mut().flatten() {
        if note.semitones != note.sent {
            processor.note_tuning(frame, note.key, f64::from(note.semitones));
            note.sent = note.semitones;
        }
    }
}

/// Writes a note down as sounding, so a slide can find it. A table that is
/// full keeps what it has: the newest note is the one that cannot be slid,
/// and it still plays.
fn remember(sounding: &mut [Option<Sounding>; MAX_SOUNDING], key: u8, context: u32) {
    if let Some(slot) = sounding.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some(Sounding {
            key,
            context,
            semitones: 0.0,
            target: 0.0,
            rate: 0.0,
            sent: 0.0,
        });
    }
}

/// Takes a note off the table, putting its pitch back first.
///
/// The zero matters for a plugin that hears pitch as a **channel** bend —
/// every LV2 one — where a slide left behind would bend the next note
/// played. For a CLAP plugin the expression is the ending note's own and
/// costs nothing. Any other note still bent is then told its own pitch
/// again, so that a channel-wide plugin lands on what is still sounding
/// rather than on the note that stopped.
fn forget(
    processor: &mut HostedProcessor,
    sounding: &mut [Option<Sounding>; MAX_SOUNDING],
    frame: usize,
    key: u8,
    context: u32,
) {
    let found = sounding
        .iter_mut()
        .find(|slot| slot.is_some_and(|note| note.key == key && note.context == context));
    let Some(slot) = found else {
        return;
    };
    let was_bent = slot.is_some_and(|note| note.sent != 0.0);
    *slot = None;
    if !was_bent {
        return;
    }
    processor.note_tuning(frame, key, 0.0);
    for note in sounding.iter_mut().flatten() {
        if note.sent != 0.0 {
            note.sent = 0.0;
        }
    }
    send_tuning(processor, sounding, frame);
}

/// Applies the channel's level and placement to a finished stereo pair.
///
/// Constant power, the law a *source* is placed with — see
/// `fontelle_model::Channel::pan` for why that is not the same control as a
/// mixer track's balance, and why applying both laws would pull a second 3 dB
/// out of every centred part.
///
/// A bus with one channel takes the whole of it: panning a source into a mono
/// bus would be throwing half of it away.
fn place(outputs: &mut [&mut [f32]], frames: usize, gain_db: f32, pan: f32) {
    let gain = if gain_db == 0.0 {
        1.0
    } else {
        10f32.powf(gain_db / 20.0)
    };
    let stereo = outputs.len() > 1;
    let (left, right) = fontelle_types::PanLaw::Minus3Db.gains(pan);
    for (index, channel) in outputs.iter_mut().enumerate() {
        let side = match (stereo, index) {
            (false, _) => 1.0,
            (true, 0) => left,
            (true, _) => right,
        };
        let scale = gain * side;
        if scale == 1.0 {
            continue;
        }
        let frames = frames.min(channel.len());
        for sample in channel[..frames].iter_mut() {
            *sample *= scale;
        }
    }
}

/// The plugin's own parameter id, from the tail of an address.
fn param_id(address: &str) -> Option<u32> {
    address.rsplit_once("/param/")?.1.parse().ok()
}

impl AudioNode for PluginNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        // The plugin itself is prepared by `HostedPlugin::activate`, which is
        // where the sample rate and block size it was given come from, and
        // which happens on the main thread before the graph is built. A rate
        // change rebuilds the plugin rather than reaching into it here. What
        // is sized here is the node's own scratch — see the type's note.
        self.scratch_frames = ctx.max_block_size as usize;
        self.scratch
            .resize(self.scratch_frames * SCRATCH_CHANNELS, 0.0);
        self.key_buffer.resize(self.scratch_frames, 0.0);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.claim();
        self.block_start = ctx.sample_range.start;
        self.take_automation(ctx);
        // A bypassed insert leaves the bus exactly as it arrived: its buffers
        // already carry the signal, so doing nothing *is* passing it through.
        //
        // Automation is still read above, so a lane keeps its place while the
        // switch is off and the effect is where the lane says the moment it
        // comes back on. Notes are not, and need not be: only an insert has a
        // bypass — `realise` gives the switch to the effect role alone.
        if self.is_bypassed() {
            return;
        }
        self.take_notes(ctx);
        let frames = ctx.outputs.first().map_or(0, |buffer| buffer.len());
        let Some(processor) = &mut self.processor else {
            return;
        };
        let (gain_db, pan) = (self.gain_db, self.pan);
        match self.role {
            PluginRole::Effect => match &self.key {
                Some(tap) if !self.key_buffer.is_empty() => {
                    let frames = frames.min(self.key_buffer.len());
                    tap.read_into(&mut self.key_buffer[..frames]);
                    processor.process_insert_keyed(ctx.outputs, &self.key_buffer[..frames], frames);
                }
                _ => processor.process_insert(ctx.outputs, frames),
            },
            PluginRole::Instrument => {
                // Into scratch, placed there, and **added** to the bus — see
                // the type's note on why not straight onto it.
                let frames = frames.min(self.scratch_frames);
                let channels = ctx.outputs.len().clamp(1, SCRATCH_CHANNELS);
                let (first, second) = self.scratch.split_at_mut(self.scratch_frames);
                let mut rendered: [&mut [f32]; SCRATCH_CHANNELS] =
                    [&mut first[..frames], &mut second[..frames]];
                processor.process_instrument(&mut rendered[..channels], frames);
                place(&mut rendered[..channels], frames, gain_db, pan);
                for (index, channel) in ctx.outputs.iter_mut().enumerate() {
                    let source = &rendered[index.min(channels - 1)];
                    for (out, sample) in channel[..frames].iter_mut().zip(source.iter()) {
                        *out += *sample;
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        // Nothing is sounding after a reset, so nothing is left for a slide
        // to bend — and the pitches go with the notes.
        self.sounding = [None; MAX_SOUNDING];
        if let Some(processor) = &mut self.processor {
            processor.reset();
        }
    }

    fn latency_samples(&self) -> u32 {
        // What the plugin declared, handed over when this node was built —
        // the graph builder holds the rest of the mix back by it (TDD §5.5).
        // Zero for a plugin that declares none, for every LV2 plugin (whose
        // answer is a control port this build does not read) and for every
        // bridged one.
        self.latency
    }

    fn debug_name(&self) -> &'static str {
        match self.role {
            PluginRole::Instrument => "plugin-instrument",
            PluginRole::Effect => "plugin-effect",
        }
    }

    fn params(&self) -> &dyn ParamSet {
        // Automation reaches this node as events, like every other node's —
        // see `take_automation`. Nothing reads a plugin node through the
        // parameter set.
        &EmptyParams
    }
}

impl Drop for PluginNode {
    /// Puts the processor back where the next graph can find it.
    ///
    /// **Off the audio thread**, which the engine already guarantees:
    /// `GraphPublisher::reclaim` is the one place a live graph dies, and it
    /// runs on the main thread. A plugin whose processor never came back would
    /// be a plugin that could never be deactivated, and `clack` leaks rather
    /// than free one on the wrong thread.
    fn drop(&mut self) {
        if let Some(processor) = self.processor.take() {
            self.bay.park(processor);
        }
    }
}
