//! One device's messages becoming timeline events (TDD §14.1-§14.3).
//!
//! This is where a live device's stream is turned into exactly the same
//! `TimedEvent`s the sequencer emits, and where the per-device mapping (§14.3)
//! is applied. It holds no locks, does no I/O and knows nothing about midir —
//! it is fed bytes and given somewhere to put the result, which is what makes
//! every behaviour below testable without plugging anything in.
//!
//! **It is also where stuck notes are prevented.** §14.2 requires that
//! disconnecting a device release its notes rather than leaving them hanging,
//! and that is only possible if something remembers what it started. The
//! router tracks every note it has sounded, per channel, in a bitset — which
//! is also what makes the sustain pedal and the panic messages work.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fontelle_types::{EventPayload, EventSink, NodeId, TimedEvent};

use crate::mapping::{DeviceMapping, InputSettings, LiveMapping, VelocityCurve};
use crate::message::{MidiMessage, decode};

/// Which instrument live MIDI is playing **right now** (TDD §14.3).
///
/// Shared between the UI thread, which moves it when the selection changes,
/// and every device callback, which reads it. One atomic rather than a lock:
/// a `midir` callback runs on the driver's own thread and may not block, and
/// reopening the port to change one integer would drop whatever was being
/// played across it.
///
/// A `NodeId` is a `slotmap` key, which is exactly a `u64` — `as_ffi` and
/// `from_ffi` are its own round trip, so nothing is being reinterpreted here.
#[derive(Debug)]
pub struct LiveTarget {
    node: AtomicU64,
}

/// The **null** node, not a zeroed `AtomicU64`: `KeyData::from_ffi(0)` is a
/// key of version one, which is a perfectly good id belonging to whatever
/// happens to be in slot zero. An app that has not realised a graph yet has to
/// be able to say "nowhere", and a live target that silently meant "the first
/// node" would play a random instrument until something set it.
impl Default for LiveTarget {
    fn default() -> Self {
        Self::new(NodeId::default())
    }
}

impl LiveTarget {
    pub fn new(node: NodeId) -> Self {
        Self {
            node: AtomicU64::new(node.to_bits()),
        }
    }

    pub fn get(&self) -> NodeId {
        NodeId::from_bits(self.node.load(Ordering::Relaxed))
    }

    /// Points live input at another instrument. Takes effect at the next
    /// message — see [`MidiRouter::handle`] for what happens to a note that is
    /// down when it does.
    pub fn set(&self, node: NodeId) {
        self.node.store(node.to_bits(), Ordering::Relaxed);
    }
}

/// Which keys are sounding **right now**, for the window to draw (TDD §14.1).
///
/// Written by every device callback and read by the UI thread once a frame.
/// Two atomics rather than a lock, for the reason [`LiveTarget`] is one: a
/// `midir` callback runs on the driver's own thread and may not block. There
/// is no `AtomicU128`, so the 128 keys are a pair of `u64`s — a reader can
/// therefore catch a chord half-written, which costs one frame of one key and
/// is not worth a lock on the audio-adjacent side to prevent.
///
/// One cell is shared by every device: there is one keyboard on screen, not
/// one per controller, so the bits are set and cleared rather than stored.
#[derive(Debug, Default)]
pub struct LiveKeys {
    /// Keys 0..64 and 64..128.
    halves: [AtomicU64; 2],
}

impl LiveKeys {
    /// Lights a key. Idempotent — a device that retriggers a key it is already
    /// holding is one lit key, not two.
    pub fn press(&self, key: u8) {
        let (half, bit) = Self::at(key);
        self.halves[half].fetch_or(bit, Ordering::Relaxed);
    }

    /// Puts it out.
    ///
    /// Two controllers holding the same key are one bit, so the first release
    /// takes the light out while the other is still holding it. A counter per
    /// key would fix that, and it is not worth 128 of them for a highlight
    /// that comes back the moment anything else happens on that key.
    pub fn release(&self, key: u8) {
        let (half, bit) = Self::at(key);
        self.halves[half].fetch_and(!bit, Ordering::Relaxed);
    }

