use fontelle_types::{EventSink, NodeId};

use crate::mapping::{LiveMapping, MappingTable};
use crate::router::{LiveTarget, MidiRouter};

/// Stable across replug/reboot: name + port + USB identifiers where available
/// (TDD §14.2).
///
/// **Today it is the port name alone.** `midir` exposes no portable way to
/// reach a device's USB identifiers, and inventing a key from what it does
/// expose (an index that renumbers when anything else is plugged in) would be
/// worse than a name: configuration would silently follow the wrong device.
/// A name is stable across replug on every backend here, and duplicate names
/// are disambiguated by the port's own id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceKey(pub String);

#[derive(Debug)]
pub struct MidiError(pub String);

impl std::fmt::Display for MidiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MidiError {}

/// What a newly connected device is wired up to.
#[derive(Debug, Clone)]
pub struct RouteTo {
    /// The engine node every device plays — **shared and live** (§14.3), so
    /// selecting another instrument in the window moves the keyboard to it
    /// without reopening a single port. See [`LiveTarget`].
    pub target: std::sync::Arc<LiveTarget>,
    /// Separates a live player's notes from the timeline's, so a sequenced
    /// note-off cannot cut a note the player is holding (TDD §11.4).
    pub voice_context: u32,
}

impl RouteTo {
    /// A route to one node, for ever — what an offline path with no window to
    /// follow wants.
    pub fn to(node: NodeId, voice_context: u32) -> Self {
        Self {
            target: std::sync::Arc::new(LiveTarget::new(node)),
            voice_context,
        }
    }
}

/// The callback's state, and what `close` hands back when a device goes away.
/// Keeping the router here rather than behind a lock is what lets a disconnect
/// release exactly the notes that device was holding without any
/// synchronisation at all.
struct Live {
    router: MidiRouter,
    sink: Box<dyn EventSink>,
}

struct Connection {
    key: DeviceKey,
    connection: midir::MidiInputConnection<Live>,
}

/// What one `poll` changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HotplugReport {
    pub connected: Vec<DeviceKey>,
    pub disconnected: Vec<DeviceKey>,
    /// Devices that appeared but could not be opened — no free port left, or
    /// the backend refused. Reported rather than logged and forgotten, because
    /// "I plugged my keyboard in and nothing happened" needs an answer.
    pub failed: Vec<DeviceKey>,
}

impl HotplugReport {
    pub fn is_empty(&self) -> bool {
        self.connected.is_empty() && self.disconnected.is_empty() && self.failed.is_empty()
    }
}

/// All input devices are opened and merged automatically into one logical
/// stream (TDD §14.2) — no device-selection step, no enable checkboxes, no
/// "which controller is this" dialog. Hot-plug is handled live: connecting a
/// device mid-session just works, and disconnecting releases all notes for
/// that device rather than leaving them stuck.
///
/// **The pipeline shape is the one §14.1 insists on**: a device callback
/// decodes bytes, routes them, and pushes `TimedEvent`s into a lock-free queue
/// the audio thread drains per block. Nothing polls on the UI thread and
/// nothing calls `note_on` directly, which is the shortcut §14.1 exists to
/// forbid.
pub struct MidiHub {
    connections: Vec<Connection>,
    mappings: MappingTable,
    route: RouteTo,
    /// The input settings the window can change while devices are open
    /// (TDD §14.3). Shared with every router this hub opens, so a change
    /// reaches a keyboard already plugged in — see [`crate::LiveMapping`].
    input: Option<std::sync::Arc<LiveMapping>>,
    /// Where every open device mirrors the keys it is holding, for the window
    /// to draw (TDD §14.1). One cell for the hub, not one per device: there is
    /// one keyboard on screen. `None` on every path with no window.
    keys: Option<std::sync::Arc<crate::LiveKeys>>,
}

impl MidiHub {
    pub fn new(route: RouteTo) -> Self {
        Self {
            connections: Vec::new(),
            mappings: MappingTable::default(),
            route,
            input: None,
            keys: None,
        }
    }

    /// Gives the hub the cell the window's settings tab writes into.
    ///
    /// Every device opened from now on follows it, and so does every device
    /// already open — they share the one `Arc`, which is the whole reason it
    /// is one rather than a value copied in at open time.
    pub fn with_input_settings(mut self, settings: std::sync::Arc<LiveMapping>) -> Self {
        self.input = Some(settings);
        self
    }

