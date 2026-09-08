//! The half of a plugin that renders, and where it lives between graphs.

use std::sync::Arc;

use clack_host::events::Match;
use clack_host::events::event_types::{
    MidiEvent, NoteExpressionEvent, NoteExpressionType, NoteOffEvent, NoteOnEvent, ParamValueEvent,
};
use clack_host::prelude::*;
use clack_host::utils::Cookie;

use crate::bridge::BridgedProcessor;
use crate::lv2::Lv2Processor;
use crate::param::ParamValues;

/// The most events one block may carry into a plugin.
///
/// Sized once at activation and never grown: a block that brought more than
/// this would otherwise allocate on the audio thread. Two hundred and fifty
/// six is a chord of every key on a keyboard twice over plus a parameter
/// sweep, at a block size where anything approaching it is already a stuck
/// note.
const MAX_EVENTS: u32 = 256;

/// How far a full pitch bend goes when it has to be said in semitones — the
/// two either way every keyboard defaults to. A plugin that takes MIDI gets
/// the raw fourteen bits and applies its own range instead.
const BEND_RANGE_SEMITONES: f64 = 2.0;

/// A plugin's audio half — what the graph holds and the RT thread calls.
///
/// [`Send`] and not [`Sync`], which is exactly the shape CLAP describes: this
/// may be moved to the audio thread and used there, and the main-thread half
/// ([`crate::HostedPlugin`]) stays behind and may be used at the same time.
/// The plugin is what synchronises the two, and that is its job rather than
/// this program's. For LV2 this *is* the plugin — see [`crate::lv2`] — and
/// the same shape holds.
///
/// The bus copies in and out are shared between the formats and live here;
/// what differs is inside [`Inner`].
pub struct HostedProcessor {
    inner: Inner,
}

enum Inner {
    Clap(ClapProcessor),
    /// Boxed: an LV2 processor carries its whole instance and is several
    /// times the size of the other two, and it is made once per activation
    /// rather than per block.
    Lv2(Box<Lv2Processor>),
    Bridged(BridgedProcessor),
}

/// The host handler set, named once so the types below stay readable.
type HostHandlersOf = crate::plugin::FontelleHost;

impl HostedProcessor {
    pub(crate) fn clap(
        processor: StartedPluginAudioProcessor<HostHandlersOf>,
        values: Arc<ParamValues>,
        inputs: crate::plugin::PortLayout,
        outputs: crate::plugin::PortLayout,
        dialect: Option<crate::plugin::NoteDialect>,
        max_block: usize,
    ) -> Self {
        Self {
            inner: Inner::Clap(ClapProcessor::new(
                processor, values, inputs, outputs, dialect, max_block,
            )),
        }
    }

    pub(crate) fn lv2(processor: Lv2Processor) -> Self {
        Self {
            inner: Inner::Lv2(Box::new(processor)),
        }
    }

    pub(crate) fn bridged(processor: BridgedProcessor) -> Self {
        Self {
            inner: Inner::Bridged(processor),
        }
    }

    /// The block size this was prepared for.
    pub fn max_block(&self) -> usize {
        match &self.inner {
            Inner::Clap(p) => p.max_block,
            Inner::Lv2(p) => p.max_block(),
            Inner::Bridged(p) => p.max_block(),
        }
    }

    /// **RT.** Adds a note to this block. `frame` is where in the block it
    /// happens, `velocity` runs 0..1.
    ///
    /// Notes must be added in time order, which is how the graph hands them
    /// over: both formats require an ordered event list and a plugin is
    /// entitled to stop reading at the first one out of order.
    pub fn note_on(&mut self, frame: usize, key: u8, velocity: f64) {
        match &mut self.inner {
            Inner::Clap(p) => p.note_on(frame, key, velocity),
            Inner::Lv2(p) => p.note_on(frame, key, velocity),
            Inner::Bridged(p) => p.note_on(frame, key, velocity),
        }
    }

