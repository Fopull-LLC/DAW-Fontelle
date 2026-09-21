//! Live input settings a person can change while playing (TDD §14.3).
//!
//! Reported from using a keyboard: *"the options [should] also be configurable
//! in a settings tab so I can adjust like my velocity for example on my midi
//! input, since every single input device will register differently."* §14.3
//! has always said per-device mapping is an "optional refinement, never
//! required setup" — [`DeviceMapping`] and [`VelocityCurve`] have existed
//! since this crate did. What was missing was any way for a person to *set*
//! them: `MappingTable` was empty, nothing wrote into it, and every device got
//! the identity mapping for ever.
//!
//! # Why a shared cell, and not a reopened port
//!
//! Exactly the reason `focus.rs` gives for the routing target, so it is worth
//! saying once rather than twice: a device callback runs on the driver's own
//! thread and may not block, so this cannot live behind a lock; and reopening
//! a `midir` connection to change one number would drop whatever was being
//! played across it. [`LiveMapping`] is one atomic the UI thread stores into
//! and the callback loads.
//!
//! # The thing that makes it correct
//!
//! Transpose is applied on the way in, so a note-off has to be mapped the same
//! way its note-on was. A router that read the cell per message would send a
//! note-on at 60, and — if you nudged transpose while holding the key — look
//! for a held note at 62 when the release arrived, find nothing, and leave the
//! first one sounding for ever. So the router *latches* the settings and lets
//! go of everything it is holding before it adopts new ones, which is the same
//! rule, and the same code shape, as the routing target.

use std::sync::Arc;

use fontelle_midi::{DeviceMapping, InputSettings, LiveMapping, MidiRouter, VelocityCurve};
use fontelle_types::{EventPayload, EventSink, NodeId, TimedEvent};

#[derive(Default)]
struct Recorder {
    events: Vec<TimedEvent>,
}

impl EventSink for Recorder {
    fn send(&mut self, event: TimedEvent) -> bool {
        self.events.push(event);
        true
    }
}

impl Recorder {
    fn notes(&self) -> Vec<(&'static str, u8, u8)> {
        self.events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::NoteOn { key, velocity, .. } => Some(("on", *key, *velocity)),
                EventPayload::NoteOff { key, .. } => Some(("off", *key, 0)),
                _ => None,
            })
            .collect()
    }
}

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;

fn node() -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(7))
}

fn rig(settings: InputSettings) -> (MidiRouter, Arc<LiveMapping>, Recorder) {
    let live = Arc::new(LiveMapping::new(settings));
    let router =
        MidiRouter::new(node(), 0, DeviceMapping::default()).following_input(Arc::clone(&live));
    (router, live, Recorder::default())
}

// ------------------------------------------------- the settings themselves ---

#[test]
fn the_settings_a_device_needs_none_of_change_nothing() {
    // §14.3's rule, restated as a test: per-device config is an optional
    // refinement. A person who never opens the tab must get exactly what they
    // got before it existed.
    let (mut r, _live, mut out) = rig(InputSettings::default());
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 60, 100), ("off", 60, 0)]);
}

#[test]
fn every_setting_survives_the_trip_through_one_atomic() {
    // The cell is a `u64` because a device callback may not take a lock, so
    // the whole of `InputSettings` has to fit in one — including the two ends
    // of the velocity window, a negative transpose, and the two fields that
    // are `Option`s.
    for settings in [
        InputSettings::default(),
        InputSettings {
            velocity_curve: VelocityCurve::Fixed(100),
            velocity_range: (12, 118),
            transpose_semitones: -24,
            channel_filter: Some(9),
        },
        InputSettings {
            velocity_curve: VelocityCurve::Soft,
            velocity_range: (0, 127),
            transpose_semitones: 24,
            channel_filter: None,
        },
        InputSettings {
            velocity_curve: VelocityCurve::Hard,
            velocity_range: (1, 1),
            transpose_semitones: 0,
            channel_filter: Some(0),
        },
    ] {
        let cell = LiveMapping::new(settings);
        assert_eq!(cell.get(), settings, "{settings:?} did not survive");
    }
}

#[test]
fn a_velocity_curve_set_from_the_window_shapes_what_is_played() {
    // The reported ask, exactly: the same key press on the same keyboard,
    // reading differently because the person said so.
    let (mut r, live, mut out) = rig(InputSettings::default());
    r.handle(&[NOTE_ON, 60, 64], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    let plain = out.notes()[0].2;

    live.set(InputSettings {
        velocity_curve: VelocityCurve::Soft,
        ..InputSettings::default()
    });
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 64], &mut out);
    assert!(
        out.notes()[0].2 > plain,
        "a soft curve has to make the same press read louder than {plain}"
    );
}