    pub fn is_down(&self, key: u8) -> bool {
        let (half, bit) = Self::at(key);
        self.halves[half].load(Ordering::Relaxed) & bit != 0
    }

    /// Every key that is down, as one bit each — what a frame draws from.
    pub fn snapshot(&self) -> u128 {
        u128::from(self.halves[0].load(Ordering::Relaxed))
            | (u128::from(self.halves[1].load(Ordering::Relaxed)) << 64)
    }

    /// Which half a key lives in, and its bit within it. A key above 127
    /// cannot exist — `map_key` drops those — and is folded rather than
    /// panicking in a MIDI callback.
    fn at(key: u8) -> (usize, u64) {
        let key = (key & 0x7f) as usize;
        (key / 64, 1u64 << (key % 64))
    }
}

/// CC 64. The one controller worth handling before the learn table exists:
/// without it, half of playing a keyboard part is missing.
const CC_SUSTAIN: u8 = 64;
/// MPE's slide — the timbre dimension, CC 74 — which on a member channel
/// is the notes' and not the channel's.
const CC_SLIDE: u8 = 74;
/// A controller is "on" at 64 and above. The MIDI spec is explicit about this
/// and it is not 1: a continuous pedal sweeping through 40 is still up.
const CC_ON_THRESHOLD: u8 = 64;

pub struct MidiRouter {
    mapping: DeviceMapping,
    /// Where live input is pointed, which the UI thread may move at any time.
    target: Arc<LiveTarget>,
    /// The node this router is **currently** playing, latched from `target`.
    ///
    /// Not read fresh per message, and that is the whole correctness argument:
    /// a note-off has to reach the instrument its note-on went to. Selecting
    /// another channel while a key is down would otherwise send the release to
    /// the new instrument and leave the old note sounding with nothing left
    /// that could ever stop it.
    current: NodeId,
    voice_context: u32,
    /// What the window has the input settings set to, when there is a window
    /// (TDD §14.3). `None` on every offline path, which has nothing that
    /// could change them.
    settings: Option<Arc<LiveMapping>>,
    /// The settings this router is **currently** mapping with, latched from
    /// `settings` — the same latch, and the same argument, as `current`.
    ///
    /// Transpose is applied on the way in, so a note-off has to be mapped the
    /// way its note-on was. Read fresh per message, a nudge to transpose while
    /// a key is down would look for a held note at a key nothing was ever
    /// started on, drop the release, and leave the first note sounding with
    /// nothing left in the system able to stop it.
    current_settings: InputSettings,
    /// Bit per key, per channel: notes this router has sounded and not yet
    /// released.
    held: [u128; 16],
    /// The window's copy of what is sounding, when there is a window
    /// (TDD §14.1). `None` on every offline path, which has no keyboard to
    /// light. Mirrors `held | sustained` rather than the bytes on the wire:
    /// what lights is what the instrument is playing, transpose and all.
    lit: Option<Arc<LiveKeys>>,
    /// Notes whose note-off arrived while the pedal was down.
    sustained: [u128; 16],
    /// Bit per channel.
    pedal_down: u16,
}

impl MidiRouter {
    /// `target` is the engine node this device plays — §14.3's "optional
    /// per-device routing to a specific instrument channel". `voice_context`
    /// separates a live player's notes from the timeline's, so a sequenced
    /// note-off cannot cut a note the player is holding (TDD §11.4).
    pub fn new(target: NodeId, voice_context: u32, mapping: DeviceMapping) -> Self {
        Self::following(Arc::new(LiveTarget::new(target)), voice_context, mapping)
    }