    /// **RT.** Ends a note. See [`note_on`](Self::note_on).
    pub fn note_off(&mut self, frame: usize, key: u8) {
        match &mut self.inner {
            Inner::Clap(p) => p.note_off(frame, key),
            Inner::Lv2(p) => p.note_off(frame, key),
            Inner::Bridged(p) => p.note_off(frame, key),
        }
    }

    /// **RT.** A continuous controller — the mod wheel is `1` — at `frame`,
    /// `value` in `0..=127`.
    ///
    /// **Performance, not automation**: this is the wheel the player moved,
    /// carried to the plugin in the language its note port speaks
    /// ([`crate::NoteDialect`]). To a plugin that takes MIDI it is the three
    /// bytes it was, on channel 0 like the notes. To one that takes only
    /// CLAP's own events it is the nearest **note expression**, on every
    /// sounding note: 1 vibrato, 7 volume, 10 pan, 11 expression, 74
    /// brightness — and any other controller is dropped, because there is
    /// no honest place to put it and a parameter is not one. A plugin with
    /// no note port ignores all of this.
    pub fn controller(&mut self, frame: usize, controller: u8, value: u8) {
        match &mut self.inner {
            Inner::Clap(p) => p.controller(frame, controller, value),
            Inner::Lv2(p) => p.controller(frame, controller, value),
            // ABI 3 carries the rest of a performance; what a bridge makes
            // of a controller is the bridged plugin's business, the same as
            // for the two hosted formats.
            Inner::Bridged(p) => p.controller(frame, controller, value),
        }
    }

    /// **RT.** A pitch bend at `frame`, centred at zero over `-8192..=8191`.
    /// MIDI bytes to a plugin that takes them; a `Tuning` expression of up
    /// to two semitones either way — the range every keyboard defaults to —
    /// otherwise.
    pub fn pitch_bend(&mut self, frame: usize, value: i16) {
        match &mut self.inner {
            Inner::Clap(p) => p.pitch_bend(frame, value),
            Inner::Lv2(p) => p.pitch_bend(frame, value),
            Inner::Bridged(p) => p.pitch_bend(frame, value),
        }
    }

    /// **RT.** Bends one **sounding note** to `semitones` away from the key
    /// it was started on — what a slide note is (TDD §16.5), and the one
    /// piece of a performance that is about a note rather than a channel.
    ///
    /// CLAP has exactly this: a `Tuning` note expression addressed to the
    /// key, in semitones, with no range limit — so a slide of an octave is
    /// an octave. MIDI has no per-note pitch at all, so an LV2 plugin gets
    /// a **channel** bend, which reaches the two semitones either way every
    /// keyboard defaults to and no further: a slide past that lands at the
    /// limit rather than being invented as something else, and the honest
    /// carrier for more is MPE, which this build does not speak. A bridge's
    /// table carries one pitch for the instrument as well, so a bridged
    /// plugin is bent the same way an LV2 one is.
    pub fn note_tuning(&mut self, frame: usize, key: u8, semitones: f64) {
        let as_bend = || {
            let fraction = (semitones / BEND_RANGE_SEMITONES).clamp(-1.0, 1.0);
            // MIDI's own asymmetry: a full bend down is 8192 steps and a
            // full bend up is 8191, because zero is a value.
            let span = if fraction < 0.0 { 8192.0 } else { 8191.0 };
            (fraction * span).round() as i16
        };
        match &mut self.inner {
            Inner::Clap(p) => p.note_tuning(frame, key, semitones),
            Inner::Lv2(p) => p.pitch_bend(frame, as_bend()),
            Inner::Bridged(p) => p.pitch_bend(frame, as_bend()),
        }
    }

    /// **RT.** Channel aftertouch at `frame`, `0..=127`. MIDI, or the
    /// `Pressure` expression.
    pub fn channel_pressure(&mut self, frame: usize, value: u8) {
        match &mut self.inner {
            Inner::Clap(p) => p.channel_pressure(frame, value),
            Inner::Lv2(p) => p.channel_pressure(frame, value),
            Inner::Bridged(p) => p.channel_pressure(frame, value),
        }
    }