#[test]
fn a_transpose_set_from_the_window_moves_the_keyboard() {
    let (mut r, live, mut out) = rig(InputSettings::default());
    live.set(InputSettings {
        transpose_semitones: 12,
        ..InputSettings::default()
    });
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 72, 100), ("off", 72, 0)]);
}

#[test]
fn a_velocity_window_set_from_the_window_filters_what_gets_through() {
    let (mut r, live, mut out) = rig(InputSettings::default());
    live.set(InputSettings {
        velocity_range: (40, 127),
        ..InputSettings::default()
    });
    r.handle(&[NOTE_ON, 60, 20], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert!(
        out.notes().is_empty(),
        "a note under the window is not this device's note, and neither is its \
         release: {:?}",
        out.notes()
    );
    r.handle(&[NOTE_ON, 62, 90], &mut out);
    assert_eq!(out.notes(), vec![("on", 62, 90)]);
}

#[test]
fn a_channel_filter_set_from_the_window_keeps_one_channel() {
    let (mut r, live, mut out) = rig(InputSettings::default());
    live.set(InputSettings {
        channel_filter: Some(3),
        ..InputSettings::default()
    });
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_ON | 3, 64, 100], &mut out);
    assert_eq!(out.notes(), vec![("on", 64, 100)]);
}

// ------------------------------------------------------ changing them live ---

#[test]
fn changing_a_setting_lets_go_of_what_is_held_before_it_takes_effect() {
    // The correctness argument in this file's own header. Without it, the
    // note-off is mapped by the *new* transpose, finds nothing held at that
    // key, and is dropped — leaving the first note sounding with nothing left
    // in the system able to stop it.
    let (mut r, live, mut out) = rig(InputSettings::default());
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    live.set(InputSettings {
        transpose_semitones: 2,
        ..InputSettings::default()
    });
    r.handle(&[NOTE_OFF, 60, 0], &mut out);

    assert_eq!(
        out.notes(),
        vec![("on", 60, 100), ("off", 60, 0)],
        "the release has to reach the note that was actually started"
    );
    assert!(r.is_silent(), "nothing may be left hanging");
}

#[test]
fn the_settings_are_read_once_and_not_per_message() {
    // A latch, like the routing target: what a note-on was mapped with is what
    // its note-off is mapped with, whatever happened in between.
    let (mut r, live, mut out) = rig(InputSettings::default());
    live.set(InputSettings {
        transpose_semitones: 5,
        ..InputSettings::default()
    });
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 65, 100), ("off", 65, 0)]);
    assert!(r.is_silent());
}

#[test]
fn a_router_with_no_shared_cell_behaves_exactly_as_it_did_before() {
    // Every offline path builds one without a window to change anything.
    let mut r = MidiRouter::new(node(), 0, DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.notes(), vec![("on", 60, 100), ("off", 60, 0)]);
}

// ------------------------------------------------- saying what was dropped ---

/// A note the settings filtered out is **counted, with why**, on the
/// same shared cell the key lights live in — so the window can say "your
/// keyboard's note on channel 1 was ignored: the channel filter is set to
/// 16" rather than nothing. *"my midi keyboard isn't working"* was a
/// settings file with `channel_filter: 15` and a velocity window of
/// 50–125 in it; the keyboard was working, and silently filtered.
#[test]
fn a_filtered_note_is_counted_with_the_reason() {
    use fontelle_midi::{Ignored, LiveKeys};
    let keys = Arc::new(LiveKeys::default());
    let live = Arc::new(LiveMapping::new(InputSettings {
        channel_filter: Some(15),
        velocity_range: (50, 125),
        ..InputSettings::default()
    }));
    let mut r = MidiRouter::new(node(), 0, DeviceMapping::default())
        .following_input(Arc::clone(&live))
        .watching_keys(Arc::clone(&keys));
    let mut out = Recorder::default();
    assert_eq!(keys.ignored(), None, "nothing dropped yet");

    // Channel 1 (index 0) under a filter for channel 16.
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    assert_eq!(
        keys.ignored(),
        Some((1, Ignored::Channel(0))),
        "one note, dropped by the channel filter"
    );
    // On the kept channel but under the velocity window.
    r.handle(&[NOTE_ON | 15, 60, 20], &mut out);
    assert_eq!(keys.ignored(), Some((2, Ignored::Velocity(20))));
    // A note that gets through changes nothing.
    r.handle(&[NOTE_ON | 15, 60, 100], &mut out);
    assert_eq!(keys.ignored(), Some((2, Ignored::Velocity(20))));
    assert_eq!(out.notes(), vec![("on", 60, 100)]);
    // Note-offs and controllers are not notes nobody heard: not counted.
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(keys.ignored(), Some((2, Ignored::Velocity(20))));
}
