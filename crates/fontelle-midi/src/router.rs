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

use fontelle_types::{EventPayload, EventSink, NodeId, TimedEvent};

use crate::mapping::{DeviceMapping, VelocityCurve};
use crate::message::{MidiMessage, decode};

/// CC 64. The one controller worth handling before the learn table exists:
/// without it, half of playing a keyboard part is missing.
const CC_SUSTAIN: u8 = 64;
/// A controller is "on" at 64 and above. The MIDI spec is explicit about this
/// and it is not 1: a continuous pedal sweeping through 40 is still up.
const CC_ON_THRESHOLD: u8 = 64;

pub struct MidiRouter {
    mapping: DeviceMapping,
    target: NodeId,
    voice_context: u32,
    /// Bit per key, per channel: notes this router has sounded and not yet
    /// released.
    held: [u128; 16],
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
        Self {
            mapping,
            target,
            voice_context,
            held: [0; 16],
            sustained: [0; 16],
            pedal_down: 0,
        }
    }

    /// Decodes one MIDI packet and queues whatever it produces. Returns the
    /// number of events queued, which is zero for the many messages that are
    /// filtered, ignored, or merely change this router's own state.
    ///
    /// A `sink` that refuses (a full queue) is not retried: see
    /// `EventSink::send`.
    pub fn handle(&mut self, bytes: &[u8], sink: &mut dyn EventSink) -> usize {
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
                self.send(
                    sink,
                    EventPayload::NoteOn {
                        key: out_key,
                        velocity: self.mapping.velocity_curve.apply(velocity),
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
                    self.sustained[channel] |= bit;
                    return 0;
                }
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
            // Every other controller is silently dropped, and that is the
            // honest state of things rather than an oversight: routing a CC
            // to a parameter is the MIDI-learn table (§14.4) resolving it to a
            // `ParamAddress`, and the nodes it would address expose no
            // parameters yet. Emitting `ParamValue` events that nothing reads
            // would look like a working feature.
            MidiMessage::ControlChange { .. }
            | MidiMessage::PitchBend { .. }
            | MidiMessage::ChannelPressure { .. }
            | MidiMessage::ProgramChange { .. } => 0,
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
    fn map_key(&self, key: u8) -> Option<u8> {
        let remapped = *self.mapping.note_remap.get(&key).unwrap_or(&key);
        let transposed = remapped as i16 + self.mapping.transpose_semitones as i16;
        (0..=127).contains(&transposed).then_some(transposed as u8)
    }

    fn send(&self, sink: &mut dyn EventSink, payload: EventPayload) -> usize {
        // `sample` is filled in by the audio thread when it drains the queue:
        // only it knows which block this landed in.
        let queued = sink.send(TimedEvent {
            sample: 0,
            target: self.target,
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