    /// **RT.** Everything sounding stops now.
    ///
    /// A note-off per key would be a hundred and twenty-eight events and would
    /// still leave a plugin's own tail ringing; CLAP's answer is `reset` and
    /// LV2's is "all sound off", and this is what the graph's `reset` reaches
    /// for.
    pub fn reset(&mut self) {
        match &mut self.inner {
            Inner::Clap(p) => p.reset(),
            Inner::Lv2(p) => p.reset(),
            Inner::Bridged(p) => p.reset(),
        }
    }

    /// **RT.** Runs one block through a plugin that takes audio in.
    pub fn process_effect<I, O>(&mut self, input: &[I], output: &mut [O], frames: usize)
    where
        I: AsRef<[f32]>,
        O: AsMut<[f32]>,
    {
        let frames = frames.min(self.max_block());
        match &mut self.inner {
            Inner::Clap(p) => {
                fill_input(p.main_input(), input, frames);
                p.run(frames, true);
                drain_output(p.main_output(), output, frames);
            }
            Inner::Lv2(p) => {
                fill_input(p.input(), input, frames);
                p.fill_key(None, frames);
                p.run(frames);
                drain_output(p.output(), output, frames);
            }
            Inner::Bridged(p) => {
                fill_input(p.input(), input, frames);
                p.run(frames);
                drain_output(p.output(), output, frames);
            }
        }
    }

    /// **RT.** Runs one block through a plugin sitting in an insert chain.
    ///
    /// In place on the bus it was handed, which is what an insert *is* — the
    /// same contract `EffectNode` follows and the same one every plugin API
    /// uses. The copy in and out is not avoidable: a plugin declares its own
    /// channel count and is handed its own port arrays, so the bus and what
    /// the plugin reads are not the same memory even when they are the same
    /// width.
    pub fn process_insert<B>(&mut self, bus: &mut [B], frames: usize)
    where
        B: AsMut<[f32]>,
    {
        let frames = frames.min(self.max_block());
        match &mut self.inner {
            Inner::Clap(p) => {
                fill_input_mut(p.main_input(), bus, frames);
                p.fill_key(None, frames);
                p.run(frames, true);
                drain_output(p.main_output(), bus, frames);
            }
            Inner::Lv2(p) => {
                fill_input_mut(p.input(), bus, frames);
                p.fill_key(None, frames);
                p.run(frames);
                drain_output(p.output(), bus, frames);
            }
            Inner::Bridged(p) => {
                fill_input_mut(p.input(), bus, frames);
                p.run(frames);
                drain_output(p.output(), bus, frames);
            }
        }
    }

    /// **RT.** [`process_insert`](Self::process_insert), with another
    /// track's bus on the plugin's **sidechain** input.
    ///
    /// `key` is mono — what a [`fontelle_engine::KeyTap`] carries, and what
    /// every detector wants — and goes to every channel of the sidechain
    /// port — a CLAP input port that is not the main one, or an LV2 audio
    /// input with `lv2:isSideChain`. A plugin with no such port (every
    /// bridged one, and one that declared only its main input) is run
    /// exactly as `process_insert` would: the key is an edge that ordered
    /// the graph and fed nothing.
    ///
    /// [`fontelle_engine::KeyTap`]: https://docs.rs/fontelle-engine
    pub fn process_insert_keyed<B>(&mut self, bus: &mut [B], key: &[f32], frames: usize)
    where
        B: AsMut<[f32]>,
    {
        let frames = frames.min(self.max_block());
        match &mut self.inner {
            Inner::Clap(p) => {
                fill_input_mut(p.main_input(), bus, frames);
                p.fill_key(Some(key), frames);
                p.run(frames, true);
                drain_output(p.main_output(), bus, frames);
            }
            Inner::Lv2(p) => {
                fill_input_mut(p.input(), bus, frames);
                p.fill_key(Some(key), frames);
                p.run(frames);
                drain_output(p.output(), bus, frames);
            }
            Inner::Bridged(_) => self.process_insert(bus, frames),
        }
    }