    /// The same, following a target the caller can move — §14.3's routing,
    /// pointed at whichever instrument the window has selected rather than at
    /// whichever channel happened to be first in the song.
    pub fn following(target: Arc<LiveTarget>, voice_context: u32, mapping: DeviceMapping) -> Self {
        let current_settings = InputSettings {
            velocity_curve: mapping.velocity_curve,
            velocity_range: mapping.velocity_range,
            transpose_semitones: mapping.transpose_semitones,
            channel_filter: mapping.channel_filter,
        };
        Self {
            current: target.get(),
            mapping,
            target,
            voice_context,
            settings: None,
            current_settings,
            held: [0; 16],
            sustained: [0; 16],
            pedal_down: 0,
            lit: None,
        }
    }

    /// Mirrors what this router is sounding into a cell the window draws from
    /// — the highlight on the roll's keyboard (TDD §14.1).
    ///
    /// Shared with every other device the hub opens, so two controllers light
    /// one keyboard. See [`LiveKeys`].
    pub fn watching_keys(mut self, keys: Arc<LiveKeys>) -> Self {
        self.lit = Some(keys);
        self
    }

    /// Follows input settings the window can change while a device is open
    /// (TDD §14.3) — the velocity curve, the velocity window, transpose and
    /// the channel filter.
    ///
    /// Adopted straight away, so a router built with one mapping and handed a
    /// cell holding another plays by the cell rather than by whichever it was
    /// constructed with.
    pub fn following_input(mut self, settings: Arc<LiveMapping>) -> Self {
        self.adopt(settings.get());
        self.settings = Some(settings);
        self
    }

    /// Writes `settings` into the mapping this router is using.
    fn adopt(&mut self, settings: InputSettings) {
        self.mapping.velocity_curve = settings.velocity_curve;
        self.mapping.velocity_range = settings.velocity_range;
        self.mapping.transpose_semitones = settings.transpose_semitones;
        self.mapping.channel_filter = settings.channel_filter;
        self.current_settings = settings;
    }

    /// Decodes one MIDI packet and queues whatever it produces. Returns the
    /// number of events queued, which is zero for the many messages that are
    /// filtered, ignored, or merely change this router's own state.
    ///
    /// A `sink` that refuses (a full queue) is not retried: see
    /// `EventSink::send`.
    pub fn handle(&mut self, bytes: &[u8], sink: &mut dyn EventSink) -> usize {
        // Before anything is decoded, so a note-on that arrives after the
        // selection moved lands on the new instrument and whatever this router
        // was holding is let go on the old one.
        let released = self.follow_target(sink) + self.follow_settings(sink);
        released + self.decode_and_route(bytes, sink)
    }

    /// Adopts the window's input settings if they have moved, releasing
    /// everything sounding under the old ones first.
    ///
    /// The same shape as [`follow_target`](Self::follow_target) and for the
    /// same reason — see `current_settings`. Zero on every message where
    /// nothing changed, which is nearly all of them.
    fn follow_settings(&mut self, sink: &mut dyn EventSink) -> usize {
        let Some(cell) = &self.settings else {
            return 0;
        };
        let wanted = cell.get();
        if wanted == self.current_settings {
            return 0;
        }
        let released = self.release_all(sink);
        self.adopt(wanted);
        released
    }

    /// Adopts `target` if it has moved, releasing everything sounding on the
    /// node being left. Returns how many note-offs that took, which is zero on
    /// every message where nothing changed — which is nearly all of them.
    fn follow_target(&mut self, sink: &mut dyn EventSink) -> usize {
        let wanted = self.target.get();
        if wanted == self.current {
            return 0;
        }
        let released = self.release_all(sink);
        self.current = wanted;
        released
    }