    /// Gives the hub the cell the window lights its keyboard from.
    ///
    /// Every device opened from now on writes into it, and so does every
    /// device already open — one `Arc`, shared, the same as the settings
    /// above. See [`crate::LiveKeys`].
    pub fn with_live_keys(mut self, keys: std::sync::Arc<crate::LiveKeys>) -> Self {
        self.keys = Some(keys);
        self
    }

    /// Where every open device is pointed. Moving it moves them all at once —
    /// there is one focus, not one per keyboard.
    pub fn target(&self) -> std::sync::Arc<LiveTarget> {
        std::sync::Arc::clone(&self.route.target)
    }

    pub fn connected_devices(&self) -> Vec<DeviceKey> {
        self.connections.iter().map(|c| c.key.clone()).collect()
    }

    /// Opens everything newly present and closes everything gone, releasing
    /// the notes of anything that went away.
    ///
    /// `sinks` supplies a queue for each newly connected device; returning
    /// `None` means there are no ports left, and the device is reported as
    /// failed rather than silently ignored. Called on a timer from a non-RT
    /// thread — `midir` has no hot-plug notification on any backend, so
    /// re-enumeration is the only mechanism available.
    pub fn poll<F>(&mut self, mut sinks: F) -> Result<HotplugReport, MidiError>
    where
        F: FnMut() -> Option<Box<dyn EventSink>>,
    {
        let present = enumerate()?;
        let mut report = HotplugReport::default();

        // Gone first, so a device that was unplugged frees its port before a
        // replacement asks for one.
        let mut index = 0;
        while index < self.connections.len() {
            if present
                .iter()
                .any(|key| *key == self.connections[index].key)
            {
                index += 1;
                continue;
            }
            let Connection { key, connection } = self.connections.remove(index);
            // `close` hands back the callback's state, which is where that
            // device's held notes are recorded. Without this a note held at
            // the moment a cable comes out sounds forever: its note-off was
            // going to come from a device that no longer exists.
            let (_input, mut live) = connection.close();
            live.router.release_all(live.sink.as_mut());
            report.disconnected.push(key);
        }

        for key in present {
            if self.connections.iter().any(|c| c.key == key) {
                continue;
            }
            match sinks() {
                Some(sink) => match self.open(&key, sink) {
                    Ok(connection) => {
                        self.connections.push(connection);
                        report.connected.push(key);
                    }
                    Err(_) => report.failed.push(key),
                },
                None => report.failed.push(key),
            }
        }

        Ok(report)
    }

    /// Closes every device, releasing whatever they were holding. Run this
    /// before the audio stream goes away, or the last notes played stay down.
    pub fn shutdown(&mut self) {
        for Connection { connection, .. } in self.connections.drain(..) {
            let (_input, mut live) = connection.close();
            live.router.release_all(live.sink.as_mut());
        }
    }

    fn open(&self, key: &DeviceKey, sink: Box<dyn EventSink>) -> Result<Connection, MidiError> {
        let input = midir::MidiInput::new("Fontelle").map_err(|e| MidiError(e.to_string()))?;
        let port = input
            .ports()
            .into_iter()
            .find(|port| input.port_name(port).map(DeviceKey).as_ref() == Ok(key))
            .ok_or_else(|| MidiError(format!("{} disappeared while opening it", key.0)))?;

        let mut router = MidiRouter::following(
            std::sync::Arc::clone(&self.route.target),
            self.route.voice_context,
            self.mappings.for_device(key),
        );
        if let Some(settings) = &self.input {
            router = router.following_input(std::sync::Arc::clone(settings));
        }
        if let Some(keys) = &self.keys {
            router = router.watching_keys(std::sync::Arc::clone(keys));
        }
        let live = Live { router, sink };
        let connection = input
            .connect(
                &port,
                "fontelle-in",
                |_timestamp, bytes, live: &mut Live| {
                    // The device's own thread. It decodes, maps and queues —
                    // and never blocks, because `EventSink::send` refuses
                    // rather than waiting when the queue is full.
                    live.router.handle(bytes, live.sink.as_mut());
                },
                live,
            )
            .map_err(|e| MidiError(e.to_string()))?;

        Ok(Connection {
            key: key.clone(),
            connection,
        })
    }
}

impl Drop for MidiHub {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Every MIDI input port present right now.
fn enumerate() -> Result<Vec<DeviceKey>, MidiError> {
    let input = midir::MidiInput::new("Fontelle scan").map_err(|e| MidiError(e.to_string()))?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .map(DeviceKey)
        .collect())
}

/// The names of every MIDI input available, for reporting.
pub fn available_inputs() -> Result<Vec<String>, MidiError> {
    Ok(enumerate()?.into_iter().map(|key| key.0).collect())
}