    /// **RT.** Runs one block through a plugin that makes its own sound.
    pub fn process_instrument<O>(&mut self, output: &mut [O], frames: usize)
    where
        O: AsMut<[f32]>,
    {
        let frames = frames.min(self.max_block());
        match &mut self.inner {
            Inner::Clap(p) => {
                p.run(frames, false);
                drain_output(p.main_output(), output, frames);
            }
            Inner::Lv2(p) => {
                p.run(frames);
                drain_output(p.output(), output, frames);
            }
            Inner::Bridged(p) => {
                p.run(frames);
                drain_output(p.output(), output, frames);
            }
        }
    }

    /// **Main thread only.** An LV2 plugin's own state, read off the instance
    /// this processor carries. `None` for the other formats, whose state is
    /// on their main-thread half, and for an LV2 plugin that keeps none.
    ///
    /// The whole reason a snapshot may need the processor — see
    /// [`crate::HostedPlugin::snapshot_with`].
    pub(crate) fn lv2_save_state(&mut self) -> Option<Vec<u8>> {
        match &mut self.inner {
            Inner::Lv2(p) => p.save_state(),
            Inner::Clap(_) | Inner::Bridged(_) => None,
        }
    }

    /// Gives a CLAP processor back so the plugin can be deactivated. `None`
    /// for an LV2 one, which is deactivated by being dropped.
    pub(crate) fn into_clap_stopped(self) -> Option<StoppedPluginAudioProcessor<HostHandlersOf>> {
        match self.inner {
            Inner::Clap(p) => Some(p.processor.stop_processing()),
            Inner::Lv2(_) | Inner::Bridged(_) => None,
        }
    }
}

/// Copies the bus into the shape the plugin asked for.
///
/// A channel the plugin wants that the bus does not have takes the last
/// one there is — which turns a mono bus into a stereo plugin's two equal
/// channels rather than into one channel and a silence.
fn fill_input<I: AsRef<[f32]>>(target: &mut [Vec<f32>], input: &[I], frames: usize) {
    for (index, channel) in target.iter_mut().enumerate() {
        let source = input.get(index.min(input.len().saturating_sub(1)));
        match source {
            Some(source) => {
                let source = source.as_ref();
                for (frame, sample) in channel[..frames].iter_mut().enumerate() {
                    *sample = source.get(frame).copied().unwrap_or(0.0);
                }
            }
            None => channel[..frames].fill(0.0),
        }
    }
}

/// [`fill_input`] for a bus that is about to be written back over.
fn fill_input_mut<B: AsMut<[f32]>>(target: &mut [Vec<f32>], bus: &mut [B], frames: usize) {
    for (index, channel) in target.iter_mut().enumerate() {
        match bus.get_mut(index.min(bus.len().saturating_sub(1))) {
            Some(source) => {
                let source = source.as_mut();
                let taken = frames.min(source.len());
                channel[..taken].copy_from_slice(&source[..taken]);
                channel[taken..frames].fill(0.0);
            }
            None => channel[..frames].fill(0.0),
        }
    }
}

/// And back out again. See [`fill_input`] for the mismatch rule, which is
/// the same in this direction: a mono plugin arrives on both sides of a
/// stereo bus rather than on the left only.
fn drain_output<O: AsMut<[f32]>>(produced: &[Vec<f32>], output: &mut [O], frames: usize) {
    for (index, channel) in output.iter_mut().enumerate() {
        let channel = channel.as_mut();
        let frames = frames.min(channel.len());
        if produced.is_empty() {
            channel[..frames].fill(0.0);
            continue;
        }
        let source = &produced[index.min(produced.len() - 1)];
        channel[..frames].copy_from_slice(&source[..frames]);
    }
}