    fn decode_and_route(&mut self, bytes: &[u8], sink: &mut dyn EventSink) -> usize {
        let Some(message) = decode(bytes) else {
            return 0;
        };
        if let Some(only) = self.mapping.channel_filter
            && message_channel(&message) != only
        {
            return 0;
        }

        match message {
            MidiMessage::NoteOn {
                channel,
                key,
                velocity,
            } => {
                let (low, high) = self.mapping.velocity_range;
                if velocity < low || velocity > high {
                    // Outside the device's velocity window: not this device's
                    // note. Nothing is recorded as held, so the matching
                    // note-off is dropped too rather than releasing a voice
                    // that was never started.
                    return 0;
                }
                let Some(out_key) = self.map_key(key) else {
                    return 0;
                };
                let channel = channel as usize;
                // Retriggering a key that the pedal was holding takes it out
                // of the pedal's set: the new note is the one that its
                // eventual note-off belongs to, and leaving the old bit set
                // would release it a second time when the pedal came up.
                self.sustained[channel] &= !(1u128 << out_key);
                self.held[channel] |= 1u128 << out_key;
                self.light(out_key, true);
                self.send(
                    sink,
                    EventPayload::NoteOn {
                        key: out_key,
                        velocity: self.mapping.velocity_curve.apply(velocity),
                        // A MIDI note-on carries none of §16.5's per-note
                        // properties — they are the score's, not the
                        // keyboard's — so a played note is a note as written.
                        pan: 0,
                        fine_pitch: 0,
                        release: 0,
                        mod_x: 0,
                        mod_y: 0,
                        voice_context: self.voice_context,
                    },
                )
            }
            MidiMessage::NoteOff { channel, key } => {
                let Some(out_key) = self.map_key(key) else {
                    return 0;
                };
                let channel = channel as usize;
                let bit = 1u128 << out_key;
                if self.held[channel] & bit == 0 {
                    // A note-off for something this router never sounded —
                    // a filtered note, or a duplicate. Forwarding it would
                    // release a voice belonging to the timeline or to another
                    // device.
                    return 0;
                }
                self.held[channel] &= !bit;
                if self.pedal_down & (1 << channel) != 0 {
                    // Still sounding, so still lit: the key is up but the
                    // pedal is what ends the note.
                    self.sustained[channel] |= bit;
                    return 0;
                }
                self.light(out_key, false);
                self.send(
                    sink,
                    EventPayload::NoteOff {
                        key: out_key,
                        voice_context: self.voice_context,
                    },
                )
            }
            MidiMessage::ControlChange {
                channel,
                controller: CC_SUSTAIN,
                value,
            } => {
                let bit = 1u16 << channel;
                if value >= CC_ON_THRESHOLD {
                    self.pedal_down |= bit;
                    0
                } else {
                    self.pedal_down &= !bit;
                    self.release(sink, channel as usize, Which::Sustained)
                }
            }
            // Every other controller, the pitch wheel and aftertouch are
            // **forwarded whole**, as performance events on the current
            // target — not as `ParamValue`s, which name one of this program's
            // controls by address and are the learn table's business (§14.4).
            // A hosted instrument gets them as the MIDI it would have
            // received; a built-in decides for itself what a wheel means.
            // The sustain pedal above is the one controller the router keeps,
            // because holding notes is its bookkeeping and not the
            // instrument's.
            //
            // **Except on a member channel** (`docs/flopsynth-next.md` §4.2,
            // MPE): a bend, a pressure or a slide (CC 74) on a channel this
            // router holds notes on, other than the first, belongs to those
            // notes and goes out as one `NoteMod` per note. MPE's own rule
            // without a mode switch — an ordinary keyboard sends everything
            // on the first channel and hears no difference, and an MPE
            // keyboard's members are two upwards. A member channel holding
            // nothing gets nothing: its bend was neither the channel's nor
            // a note's.
            MidiMessage::ControlChange {
                channel,
                controller: CC_SLIDE,
                value,
            } if self.is_member(channel) => {
                self.note_mods(sink, channel, |key| EventPayload::NoteMod {
                    key,
                    voice_context: self.voice_context,
                    pressure: None,
                    bend: None,
                    slide: Some(value),
                    mod_x: None,
                })
            }
            MidiMessage::ControlChange {
                channel,
                controller: CC_SLIDE,
                ..
            } if channel != 0 => 0,
            MidiMessage::ControlChange {
                controller, value, ..
            } => self.send(sink, EventPayload::Controller { controller, value }),
            MidiMessage::PitchBend { channel, value } if self.is_member(channel) => {
                self.note_mods(sink, channel, |key| EventPayload::NoteMod {
                    key,
                    voice_context: self.voice_context,
                    pressure: None,
                    bend: Some(value),
                    slide: None,
                    mod_x: None,
                })
            }
            MidiMessage::ChannelPressure { channel, value } if self.is_member(channel) => self
                .note_mods(sink, channel, |key| EventPayload::NoteMod {
                    key,
                    voice_context: self.voice_context,
                    pressure: Some(value),
                    bend: None,
                    slide: None,
                    mod_x: None,
                }),
            // A bend on a channel other than the first with nothing held:
            // MPE sends the bend just before the note, and a bend meant for
            // one note that is not yet sounding must not bend every note
            // that is.
            MidiMessage::PitchBend { channel, .. }
            | MidiMessage::ChannelPressure { channel, .. }
                if channel != 0 =>
            {
                0
            }
            MidiMessage::PitchBend { value, .. } => {
                self.send(sink, EventPayload::PitchBend { value })
            }
            MidiMessage::ChannelPressure { value, .. } => {
                self.send(sink, EventPayload::ChannelPressure { value })
            }
            // Poly aftertouch is its key's on any channel — for a key this
            // router is holding; a stray one releases nothing and moves
            // nothing.
            MidiMessage::PolyPressure {
                channel,
                key,
                value,
            } => {
                let Some(out_key) = self.map_key(key) else {
                    return 0;
                };
                let held = self.held[channel as usize] | self.sustained[channel as usize];
                if held & (1u128 << out_key) == 0 {
                    return 0;
                }
                self.send(
                    sink,
                    EventPayload::NoteMod {
                        key: out_key,
                        voice_context: self.voice_context,
                        pressure: Some(value),
                        bend: None,
                        slide: None,
                        mod_x: None,
                    },
                )
            }
            // A program change still goes nowhere: nothing here takes one,
            // and an event nothing reads would look like a working feature.
            MidiMessage::ProgramChange { .. } => 0,
            MidiMessage::AllNotesOff { channel } | MidiMessage::AllSoundOff { channel } => {
                // The two differ in whether release tails are allowed to ring,
                // and at this layer there is no way to say "stop now" — a
                // note-off is a release. Treating them alike is a real
                // limitation, and the panic button still does the important
                // half: nothing stays down.
                self.pedal_down &= !(1u16 << channel);
                self.release(sink, channel as usize, Which::Everything)
            }
        }
    }

