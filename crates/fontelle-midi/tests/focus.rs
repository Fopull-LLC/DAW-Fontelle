//! Live MIDI follows the instrument you have selected (TDD §14.3).
//!
//! Reported from using the window: *"I connected my usb midi controller and
//! was pressing keys but got no output"*. Two things were wrong, and this file
//! is the second of them — the first is that the window never opened a port at
//! all (`fontelle-app/src/main.rs`).
//!
//! The routing target used to be baked into each device's `MidiRouter` when
//! the port was opened, and §14.3's answer for "which instrument does a
//! keyboard play" was *the first channel in the song*. There is a focus now:
//! the channel rack has a selection, the mixer has one, and a keyboard that
//! plays whatever the roll is open on is the only behaviour that needs no
//! explaining.
//!
//! # Why a shared cell rather than reopening the port
//!
//! A device callback runs on the driver's own thread and may not block, so the
//! target cannot live behind a lock, and reopening a `midir` connection to
//! change one integer would drop whatever was being played across it. A
//! `LiveTarget` is one atomic the UI thread stores into and the callback
//! loads.
//!
//! # The thing that makes it correct
//!
//! A note-off has to reach the instrument the note-on went to. A router that
//! simply read the cell per message would send the note-on to the piano, and
//! then — if you clicked another channel while holding the key — the note-off
//! to the strings, leaving the piano note sounding forever. So the router
//! *latches* the target and releases everything it is holding on the old one
//! before it adopts the new.

use std::sync::Arc;

use fontelle_midi::{DeviceMapping, LiveTarget, MidiRouter};
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
    /// What each event was, and which node it went to.
    fn routed(&self) -> Vec<(&'static str, u8, NodeId)> {
        self.events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::NoteOn { key, .. } => Some(("on", *key, e.target)),
                EventPayload::NoteOff { key, .. } => Some(("off", *key, e.target)),
                _ => None,
            })
            .collect()
    }
}

fn node(id: u64) -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(id))
}

const PIANO: u64 = 7;
const STRINGS: u64 = 9;

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;
const CC: u8 = 0xB0;
const SUSTAIN: u8 = 64;

fn following(target: &Arc<LiveTarget>) -> MidiRouter {
    MidiRouter::following(Arc::clone(target), 42, DeviceMapping::default())
}

#[test]
fn a_router_plays_the_node_its_target_names() {
    let target = Arc::new(LiveTarget::new(node(PIANO)));
    let mut r = following(&target);
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);

    assert_eq!(
        out.routed(),
        vec![("on", 60, node(PIANO)), ("off", 60, node(PIANO))]
    );
}

#[test]
fn changing_the_target_moves_where_the_next_note_goes() {
    let target = Arc::new(LiveTarget::new(node(PIANO)));
    let mut r = following(&target);
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    // Somebody clicked another channel in the rack.
    target.set(node(STRINGS));
    r.handle(&[NOTE_ON, 64, 100], &mut out);

    assert_eq!(
        out.routed(),
        vec![
            ("on", 60, node(PIANO)),
            ("off", 60, node(PIANO)),
            ("on", 64, node(STRINGS)),
        ]
    );
}

#[test]
fn a_note_held_across_a_change_is_released_on_the_instrument_that_sounded_it() {
    // The whole reason the target is latched rather than read per message.
    // Sending this note-off to the strings would leave the piano note sounding
    // for the rest of the session, with nothing left that could ever stop it.
    let target = Arc::new(LiveTarget::new(node(PIANO)));
    let mut r = following(&target);
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    target.set(node(STRINGS));
    r.handle(&[NOTE_ON, 64, 100], &mut out);

    assert_eq!(
        out.routed(),
        vec![
            ("on", 60, node(PIANO)),
            // Let go on the way past, by the router itself.
            ("off", 60, node(PIANO)),
            ("on", 64, node(STRINGS)),
        ]
    );
    // And the released note is genuinely forgotten, so its own note-off — when
    // the player finally lifts the key — is dropped rather than cutting a
    // voice on the new instrument.
    let before = out.routed().len();
    r.handle(&[NOTE_OFF, 60, 0], &mut out);
    assert_eq!(out.routed().len(), before, "key 60 was already released");
}

#[test]
fn a_pedal_held_across_a_change_does_not_strand_its_notes() {
    // Sustained notes are held by the router just as firmly as pressed ones,
    // and the pedal is lifted with them: a device whose pedal is down when the
    // target moves would otherwise carry a sustained set belonging to an
    // instrument it is no longer playing.
    let target = Arc::new(LiveTarget::new(node(PIANO)));
    let mut r = following(&target);
    let mut out = Recorder::default();

    r.handle(&[CC, SUSTAIN, 127], &mut out);
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    r.handle(&[NOTE_OFF, 60, 0], &mut out); // held by the pedal, not sent
    assert_eq!(out.routed(), vec![("on", 60, node(PIANO))]);

    target.set(node(STRINGS));
    r.handle(&[NOTE_ON, 64, 100], &mut out);

    assert_eq!(
        out.routed(),
        vec![
            ("on", 60, node(PIANO)),
            ("off", 60, node(PIANO)),
            ("on", 64, node(STRINGS)),
        ]
    );
    assert!(!r.is_silent(), "key 64 is still down");
}

#[test]
fn a_target_that_has_not_moved_costs_nothing() {
    // Storing the same node again must not release anything: the studio
    // republishes the target after every graph rebuild, and a rebuild that cut
    // the note somebody was holding would be its own bug report.
    let target = Arc::new(LiveTarget::new(node(PIANO)));
    let mut r = following(&target);
    let mut out = Recorder::default();

    r.handle(&[NOTE_ON, 60, 100], &mut out);
    target.set(node(PIANO));
    r.handle(&[NOTE_ON, 64, 100], &mut out);

    assert_eq!(
        out.routed(),
        vec![("on", 60, node(PIANO)), ("on", 64, node(PIANO))]
    );
}

#[test]
fn a_fixed_target_is_still_a_target() {
    // `MidiRouter::new` is what the offline paths and every existing test use,
    // and it has to keep meaning "this node, for ever".
    let mut r = MidiRouter::new(node(PIANO), 42, DeviceMapping::default());
    let mut out = Recorder::default();
    r.handle(&[NOTE_ON, 60, 100], &mut out);
    assert_eq!(out.routed(), vec![("on", 60, node(PIANO))]);
}

#[test]
fn a_target_starts_wherever_it_was_made() {
    let target = LiveTarget::new(node(STRINGS));
    assert_eq!(target.get(), node(STRINGS));
    target.set(node(PIANO));
    assert_eq!(target.get(), node(PIANO));
    // The default is the null node, which is what an app that has not realised
    // a graph yet has to be able to say.
    assert_eq!(LiveTarget::default().get(), NodeId::default());
}