/// The CLAP half.
struct ClapProcessor {
    processor: StartedPluginAudioProcessor<HostHandlersOf>,
    values: Arc<ParamValues>,
    /// This block's events, in time order: parameters first, then notes.
    events: EventBuffer,
    /// Whatever the plugin says back. Read for nothing yet — a plugin's own
    /// parameter gestures land here — but it must exist, because a plugin
    /// given nowhere to write is entitled to misbehave.
    ///
    /// Sized like the input list and cleared each block. A plugin that writes
    /// more than [`MAX_EVENTS`] in one block grows it, which is an allocation
    /// on the audio thread — the plugin's, not ours, and nothing a host can
    /// prevent short of dropping what it said.
    replies: EventBuffer,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    /// What the plugin reads: **one buffer set per port it declared**, each
    /// in the channel count *it* declared. The bus goes into `main_in`, a
    /// key into `key_in`; the others stay at the silence `new` left there.
    /// See [`crate::plugin::PortLayout`] for the crash that made this every
    /// port and not the main one.
    input: Vec<Vec<Vec<f32>>>,
    main_in: usize,
    /// The sidechain — the first input port that is not the main one — when
    /// the plugin declared one. See [`fill_key`](Self::fill_key).
    key_in: Option<usize>,
    /// What it writes, likewise. The main port is copied out afterwards,
    /// which is what lets a mono plugin sit on a stereo bus and a stereo one
    /// on a mono bus; the rest is rendered and dropped.
    output: Vec<Vec<Vec<f32>>>,
    main_out: usize,
    max_block: usize,
    /// CLAP's steady sample counter. Must never go backwards without a reset.
    steady: u64,
    /// How the note port is spoken to — see [`crate::NoteDialect`]. `None`
    /// for a plugin with no note port, which is handed no controllers.
    dialect: Option<crate::plugin::NoteDialect>,
}

impl ClapProcessor {
    fn new(
        processor: StartedPluginAudioProcessor<HostHandlersOf>,
        values: Arc<ParamValues>,
        inputs: crate::plugin::PortLayout,
        outputs: crate::plugin::PortLayout,
        dialect: Option<crate::plugin::NoteDialect>,
        max_block: usize,
    ) -> Self {
        let buffers = |layout: &crate::plugin::PortLayout| -> Vec<Vec<Vec<f32>>> {
            layout
                .channels
                .iter()
                .map(|&channels| vec![vec![0.0; max_block]; channels as usize])
                .collect()
        };
        // Room for every channel of every port, so the pointer lists never
        // grow on the audio thread.
        let ports = |layout: &crate::plugin::PortLayout| {
            AudioPorts::with_capacity(
                layout
                    .channels
                    .iter()
                    .map(|&c| c as usize)
                    .sum::<usize>()
                    .max(1),
                layout.channels.len().max(1),
            )
        };
        Self {
            processor,
            values,
            events: EventBuffer::with_capacity(MAX_EVENTS as usize),
            replies: EventBuffer::with_capacity(MAX_EVENTS as usize),
            input_ports: ports(&inputs),
            output_ports: ports(&outputs),
            input: buffers(&inputs),
            main_in: inputs.main,
            key_in: inputs.key(),
            output: buffers(&outputs),
            main_out: outputs.main,
            max_block,
            steady: 0,
            dialect,
        }
    }

    /// The main input port's channels — where the bus goes. Empty for an
    /// instrument.
    fn main_input(&mut self) -> &mut [Vec<f32>] {
        self.input
            .get_mut(self.main_in)
            .map_or(&mut [], |port| port.as_mut_slice())
    }

    /// Puts `key` on every channel of the sidechain port, or silence when
    /// there is none to put — **every block**, so a key handed over once
    /// does not go on ducking after the tap it came from is gone.
    fn fill_key(&mut self, key: Option<&[f32]>, frames: usize) {
        let Some(port) = self.key_in.and_then(|index| self.input.get_mut(index)) else {
            return;
        };
        for channel in port.iter_mut() {
            match key {
                Some(key) => {
                    let taken = frames.min(key.len());
                    channel[..taken].copy_from_slice(&key[..taken]);
                    channel[taken..frames].fill(0.0);
                }
                None => channel[..frames].fill(0.0),
            }
        }
    }

    /// The main output port's channels — what the bus takes.
    fn main_output(&self) -> &[Vec<f32>] {
        self.output
            .get(self.main_out)
            .map_or(&[], |port| port.as_slice())
    }