    /// Releases every note this router has sounding, on every channel.
    ///
    /// Called when a device is unplugged (§14.2: disconnecting "does not
    /// produce stuck notes") and when live input is shut down. The pedal is
    /// lifted too — a device that goes away while its pedal is down would
    /// otherwise leave the sustained set unreleasable.
    pub fn release_all(&mut self, sink: &mut dyn EventSink) -> usize {
        self.pedal_down = 0;
        (0..16)
            .map(|c| self.release(sink, c, Which::Everything))
            .sum()
    }

    /// Whether anything this router started is still sounding.
    pub fn is_silent(&self) -> bool {
        self.held.iter().all(|h| *h == 0) && self.sustained.iter().all(|s| *s == 0)
    }

    fn release(&mut self, sink: &mut dyn EventSink, channel: usize, which: Which) -> usize {
        let mut keys = match which {
            Which::Sustained => self.sustained[channel],
            Which::Everything => self.held[channel] | self.sustained[channel],
        };
        match which {
            Which::Sustained => self.sustained[channel] = 0,
            Which::Everything => {
                self.held[channel] = 0;
                self.sustained[channel] = 0;
            }
        }

        let mut sent = 0;
        while keys != 0 {
            let key = keys.trailing_zeros() as u8;
            keys &= keys - 1;
            self.light(key, false);
            sent += self.send(
                sink,
                EventPayload::NoteOff {
                    key,
                    voice_context: self.voice_context,
                },
            );
        }
        sent
    }

