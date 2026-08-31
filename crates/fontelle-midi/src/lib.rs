//! MIDI device abstraction, routing, and mapping (FONTELLE_TDD.md §14). Staged as
//! one complete pass at M5 (§14.1) — but the scaffold's event pipeline shape must
//! already be the RT-safe, device-agnostic one it will keep, so that landing the
//! full feature set later is additive rather than a transport rewrite.

mod clock;
mod device;
mod file_io;
mod learn;
mod mapping;
mod message;
mod router;

pub use clock::{ClockSource, ClockSync};
pub use device::{DeviceKey, HotplugReport, MidiError, MidiHub, RouteTo, available_inputs};
pub use file_io::{MidiExportError, export_midi_file};
pub use learn::{CcKey, LearnMode, MidiLearnTable, TakeoverMode};
pub use mapping::{DeviceMapping, MappingTable, VelocityCurve};
pub use message::{MidiMessage, decode};
pub use router::{LiveTarget, MidiRouter};