    fn note_on(&mut self, frame: usize, key: u8, velocity: f64) {
        if self.events.len() >= MAX_EVENTS {
            return;
        }
        self.events.push(&NoteOnEvent::new(
            frame as u32,
            Pckn::new(0u16, 0u16, key as u16, Match::All),
            velocity.clamp(0.0, 1.0),
        ));
    }

    fn note_off(&mut self, frame: usize, key: u8) {
        if self.events.len() >= MAX_EVENTS {
            return;
        }
        self.events.push(&NoteOffEvent::new(
            frame as u32,
            Pckn::new(0u16, 0u16, key as u16, Match::All),
            0.0,
        ));
    }

    /// A controller, in the note port's dialect — see
    /// [`crate::HostedProcessor::controller`] for the mapping.
    fn controller(&mut self, frame: usize, controller: u8, value: u8) {
        match self.dialect {
            None => {}
            Some(crate::plugin::NoteDialect::Midi) => {
                self.midi(frame, [0xB0, controller.min(127), value.min(127)]);
            }
            Some(crate::plugin::NoteDialect::Clap) => {
                let expression = match controller {
                    1 => NoteExpressionType::Vibrato,
                    7 => NoteExpressionType::Volume,
                    10 => NoteExpressionType::Pan,
                    11 => NoteExpressionType::Expression,
                    74 => NoteExpressionType::Brightness,
                    // No honest place to put it — see the public method.
                    _ => return,
                };
                self.expression(frame, expression, f64::from(value.min(127)) / 127.0);
            }
        }
    }

    fn pitch_bend(&mut self, frame: usize, value: i16) {
        let value = value.clamp(-8192, 8191);
        match self.dialect {
            None => {}
            Some(crate::plugin::NoteDialect::Midi) => {
                let raw = (i32::from(value) + 8192) as u16;
                self.midi(frame, [0xE0, (raw & 0x7F) as u8, ((raw >> 7) & 0x7F) as u8]);
            }
            Some(crate::plugin::NoteDialect::Clap) => {
                // In semitones, over the two either way a keyboard defaults
                // to. CLAP's tuning expression is relative and in semitones.
                let semitones = f64::from(value) / 8192.0 * BEND_RANGE_SEMITONES;
                self.expression(frame, NoteExpressionType::Tuning, semitones);
            }
        }
    }

    fn channel_pressure(&mut self, frame: usize, value: u8) {
        match self.dialect {
            None => {}
            Some(crate::plugin::NoteDialect::Midi) => {
                self.midi(frame, [0xD0, value.min(127), 0]);
            }
            Some(crate::plugin::NoteDialect::Clap) => {
                self.expression(
                    frame,
                    NoteExpressionType::Pressure,
                    f64::from(value.min(127)) / 127.0,
                );
            }
        }
    }

    /// A tuning expression on **one key** — see
    /// [`crate::HostedProcessor::note_tuning`]. Notes reach a CLAP plugin as
    /// CLAP's own events whatever dialect its port speaks, so this does too;
    /// what the dialect decides is only how a *channel* controller travels.
    fn note_tuning(&mut self, frame: usize, key: u8, semitones: f64) {
        if self.dialect.is_none() || self.events.len() >= MAX_EVENTS {
            return;
        }
        self.events.push(&NoteExpressionEvent::new(
            frame as u32,
            Pckn::new(0u16, 0u16, key as u16, Match::All),
            NoteExpressionType::Tuning,
            semitones,
        ));
    }

    /// Three bytes on the first note port, at `frame`.
    fn midi(&mut self, frame: usize, bytes: [u8; 3]) {
        if self.events.len() >= MAX_EVENTS {
            return;
        }
        self.events.push(&MidiEvent::new(frame as u32, 0, bytes));
    }

    /// A note expression **on every note** — key, channel and note id all
    /// matched — which is what a channel-wide controller means.
    fn expression(&mut self, frame: usize, expression: NoteExpressionType, value: f64) {
        if self.events.len() >= MAX_EVENTS {
            return;
        }
        self.events.push(&NoteExpressionEvent::new(
            frame as u32,
            Pckn::new(0u16, Match::All, Match::All, Match::All),
            expression,
            value,
        ));
    }

