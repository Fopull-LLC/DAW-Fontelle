use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::device::DeviceKey;

/// Per-device config, saved to the user config directory rather than the project —
/// it follows the user across projects, not the other way round (TDD §14.3).
#[derive(Debug, Clone)]
pub struct DeviceMapping {
    /// Incoming note -> outgoing note. The drum-pad-to-drum-soundfont workflow:
    /// click the target key, hit the pad.
    pub note_remap: HashMap<u8, u8>,
    pub channel_filter: Option<u8>,
    pub transpose_semitones: i8,
    pub velocity_curve: VelocityCurve,
    pub velocity_range: (u8, u8),
    pub routed_channel: Option<fontelle_types::ChannelId>,
}

impl Default for DeviceMapping {
    /// A device nobody has configured passes everything through unchanged.
    ///
    /// **Not `#[derive(Default)]`**, which is what this was: a derived
    /// `velocity_range` is `(0, 0)`, and since the range is a window a note
    /// must fall inside, that is a default which silently discards every note
    /// from every device. §14.3's rule that per-device config is "optional
    /// refinement, never required setup" makes the identity mapping the only
    /// correct default, and a range is the one field whose identity value is
    /// not its zero.
    fn default() -> Self {
        Self {
            note_remap: HashMap::new(),
            channel_filter: None,
            transpose_semitones: 0,
            velocity_curve: VelocityCurve::Linear,
            velocity_range: (0, 127),
            routed_channel: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VelocityCurve {
    #[default]
    Linear,
    Soft,
    Hard,
    Fixed(u8),
}

#[derive(Debug, Default)]
pub struct MappingTable {
    per_device: HashMap<DeviceKey, DeviceMapping>,
}

impl MappingTable {
    pub fn for_device(&self, key: &DeviceKey) -> DeviceMapping {
        self.per_device.get(key).cloned().unwrap_or_default()
    }
}

/// The part of a [`DeviceMapping`] a person changes while playing (TDD §14.3).
///
/// Every field here is a scalar, and that is the point: this is the half of
/// the mapping that has to reach a `midir` callback on the driver's own
/// thread, where a lock is not allowed and a `HashMap` cannot go. The note
/// remap — the drum-pad workflow — stays in `DeviceMapping`, because it is set
/// by clicking a target key and a pad rather than by nudging a number, and it
/// is not something anybody changes mid-phrase.
///
/// `Default` is the identity, for §14.3's reason: per-device config is an
/// "optional refinement, never required setup", so a person who never opens
/// the settings tab gets exactly what they got before it existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSettings {
    pub velocity_curve: VelocityCurve,
    /// The window an incoming velocity must fall inside to be this device's
    /// note at all. Both ends inclusive.
    pub velocity_range: (u8, u8),
    pub transpose_semitones: i8,
    /// Which MIDI channel to listen to. `None` is all of them.
    pub channel_filter: Option<u8>,
}

impl Default for InputSettings {
    fn default() -> Self {
        let identity = DeviceMapping::default();
        Self {
            velocity_curve: identity.velocity_curve,
            velocity_range: identity.velocity_range,
            transpose_semitones: identity.transpose_semitones,
            channel_filter: identity.channel_filter,
        }
    }
}

/// The value a curve that carries one is stored with, and the marker for the
/// three that carry none.
const CURVE_LINEAR: u8 = 0;
const CURVE_SOFT: u8 = 1;
const CURVE_HARD: u8 = 2;
const CURVE_FIXED: u8 = 3;
/// The byte that means "every channel", which is not a channel: 0..=15 are.
const NO_CHANNEL: u8 = 0xFF;

impl InputSettings {
    /// The whole of it in one integer, so it can cross onto a device callback
    /// thread as a single atomic load. Six bytes of a `u64`, laid out low to
    /// high; the top two are spare for whatever §14.4's learn table needs.
    pub fn to_bits(self) -> u64 {
        let (kind, value) = match self.velocity_curve {
            VelocityCurve::Linear => (CURVE_LINEAR, 0),
            VelocityCurve::Soft => (CURVE_SOFT, 0),
            VelocityCurve::Hard => (CURVE_HARD, 0),
            VelocityCurve::Fixed(v) => (CURVE_FIXED, v),
        };
        let byte = |b: u8, shift: u32| (b as u64) << shift;
        byte(self.velocity_range.0, 0)
            | byte(self.velocity_range.1, 8)
            | byte(self.transpose_semitones as u8, 16)
            | byte(self.channel_filter.unwrap_or(NO_CHANNEL), 24)
            | byte(kind, 32)
            | byte(value, 40)
    }

    /// The inverse. **Total**, because what it reads is one relaxed load off
    /// a cell another thread is writing, and a device callback has nowhere to
    /// report a malformed one to: an unknown curve reads as `Linear` and a
    /// channel outside 0..=15 as "every channel", both of which are the
    /// identity rather than a guess.
    pub fn from_bits(bits: u64) -> Self {
        let byte = |shift: u32| ((bits >> shift) & 0xFF) as u8;
        let channel = byte(24);
        Self {
            velocity_range: (byte(0), byte(8)),
            transpose_semitones: byte(16) as i8,
            channel_filter: (channel < 16).then_some(channel),
            velocity_curve: match byte(32) {
                CURVE_SOFT => VelocityCurve::Soft,
                CURVE_HARD => VelocityCurve::Hard,
                CURVE_FIXED => VelocityCurve::Fixed(byte(40).min(127)),
                _ => VelocityCurve::Linear,
            },
        }
    }
}

/// [`InputSettings`] as the window and a device callback share them
/// (TDD §14.3).
///
/// The same shape, and for the same reasons, as
/// [`LiveTarget`](crate::LiveTarget): one atomic, stored into by the UI thread
/// and loaded by every device callback. A lock would let a UI thread block a
/// driver thread, and reopening the port to change one number would drop
/// whatever was being played across it.
#[derive(Debug)]
pub struct LiveMapping {
    bits: AtomicU64,
}

impl Default for LiveMapping {
    fn default() -> Self {
        Self::new(InputSettings::default())
    }
}

impl LiveMapping {
    pub fn new(settings: InputSettings) -> Self {
        Self {
            bits: AtomicU64::new(settings.to_bits()),
        }
    }

    pub fn get(&self) -> InputSettings {
        InputSettings::from_bits(self.bits.load(Ordering::Relaxed))
    }

    /// Changes what live input does from the next note on. What is *already*
    /// down is let go of first — see [`MidiRouter::handle`].
    ///
    /// [`MidiRouter::handle`]: crate::MidiRouter::handle
    pub fn set(&self, settings: InputSettings) {
        self.bits.store(settings.to_bits(), Ordering::Relaxed);
    }
}
