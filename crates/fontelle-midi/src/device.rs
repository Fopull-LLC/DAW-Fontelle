/// Stable across replug/reboot: name + port + USB identifiers where available
/// (TDD §14.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceKey(pub String);

/// All input devices are opened and merged automatically into one logical stream
/// (TDD §14.2) — no device-selection step, no enable checkboxes, no "which
/// controller is this" dialog. Hot-plug is handled live: connecting a device
/// mid-session just works, and disconnecting releases all notes for that device
/// rather than leaving them stuck.
///
/// **Staging note (TDD §14.1):** even at skeleton stage, this must already feed
/// the same RT-safe, sample-accurate event pipeline as notes and automation
/// (`fontelle_types::TimedEvent`) — not poll on the UI thread and inject notes
/// directly. Shortcutting that here turns the eventual full MIDI pass (M5) into a
/// transport rewrite instead of an addition.
pub struct MidiHub {
    devices: Vec<DeviceKey>,
}

impl MidiHub {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub fn connected_devices(&self) -> &[DeviceKey] {
        &self.devices
    }

    pub fn poll_hotplug(&mut self) {
        todo!("midir port enumeration diffed against `devices`, open/close as needed")
    }
}

impl Default for MidiHub {
    fn default() -> Self {
        Self::new()
    }
}