    fn reset(&mut self) {
        self.processor.reset();
        self.events.clear();
        self.steady = 0;
    }

    fn run(&mut self, frames: usize, with_input: bool) {
        // Whatever moved since the last block, at the top of this one. Before
        // the notes, because the list has to be in time order and these are
        // all at frame zero.
        //
        // A parameter change is written at the *start* of the block rather
        // than where the knob moved, and that is the same trade
        // `EffectNode::take_automation` makes: sample-accurate parameter
        // changes would mean a plugin recomputing coefficients part way
        // through a block, and no knob a person turns is worth that.
        // Two disjoint field borrows rather than a scratch `Vec` collected
        // from the drain: the obvious spelling of this allocates once a block
        // on the audio thread, which is INVARIANT 1 exactly.
        let events = &mut self.events;
        self.values.drain(|id, value| {
            if events.len() >= MAX_EVENTS {
                return;
            }
            events.push(&ParamValueEvent::new(
                0,
                ClapId::new(id),
                Pckn::match_all(),
                value,
                Cookie::empty(),
            ));
        });

        self.replies.clear();
        let input_events = InputEvents::from_buffer(&self.events);
        let mut output_events = OutputEvents::from_buffer(&mut self.replies);

        let input_ports = &mut self.input_ports;
        let output_ports = &mut self.output_ports;
        let input = &mut self.input;
        let output = &mut self.output;

        // **Every port the plugin declared, every block** — even for an
        // instrument, whose input is silence, and *including the ports the
        // bus never touches*. CLAP says the host passes as many ports as the
        // plugin declared, and a plugin is entitled to take that at its
        // word: several refuse to render when `audio_inputs_count` is not
        // what they asked for, and nih-plug reads its auxiliary ports off
        // the end of whatever array it was handed — which, handed the main
        // port alone, was a crash in the first block of every OneTrick drum
        // synth. `with_input` decides whether the bus was *copied* into the
        // main port, not whether the ports are handed over; an instrument's
        // stay at the zeroes `new` left there.
        let audio_in = if input.is_empty() {
            InputAudioBuffers::empty()
        } else {
            if !with_input {
                for channel in input.iter_mut().flatten() {
                    channel[..frames].fill(0.0);
                }
            }
            input_ports.with_input_buffers(input.iter_mut().map(|port| {
                AudioPortBuffer {
                    latency: 0,
                    channels: AudioPortBufferType::f32_input_only(
                        port.iter_mut()
                            .map(|b| InputChannel::variable(&mut b[..frames])),
                    ),
                }
            }))
        };
        let mut audio_out = if output.is_empty() {
            OutputAudioBuffers::empty()
        } else {
            output_ports.with_output_buffers(output.iter_mut().map(|port| AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    port.iter_mut().map(|b| &mut b[..frames]),
                ),
            }))
        };

        let _ = self.processor.process(
            &audio_in,
            &mut audio_out,
            &input_events,
            &mut output_events,
            Some(self.steady),
            None,
        );
        self.steady = self.steady.wrapping_add(frames as u64);
        self.events.clear();
    }
}