    /// Applies the device's note remap and transpose. `None` when the result
    /// falls off the keyboard, which is a note that cannot be played rather
    /// than one that should wrap around to the other end.
    ///
    /// Both are fixed for the router's life. If they ever become live
    /// controls, they must be captured at note-on and reused for the matching
    /// note-off, or a change while a key is down leaves that note hanging.
    /// Whether `channel` is an MPE member right now: not the first, and
    /// holding notes of this router's.
    fn is_member(&self, channel: u8) -> bool {
        let channel = channel as usize;
        channel != 0 && (self.held[channel] | self.sustained[channel]) != 0
    }

    /// One `NoteMod` per note held on `channel`, built by `payload`.
    fn note_mods(
        &self,
        sink: &mut dyn EventSink,
        channel: u8,
        payload: impl Fn(u8) -> EventPayload,
    ) -> usize {
        let held = self.held[channel as usize] | self.sustained[channel as usize];
        let mut sent = 0;
        for key in 0..128u8 {
            if held & (1u128 << key) != 0 {
                sent += self.send(sink, payload(key));
            }
        }
        sent
    }

    fn map_key(&self, key: u8) -> Option<u8> {
        let remapped = *self.mapping.note_remap.get(&key).unwrap_or(&key);
        let transposed = remapped as i16 + self.mapping.transpose_semitones as i16;
        (0..=127).contains(&transposed).then_some(transposed as u8)
    }

    /// Turns one key's light on or off, when the window has asked for them.
    fn light(&self, key: u8, on: bool) {
        let Some(lit) = &self.lit else {
            return;
        };
        if on {
            lit.press(key);
        } else {
            lit.release(key);
        }
    }

    fn send(&self, sink: &mut dyn EventSink, payload: EventPayload) -> usize {
        // `sample` is filled in by the audio thread when it drains the queue:
        // only it knows which block this landed in.
        let queued = sink.send(TimedEvent {
            sample: 0,
            target: self.current,
            payload,
        });
        queued as usize
    }
}

#[derive(Clone, Copy)]
enum Which {
    Sustained,
    Everything,
}

fn message_channel(message: &MidiMessage) -> u8 {
    match *message {
        MidiMessage::NoteOn { channel, .. }
        | MidiMessage::NoteOff { channel, .. }
        | MidiMessage::ControlChange { channel, .. }
        | MidiMessage::PitchBend { channel, .. }
        | MidiMessage::ChannelPressure { channel, .. }
        | MidiMessage::PolyPressure { channel, .. }
        | MidiMessage::ProgramChange { channel, .. }
        | MidiMessage::AllNotesOff { channel }
        | MidiMessage::AllSoundOff { channel } => channel,
    }
}

impl VelocityCurve {
    /// Maps an incoming velocity to an outgoing one (TDD §14.3).
    ///
    /// Both curves are exact at full scale, so a device's loudest note is
    /// still the instrument's loudest note whatever curve is selected and the
    /// choice only changes the feel on the way there. They are *not* exact at
    /// the bottom, and should not be: making quiet playing carry further is
    /// the whole purpose of `Soft`.
    ///
    /// The result is floored at 1 rather than 0. `Hard` squares a normalised
    /// velocity, so the softest playable note would otherwise round to zero —
    /// and a velocity of zero is a note-off by convention everywhere in MIDI,
    /// including this crate's own decoder.
    pub fn apply(self, velocity: u8) -> u8 {
        let normalised = velocity as f32 / 127.0;
        let shaped = match self {
            Self::Linear => return velocity,
            Self::Fixed(value) => return value.min(127),
            // Quiet playing gets louder: the same physical effort reaches
            // further up the range.
            Self::Soft => normalised.sqrt(),
            Self::Hard => normalised * normalised,
        };
        (shaped * 127.0).round().clamp(1.0, 127.0) as u8
    }
}
