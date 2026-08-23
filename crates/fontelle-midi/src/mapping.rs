use std::collections::HashMap;

use crate::device::DeviceKey;

/// Per-device config, saved to the user config directory rather than the project —
/// it follows the user across projects, not the other way round (TDD §14.3).
#[derive(Debug, Clone, Default)]
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