/// Where a plugin's processor waits between graphs.
///
/// # Why this exists
///
/// A plugin may be activated once. The graph, meanwhile, is rebuilt whole
/// whenever anything structural changes, and the new graph is built and
/// prepared **while the old one is still playing** — so at the moment a new
/// plugin node is made, the processor it needs is inside the node it is
/// replacing.
///
/// The engine already settles exactly this for graphs themselves:
/// `GraphPublisher::reclaim` is where a retired graph dies, on the main
/// thread. So a retired node's `Drop` parks its processor here, and the new
/// node takes it the first time it renders. The cost is the handful of blocks
/// between the swap and the next reclaim, in which that plugin is silent —
/// against a structural edit that already restarts every sampler voice in the
/// project, which is what the engine has always done.
///
/// The lock is never *waited* on by the audio thread: the node tries, and a
/// block that cannot have it renders silence and tries again next time. A
/// contended try is a compare-exchange, which is what INVARIANT 1 allows and
/// a blocking wait is not.
///
/// # Asking for it back
///
/// The bay also carries a **request**: the main thread may need the processor
/// while a graph is playing it — an LV2 plugin's own state lives on the
/// instance inside the processor, and LV2 forbids reading it while `run`
/// executes. [`recall`](Self::recall) raises the request, the node sees it at
/// the top of its next block and parks the processor, the main thread takes
/// it, does what it needs, and parks it again for the node to pick up. The
/// blocks in between are the plugin's silence, and there are few of them: one
/// to hand over, one to take back, and however long the main thread held it.
#[derive(Default)]
pub struct ProcessorBay {
    parked: std::sync::Mutex<Option<HostedProcessor>>,
    /// The main thread wants the processor home. See [`recall`](Self::recall).
    wanted: std::sync::atomic::AtomicBool,
}

impl ProcessorBay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a processor in. Off the audio thread.
    pub fn park(&self, processor: HostedProcessor) {
        if let Ok(mut parked) = self.parked.lock() {
            *parked = Some(processor);
        }
    }

    /// **RT-safe.** Takes it out, if it is there and nobody else has the lock.
    pub fn take(&self) -> Option<HostedProcessor> {
        self.parked.try_lock().ok()?.take()
    }

    /// Whether the processor is home — which is what says a retired plugin can
    /// finally be dropped.
    pub fn is_parked(&self) -> bool {
        self.parked
            .lock()
            .map(|parked| parked.is_some())
            .unwrap_or(false)
    }

    /// Takes it out, waiting for the lock. Off the audio thread only.
    pub fn reclaim(&self) -> Option<HostedProcessor> {
        self.parked.lock().ok()?.take()
    }

    /// **RT-safe.** Puts a processor in without waiting: the one it was
    /// handed comes back if the lock was busy, and the node tries again next
    /// block.
    ///
    /// The `Err` is the processor itself and it is large; boxing it to make
    /// clippy happy would be an allocation on the audio thread, which is the
    /// one thing this call exists to avoid.
    #[allow(clippy::result_large_err)]
    pub fn try_park(&self, processor: HostedProcessor) -> Result<(), HostedProcessor> {
        match self.parked.try_lock() {
            Ok(mut parked) => {
                *parked = Some(processor);
                Ok(())
            }
            Err(_) => Err(processor),
        }
    }

    /// Asks whoever holds the processor to park it. The audio thread reads
    /// this through [`wants_return`](Self::wants_return) at the top of every
    /// block, and a node must not take a processor out while it is set.
    pub fn request_return(&self) {
        self.wanted
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Takes the request back — after it was answered, or after nobody did.
    pub fn withdraw_return(&self) {
        self.wanted
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// **RT-safe.** Whether the main thread has asked for the processor.
    pub fn wants_return(&self) -> bool {
        self.wanted.load(std::sync::atomic::Ordering::Acquire)
    }

    /// **Main thread.** Asks for the processor back and waits for it, up to
    /// `timeout`.
    ///
    /// Answered within a block or two when a graph is playing it, at once
    /// when it is already home, and `None` when nobody answers — an audio
    /// thread that has stopped, or a graph nothing is processing — in which
    /// case the request is withdrawn so a node that wakes later does not park
    /// a processor nobody is waiting for. The caller **parks it again** when
    /// it is done; the node picks it up on its next block.
    pub fn recall(&self, timeout: std::time::Duration) -> Option<HostedProcessor> {
        self.request_return();
        let started = std::time::Instant::now();
        let deadline = started + timeout;
        loop {
            if let Some(processor) = self.reclaim() {
                self.withdraw_return();
                if crate::atom::trace() {
                    eprintln!("[bay] recalled after {:?}", started.elapsed());
                }
                return Some(processor);
            }
            if std::time::Instant::now() >= deadline {
                self.withdraw_return();
                if crate::atom::trace() {
                    eprintln!("[bay] recall timed out after {:?}", started.elapsed());
                }
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
